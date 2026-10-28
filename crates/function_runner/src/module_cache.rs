use std::sync::Arc;

use async_lru::async_lru::AsyncLru;
use async_trait::async_trait;
use common::{
    document::ParsedDocument,
    knobs::{
        FUNRUN_CODE_CACHE_SIZE,
        FUNRUN_MODULE_CACHE_SIZE,
        FUNRUN_MODULE_MAX_CONCURRENCY,
        FUNRUN_MODULE_QUEUE_SIZE,
    },
    runtime::{
        propagate_tracing,
        try_join,
        Runtime,
    },
};
use isolate::module_cache::V8ModuleSource;
use model::{
    modules::{
        module_versions::FullModuleSource,
        types::ModuleMetadata,
    },
    source_packages::types::SourcePackage,
};
use moka::sync::Cache;
use storage::Storage;
use sync_types::CanonicalizedModulePath;
use value::{
    heap_size::HeapSize,
    sha256::Sha256Digest,
};

use crate::{
    metrics::module_load_timer,
    record_module_sizes,
    server::StorageForDeployment,
};

/// Identifies a module by the source package it was loaded from, so filling
/// the cache from a downloaded package doesn't need to hash every module in it.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ModuleCacheKey {
    deployment_name: String,
    source_package_sha256: Sha256Digest,
    module_path: CanonicalizedModulePath,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct CodeCacheKey {
    deployment_name: String,
    module_path: CanonicalizedModulePath,
    sha256: Sha256Digest,
}

#[derive(Clone)]
pub(crate) struct ModuleCache<RT: Runtime>(
    AsyncLru<RT, ModuleCacheKey, V8ModuleSource, (String, Sha256Digest)>,
);

impl<RT: Runtime> ModuleCache<RT> {
    pub(crate) fn new(rt: RT) -> Self {
        Self(AsyncLru::new(
            rt,
            *FUNRUN_MODULE_CACHE_SIZE,
            *FUNRUN_MODULE_MAX_CONCURRENCY,
            *FUNRUN_MODULE_QUEUE_SIZE,
            "function_runner_module_cache",
        ))
    }
}

#[derive(Clone)]
pub(crate) struct CodeCache(Arc<Cache<CodeCacheKey, Arc<[u8]>>>);
impl CodeCache {
    pub(crate) fn new() -> Self {
        Self(Arc::new(
            Cache::builder()
                .max_capacity(*FUNRUN_CODE_CACHE_SIZE)
                .weigher(|_, data: &Arc<[u8]>| u32::try_from(data.len()).unwrap_or(u32::MAX))
                .build(),
        ))
    }
}

pub(crate) struct FunctionRunnerModuleLoader<RT: Runtime, S: StorageForDeployment<RT>> {
    pub cache: ModuleCache<RT>,
    pub code_cache: CodeCache,
    pub deployment_name: String,
    pub modules_storage: Arc<dyn Storage>,
    pub storage: S,
}

impl<RT: Runtime, S: StorageForDeployment<RT>> FunctionRunnerModuleLoader<RT, S> {
    fn code_cache_key(&self, module_metadata: &ModuleMetadata) -> CodeCacheKey {
        CodeCacheKey {
            deployment_name: self.deployment_name.clone(),
            module_path: module_metadata.path.clone(),
            sha256: module_metadata.sha256.clone(),
        }
    }
}

#[async_trait]
impl<RT: Runtime, S: StorageForDeployment<RT>> isolate::module_cache::ModuleCache<RT>
    for FunctionRunnerModuleLoader<RT, S>
{
    #[fastrace::trace]
    async fn get_module_with_metadata(
        &self,
        module_metadata: &ParsedDocument<ModuleMetadata>,
        source_package: &ParsedDocument<SourcePackage>,
    ) -> anyhow::Result<Arc<V8ModuleSource>> {
        let key = ModuleCacheKey {
            deployment_name: self.deployment_name.clone(),
            source_package_sha256: source_package.sha256.clone(),
            module_path: module_metadata.path.clone(),
        };
        let result = self
            .cache
            .0
            .get_and_prepopulate(&key, || {
                let deployment_name = self.deployment_name.clone();
                let modules_storage = self.modules_storage.clone();
                let storage = self.storage.clone();
                let source_package = source_package.clone();
                let source_package_sha256 = source_package.sha256.clone();
                let fetch_key = (self.deployment_name.clone(), source_package.sha256.clone());
                (
                    fetch_key,
                    // async_lru polls this future from its own task, which only
                    // inherits the fastrace span. Reattaching the caller's
                    // tracing span (the isolate worker's, which records
                    // `instance_name`) puts the deployment's name on the logs
                    // emitted while downloading the package.
                    try_join(
                        "get_modules_and_prefetch",
                        propagate_tracing(async move {
                            let _timer = module_load_timer("package");
                            let package = storage
                                .download_package(modules_storage, &source_package)
                                .await?;
                            Ok(package
                                .into_iter()
                                .map(move |(module_path, module_config)| {
                                    (
                                        ModuleCacheKey {
                                            deployment_name: deployment_name.clone(),
                                            source_package_sha256: source_package_sha256.clone(),
                                            module_path,
                                        },
                                        Arc::new(V8ModuleSource::new(FullModuleSource {
                                            source: module_config.source,
                                            source_map: module_config.source_map,
                                        })),
                                    )
                                })
                                .collect())
                        }),
                    ),
                )
            })
            .await?;
        record_module_sizes(
            result.source().heap_size(),
            result.source_map().map(|sm| sm.len()),
        );
        Ok(result)
    }

    fn put_cached_code(&self, module_metadata: &ModuleMetadata, cached_data: Arc<[u8]>) {
        self.code_cache
            .0
            .insert(self.code_cache_key(module_metadata), cached_data);
        crate::metrics::record_code_cache_size(self.code_cache.0.weighted_size());
    }

    fn get_cached_code(&self, module_metadata: &ModuleMetadata) -> Option<Arc<[u8]>> {
        self.code_cache.0.get(&self.code_cache_key(module_metadata))
    }
}
