use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use common::{
    components::CanonicalizedComponentModulePath,
    document::ParsedDocument,
    runtime::Runtime,
};
use database::Transaction;
use storage::Storage;

use crate::{
    modules::{
        module_versions::FullModuleSource,
        types::ModuleMetadata,
        ModuleModel,
    },
    source_packages::{
        types::SourcePackage,
        upload_download::download_package,
        SourcePackageModel,
    },
};

#[async_trait]
pub trait ModuleLoader<RT: Runtime>: Sync + Send + 'static {
    /// The passed in [`SourcePackage`] is the source package that we would like
    /// to load the given [`ModuleMetadata`] from. Thus, it is valid to pass in
    /// a different source package than the one specified in [`ModuleMetadata`]
    /// since later source packages can contain the same module.
    async fn get_module_with_metadata(
        &self,
        module_metadata: &ParsedDocument<ModuleMetadata>,
        source_package: &ParsedDocument<SourcePackage>,
    ) -> anyhow::Result<Arc<FullModuleSource>>;

    async fn get_module(
        &self,
        tx: &mut Transaction<RT>,
        path: CanonicalizedComponentModulePath,
    ) -> anyhow::Result<Option<Arc<FullModuleSource>>> {
        let component = path.component;
        let module_metadata = match ModuleModel::new(tx).get_metadata(path).await? {
            Some(r) => r,
            None => return Ok(None),
        };
        let source_package = SourcePackageModel::new(tx, component.into())
            .get(module_metadata.source_package_id)
            .await?;
        self.get_module_with_metadata(&module_metadata, &source_package)
            .await
            .map(Some)
    }
}

/// Loads module versions directly from storage, without caching.
pub struct UncachedModuleLoader {
    pub modules_storage: Arc<dyn Storage>,
}

#[async_trait]
impl<RT: Runtime> ModuleLoader<RT> for UncachedModuleLoader {
    #[fastrace::trace]
    async fn get_module_with_metadata(
        &self,
        module_metadata: &ParsedDocument<ModuleMetadata>,
        source_package: &ParsedDocument<SourcePackage>,
    ) -> anyhow::Result<Arc<FullModuleSource>> {
        let package = download_package(self.modules_storage.clone(), source_package).await?;
        let module_config = package
            .get(&module_metadata.path)
            .with_context(|| format!("Missing module source {}", module_metadata.id()))?;
        Ok(Arc::new(FullModuleSource {
            source: module_config.source.clone(),
            source_map: module_config.source_map.clone(),
        }))
    }
}
