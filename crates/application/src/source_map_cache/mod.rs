use std::{
    mem,
    sync::Arc,
};

use async_lru::async_lru::{
    AsyncLru,
    SizedValue,
};
use common::{
    document::ParsedDocument,
    knobs::{
        SOURCE_MAP_CACHE_MAX_CONCURRENCY,
        SOURCE_MAP_CACHE_MAX_SIZE_BYTES,
        SOURCE_MAP_CACHE_QUEUE_SIZE,
    },
    runtime::Runtime,
};
use model::{
    modules::{
        hash_module_source,
        module_versions::SourceMap,
        types::ModuleMetadata,
    },
    source_packages::{
        types::SourcePackage,
        upload_download::download_package,
    },
};
use storage::Storage;
use sync_types::CanonicalizedModulePath;
use value::{
    heap_size::HeapSize,
    sha256::Sha256Digest,
};

mod metrics;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct SourceMapCacheKey {
    deployment_name: String,
    module_path: CanonicalizedModulePath,
    sha256: Sha256Digest,
}

struct CachedSourceMap(Option<SourceMap>);

impl SizedValue for CachedSourceMap {
    fn size(&self) -> u64 {
        (mem::size_of::<SourceMapCacheKey>() + mem::size_of::<Self>() + self.0.heap_size()) as u64
    }
}

/// Caches the source maps used to symbolicate Node action stack traces.
#[derive(Clone)]
pub struct SourceMapCache<RT: Runtime> {
    cache: AsyncLru<RT, SourceMapCacheKey, CachedSourceMap, (String, Sha256Digest)>,
}

impl<RT: Runtime> SourceMapCache<RT> {
    pub fn new(rt: RT) -> Self {
        let cache = AsyncLru::new(
            rt,
            *SOURCE_MAP_CACHE_MAX_SIZE_BYTES,
            *SOURCE_MAP_CACHE_MAX_CONCURRENCY,
            *SOURCE_MAP_CACHE_QUEUE_SIZE,
            "source_map_cache",
        );
        Self { cache }
    }

    #[fastrace::trace]
    pub async fn get_source_map(
        &self,
        deployment_name: &str,
        modules_storage: &Arc<dyn Storage>,
        module_metadata: &ParsedDocument<ModuleMetadata>,
        source_package: &ParsedDocument<SourcePackage>,
    ) -> anyhow::Result<Option<SourceMap>> {
        let timer = metrics::source_map_cache_get_timer();

        let key = SourceMapCacheKey {
            deployment_name: deployment_name.to_owned(),
            module_path: module_metadata.path.clone(),
            sha256: module_metadata.sha256.clone(),
        };
        let result = self
            .cache
            .get_and_prepopulate(&key, || {
                let deployment_name = deployment_name.to_owned();
                let modules_storage = modules_storage.clone();
                let source_package = source_package.clone();
                let fetch_key = (deployment_name.clone(), source_package.sha256.clone());
                (fetch_key, async move {
                    let package = download_package(modules_storage, &source_package).await?;
                    Ok(package
                        .into_iter()
                        .map(|(module_path, module_config)| {
                            (
                                SourceMapCacheKey {
                                    deployment_name: deployment_name.clone(),
                                    module_path,
                                    sha256: hash_module_source(
                                        &module_config.source,
                                        module_config.source_map.as_ref(),
                                    ),
                                },
                                Arc::new(CachedSourceMap(module_config.source_map)),
                            )
                        })
                        .collect())
                })
            })
            .await?;

        timer.finish();
        Ok(result.0.clone())
    }
}
