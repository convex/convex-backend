use std::{
    collections::BTreeMap,
    str::FromStr,
    sync::LazyLock,
};

use anyhow::Context as _;
use common::{
    bootstrap_model::components::EnvBinding,
    components::ComponentId,
    document::ParsedDocument,
    http::RequestDestination,
    runtime::Runtime,
    types::{
        EnvVarName,
        EnvVarValue,
    },
};
use database::{
    BootstrapComponentsModel,
    Transaction,
};
use model::{
    canonical_urls::{
        types::CanonicalUrl,
        CanonicalUrlsModel,
    },
    environment_variables::{
        EnvironmentVariablesModel,
        PreloadedEnvironmentVariables,
    },
};
use value::identifier::Identifier;

pub static CONVEX_ORIGIN: LazyLock<EnvVarName> = LazyLock::new(|| {
    "CONVEX_CLOUD_URL"
        .parse()
        .expect("CONVEX_CLOUD_URL should be a valid EnvVarName")
});

pub static CONVEX_SITE: LazyLock<EnvVarName> = LazyLock::new(|| {
    "CONVEX_SITE_URL"
        .parse()
        .expect("CONVEX_SITE_URL should be a valid EnvVarName")
});

// Definitions used throughout the codebase:
// - `default_system_env_vars` means the .convex.cloud/.convex.site urls (or
//   otherwise statically configured urls),
// - `system_env_var_overrides` means the canonical urls,
// - `system_env_vars` means the merged `default_system_env_vars` and
//   `system_env_var_overrides`.
// - `user_environment_variables` means user-defined env vars in the dashboard.
// - `environment_variables` means the merged `system_env_vars` and
//   `user_environment_variables`.
// In most cases, function executions use `environment_variables`, although
// often they are computed at different times and merged later.
pub async fn system_env_vars<RT: Runtime>(
    tx: &mut Transaction<RT>,
    default_system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
) -> anyhow::Result<BTreeMap<EnvVarName, EnvVarValue>> {
    let system_env_var_overrides = system_env_var_overrides(tx).await?;
    let mut system_env_vars = default_system_env_vars;
    system_env_vars.extend(system_env_var_overrides);
    Ok(system_env_vars)
}

pub async fn system_env_var_overrides<RT: Runtime>(
    tx: &mut Transaction<RT>,
) -> anyhow::Result<BTreeMap<EnvVarName, EnvVarValue>> {
    let canonical_urls = CanonicalUrlsModel::new(tx).get_canonical_urls().await?;
    parse_system_env_var_overrides(canonical_urls)
}

pub fn parse_system_env_var_overrides(
    canonical_urls: BTreeMap<RequestDestination, ParsedDocument<CanonicalUrl>>,
) -> anyhow::Result<BTreeMap<EnvVarName, EnvVarValue>> {
    let mut system_env_var_overrides = BTreeMap::new();
    for (request_destination, canonical_url) in canonical_urls {
        let env_var_name = match request_destination {
            RequestDestination::ConvexCloud => CONVEX_ORIGIN.clone(),
            RequestDestination::ConvexSite => CONVEX_SITE.clone(),
        };
        system_env_var_overrides.insert(env_var_name, canonical_url.url.parse()?);
    }
    Ok(system_env_var_overrides)
}

/// [`system_env_vars`] as `component` sees them. A child component mounted
/// under an HTTP prefix gets `CONVEX_SITE_URL` with that prefix appended, so
/// absolute URLs it builds point back at itself.
async fn system_env_vars_for_component<RT: Runtime>(
    tx: &mut Transaction<RT>,
    component: ComponentId,
    default_system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
) -> anyhow::Result<BTreeMap<EnvVarName, EnvVarValue>> {
    let mut system_env_vars = system_env_vars(tx, default_system_env_vars).await?;
    if component.is_root() {
        return Ok(system_env_vars);
    }
    let component_metadata = BootstrapComponentsModel::new(tx)
        .load_component(component)
        .await?;
    if let Some(http_prefix) = component_metadata
        .as_ref()
        .and_then(|m| m.http_prefix.as_deref())
        && let Some(base_url) = system_env_vars.get(&*CONVEX_SITE).cloned()
    {
        let prefixed_url = format!(
            "{}{}",
            base_url.as_ref().trim_end_matches('/'),
            http_prefix.trim_end_matches('/')
        );
        system_env_vars.insert(CONVEX_SITE.clone(), prefixed_url.parse()?);
    }
    Ok(system_env_vars)
}

/// Which user-defined variables a function can see through `process.env`.
enum Scope {
    /// The root component reads the deployment's own variables.
    Root(PreloadedEnvironmentVariables),
    /// A child component sees only what `app.use(c, { env })` bound: a literal,
    /// or one of the parent's variables. `parent_env_vars` is preloaded only
    /// when some binding refers to a parent variable.
    Component {
        bindings: BTreeMap<Identifier, EnvBinding>,
        parent_env_vars: Option<PreloadedEnvironmentVariables>,
    },
}

/// Everything `process.env[name]` resolves against for one function, loaded
/// once so each read is a synchronous point lookup that records itself in the
/// transaction's read set. The V8 phase and the wasm import-phase host both
/// answer reads through this.
pub struct PreloadedEnvVars {
    scope: Scope,
    system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
}

impl PreloadedEnvVars {
    pub async fn load<RT: Runtime>(
        tx: &mut Transaction<RT>,
        component: ComponentId,
        default_system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
    ) -> anyhow::Result<Self> {
        let scope = if component.is_root() {
            Scope::Root(EnvironmentVariablesModel::new(tx).preload().await?)
        } else {
            let bindings = BootstrapComponentsModel::new(tx)
                .load_component_env(component)
                .await?;
            let parent_env_vars = if bindings
                .values()
                .any(|binding| matches!(binding, EnvBinding::EnvVar(_)))
            {
                Some(EnvironmentVariablesModel::new(tx).preload().await?)
            } else {
                None
            };
            Scope::Component {
                bindings,
                parent_env_vars,
            }
        };
        let system_env_vars =
            system_env_vars_for_component(tx, component, default_system_env_vars).await?;
        Ok(Self {
            scope,
            system_env_vars,
        })
    }

    /// The scope's user-defined variables first, then the system ones.
    pub fn get<RT: Runtime>(
        &self,
        tx: &mut Transaction<RT>,
        name: &EnvVarName,
    ) -> anyhow::Result<Option<EnvVarValue>> {
        match &self.scope {
            Scope::Root(env_vars) => {
                if let Some(var) = env_vars.get(tx, name)? {
                    return Ok(Some(var));
                }
            },
            Scope::Component {
                bindings,
                parent_env_vars,
            } => {
                if let Ok(identifier) = Identifier::from_str(name.as_ref())
                    && let Some(binding) = bindings.get(&identifier)
                {
                    return match binding {
                        EnvBinding::Value(s) => Ok(Some(s.parse()?)),
                        EnvBinding::EnvVar(parent_name) => parent_env_vars
                            .as_ref()
                            .context("parent env vars not preloaded")?
                            .get(tx, parent_name),
                    };
                }
            },
        }
        Ok(self.system_env_vars.get(name).cloned())
    }
}
