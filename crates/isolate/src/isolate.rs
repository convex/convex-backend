use std::{
    cell::Cell,
    ffi,
    mem::ManuallyDrop,
    os::raw::c_void,
    ptr,
    sync::{
        atomic::{
            AtomicU32,
            Ordering,
        },
        Arc,
    },
    time::Duration,
};

use anyhow::Context as _;
use common::{
    knobs::{
        ISOLATE_MAX_ARRAY_BUFFER_TOTAL_SIZE,
        ISOLATE_MAX_USER_HEAP_SIZE,
    },
    runtime::Runtime,
};
use deno_core::v8::{
    self,
    callback_scope,
};
use derive_more::{
    Add,
    AddAssign,
};
use fastrace::{
    local::LocalSpan,
    Event,
    Span,
};
use humansize::{
    FormatSize,
    BINARY,
};
use itertools::Itertools as _;

use crate::{
    array_buffer_allocator::ArrayBufferMemoryLimit,
    client::SharedIsolateHeapStats,
    context_cache::ContextCache,
    environment::V8IsolateEnvironment,
    helpers::pump_message_loop,
    metrics::{
        create_isolate_timer,
        destroy_isolate_timer,
        log_heap_statistics,
        log_isolate_heap_limit_callback_repeated,
        rejected_before_execution_error,
        RejectedBeforeExecutionReason,
    },
    request_scope::RequestState,
    strings,
    termination::{
        ExecutionHandle,
        IsolateTerminationReason,
    },
    timeout::start_request_on_handle,
    ConcurrencyPermit,
    Timeout,
};

pub const CONVEX_SCHEME: &str = "convex";
pub const SYSTEM_PREFIX: &str = "_system/";
pub const SETUP_URL: &str = "convex:/_system/setup.js";

/// Thin wrapper over `v8::Isolate` that includes our Convex-specific
/// configuration.
pub struct Isolate<RT: Runtime> {
    rt: RT,
    v8_isolate: ManuallyDrop<v8::OwnedIsolate>,
    handle: ExecutionHandle,
    // Typically, the user timeout is configured based on environment. This
    // allows us to set an upper bound to it that we use for tests.
    max_user_timeout: Option<Duration>,
    // The heap limit callback takes ownership of this `Box` allocation, which
    // we reclaim after removing the callback.
    heap_ctx_ptr: *mut HeapContext,
    array_buffer_memory_limit: Arc<ArrayBufferMemoryLimit>,
    max_user_heap_size: usize,

    created: tokio::time::Instant,
}

#[derive(thiserror::Error, Debug)]
pub enum IsolateNotClean {
    #[error("Isolate failed with system error.")]
    SystemError,
    #[error("Isolate hit user timeout")]
    UserTimeout,
    #[error("Isolate hit system timeout")]
    SystemTimeout,
    #[error("Isolate ran out of memory")]
    OutOfMemory,

    #[error(
        "Possible memory leak: not enough room for user heap. Total available size {0} out of {1}."
    )]
    TooMuchMemoryCarryOver(String, String),
    #[error("Possible memory leak: {0} contexts have not been garbage collected.")]
    DetachedContext(usize),
}

impl IsolateNotClean {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::SystemError => "system_error",
            Self::UserTimeout => "user_timeout",
            Self::SystemTimeout => "system_timeout",
            Self::OutOfMemory => "out_of_memory",
            Self::TooMuchMemoryCarryOver(..) => "memory_carry_over",
            Self::DetachedContext(_) => "detached_context",
        }
    }
}

#[derive(Debug, Default, Copy, Clone, Add, AddAssign)]
pub struct IsolateHeapStats {
    pub v8_total_heap_size: usize,
    pub v8_total_heap_size_executable: usize,
    pub v8_total_physical_size: usize,
    pub v8_used_heap_size: usize,

    pub v8_external_memory_bytes: usize,

    // Malloced memory from v8. This is the heap size. See
    // https://stackoverflow.com/questions/69418109/why-is-malloced-memory-lower-than-used-heap-size-in-node-js.
    pub v8_malloced_memory: usize,

    // Heap used for syscalls and similar related to processing the request.
    pub environment_heap_size: usize,

    pub streams_heap_size: usize,
    pub array_buffer_size: usize,
}

impl IsolateHeapStats {
    pub fn new(
        stats: v8::HeapStatistics,
        streams_heap_size: usize,
        array_buffer_size: usize,
    ) -> Self {
        Self {
            v8_total_heap_size: stats.total_heap_size(),
            v8_total_heap_size_executable: stats.total_heap_size_executable(),
            v8_total_physical_size: stats.total_physical_size(),
            v8_used_heap_size: stats.used_heap_size(),
            v8_malloced_memory: stats.malloced_memory(),
            v8_external_memory_bytes: stats.external_memory(),
            environment_heap_size: 0,
            streams_heap_size,
            array_buffer_size,
        }
    }

    pub fn env_heap_size(&self) -> usize {
        self.environment_heap_size + self.streams_heap_size
    }
}

impl<RT: Runtime> Isolate<RT> {
    pub fn new(rt: RT, max_user_timeout: Option<Duration>, max_user_heap_size: usize) -> Self {
        let _timer = create_isolate_timer();
        let (array_buffer_memory_limit, array_buffer_allocator) =
            crate::array_buffer_allocator::limited_array_buffer_allocator(
                *ISOLATE_MAX_ARRAY_BUFFER_TOTAL_SIZE,
            );
        let mut v8_isolate = crate::udf_runtime::create_isolate_with_udf_runtime(
            v8::CreateParams::default().array_buffer_allocator(array_buffer_allocator),
            max_user_heap_size,
        );

        // Tells V8 to capture current stack trace when uncaught exception occurs and
        // report it to the message listeners. The option is off by default.
        v8_isolate.set_capture_stack_trace_for_uncaught_exceptions(
            true, // capture
            10,   // frame_limit
        );

        // This specifies the callback called by the upcoming import.meta language
        // feature to retrieve host-defined meta data for a module.
        v8_isolate.set_host_initialize_import_meta_object_callback(Self::import_meta_callback);

        // This specifies the callback called by the upcoming dynamic import() language
        // feature to load modules.
        v8_isolate.set_host_import_module_dynamically_callback(Self::error_dynamic_import_callback);

        // Disallow synchronous `Atomics.wait`.
        v8_isolate.set_allow_atomics_wait(false);

        v8_isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);

        let handle = ExecutionHandle::new(v8_isolate.thread_safe_handle());

        // Pass ownership of the HeapContext struct to the heap limit callback, which
        // we'll take back in the `Isolate`'s destructor.
        let heap_context = Box::new(HeapContext {
            handle: handle.clone(),
            callback_count: AtomicU32::new(0),
        });
        let heap_ctx_ptr = Box::into_raw(heap_context);
        v8_isolate.add_near_heap_limit_callback(
            near_heap_limit_callback,
            heap_ctx_ptr as *mut ffi::c_void,
        );

        v8_isolate.add_gc_prologue_callback(
            gc_prologue_callback,
            ptr::null_mut(),
            v8::GCType::kGCTypeAll,
        );
        v8_isolate.add_gc_epilogue_callback(
            gc_epilogue_callback,
            ptr::null_mut(),
            v8::GCType::kGCTypeAll,
        );

        assert!(v8_isolate.set_slot(handle.clone()));
        assert!(v8_isolate.set_slot(array_buffer_memory_limit.clone()));

        Self {
            created: rt.monotonic_now(),
            rt,
            v8_isolate: ManuallyDrop::new(v8_isolate),
            handle,
            heap_ctx_ptr,
            max_user_timeout,
            array_buffer_memory_limit,
            max_user_heap_size,
        }
    }

    extern "C" fn import_meta_callback(
        context: v8::Local<v8::Context>,
        _module: v8::Local<v8::Module>,
        _meta: v8::Local<v8::Object>,
    ) {
        callback_scope!(unsafe let scope, context);
        let message = strings::import_meta_unsupported
            .create(scope)
            .expect("Failed to create exception string");
        let exception = v8::Exception::type_error(scope, message);
        scope.throw_exception(exception);
    }

    pub fn error_dynamic_import_callback<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        _host_defined_options: v8::Local<'s, v8::Data>,
        _resource_name: v8::Local<'s, v8::Value>,
        _specifier: v8::Local<'s, v8::String>,
        _import_assertions: v8::Local<'s, v8::FixedArray>,
    ) -> Option<v8::Local<'s, v8::Promise>> {
        let message = strings::dynamic_import_unsupported
            .create(scope)
            .expect("Failed to crate exception string");
        let exception = v8::Exception::type_error(scope, message);

        let resolver = v8::PromiseResolver::new(scope).unwrap();
        let promise = resolver.get_promise(scope);
        resolver.reject(scope, exception);
        Some(promise)
    }

    pub fn max_user_heap_size(&self) -> usize {
        self.max_user_heap_size
    }

    // Heap stats for an isolate that has no associated state or environment.
    pub fn heap_stats(&mut self) -> IsolateHeapStats {
        let stats = self.v8_isolate.get_heap_statistics();
        IsolateHeapStats::new(stats, 0, self.array_buffer_memory_limit.used())
    }

    pub fn check_isolate_clean(
        &mut self,
        context_cache: &mut ContextCache,
    ) -> Result<(), IsolateNotClean> {
        // A terminated isolate must not enter V8 again. Each V8 call below can
        // reach the heap limit and terminate the isolate, so check after each.
        if let Some(not_clean) = self.handle.is_not_clean() {
            return Err(not_clean);
        }
        // The microtask queue should be empty.
        // v8 doesn't expose whether it's empty, so we empty it ourselves.
        // TODO(CX-2874) use a different microtask queue for each context.
        self.v8_isolate.perform_microtask_checkpoint();
        if let Some(not_clean) = self.handle.is_not_clean() {
            return Err(not_clean);
        }
        pump_message_loop(&self.v8_isolate);
        if let Some(not_clean) = self.handle.is_not_clean() {
            return Err(not_clean);
        }

        // The heap should have enough memory available.
        let stats = self.v8_isolate.get_heap_statistics();
        log_heap_statistics(&stats);
        if stats.total_available_size() < *ISOLATE_MAX_USER_HEAP_SIZE
            && !context_cache.has_saved_context()
        {
            self.handle
                .terminate(IsolateTerminationReason::OutOfMemory.into());
            return Err(IsolateNotClean::TooMuchMemoryCarryOver(
                stats.total_available_size().format_size(BINARY),
                stats.heap_size_limit().format_size(BINARY),
            ));
        }
        if stats.number_of_detached_contexts() > 0 {
            return Err(IsolateNotClean::DetachedContext(
                stats.number_of_detached_contexts(),
            ));
        }

        Ok(())
    }

    pub async fn start_request<E: V8IsolateEnvironment<RT>>(
        &mut self,
        context_cache: &mut ContextCache,
        permit: ConcurrencyPermit,
        environment: E,
    ) -> anyhow::Result<(ExecutionHandle, RequestState<RT, E>, Timeout<RT>)> {
        // Double check that the isolate is clean.
        // It's unexpected to encounter this error, since we are supposed to
        // have already checked after the last request finished, but in practice
        // it does happen - so make this error retryable.
        self.check_isolate_clean(context_cache).with_context(|| {
            rejected_before_execution_error(RejectedBeforeExecutionReason::IsolateNotClean)
        })?;
        let (context_id, timeout) = start_request_on_handle(
            self.rt.clone(),
            &self.handle,
            permit,
            &environment,
            self.max_user_timeout,
        );
        let state = RequestState::new(self.rt.clone(), environment, context_id);
        Ok((self.handle.clone(), state, timeout))
    }

    pub fn isolate(&mut self) -> &mut v8::Isolate {
        &mut self.v8_isolate
    }

    pub fn created(&self) -> &tokio::time::Instant {
        &self.created
    }

    pub fn set_heap_stats_handle(&mut self, heap_stats: SharedIsolateHeapStats) {
        self.handle.set_heap_stats_handle(heap_stats);
    }
}

impl<RT: Runtime> Drop for Isolate<RT> {
    fn drop(&mut self) {
        let _timer = destroy_isolate_timer();
        if !self.heap_ctx_ptr.is_null() {
            // First remove the callback, so V8 can no longer invoke it.
            self.v8_isolate
                .remove_near_heap_limit_callback(near_heap_limit_callback, 0);

            // Now that the callback is gone, we can free its context.
            let heap_ctx = unsafe { Box::from_raw(self.heap_ctx_ptr) };
            let repeats = heap_ctx
                .callback_count
                .load(Ordering::Relaxed)
                .saturating_sub(1);
            if repeats > 0 {
                log_isolate_heap_limit_callback_repeated(repeats.into());
            }
            drop(heap_ctx);

            self.heap_ctx_ptr = ptr::null_mut();
        }

        // XXX: our version of rusty_v8 is missing a call to
        // NotifyIsolateShutdown, so the isolate's foreground task runner is
        // going to leak. Before that happens, let's pump the message loop to at
        // least drain all the tasks from it.
        pump_message_loop(&self.v8_isolate);
        // SAFETY: `self` is about to be destroyed and we are not going to use
        // v8_isolate again
        unsafe { ManuallyDrop::drop(&mut self.v8_isolate) };
    }
}

struct HeapContext {
    handle: ExecutionHandle,
    // Number of times `near_heap_limit_callback` ran for this isolate.
    callback_count: AtomicU32,
}

// V8 keeps the returned limit only if it is greater than the current one;
// otherwise it aborts the process (`Heap::InvokeNearHeapLimitCallback`).
const REPEAT_HEAP_LIMIT_INCREASE: usize = 8 << 20;

extern "C" fn near_heap_limit_callback(
    data: *mut ffi::c_void,
    current_heap_limit: usize,
    initial_heap_limit: usize,
) -> usize {
    let heap_ctx = unsafe { &*(data as *const HeapContext) };
    // V8 calls this from its GC epilogue: no metrics registration here. Only
    // the first call terminates the isolate; `Drop for Isolate` reports repeats.
    if heap_ctx.callback_count.fetch_add(1, Ordering::Relaxed) > 0 {
        return current_heap_limit + REPEAT_HEAP_LIMIT_INCREASE;
    }
    LocalSpan::add_event(Event::new("isolate_out_of_memory").with_properties(|| {
        [
            ("current_heap_limit", current_heap_limit.to_string()),
            ("initial_heap_limit", initial_heap_limit.to_string()),
        ]
    }));
    heap_ctx
        .handle
        .terminate(IsolateTerminationReason::OutOfMemory.into());

    // Raise the heap limit a lot to avoid a hard OOM.
    // This is unfortunate but there is some C++ code in V8 that will abort the
    // process if it fails to allocate. We're about to terminate the isolate
    // anyway so any allocation will be very short-lived.
    current_heap_limit * 4
}

thread_local! {
    static GC_SPAN: Cell<Option<Span>> = const { Cell::new(None) };
}
extern "C" fn gc_prologue_callback(
    _isolate: v8::UnsafeRawIsolatePtr,
    gc_type: v8::GCType,
    gc_flags: v8::GCCallbackFlags,
    _data: *mut c_void,
) {
    GC_SPAN.set(Some(
        Span::enter_with_local_parent("v8_collect_garbage")
            .with_property(|| {
                let gc_type = match gc_type {
                    v8::GCType::kGCTypeScavenge => "kGCTypeScavenge",
                    v8::GCType::kGCTypeMinorMarkSweep => "kGCTypeMinorMarkSweep",
                    v8::GCType::kGCTypeMarkSweepCompact => "kGCTypeMarkSweepCompact",
                    v8::GCType::kGCTypeIncrementalMarking => "kGCTypeIncrementalMarking",
                    v8::GCType::kGCTypeProcessWeakCallbacks => "kGCTypeProcessWeakCallbacks",
                    _ => "unknown",
                };
                ("gc_type", gc_type)
            })
            .with_property(|| {
                let gc_flags = [
                    (
                        v8::GCCallbackFlags::kGCCallbackFlagConstructRetainedObjectInfos,
                        "kGCCallbackFlagConstructRetainedObjectInfos",
                    ),
                    (
                        v8::GCCallbackFlags::kGCCallbackFlagForced,
                        "kGCCallbackFlagForced",
                    ),
                    (
                        v8::GCCallbackFlags::kGCCallbackFlagSynchronousPhantomCallbackProcessing,
                        "kGCCallbackFlagSynchronousPhantomCallbackProcessing",
                    ),
                    (
                        v8::GCCallbackFlags::kGCCallbackFlagCollectAllAvailableGarbage,
                        "kGCCallbackFlagCollectAllAvailableGarbage",
                    ),
                    (
                        v8::GCCallbackFlags::kGCCallbackFlagCollectAllExternalMemory,
                        "kGCCallbackFlagCollectAllExternalMemory",
                    ),
                    (
                        v8::GCCallbackFlags::kGCCallbackScheduleIdleGarbageCollection,
                        "kGCCallbackScheduleIdleGarbageCollection",
                    ),
                    (
                        v8::GCCallbackFlags::kGCCallbackFlagLastResort,
                        "kGCCallbackFlagLastResort",
                    ),
                ]
                .into_iter()
                .filter(|(flag, _)| (gc_flags & *flag).0 != 0)
                .map(|(_, name)| name)
                .join("|");
                ("gc_flags", gc_flags)
            }),
    ))
}
extern "C" fn gc_epilogue_callback(
    _isolate: v8::UnsafeRawIsolatePtr,
    _gc_type: v8::GCType,
    _gc_flags: v8::GCCallbackFlags,
    _data: *mut c_void,
) {
    GC_SPAN.take(); // drop the span to finish recording it
}
