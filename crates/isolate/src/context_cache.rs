use std::sync::Arc;

use common::{
    components::CanonicalizedComponentModulePath,
    interval::IntervalSet,
    runtime::Runtime,
    types::TabletIndexName,
};
use database::TransactionReadSet;
use deno_core::v8::{
    self,
    scope,
};
use fastrace::{
    local::LocalSpan,
    Event,
};
use parking_lot::Mutex;
use value::{
    sha256::Sha256Digest,
    TableName,
    TableNamespace,
};

use crate::{
    client::{
        Request,
        RequestType,
    },
    metrics::create_context_timer,
    module_map::ModuleMap,
};

enum SavedContext {
    Fresh(v8::Global<v8::Context>),
    DatabaseUdf {
        module_path: CanonicalizedComponentModulePath,
        context: v8::Global<v8::Context>,
        module_map: ModuleMap,
        read_set: ContextReadSet,
    },
}

pub struct ContextCache {
    saved_context: Option<SavedContext>,
    cached_contexts: Arc<CachedContexts>,
}

/// A mirror of the cache keys present in a `ContextCache`.
/// This struct is `Send + Sync` so that it can be used by the isolate
/// scheduler.
pub struct CachedContexts {
    inner: Mutex<CachedContextsInner>,
}

struct CachedContextsInner {
    saved_database_udf_context: Option<CanonicalizedComponentModulePath>,
}

pub struct ContextReadSet {
    pub read_set: TransactionReadSet,
    pub range_hashes: Vec<(
        TableNamespace,
        TabletIndexName,
        TableName,
        IntervalSet,
        Sha256Digest,
    )>,
}

impl ContextCache {
    pub fn new() -> Self {
        Self {
            saved_context: None,
            cached_contexts: Arc::new(CachedContexts {
                inner: Mutex::new(CachedContextsInner {
                    saved_database_udf_context: None,
                }),
            }),
        }
    }

    pub(crate) fn prepare(&mut self, isolate: &mut v8::Isolate) {
        if self.saved_context.is_none() {
            scope!(let scope, isolate);
            let context = make_context(scope);
            self.saved_context = Some(SavedContext::Fresh(v8::Global::new(scope, context)));
        }
    }

    pub(crate) fn has_saved_context(&mut self) -> bool {
        matches!(self.saved_context, Some(SavedContext::DatabaseUdf { .. }))
    }

    pub(crate) fn clear(&mut self) {
        self.saved_context = None;
        self.cached_contexts.inner.lock().saved_database_udf_context = None;
    }

    pub(crate) fn get_or_create_fresh_context<'s>(
        &mut self,
        scope: &v8::PinScope<'s, '_, ()>,
    ) -> v8::Local<'s, v8::Context> {
        let saved_context = self.saved_context.take();
        self.cached_contexts.inner.lock().saved_database_udf_context = None;
        if let Some(SavedContext::DatabaseUdf { .. }) = saved_context {
            LocalSpan::add_event(Event::new("clobbered_saved_context"));
        }
        if let Some(SavedContext::Fresh(context)) = saved_context {
            v8::Local::new(scope, context)
        } else {
            make_context(scope)
        }
    }

    pub(crate) fn save_context(
        &mut self,
        module_path: CanonicalizedComponentModulePath,
        context: v8::Global<v8::Context>,
        module_map: ModuleMap,
        read_set: ContextReadSet,
    ) {
        self.saved_context = Some(SavedContext::DatabaseUdf {
            module_path: module_path.clone(),
            context,
            module_map,
            read_set,
        });
        self.cached_contexts.inner.lock().saved_database_udf_context = Some(module_path);
    }

    pub(crate) fn take_reused_context(
        &mut self,
        module_path: &CanonicalizedComponentModulePath,
    ) -> Option<(v8::Global<v8::Context>, ModuleMap, ContextReadSet)> {
        if let Some(SavedContext::DatabaseUdf {
            module_path: saved_path,
            ..
        }) = &self.saved_context
            && saved_path == module_path
        {
            let Some(SavedContext::DatabaseUdf {
                module_path: _,
                context,
                module_map,
                read_set,
            }) = self.saved_context.take()
            else {
                unreachable!()
            };
            self.cached_contexts.inner.lock().saved_database_udf_context = None;
            Some((context, module_map, read_set))
        } else {
            None
        }
    }

    pub fn cached_contexts(&self) -> &Arc<CachedContexts> {
        &self.cached_contexts
    }
}

impl CachedContexts {
    pub fn can_serve_request<RT: Runtime>(&self, request: &Request<RT>) -> bool {
        let this = self.inner.lock();
        match &request.inner {
            RequestType::Udf { request: inner, .. } if inner.path_and_args.reuse_context() => {
                request
                    .module()
                    .is_some_and(|m| this.saved_database_udf_context.as_ref() == Some(&m))
            },
            // Prefer routing other requests to isolates that don't have warmed contexts
            _ => this.saved_database_udf_context.is_none(),
        }
    }
}

fn make_context<'s>(scope: &v8::PinScope<'s, '_, ()>) -> v8::Local<'s, v8::Context> {
    let _create_context_timer = create_context_timer();
    v8::Context::new(scope, v8::ContextOptions::default())
}
