pub mod module_loader;
pub mod types;

use std::{
    collections::BTreeMap,
    sync::Arc,
};

use common::{
    components::ComponentId,
    document::ParsedDocument,
    runtime::Runtime,
    schemas::DatabaseSchema,
};
use database::{
    unauthorized_error,
    IndexModel,
    SchemaModel,
    Transaction,
};
use storage::Storage;
use sync_types::CanonicalizedModulePath;
use value::ResolvedDocumentId;

use crate::{
    auth::AuthInfoModel,
    config::types::{
        ConfigDiff,
        ConfigMetadata,
        ModuleConfig,
        AUTH_CONFIG_FILE_NAME,
    },
    cron_jobs::CronModel,
    modules::{
        module_versions::AnalyzedModule,
        types::ModuleMetadata,
        ModuleModel,
    },
    source_packages::{
        types::SourcePackage,
        SourcePackageModel,
    },
    udf_config::{
        types::UdfConfig,
        UdfConfigModel,
    },
};

pub struct ConfigModel<'a, RT: Runtime> {
    pub tx: &'a mut Transaction<RT>,
    component: ComponentId,
}

impl<'a, RT: Runtime> ConfigModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>, component: ComponentId) -> Self {
        Self { tx, component }
    }

    #[fastrace::trace]
    pub async fn apply(
        &mut self,
        config: ConfigMetadata,
        modules: Vec<ModuleConfig>,
        new_config: UdfConfig,
        source_package: Option<SourcePackage>,
        analyze_results: BTreeMap<CanonicalizedModulePath, AnalyzedModule>,
        schema_id: Option<ResolvedDocumentId>,
    ) -> anyhow::Result<(ConfigDiff, Option<DatabaseSchema>)> {
        // TODO: Move this check up to `Application`.
        if !(self.tx.identity().is_admin() || self.tx.identity().is_system()) {
            anyhow::bail!(unauthorized_error("apply_config"));
        }

        let source_package_id = match source_package {
            Some(source_package) => Some(
                SourcePackageModel::new(self.tx, self.component.into())
                    .put(source_package)
                    .await?,
            ),
            None => None,
        };

        let cron_diff = CronModel::new(self.tx, self.component)
            .apply(&analyze_results)
            .await?;

        let (schema_diff, next_schema) = SchemaModel::new(self.tx, self.component.into())
            .apply(schema_id)
            .await?;

        let index_diff = IndexModel::new(self.tx)
            .apply(self.component.into(), &next_schema)
            .await?;

        let module_diff = ModuleModel::new(self.tx)
            .apply(self.component, modules, source_package_id, analyze_results)
            .await?;

        // Update auth info.
        let auth_diff = AuthInfoModel::new(self.tx).put(config.auth_info).await?;
        let udf_server_version_diff = UdfConfigModel::new(self.tx, self.component.into())
            .set(new_config)
            .await?;
        let config_diff = ConfigDiff {
            module_diff,
            auth_diff,
            udf_server_version_diff,
            cron_diff,
            // TODO(CX-3851): Consider logging the mutated indexes separately.
            // This now includes added, mutated and dropped indexes. Mutated
            // indexes are shown both in 'added' and in 'dropped'
            index_diff: index_diff.into(),
            schema_diff,
        };

        Ok((config_diff, next_schema))
    }

    /// Return the latest database configuration. This includes only the
    /// user-configurable state and not internal derived state like shapes. We
    /// might want to store this config in memory but for now just reading it
    /// out of the metadata tables to avoid keeping too many sources of truth.
    pub async fn get_with_module_source(
        &mut self,
        modules_storage: &Arc<dyn Storage>,
    ) -> anyhow::Result<(ConfigMetadata, Vec<ModuleConfig>, Option<UdfConfig>)> {
        // TODO: Move to `application/`.
        if !(self.tx.identity().is_admin() || self.tx.identity().is_system()) {
            anyhow::bail!(unauthorized_error("get_config"));
        }
        let mut config = ConfigMetadata::new();
        let modules: Vec<_> = ModuleModel::new(self.tx)
            .get_application_modules(self.component, modules_storage)
            .await?
            .into_values()
            .collect();

        // If we have an auth config module do not include auth_info in the config
        if !modules
            .iter()
            .any(|module| module.path == AUTH_CONFIG_FILE_NAME.parse().unwrap())
        {
            let auth_info = AuthInfoModel::new(self.tx).get().await?;
            config.auth_info = auth_info.into_iter().map(|doc| doc.into_value()).collect();
        }

        let udf_config = UdfConfigModel::new(self.tx, self.component.into())
            .get()
            .await?
            .map(|u| (**u).clone());
        Ok((config, modules, udf_config))
    }

    /// Return the latest database configuration. This includes only the
    /// user-configurable state and not internal derived state like shapes. We
    /// might want to store this config in memory but for now just reading it
    /// out of the metadata tables to avoid keeping too many sources of truth.
    pub async fn get_with_module_metadata(
        &mut self,
    ) -> anyhow::Result<(
        ConfigMetadata,
        Vec<Arc<ParsedDocument<ModuleMetadata>>>,
        Option<UdfConfig>,
    )> {
        // TODO: Move to `application/`.
        if !(self.tx.identity().is_admin() || self.tx.identity().is_system()) {
            anyhow::bail!(unauthorized_error("get_config"));
        }
        let mut config = ConfigMetadata::new();
        let modules = ModuleModel::new(self.tx)
            .get_application_metadata(self.component)
            .await?;

        // If we have an auth config module do not include auth_info in the config
        if !modules
            .iter()
            .any(|module| module.path == AUTH_CONFIG_FILE_NAME.parse().unwrap())
        {
            let auth_info = AuthInfoModel::new(self.tx).get().await?;
            config.auth_info = auth_info.into_iter().map(|doc| doc.into_value()).collect();
        }

        let udf_config = UdfConfigModel::new(self.tx, self.component.into())
            .get()
            .await?
            .map(|u| (**u).clone());
        Ok((config, modules, udf_config))
    }
}
