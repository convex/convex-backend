use std::{
    collections::BTreeMap,
    mem,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use common::{
    bootstrap_model::components::EnvBinding,
    components::{
        ComponentId,
        Reference,
        Resource,
    },
    document::ParsedDocument,
    http::RequestDestination,
    runtime::{
        Runtime,
        UnixTimestamp,
    },
    types::{
        ConvexOrigin,
        ModuleEnvironment,
    },
};
use database::{
    BootstrapComponentsModel,
    Transaction,
};
use errors::ErrorMetadata;
use model::{
    canonical_urls::CanonicalUrlsModel,
    components::ComponentsModel,
    environment_variables::{
        types::{
            EnvVarName,
            EnvVarValue,
        },
        EnvironmentVariablesModel,
    },
    modules::{
        types::ModuleMetadata,
        ModuleModel,
    },
    source_packages::SourcePackageModel,
    udf_config::UdfConfigModel,
};
use parking_lot::Mutex;
use rand::{
    Rng,
    SeedableRng,
};
use rand_chacha::ChaCha12Rng;
use sync_types::{
    CanonicalizedModulePath,
    ModulePath,
};
use udf::environment::{
    parse_system_env_var_overrides,
    CONVEX_SITE,
};
use value::{
    identifier::Identifier,
    ConvexValue,
};

use crate::{
    environment::{
        action::task::TaskRequestEnum,
        helpers::{
            performance_unsupported,
            PerformanceApi,
            Phase,
        },
        ModuleCodeCacheResult,
    },
    module_cache::{
        ModuleCache,
        V8ModuleSource,
    },
    timeout::{
        PauseReason,
        Timeout,
    },
};

/// This struct is similar to UdfPhase. Action execution also has two
/// phases: 1. We start by loading all imported modules, evaluating them, and
/// inserting them into the module map. 2. We find the endpoint and run it.
///
/// Unlike `UdfPhase`, the DB transaction is read-only (used for reading modules
/// and environment variables), and all writes will be handled in their own
/// separate transactions.
pub struct ActionPhase<RT: Runtime> {
    component: ComponentId,
    phase: Phase,
    pub rt: RT,
    preloaded: ActionPreloaded<RT>,
}

/// Populated for non-root components, pairing the component's env bindings
/// with a snapshot of the root-app env vars (only fetched when any binding is
/// `EnvVar`, since actions don't need reactive read deps).
struct ComponentEnvCtx {
    env: BTreeMap<Identifier, EnvBinding>,
    parent_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
}

enum ActionPreloaded<RT: Runtime> {
    Created {
        tx: Transaction<RT>,
        module_loader: Arc<dyn ModuleCache<RT>>,
        default_system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
        resources: Arc<Mutex<BTreeMap<Reference, Resource>>>,
        convex_origin_override: Arc<Mutex<Option<ConvexOrigin>>>,
    },
    Preloading,
    Ready {
        module_loader: Arc<dyn ModuleCache<RT>>,
        modules: BTreeMap<
            CanonicalizedModulePath,
            (Arc<ParsedDocument<ModuleMetadata>>, Arc<V8ModuleSource>),
        >,
        env_vars: BTreeMap<EnvVarName, EnvVarValue>,
        component_arguments: Option<BTreeMap<Identifier, ConvexValue>>,
        component_env: Option<ComponentEnvCtx>,
        rng: Option<ChaCha12Rng>,
        import_time_unix_timestamp: Option<UnixTimestamp>,
        performance_api: Option<PerformanceApi>,
    },
}

impl<RT: Runtime> ActionPhase<RT> {
    pub fn new(
        rt: RT,
        component: ComponentId,
        tx: Transaction<RT>,
        module_loader: Arc<dyn ModuleCache<RT>>,
        default_system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
        resources: Arc<Mutex<BTreeMap<Reference, Resource>>>,
        convex_origin_override: Arc<Mutex<Option<ConvexOrigin>>>,
    ) -> Self {
        Self {
            component,
            phase: Phase::Importing,
            rt,
            preloaded: ActionPreloaded::Created {
                tx,
                module_loader,
                default_system_env_vars,
                resources,
                convex_origin_override,
            },
        }
    }

    #[fastrace::trace]
    pub async fn initialize(&mut self, timeout: &mut Timeout<RT>) -> anyhow::Result<()> {
        anyhow::ensure!(self.phase == Phase::Importing);

        let preloaded = mem::replace(&mut self.preloaded, ActionPreloaded::Preloading);
        let ActionPreloaded::Created {
            mut tx,
            module_loader,
            default_system_env_vars,
            resources,
            convex_origin_override,
        } = preloaded
        else {
            anyhow::bail!("ActionPhase initialized twice");
        };

        let component_id = self.component;
        self.preloaded = timeout
            .with_release_permit(PauseReason::UdfInitialize, async {
                let udf_config = UdfConfigModel::new(&mut tx, component_id.into())
                    .get()
                    .await?;

                let rng = udf_config
                    .as_ref()
                    .map(|c| ChaCha12Rng::from_seed(c.import_phase_rng_seed));
                let import_time_unix_timestamp =
                    udf_config.as_ref().map(|c| c.import_phase_unix_timestamp);

                let module_metadata = ModuleModel::new(&mut tx)
                    .get_all_metadata(component_id)
                    .await?;
                let source_package = SourcePackageModel::new(&mut tx, component_id.into())
                    .get_latest()
                    .await?;
                let mut modules = BTreeMap::new();
                for metadata in module_metadata {
                    if metadata.path.is_system() {
                        continue;
                    }
                    let path = metadata.path.clone();
                    let module = module_loader
                        .get_module_with_metadata(
                            &metadata,
                            source_package
                                .as_ref()
                                .context("source package not found")?,
                        )
                        .await?;
                    modules.insert(path, (metadata, module));
                }

                {
                    let loaded_resources = ComponentsModel::new(&mut tx)
                        .preload_resources(component_id)
                        .await?;
                    let mut resources = resources.lock();
                    *resources = loaded_resources;
                }
                let canonical_urls = CanonicalUrlsModel::new(&mut tx)
                    .get_canonical_urls()
                    .await?;

                if let Some(cloud_url) = canonical_urls.get(&RequestDestination::ConvexCloud) {
                    *convex_origin_override.lock() = Some(ConvexOrigin::from(&cloud_url.url));
                }
                // System env vars are visible to every function; user-defined env
                // vars are visible only to root functions.
                let system_env_var_overrides = parse_system_env_var_overrides(canonical_urls)?;
                let mut env_vars = default_system_env_vars;
                env_vars.extend(system_env_var_overrides);
                if self.component.is_root() {
                    let user_env_vars = EnvironmentVariablesModel::new(&mut tx).get_all().await?;
                    env_vars.extend(user_env_vars);
                } else {
                    // Non-root components with an HTTP prefix see a prefixed
                    // CONVEX_SITE_URL so they can construct correct absolute URLs.
                    let component_metadata = BootstrapComponentsModel::new(&mut tx)
                        .load_component(self.component)
                        .await?;
                    if let Some(http_prefix) = component_metadata
                        .as_ref()
                        .and_then(|m| m.http_prefix.as_deref())
                        && let Some(base_url) = env_vars.get(&*CONVEX_SITE).cloned()
                    {
                        let prefixed_url = format!(
                            "{}{}",
                            base_url.as_ref().trim_end_matches('/'),
                            http_prefix.trim_end_matches('/')
                        );
                        env_vars.insert(CONVEX_SITE.clone(), prefixed_url.parse()?);
                    }
                }

                let component_env = if self.component.is_root() {
                    None
                } else {
                    let env = BootstrapComponentsModel::new(&mut tx)
                        .load_component_env(component_id)
                        .await?;
                    let parent_env_vars =
                        if env.values().any(|b| matches!(b, EnvBinding::EnvVar(_))) {
                            EnvironmentVariablesModel::new(&mut tx).get_all().await?
                        } else {
                            BTreeMap::new()
                        };
                    Some(ComponentEnvCtx {
                        env,
                        parent_env_vars,
                    })
                };

                let component_arguments = if self.component.is_root() {
                    None
                } else {
                    Some(
                        BootstrapComponentsModel::new(&mut tx)
                            .load_component_args(component_id)
                            .await?,
                    )
                };

                Ok(ActionPreloaded::Ready {
                    module_loader,
                    modules,
                    env_vars,
                    component_arguments,
                    component_env,
                    rng,
                    import_time_unix_timestamp,
                    performance_api: import_time_unix_timestamp.map(PerformanceApi::new),
                })
            })
            .await?;

        Ok(())
    }

    pub fn component(&self) -> ComponentId {
        self.component
    }

    pub fn get_module(
        &mut self,
        module_path: &ModulePath,
        _timeout: &mut Timeout<RT>,
    ) -> anyhow::Result<Option<(Arc<V8ModuleSource>, ModuleCodeCacheResult)>> {
        let ActionPreloaded::Ready {
            ref module_loader,
            ref modules,
            ..
        } = self.preloaded
        else {
            anyhow::bail!("Phase not initialized");
        };
        let module = modules.get(&module_path.clone().canonicalize());

        let Some((module, source)) = module else {
            return Ok(None);
        };

        anyhow::ensure!(
            module.environment == ModuleEnvironment::Isolate,
            "Trying to execute {:?} in isolate, but it is bundled for {:?}.",
            module_path,
            module.environment
        );

        let code_cache_result = module_loader.clone().code_cache_result(module);
        Ok(Some((source.clone(), code_cache_result)))
    }

    pub fn begin_execution(&mut self) -> anyhow::Result<()> {
        if self.phase != Phase::Importing {
            anyhow::bail!("Phase was already {:?}", self.phase)
        }
        let ActionPreloaded::Ready {
            ref mut rng,
            ref mut performance_api,
            ..
        } = self.preloaded
        else {
            anyhow::bail!("Phase not initialized");
        };
        self.phase = Phase::Executing;
        let rng_seed = self.rt.rng().random();
        *rng = Some(ChaCha12Rng::from_seed(rng_seed));
        if let Some(performance_api) = performance_api {
            performance_api.begin_execution(&self.rt, self.rt.unix_timestamp())?;
        }
        Ok(())
    }

    pub fn get_environment_variable(
        &mut self,
        name: EnvVarName,
    ) -> anyhow::Result<Option<EnvVarValue>> {
        let ActionPreloaded::Ready {
            ref env_vars,
            ref component_env,
            ..
        } = self.preloaded
        else {
            anyhow::bail!("Phase not initialized");
        };
        if let Some(component_env) = component_env
            && let Ok(identifier) = Identifier::from_str(name.as_ref())
            && let Some(binding) = component_env.env.get(&identifier)
        {
            match binding {
                EnvBinding::Value(s) => {
                    return Ok(Some(s.parse()?));
                },
                EnvBinding::EnvVar(parent_name) => {
                    return Ok(component_env.parent_env_vars.get(parent_name).cloned());
                },
            }
        }
        Ok(env_vars.get(&name).cloned())
    }

    pub fn component_arguments(&self) -> anyhow::Result<&BTreeMap<Identifier, ConvexValue>> {
        let ActionPreloaded::Ready {
            ref component_arguments,
            ..
        } = self.preloaded
        else {
            anyhow::bail!("Phase not initialized");
        };
        let Some(component_arguments) = component_arguments else {
            anyhow::bail!(ErrorMetadata::bad_request(
                "NoComponentArgs",
                "Component arguments are not available within the app",
            ));
        };
        if self.phase != Phase::Executing {
            anyhow::bail!(ErrorMetadata::bad_request(
                "NoComponentArgsDuringImport",
                "Can't use `componentArg` at import time",
            ));
        }
        Ok(component_arguments)
    }

    pub fn rng(&mut self) -> anyhow::Result<&mut ChaCha12Rng> {
        let ActionPreloaded::Ready { ref mut rng, .. } = self.preloaded else {
            anyhow::bail!("Phase not initialized");
        };
        let Some(rng) = rng else {
            // Fail for old module without import time rng populated.
            anyhow::bail!(ErrorMetadata::bad_request(
                "NoRandomDuringImport",
                "Math.random unsupported at import time"
            ));
        };
        Ok(rng)
    }

    pub fn unix_timestamp(&self) -> anyhow::Result<UnixTimestamp> {
        let ActionPreloaded::Ready {
            import_time_unix_timestamp,
            ..
        } = self.preloaded
        else {
            anyhow::bail!("Phase not initialized");
        };
        let timestamp = if self.phase == Phase::Importing {
            let Some(unix_timestamp) = import_time_unix_timestamp else {
                // Fail for old modules without import time timestamp populated.
                anyhow::bail!(ErrorMetadata::bad_request(
                    "NoDateDuringImport",
                    "Date unsupported at import time"
                ));
            };
            unix_timestamp
        } else {
            self.rt.unix_timestamp()
        };
        Ok(timestamp)
    }

    pub fn performance_now(&self) -> anyhow::Result<Duration> {
        let ActionPreloaded::Ready {
            performance_api, ..
        } = &self.preloaded
        else {
            anyhow::bail!("Phase not initialized");
        };
        match (&self.phase, performance_api) {
            (Phase::Importing, Some(PerformanceApi::Importing(api))) => Ok(api.now()),
            (Phase::Executing, Some(PerformanceApi::Executing(api))) => {
                Ok(api.now_incrementing(&self.rt))
            },
            (_, None) => anyhow::bail!(performance_unsupported()),
            _ => anyhow::bail!("performance API state does not match phase"),
        }
    }

    pub fn performance_time_origin(&self) -> anyhow::Result<UnixTimestamp> {
        let ActionPreloaded::Ready {
            performance_api, ..
        } = &self.preloaded
        else {
            anyhow::bail!("Phase not initialized");
        };
        let Some(performance_api) = performance_api else {
            anyhow::bail!(performance_unsupported());
        };
        Ok(performance_api.time_origin())
    }

    pub fn require_executing(&self, request: &TaskRequestEnum) -> anyhow::Result<()> {
        if self.phase == Phase::Importing {
            anyhow::bail!(ErrorMetadata::bad_request(
                format!("No{}DuringImport", request.name_for_error()),
                format!(
                    "{} unsupported at import time",
                    request.description_for_error()
                ),
            ));
        }
        Ok(())
    }
}
