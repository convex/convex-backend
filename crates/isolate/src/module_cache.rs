use std::{
    ffi::c_char,
    sync::Arc,
};

use async_lru::async_lru::SizedValue;
use async_trait::async_trait;
use common::{
    document::ParsedDocument,
    runtime::Runtime,
};
use deno_core::v8;
use model::{
    modules::{
        module_versions::{
            FullModuleSource,
            SourceMap,
        },
        types::ModuleMetadata,
    },
    source_packages::types::SourcePackage,
};
use value::heap_size::HeapSize;

use crate::environment::ModuleCodeCacheResult;

/// A `ModuleLoader` that also has the ability to store V8 code caches.
#[async_trait]
pub trait ModuleCache<RT: Runtime>: Sync + Send + 'static {
    /// Approximately the same thing as
    /// [`ModuleLoader::get_module_with_metadata`][model::config::module_loader::ModuleLoader::get_module_with_metadata],
    /// but returns a different type. As with that method, note that
    /// `source_package` may differ from `module_metadata.source_package_id`.
    async fn get_module_with_metadata(
        &self,
        module_metadata: &ParsedDocument<ModuleMetadata>,
        source_package: &ParsedDocument<SourcePackage>,
    ) -> anyhow::Result<Arc<V8ModuleSource>>;
    fn put_cached_code(&self, module_metadata: &ModuleMetadata, cached_data: Arc<[u8]>);
    fn get_cached_code(&self, module_metadata: &ModuleMetadata) -> Option<Arc<[u8]>>;
}

impl<RT: Runtime> dyn ModuleCache<RT> {
    pub fn code_cache_result(
        self: Arc<Self>,
        module_metadata: &ModuleMetadata,
    ) -> ModuleCodeCacheResult {
        if let Some(cached_data) = self.get_cached_code(module_metadata) {
            ModuleCodeCacheResult::Cached(cached_data)
        } else {
            let module_metadata = module_metadata.clone();
            ModuleCodeCacheResult::Uncached(Box::new(move |cached_data| {
                self.put_cached_code(&module_metadata, cached_data);
            }))
        }
    }
}

pub enum V8ExternalString {
    OneByte(Arc<[u8]>),
    TwoByte(Arc<[u16]>),
}

impl V8ExternalString {
    pub fn to_source_string(&self) -> String {
        match self {
            Self::OneByte(s) => s.iter().map(|&b| char::from(b)).collect(),
            Self::TwoByte(s) => String::from_utf16_lossy(s),
        }
    }

    fn new(s: &str) -> Self {
        if s.is_ascii() {
            // common case
            Self::OneByte(s.as_bytes().into())
        } else if s.chars().all(|c| (c as u32) < 256) {
            // latin-1 (one-byte) case
            Self::OneByte(s.chars().map(|c| c as u32 as u8).collect::<Vec<_>>().into())
        } else {
            Self::TwoByte(s.encode_utf16().collect::<Vec<_>>().into())
        }
    }

    /// The inverse of [`Self::new`], for consumers that need the source as
    /// plain text rather than as a V8 string — the wasm runtime bundles module
    /// text with esbuild and never builds a V8 string out of it.
    pub fn to_utf8(&self) -> String {
        match self {
            Self::OneByte(s) => s.iter().map(|&byte| byte as char).collect(),
            Self::TwoByte(s) => String::from_utf16_lossy(s),
        }
    }

    pub fn create_v8_string<'s>(
        &self,
        scope: &v8::PinScope<'s, '_, ()>,
    ) -> Option<v8::Local<'s, v8::String>> {
        match self {
            V8ExternalString::OneByte(s) => {
                let p = Arc::into_raw(s.clone());
                unsafe extern "C" fn destructor(p: *mut c_char, len: usize) {
                    unsafe {
                        drop(<Arc<[u8]>>::from_raw(std::ptr::slice_from_raw_parts(
                            p.cast::<u8>(),
                            len,
                        )));
                    }
                }
                unsafe {
                    let v8_string = v8::String::new_external_onebyte_raw(
                        scope,
                        p as *const u8 as *mut c_char,
                        p.len(),
                        destructor,
                    );
                    if v8_string.is_none() {
                        // N.B.: V8 doesn't take ownership in this case
                        drop(Arc::from_raw(p));
                    }
                    v8_string
                }
            },
            V8ExternalString::TwoByte(s) => {
                let p = Arc::into_raw(s.clone());
                unsafe extern "C" fn destructor(p: *mut u16, len: usize) {
                    unsafe {
                        drop(<Arc<[u16]>>::from_raw(std::ptr::slice_from_raw_parts(
                            p, len,
                        )));
                    }
                }
                unsafe {
                    let v8_string = v8::String::new_external_twobyte_raw(
                        scope,
                        p as *const u16 as *mut u16,
                        p.len(),
                        destructor,
                    );
                    if v8_string.is_none() {
                        drop(Arc::from_raw(p));
                    }
                    v8_string
                }
            },
        }
    }
}

pub struct V8ModuleSource {
    source: V8ExternalString,
    source_map: Option<SourceMap>,
}

impl V8ModuleSource {
    pub fn new(source: FullModuleSource) -> Self {
        Self {
            source: V8ExternalString::new(&source.source),
            source_map: source.source_map,
        }
    }

    pub fn source(&self) -> &V8ExternalString {
        &self.source
    }

    pub fn source_map(&self) -> Option<&SourceMap> {
        self.source_map.as_ref()
    }
}

impl HeapSize for V8ExternalString {
    fn heap_size(&self) -> usize {
        match self {
            V8ExternalString::OneByte(s) => s.len(),
            V8ExternalString::TwoByte(s) => s.len() * 2,
        }
    }
}

impl SizedValue for V8ModuleSource {
    fn size(&self) -> u64 {
        (self.source.heap_size() + self.source_map.heap_size()) as u64
    }
}
