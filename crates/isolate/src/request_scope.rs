use std::{
    collections::{
        BTreeMap,
        VecDeque,
    },
    marker::PhantomData,
};

use anyhow::{
    anyhow,
    Context as _,
};
use common::{
    runtime::{
        Runtime,
        UnixTimestamp,
    },
    sync::spsc,
    try_anyhow,
};
use deno_core::{
    serde_v8,
    v8::{
        self,
        callback_scope,
    },
};
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use rand::Rng as _;
use value::heap_size::{
    HeapSize,
    WithHeapSize,
};

use crate::{
    context_local_state::GetContextSlot,
    convert_v8::{
        JsException,
        ToV8 as _,
    },
    environment::{
        OpProvider,
        UncatchableDeveloperError,
        V8IsolateEnvironment,
    },
    execution_scope::{
        ExecutionScope,
        PendingDynamicImports,
        PendingUnhandledPromiseRejections,
    },
    helpers::{
        self,
        pump_message_loop,
    },
    isolate::Isolate,
    metrics::{
        context_build_timer,
        log_promise_handler_added_after_reject,
        log_promise_rejected_after_resolved,
        log_promise_resolved_after_resolved,
    },
    module_map::ModuleMap,
    ops::{
        run_v8_op,
        start_async_op,
        uncatchable_developer_error,
    },
    strings,
    termination::{
        ContextId,
        ContextTerminationReason,
        ExecutionHandle,
        IsolateTerminationReason,
    },
};

/// This structure maintains a `v8::Context` (inside a `v8::HandleScope`)
/// that's set up with our `RequestState` and `ModuleMap`. This scope lasts for
/// the entirety of a request, where executing code may enter into potentially
/// nested [`ExecutionScope`]s.
pub struct RequestScope<'a, 's: 'a, 'i: 'a, RT: Runtime, E: V8IsolateEnvironment<RT>> {
    // NB: The default type parameter to `PinScope` indicates that it has a `Context`, so
    // this scope is attached to our request's context.
    pub(crate) scope: &'a mut v8::PinScope<'s, 'i>,
    pub(crate) handle: ExecutionHandle,
    pub(crate) _pd: PhantomData<(RT, E)>,
}

/// Custom per-request state. All environments have a timeout.
/// Note the ExecutionHandle and ModuleMap are stored on separate slots, so
/// they can be fetched without needing the environment type E.
pub struct RequestState<RT: Runtime, E: V8IsolateEnvironment<RT>> {
    pub rt: RT,
    pub environment: E,
    pub context_id: ContextId,

    pub streams: WithHeapSize<BTreeMap<uuid::Uuid, anyhow::Result<ReadableStream>>>,
    pub stream_listeners: WithHeapSize<BTreeMap<uuid::Uuid, StreamListener>>,
    /// Tracks bytes read in HTTP action requests
    pub request_stream_state: Option<RequestStreamState>,
    pub console_timers: WithHeapSize<BTreeMap<String, UnixTimestamp>>,
}

impl<RT: Runtime, E: V8IsolateEnvironment<RT>> RequestState<RT, E> {
    pub fn new(rt: RT, environment: E, context_id: ContextId) -> Self {
        RequestState {
            rt,
            environment,
            context_id,
            streams: WithHeapSize::default(),
            stream_listeners: WithHeapSize::default(),
            request_stream_state: None,
            console_timers: WithHeapSize::default(),
        }
    }
}

pub struct RequestStreamState {
    stream_id: uuid::Uuid,
    bytes_read: usize,
}

impl RequestStreamState {
    fn new(stream_id: uuid::Uuid) -> Self {
        Self {
            stream_id,
            bytes_read: 0,
        }
    }

    pub fn stream_id(&self) -> uuid::Uuid {
        self.stream_id
    }

    pub fn track_bytes_read(&mut self, bytes_read: usize) {
        self.bytes_read += bytes_read
    }

    pub fn bytes_read(&self) -> usize {
        self.bytes_read
    }
}

#[derive(Debug, Default)]
pub struct ReadableStream {
    pub parts: WithHeapSize<VecDeque<bytes::Bytes>>,
    pub done: bool,
}

impl HeapSize for ReadableStream {
    fn heap_size(&self) -> usize {
        self.parts.heap_size()
    }
}

pub enum StreamListener {
    JsPromise(v8::Global<v8::PromiseResolver>),
    RustStream(spsc::UnboundedSender<anyhow::Result<bytes::Bytes>>),
}

impl HeapSize for StreamListener {
    fn heap_size(&self) -> usize {
        // TODO: Implement HeapSize for `spsc::UnboundedSender` and fill this out.
        0
    }
}

impl<RT: Runtime, E: V8IsolateEnvironment<RT>> RequestState<RT, E> {
    pub fn create_stream(&mut self) -> anyhow::Result<uuid::Uuid> {
        let uuid =
            uuid::Builder::from_random_bytes(self.environment.syscall_provider().rng()?.random())
                .into_uuid();
        self.streams.insert(uuid, Ok(ReadableStream::default()));
        Ok(uuid)
    }

    pub fn create_request_stream(&mut self) -> anyhow::Result<uuid::Uuid> {
        let uuid = self.create_stream()?;
        self.request_stream_state = Some(RequestStreamState::new(uuid));
        Ok(uuid)
    }

    /// As the name implies, the time returned by this function would be a
    /// source of non-determinism, so should not be externalized to
    /// functions. An example of a safe use of this function is `console.time`
    /// where we log a duration, but do not leak this time or duration to the
    /// function.
    pub fn unix_timestamp_non_deterministic(&self) -> UnixTimestamp {
        self.rt.unix_timestamp()
    }
}

impl<'a, 's: 'a, 'i: 'a, RT: Runtime, E: V8IsolateEnvironment<RT>> RequestScope<'a, 's, 'i, RT, E> {
    pub fn with_existing_context(
        scope: &'a mut v8::PinScope<'s, 'i>,
        handle: ExecutionHandle,
        state: RequestState<RT, E>,
        allow_dynamic_imports: bool,
        module_map: ModuleMap,
    ) -> Self {
        // These callbacks are global for the entire isolate, so we rely on the
        // isolate only running one request at a time.
        // The callbacks are removed in `Drop` so if they happen between requests
        // they go back to the default behavior.
        scope.set_promise_reject_callback(Self::promise_reject_callback);

        if allow_dynamic_imports {
            scope.set_host_import_module_dynamically_callback(Self::dynamic_import_callback);
        }

        let context = scope.get_current_context();
        assert!(context.set_context_slot(scope, state));
        assert!(context.set_context_slot(scope, module_map));
        assert!(context.set_context_slot(scope, PendingUnhandledPromiseRejections::new()));
        assert!(context.set_context_slot(scope, PendingDynamicImports::new(allow_dynamic_imports)));

        Self {
            scope,
            handle,
            _pd: PhantomData,
        }
    }

    #[fastrace::trace]
    pub fn new(
        scope: &'a mut v8::PinScope<'s, 'i>,
        handle: ExecutionHandle,
        state: RequestState<RT, E>,
        allow_dynamic_imports: bool,
    ) -> anyhow::Result<Self> {
        let timer = context_build_timer();
        let mut isolate_context = Self::with_existing_context(
            scope,
            handle,
            state,
            allow_dynamic_imports,
            ModuleMap::new(),
        );
        let scope = &mut isolate_context.scope;
        let context = scope.get_current_context();
        let global = context.global(scope);

        let syscall_template = v8::FunctionTemplate::new(scope, Self::syscall);
        let syscall_value = syscall_template
            .get_function(scope)
            .ok_or_else(|| anyhow!("Failed to retrieve function from FunctionTemplate"))?;

        let async_syscall_template = v8::FunctionTemplate::new(scope, Self::async_syscall);
        let async_syscall_value = async_syscall_template
            .get_function(scope)
            .ok_or_else(|| anyhow!("Failed to retrieve function from FunctionTemplate"))?;

        let op_template = v8::FunctionTemplate::new(scope, Self::op);
        let op_value = op_template
            .get_function(scope)
            .ok_or_else(|| anyhow!("Failed to retrieve function from FunctionTemplate"))?;

        let async_op_template = v8::FunctionTemplate::new(scope, Self::async_op);
        let async_op_value = async_op_template
            .get_function(scope)
            .ok_or_else(|| anyhow!("Failed to retrieve function from FunctionTemplate"))?;

        let convex_key = strings::Convex.create(scope)?;
        let convex_value: v8::Local<v8::Object> = global
            .get(scope, convex_key.into())
            .context("Missing global.Convex")?
            .try_into()
            .context("Wrong type of global.Convex")?;

        let syscall_key = strings::syscall.create(scope)?;
        convex_value.set(scope, syscall_key.into(), syscall_value.into());

        let op_key = strings::op.create(scope)?;
        convex_value.set(scope, op_key.into(), op_value.into());

        let async_syscall_key = strings::asyncSyscall.create(scope)?;
        convex_value.set(scope, async_syscall_key.into(), async_syscall_value.into());

        let async_op_key = strings::asyncOp.create(scope)?;
        convex_value.set(scope, async_op_key.into(), async_op_value.into());

        let setup_temporal_key = strings::setupTemporal.create(scope)?;
        let setup_temporal: v8::Local<v8::Function> = convex_value
            .get(scope, setup_temporal_key.into())
            .context("Missing Temporal setup")?
            .try_into()
            .context("Temporal setup must be a function")?;
        convex_value.delete(scope, setup_temporal_key.into());
        setup_temporal
            .call(scope, convex_value.into(), &[])
            .context("Failed to set up Temporal")?;

        timer.finish();
        Ok(isolate_context)
    }

    pub fn handle(&self) -> ExecutionHandle {
        self.handle.clone()
    }

    pub(crate) fn op(
        scope: &mut v8::PinScope,
        args: v8::FunctionCallbackArguments,
        rv: v8::ReturnValue,
    ) {
        let mut scope = ExecutionScope::<RT, E>::new(scope);
        if let Err(e) = run_v8_op(&mut scope, args, rv) {
            Self::handle_syscall_or_op_error(&mut scope, e)
        }
    }

    pub(crate) fn async_op(
        scope: &mut v8::PinScope,
        args: v8::FunctionCallbackArguments,
        rv: v8::ReturnValue,
    ) {
        let mut scope = ExecutionScope::<RT, E>::new(scope);
        if let Err(e) = start_async_op(&mut scope, args, rv) {
            Self::handle_syscall_or_op_error(&mut scope, e)
        }
    }

    pub(crate) fn syscall(
        scope: &mut v8::PinScope,
        args: v8::FunctionCallbackArguments,
        rv: v8::ReturnValue,
    ) {
        let mut scope = ExecutionScope::<RT, E>::new(scope);
        if let Err(e) = scope.syscall(args, rv) {
            Self::handle_syscall_or_op_error(&mut scope, e)
        }
    }

    pub(crate) fn async_syscall(
        scope: &mut v8::PinScope,
        args: v8::FunctionCallbackArguments,
        rv: v8::ReturnValue,
    ) {
        let mut scope = ExecutionScope::<RT, E>::new(scope);
        if let Err(e) = scope.async_syscall(args, rv) {
            Self::handle_syscall_or_op_error(&mut scope, e)
        }
    }

    fn handle_syscall_or_op_error(scope: &mut ExecutionScope<RT, E>, err: anyhow::Error) {
        let err = match err.downcast::<UncatchableDeveloperError>() {
            Ok(UncatchableDeveloperError { message }) => {
                let js_error = uncatchable_developer_error(scope, message);
                let message_v8 = v8::String::new(scope, &js_error.message[..]).unwrap();
                scope.handle().terminate(
                    ContextTerminationReason::UncatchableDeveloperError(js_error).into(),
                );
                let exception = v8::Exception::error(scope, message_v8);
                scope.throw_exception(exception);
                return;
            },
            Err(err) => err,
        };

        let js_exception = err.downcast::<JsException>();
        let err = match js_exception {
            Ok(js_exception) => match js_exception.to_v8(scope) {
                Ok(e) => {
                    scope.throw_exception(e);
                    return;
                },
                Err(e) => e,
            },
            Err(e) => e,
        };

        if err.is_deterministic_user_error() {
            let message = err.user_facing_message();
            let message_v8 = v8::String::new(scope, &message[..]).unwrap();
            let exception = v8::Exception::error(scope, message_v8);
            scope.throw_exception(exception);
            return;
        }

        // This error is our fault, and we won't externalize it to userspace. Stash it
        // on the UDF context and and forcibly abort the isolate.
        scope
            .handle()
            .terminate(IsolateTerminationReason::SystemError(Some(err)).into());
        // It turns out that `terminate_execution` doesn't, well, terminate execution
        // immediately [1]. So, throw an exception in case the rest of
        // `convex/server`'s syscall handler still runs after this call.
        // Otherwise, it'll observe `syscall` returning successfully and
        // returning `undefined`.
        //
        // [1] https://groups.google.com/g/v8-users/c/PMqxTd7k2wM/m/Io45pgwmgDIJ
        let Ok(message_v8) = strings::internal_error.create(scope) else {
            // We're really in a bad place if we can't allocate a new string. Just return
            // and reenter into JS, since we've already terminated execution
            // above. Even though V8 will continue to execute more user code,
            // our top-level checks will never consider the UDF execution
            // successful.
            return;
        };
        let exception = v8::Exception::error(scope, message_v8);
        scope.throw_exception(exception);
    }

    pub fn scope(&mut self) -> &mut v8::PinScope<'s, 'i> {
        self.scope
    }

    /// Begin executing code within a single isolate's scope.
    pub fn enter<'c, 's2>(
        v8_scope: &'c mut v8::PinScope<'s2, 'i>,
    ) -> ExecutionScope<'c, 's2, 'i, RT, E> {
        ExecutionScope::new(v8_scope)
    }

    /// Drains the microtask queue and the message loop.
    ///
    /// An isolate with an isolate-level termination is discarded and must not
    /// enter V8 again. Each drain can reach the heap limit and terminate the
    /// isolate, so the check runs before each V8 call.
    pub fn checkpoint(&mut self) {
        if self.handle.is_not_clean().is_some() {
            return;
        }
        self.scope.perform_microtask_checkpoint();
        if self.handle.is_not_clean().is_some() {
            return;
        }
        pump_message_loop(self.scope);
    }

    fn clear_continuation_preserved_embedder_data(&mut self) {
        let undefined = v8::undefined(self.scope);
        self.scope
            .set_continuation_preserved_embedder_data(undefined.into());
    }

    pub(crate) fn take_state(&mut self) -> Option<RequestState<RT, E>> {
        self.clear_continuation_preserved_embedder_data();
        let context = self.scope.get_current_context();
        context.remove_context_slot(self.scope)
    }

    pub(crate) fn take_module_map(&mut self) -> Option<ModuleMap> {
        let context = self.scope.get_current_context();
        context.remove_context_slot(self.scope)
    }

    pub(crate) fn take_pending_unhandled_promise_rejections(
        &mut self,
    ) -> Option<PendingUnhandledPromiseRejections> {
        let context = self.scope.get_current_context();
        context.remove_context_slot(self.scope)
    }

    pub fn take_pending_dynamic_imports(&mut self) -> Option<PendingDynamicImports> {
        let context = self.scope.get_current_context();
        context.remove_context_slot(self.scope)
    }

    pub fn take_environment(mut self) -> E {
        let state = self.take_state().expect("Lost ContextState?");
        state.environment
    }

    #[allow(unused)]
    pub fn print_heap_statistics(&mut self) {
        let stats = self.scope.get_heap_statistics();

        println!("Heap statistics:");
        println!("  total_heap_size: {}", stats.total_heap_size());
        println!(
            "  total_heap_size_executable: {}",
            stats.total_heap_size_executable()
        );
        println!("  total_physical_size: {}", stats.total_physical_size());
        println!("  total_available_size: {}", stats.total_available_size());
        println!(
            "  total_global_handles_size: {}",
            stats.total_global_handles_size()
        );
        println!(
            "  used_global_handles_size: {}",
            stats.used_global_handles_size()
        );
        println!("  used_heap_size: {}", stats.used_heap_size());
        println!("  heap_size_limit: {}", stats.heap_size_limit());
        println!("  malloced_memory: {}", stats.malloced_memory());
        println!("  external_memory: {}", stats.external_memory());
        println!("  peak_malloced_memory: {}", stats.peak_malloced_memory());
        println!(
            "  number_of_native_contexts: {}",
            stats.number_of_native_contexts()
        );
        println!(
            "  number_of_detached_contexts: {}",
            stats.number_of_detached_contexts()
        );
        println!("  does_zap_garbage: {}", stats.does_zap_garbage());
    }

    extern "C" fn promise_reject_callback(message: v8::PromiseRejectMessage) {
        callback_scope!(unsafe let scope, &message);

        match message.get_event() {
            v8::PromiseRejectEvent::PromiseRejectWithNoHandler => {
                // See comment on PendingUnhandledPromiseRejections.
                // A promise rejection is necessary but not sufficient for an
                // 'unhandledRejection' event, which throws in our runtime.
                // Save the promise and check back in on it once the microtask
                // queue has drained. If it remains unhandled then, throw.
                let Some(e) = message.get_value() else {
                    tracing::warn!("Message missing from call to promise_reject_callback");
                    return;
                };
                let error_global = v8::Global::new(scope, e);
                let promise_global = v8::Global::new(scope, message.get_promise());

                let mut exec_scope = ExecutionScope::<RT, E>::new(scope);
                let rejections = exec_scope.pending_unhandled_promise_rejections_mut();
                rejections.exceptions.insert(promise_global, error_global);
            },
            v8::PromiseRejectEvent::PromiseHandlerAddedAfterReject => {
                tracing::warn!("Promise handler added after reject");
                // If this promise was previously a candidate for an
                // 'unhandledRejection' event, disqualify it by removing it
                // from `pending_unhandled_promise_rejections`.
                let promise_global = v8::Global::new(scope, message.get_promise());
                let mut exec_scope = ExecutionScope::<RT, E>::new(scope);
                let rejections = exec_scope.pending_unhandled_promise_rejections_mut();
                rejections.exceptions.remove(&promise_global);
                log_promise_handler_added_after_reject();
            },
            v8::PromiseRejectEvent::PromiseRejectAfterResolved => {
                log_promise_rejected_after_resolved();
            },
            v8::PromiseRejectEvent::PromiseResolveAfterResolved => {
                log_promise_resolved_after_resolved();
            },
        }
    }

    fn dynamic_import_callback<'s2>(
        scope: &mut v8::PinScope<'s2, '_>,
        _host_defined_options: v8::Local<'s2, v8::Data>,
        resource_name: v8::Local<'s2, v8::Value>,
        specifier: v8::Local<'s2, v8::String>,
        _import_assertions: v8::Local<'s2, v8::FixedArray>,
    ) -> Option<v8::Local<'s2, v8::Promise>> {
        let r: anyhow::Result<_> = try_anyhow!({
            let promise_resolver = v8::PromiseResolver::new(scope)
                .ok_or_else(|| anyhow::anyhow!("Failed to create v8::PromiseResolver"))?;
            let promise = promise_resolver.get_promise(scope);
            let promise_resolver = v8::Global::new(scope, promise_resolver);

            let referrer_name: String = serde_v8::from_v8(scope, resource_name)?;
            let specifier_str = helpers::to_rust_string(scope, &specifier)?;

            let resolved_specifier = deno_core::resolve_import(&specifier_str, &referrer_name)
                .map_err(|e| ErrorMetadata::bad_request("InvalidImport", e.to_string()))?;

            let mut exec_scope = ExecutionScope::<RT, E>::new(scope);
            let dynamic_imports = exec_scope.pending_dynamic_imports_mut();
            if !dynamic_imports.allow_dynamic_imports {
                Err(anyhow::anyhow!(
                    "dynamic_import_callback registered without allow_dynamic_imports?"
                ))?;
            }
            dynamic_imports.push(resolved_specifier, promise_resolver);

            promise
        });
        match r {
            Ok(promise) => Some(promise),
            Err(e) => {
                let mut exec_scope = ExecutionScope::<RT, E>::new(scope);
                Self::handle_syscall_or_op_error(&mut exec_scope, e);
                None
            },
        }
    }
}

impl<'a, 's: 'a, 'i: 'a, RT: Runtime, E: V8IsolateEnvironment<RT>> Drop
    for RequestScope<'a, 's, 'i, RT, E>
{
    fn drop(&mut self) {
        // Remove state from slot to stop Timeouts.
        self.take_state();
        // Remove module map from slot to avoid memory leak.
        self.take_module_map();
        // Remove rejected promises which shouldn't persist between requests.
        self.take_pending_unhandled_promise_rejections();
        // Remove pending dynamic imports.
        self.take_pending_dynamic_imports();
        // Remove promise reject callback to clean up the isolate between contexts.
        // Ideally we would have a `remove_promise_reject_callback` or set the
        // function pointer to null, but rusty_v8 doesn't seem to support either,
        // so we set the callback to an empty function instead.
        extern "C" fn noop(_: v8::PromiseRejectMessage) {}
        self.scope.set_promise_reject_callback(noop);

        self.scope.set_host_import_module_dynamically_callback(
            Isolate::<RT>::error_dynamic_import_callback,
        )
    }
}
