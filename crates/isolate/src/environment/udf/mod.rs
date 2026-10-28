use ::metrics::IntoLabel;
use astral_future::AstralBody;
use common::{
    self,
    audit_log_lines::{
        AuditLogLine,
        AuditLogLines,
    },
    components::{
        CanonicalizedComponentFunctionPath,
        CanonicalizedComponentModulePath,
        ResolvedComponentFunctionPath,
    },
    document::{
        MAX_DOCUMENT_NESTING,
        MAX_USER_SIZE,
    },
    errors::report_error_sync,
    execution_context::ExecutionContext,
    knobs::{
        ISOLATE_MAX_USER_HEAP_SIZE,
        MAX_LOG_LINES_DATABASE_UDFS,
    },
};
use fastrace::local::LocalSpan;
use futures::{
    future::{
        self,
        BoxFuture,
    },
    FutureExt,
};
use itertools::Either;
use model::{
    environment_variables::types::{
        EnvVarName,
        EnvVarValue,
    },
    modules::user_error::FunctionNotFoundError,
};
use sync_types::types::SerializedArgs;
use tokio::{
    select,
    sync::oneshot,
};
use udf::{
    helpers::{
        parse_pending_udf_args,
        pending_udf_args_size,
    },
    warnings::scheduled_arg_size_warning,
    FunctionOutcome,
    NestedUdfOutcome,
    SyscallTrace,
};

use crate::{
    context_cache::{
        ContextCache,
        ContextReadSet,
    },
    environment::udf::{
        astral_future::RecursiveExecutor,
        async_syscall::run_async_syscall_batch,
    },
    module_cache::V8ModuleSource,
    module_map::ModuleMap,
    termination::{
        ContextTerminationReason,
        IsolateTerminationReason,
    },
    timeout::PauseReason,
    ConcurrencyPermit,
    IsolateClient,
};
pub mod async_syscall;

mod astral_future;
mod phase;
pub mod syscall;
use std::{
    cmp::Ordering,
    collections::VecDeque,
    marker::PhantomData,
    sync::Arc,
    time::Duration,
};

use anyhow::{
    anyhow,
    Context as _,
};
use common::{
    errors::JsError,
    identity::InertIdentity,
    knobs::{
        AUDIT_LOG_MAX_HEAP_SIZE_BYTES,
        DATABASE_UDF_SYSTEM_TIMEOUT,
        DATABASE_UDF_USER_TIMEOUT,
        FUNCTION_MAX_ARGS_SIZE,
        FUNCTION_MAX_RESULT_SIZE,
        TRANSACTION_MAX_FILE_WRITE_SIZE_BYTES,
        TRANSACTION_MAX_NUM_FILES_WRITTEN,
        TRANSACTION_MAX_NUM_SCHEDULED,
        TRANSACTION_MAX_NUM_USER_WRITES,
        TRANSACTION_MAX_READ_SET_INTERVALS,
        TRANSACTION_MAX_READ_SIZE_BYTES,
        TRANSACTION_MAX_READ_SIZE_ROWS,
        TRANSACTION_MAX_SCHEDULED_TOTAL_ARGUMENT_SIZE_BYTES,
        TRANSACTION_MAX_USER_WRITE_SIZE_BYTES,
    },
    log_lines::{
        LogLevel,
        LogLine,
        LogLines,
    },
    query_journal::QueryJournal,
    runtime::{
        Runtime,
        UnixTimestamp,
    },
    types::{
        DeploymentMetadata,
        UdfType,
    },
};
use database::{
    BiggestDocumentWrites,
    FunctionExecutionSize,
    Transaction,
    TransactionReadSet,
    OVER_LIMIT_HELP,
};
use deno_core::{
    serde_v8,
    v8::{
        self,
        scope,
    },
};
use errors::ErrorMetadata;
use file_storage::{
    FileDownloads,
    TransactionalFileStorage,
};
use keybroker::FunctionRunnerKeyBroker;
use rand_chacha::ChaCha12Rng;
use serde_json::Value as JsonValue;
use udf::{
    warnings::{
        approaching_duration_limit_warning,
        approaching_limit_warning,
        SystemWarning,
    },
    UdfOutcome,
};
use value::{
    heap_size::{
        HeapSize,
        WithHeapSize,
    },
    serialized_args_ext::SerializedArgsExt,
    JsonPackedValue,
    NamespacedTableMapping,
    PendingValue,
    Size,
    VALUE_TOO_LARGE_SHORT_MSG,
};

use self::{
    async_syscall::{
        AsyncSyscallBatch,
        PendingSyscall,
        QueryManager,
    },
    phase::UdfPhase,
    syscall::syscall_impl,
};
use super::ModuleCodeCacheResult;
use crate::{
    client::{
        EnvironmentData,
        UdfCallback,
        UdfRequest,
    },
    environment::{
        helpers::{
            log_overflow_log_line,
            module_loader::module_specifier_from_path,
            resolve_promise,
        },
        AsyncOpRequest,
        JsEnvironment,
        OpProvider,
        SyscallProvider,
        UncatchableDeveloperError,
    },
    helpers::{
        self,
        deserialize_udf_result_pending,
        pump_message_loop,
    },
    isolate::Isolate,
    metrics::{
        self,
        log_isolate_request_cancelled,
    },
    request_scope::{
        RequestScope,
        RequestState,
    },
    strings,
    termination::ExecutionHandle,
    timeout::{
        FunctionExecutionTime,
        PauseGuard,
        Timeout,
    },
};

pub struct DatabaseUdfSyscallProvider<RT: Runtime> {
    rt: RT,

    udf_type: UdfType,
    path: ResolvedComponentFunctionPath,
    deployment: DeploymentMetadata,
    client_id: String,
    udf_server_version: Option<semver::Version>,

    phase: UdfPhase<RT>,
    file_storage: TransactionalFileStorage<RT>,
    file_downloads: FileDownloads,

    query_manager: QueryManager<RT>,

    key_broker: FunctionRunnerKeyBroker,
    log_lines: LogLines,
    audit_log_lines: AuditLogLines,

    /// Journal from a previous computation of this UDF used as an input to this
    /// UDF. If this is the first run, the journal will be blank.
    prev_journal: QueryJournal,

    /// Journal to write decisions made during this UDF computation.
    next_journal: QueryJournal,

    syscall_trace: SyscallTrace,

    context: ExecutionContext,

    reactor_depth: usize,
}

pub struct DatabaseUdfEnvironment<RT: Runtime, S = DatabaseUdfSyscallProvider<RT>> {
    pub syscall_provider: S,
    pub pending_syscalls: WithHeapSize<VecDeque<PendingSyscall>>,
    pub _phantom: PhantomData<RT>,
}

pub fn async_op_not_allowed_in_udf(request: &AsyncOpRequest) -> ErrorMetadata {
    not_allowed_in_udf(request.name_for_error(), &request.description_for_error())
}

fn not_allowed_in_udf(name: &str, description: &str) -> ErrorMetadata {
    ErrorMetadata::bad_request(
        format!("No{name}InQueriesOrMutations"),
        format!(
            "Can't use {description} in queries and mutations. Please consider using an action. \
            See https://docs.convex.dev/functions/actions for more details.",
        ),
    )
}

impl<RT: Runtime> OpProvider for DatabaseUdfSyscallProvider<RT> {
    fn trace(&mut self, level: LogLevel, messages: Vec<String>) -> anyhow::Result<()> {
        self.emit_log_line(LogLine::new_developer_log_line(
            level,
            messages,
            // Note: accessing the current time here is still deterministic since
            // we don't externalize the time to the function.
            self.rt.unix_timestamp(),
        ));
        Ok(())
    }

    fn rng(&mut self) -> anyhow::Result<&mut ChaCha12Rng> {
        self.phase.rng()
    }

    fn crypto_rng(&mut self) -> anyhow::Result<super::crypto_rng::CryptoRng> {
        anyhow::bail!(not_allowed_in_udf("CryptoRng", "cryptographic randomness"))
    }

    fn unix_timestamp(&mut self) -> anyhow::Result<UnixTimestamp> {
        self.phase.unix_timestamp()
    }

    fn performance_now(&mut self) -> anyhow::Result<Duration> {
        match self.udf_type {
            // Queries must be deterministic, so now() is fixed at its value at the
            // start of execution and doesn't observe elapsed time.
            UdfType::Query => self.phase.performance_now_fixed(),
            UdfType::Mutation => self.phase.performance_now_incrementing(),
            _ => anyhow::bail!("Expected a query or mutation"),
        }
    }

    fn performance_time_origin(&mut self) -> anyhow::Result<UnixTimestamp> {
        match self.udf_type {
            UdfType::Query | UdfType::Mutation => self.phase.performance_time_origin(),
            _ => anyhow::bail!("Expected a query or mutation"),
        }
    }

    fn get_environment_variable(
        &mut self,
        name: EnvVarName,
    ) -> anyhow::Result<Option<EnvVarValue>> {
        self.phase.get_environment_variable(name)
    }

    fn get_all_table_mappings(&mut self) -> anyhow::Result<NamespacedTableMapping> {
        let namespace = self.phase.component()?.into();
        let tx = self.phase.tx()?;
        Ok(tx.table_mapping().namespace(namespace))
    }
}

impl<RT: Runtime> SyscallProvider<RT> for DatabaseUdfSyscallProvider<RT> {
    async fn lookup_source(
        &mut self,
        path: &str,
        timeout: &mut Timeout<RT>,
    ) -> anyhow::Result<Option<(Arc<V8ModuleSource>, ModuleCodeCacheResult)>> {
        let user_module_path = path.parse()?;
        let result = self.phase.get_module(&user_module_path, timeout).await?;
        Ok(result)
    }

    fn syscall(&mut self, name: &str, args: JsonValue) -> anyhow::Result<JsonValue> {
        syscall_impl(self, name, args)
    }
}

impl<RT: Runtime, S: DatabaseUdfInnerProvider<RT>> JsEnvironment<RT>
    for DatabaseUdfEnvironment<RT, S>
{
    type AsyncResolver = v8::Global<v8::PromiseResolver>;
    type SyscallProvider = S;

    fn syscall_provider(&mut self) -> &mut Self::SyscallProvider {
        &mut self.syscall_provider
    }

    fn start_async_syscall(
        &mut self,
        name: String,
        args: JsonValue,
        resolver: Self::AsyncResolver,
    ) -> anyhow::Result<()> {
        self.pending_syscalls.push_back(PendingSyscall {
            name,
            args,
            resolver,
        });
        Ok(())
    }

    fn start_async_op(
        &mut self,
        request: AsyncOpRequest,
        _resolver: Self::AsyncResolver,
    ) -> anyhow::Result<()> {
        anyhow::bail!(async_op_not_allowed_in_udf(&request))
    }

    fn environment_heap_size(&self) -> usize {
        self.pending_syscalls.heap_size() + self.syscall_provider.heap_size()
    }

    fn user_timeout(&self) -> std::time::Duration {
        *DATABASE_UDF_USER_TIMEOUT
    }

    fn system_timeout(&self) -> std::time::Duration {
        *DATABASE_UDF_SYSTEM_TIMEOUT
    }
}

impl<RT: Runtime> HeapSize for DatabaseUdfSyscallProvider<RT> {
    fn heap_size(&self) -> usize {
        self.syscall_trace.heap_size()
    }
}

impl<RT: Runtime> DatabaseUdfInnerProvider<RT> for DatabaseUdfSyscallProvider<RT> {
    type Outcome = (Transaction<RT>, FunctionOutcome);

    async fn run_async_syscall_batch(
        &mut self,
        batch: AsyncSyscallBatch,
        udf_callback: impl UdfCallback<RT>,
    ) -> Vec<anyhow::Result<String>> {
        run_async_syscall_batch(self, batch, udf_callback).await
    }

    fn rt(&self) -> &RT {
        &self.rt
    }

    fn path(&self) -> &ResolvedComponentFunctionPath {
        &self.path
    }

    fn udf_type(&self) -> UdfType {
        self.udf_type
    }

    fn reactor_depth(&self) -> usize {
        self.reactor_depth
    }

    async fn initialize(&mut self, timeout: &mut Timeout<RT>) -> anyhow::Result<()> {
        self.phase.initialize(timeout).await
    }

    fn begin_execution(
        &mut self,
        rng_seed: [u8; 32],
        execution_unix_timestamp: UnixTimestamp,
    ) -> anyhow::Result<()> {
        self.phase
            .begin_execution(rng_seed, execution_unix_timestamp)
    }

    fn into_outcome(
        self,
        args: DatabaseUdfArgs,
        result: Result<PendingValue, JsError>,
        execution_time: FunctionExecutionTime,
    ) -> anyhow::Result<(Transaction<RT>, FunctionOutcome)> {
        let user_execution_time = execution_time.elapsed;

        let success_result_value = result.as_ref().ok();
        let mut log_lines = self.log_lines;
        // System functions receive their arguments unparsed and check them in
        // JS, so arguments that don't parse (e.g. an array over the length
        // limit) have already failed the function before it did any work, and
        // there's nothing to warn about.
        if let Ok(parsed_args) =
            parse_pending_udf_args(&self.path.udf_path, args.udf_args.clone().into_args()?)
        {
            add_warnings_to_log_lines(
                &self.path.clone().for_logging(),
                &parsed_args,
                execution_time,
                self.phase.execution_size()?,
                self.phase.biggest_document_writes()?,
                success_result_value,
                |warning| {
                    // Note: accessing the current time here is still deterministic since
                    // we don't externalize the time to the function.
                    log_lines.push(warning.into_log_line(self.rt.unix_timestamp()));
                },
            )?;
        }
        let memory_in_mb = (*ISOLATE_MAX_USER_HEAP_SIZE / (1 << 20))
            .try_into()
            .unwrap();
        // TODO: Add num_writes and write_bandwidth to UdfOutcome,
        // and use them in log_mutation.
        let outcome = UdfOutcome {
            path: self.path.for_logging(),
            arguments: args.udf_args,
            identity: args.identity,
            observed_identity: self.phase.observed_identity(),
            rng_seed: args.rng_seed,
            observed_rng: self.phase.observed_rng(),
            unix_timestamp: args.unix_timestamp,
            observed_time: self.phase.observed_time(),
            log_lines,
            audit_log_lines: self.audit_log_lines,
            journal: self.next_journal,
            result: match result {
                Ok(v) => Ok(JsonPackedValue::pack(v)),
                Err(e) => Err(e),
            },
            syscall_trace: self.syscall_trace,
            udf_server_version: args.udf_server_version,
            memory_in_mb,
            user_execution_time: Some(user_execution_time),
        };
        let outcome = match self.udf_type {
            UdfType::Query => FunctionOutcome::Query(outcome),
            UdfType::Mutation => FunctionOutcome::Mutation(outcome),
            _ => anyhow::bail!("UdfEnvironment should only run queries and mutations"),
        };
        Ok((self.phase.into_transaction()?, outcome))
    }

    fn snoop_reads(&mut self) -> anyhow::Result<()> {
        self.phase.snoop_reads()
    }

    async fn finish_snoop(
        &mut self,
        should_capture: bool,
    ) -> anyhow::Result<Option<ContextReadSet>> {
        let read_set = self.phase.finish_snoop()?;
        Ok(if should_capture {
            capture_context_read_set(read_set, self.phase.tx_mut()?).await?
        } else {
            None
        })
    }
}

type UdfRecursiveExecutor<RT> = RecursiveExecutor<
    anyhow::Result<(
        DatabaseUdfEnvironment<RT>,
        anyhow::Result<Result<PendingValue, JsError>>,
    )>,
>;
struct RunUdf<'a, 'b, RT: Runtime> {
    rt: &'a RT,
    v8_scope: &'a mut v8::Isolate,
    paused_timeout: &'a mut PauseGuard<'b, RT>,
    context_cache: &'a mut ContextCache,
    execution_handle: &'a ExecutionHandle,
    executor: &'a UdfRecursiveExecutor<RT>,
}

impl<'a, 'b, RT: Runtime> UdfCallback<RT> for RunUdf<'a, 'b, RT> {
    async fn execute_nested_udf(
        self,
        client_id: String,
        udf_request: UdfRequest<RT>,
        rng_seed: [u8; 32],
        reactor_depth: usize,
    ) -> anyhow::Result<(Transaction<RT>, NestedUdfOutcome)> {
        let (nested_provider, args) = DatabaseUdfEnvironment::new(
            self.rt.clone(),
            udf_request,
            reactor_depth,
            client_id,
            rng_seed,
        );
        // it is not necessary to propagate cancellation as the parent will already
        // cancel the entire tree of futures.
        let cancellation = future::pending().boxed();
        // N.B.: `run_nested` calls the corresponding `pop_context`.
        // This may not happen in case of a system error, but in that case we
        // are going to throw away the entire context stack anyway.
        let context_id = self.execution_handle.push_context(true /* nested */);
        let request_state = RequestState::new(self.rt.clone(), nested_provider, context_id);
        // N.B.: we don't use this value here; only the top-level `run_nested` matters.
        let mut isolate_clean = false;
        // User code is going to run again; regain the concurrency permit.
        let mut unpause_guard = self.paused_timeout.regain().await?;
        // Actually run the UDF.
        let future = DatabaseUdfEnvironment::<RT>::run_nested(
            self.executor,
            &args,
            self.v8_scope,
            self.context_cache,
            self.execution_handle.clone(),
            request_state,
            &mut *unpause_guard,
            &mut isolate_clean,
            cancellation,
            None, /* udf_callback */
        );
        // Use an AstralFuture to move the responsibility of polling `future`
        // to the `RecursiveExecutor` (created by DatabaseUdfEnvironment::run()).
        // This avoids creating a deep stack of recursive `run_udf` calls.
        let body = std::pin::pin!(AstralBody::new(future));
        // safety: this future must not be leaked
        let (nested_provider, result) = self.executor.spawn(unsafe { body.project() }).await??;
        let outcome = NestedUdfOutcome {
            observed_identity: nested_provider.syscall_provider.phase.observed_identity(),
            observed_rng: nested_provider.syscall_provider.phase.observed_rng(),
            observed_time: nested_provider.syscall_provider.phase.observed_time(),
            audit_log_lines: nested_provider.syscall_provider.audit_log_lines,
            log_lines: nested_provider.syscall_provider.log_lines,
            journal: nested_provider.syscall_provider.next_journal,
            result: result?,
            syscall_trace: nested_provider.syscall_provider.syscall_trace,
        };
        Ok((
            nested_provider.syscall_provider.phase.into_transaction()?,
            outcome,
        ))
    }
}

#[derive(Clone)]
pub struct DatabaseUdfArgs {
    pub unix_timestamp: UnixTimestamp,
    pub rng_seed: [u8; 32],
    pub udf_args: SerializedArgs,
    pub identity: InertIdentity,
    pub udf_server_version: Option<semver::Version>,
    pub reuse_context: bool,
}

#[allow(async_fn_in_trait)]
pub trait DatabaseUdfInnerProvider<RT: Runtime>:
    SyscallProvider<RT> + ValidateContext + HeapSize + 'static
{
    type Outcome;

    /// Runs a batch of syscalls, each of which can succeed or fail
    /// independently. The returned vec is the same length as the batch.
    async fn run_async_syscall_batch(
        &mut self,
        batch: AsyncSyscallBatch,
        udf_callback: impl UdfCallback<RT>,
    ) -> Vec<anyhow::Result<String>>;

    fn rt(&self) -> &RT;
    fn path(&self) -> &ResolvedComponentFunctionPath;
    fn udf_type(&self) -> UdfType;
    fn reactor_depth(&self) -> usize;
    async fn initialize(&mut self, timeout: &mut Timeout<RT>) -> anyhow::Result<()>;
    fn begin_execution(
        &mut self,
        rng_seed: [u8; 32],
        execution_unix_timestamp: UnixTimestamp,
    ) -> anyhow::Result<()>;
    fn into_outcome(
        self,
        args: DatabaseUdfArgs,
        result: Result<PendingValue, JsError>,
        execution_time: FunctionExecutionTime,
    ) -> anyhow::Result<Self::Outcome>;

    fn snoop_reads(&mut self) -> anyhow::Result<()>;
    /// If `should_capture` is false, always returns None
    async fn finish_snoop(
        &mut self,
        should_capture: bool,
    ) -> anyhow::Result<Option<ContextReadSet>>;
}

#[allow(async_fn_in_trait)]
pub trait ValidateContext {
    async fn validate_reused_context(&mut self, read_set: &ContextReadSet) -> anyhow::Result<bool>;
}

impl<RT: Runtime> DatabaseUdfEnvironment<RT> {
    pub fn new(
        rt: RT,
        UdfRequest {
            path_and_args,
            udf_type,
            transaction,
            unix_timestamp,
            journal,
            context,
            environment_data,
            file_downloads,
        }: UdfRequest<RT>,
        reactor_depth: usize,
        client_id: String,
        rng_seed: [u8; 32],
    ) -> (Self, DatabaseUdfArgs) {
        let EnvironmentData {
            key_broker,
            default_system_env_vars,
            file_storage,
            module_loader,
            deployment,
        } = environment_data;
        let reuse_context = path_and_args.reuse_context();
        let (path, arguments, udf_server_version) = path_and_args.consume();
        let component = path.component;
        let identity = transaction.inert_identity();
        (
            Self {
                syscall_provider: DatabaseUdfSyscallProvider {
                    rt: rt.clone(),
                    udf_type,
                    path,
                    udf_server_version: udf_server_version.clone(),

                    phase: UdfPhase::new(
                        transaction,
                        rt,
                        module_loader.clone(),
                        default_system_env_vars,
                        component,
                    ),
                    file_storage,
                    file_downloads,

                    query_manager: QueryManager::new(),

                    key_broker,
                    log_lines: vec![].into(),
                    audit_log_lines: vec![].into(),
                    prev_journal: journal,
                    next_journal: QueryJournal::new(),

                    syscall_trace: SyscallTrace::new(),
                    context,

                    reactor_depth,
                    deployment,
                    client_id,
                },
                pending_syscalls: WithHeapSize::default(),
                _phantom: PhantomData,
            },
            DatabaseUdfArgs {
                unix_timestamp,
                rng_seed,
                udf_args: arguments,
                identity,
                udf_server_version,
                reuse_context,
            },
        )
    }
}

impl<RT, S> DatabaseUdfEnvironment<RT, S>
where
    RT: Runtime,
    S: DatabaseUdfInnerProvider<RT>,
{
    /// Runs a top-level query or mutation.
    #[fastrace::trace]
    pub async fn run(
        self,
        isolate: &mut Isolate<RT>,
        context_cache: &mut ContextCache,
        permit: ConcurrencyPermit,
        isolate_clean: &mut bool,
        cancellation: BoxFuture<'_, ()>,
        args: DatabaseUdfArgs,
        function_started: Option<oneshot::Sender<()>>,
        udf_callback: Option<IsolateClient<RT>>,
    ) -> anyhow::Result<S::Outcome> {
        let executor = UdfRecursiveExecutor::new();

        let path_for_logging = format!("{:?}", self.syscall_provider.path().clone().for_logging());
        let (handle, state, mut timeout) =
            isolate.start_request(context_cache, permit, self).await?;
        if let Some(tx) = function_started {
            // At this point we have acquired a permit and aren't going to
            // reject the function for capacity reasons.
            _ = tx.send(());
        }
        let (this, mut result) = executor
            .run_until(Self::run_nested(
                &executor,
                &args,
                isolate.isolate(),
                context_cache,
                handle.clone(),
                state,
                &mut timeout,
                isolate_clean,
                cancellation,
                udf_callback,
            ))
            .await?;

        // Override the top-level result if there was an isolate termination error.
        // Our environment may be in an inconsistent state after a system error (e.g.
        // the transaction may be missing if we hit a system error during a
        // cross-component call), so be sure to error out here before using the
        // environment.
        match handle.take_termination_error(&path_for_logging) {
            Ok(Ok(())) => (),
            Ok(Err(e)) => result = Ok(Err(e)),
            Err(e) => result = Err(e),
        }
        let result = result?;

        let execution_time = timeout.into_function_execution_time(this.syscall_provider.udf_type());
        this.syscall_provider
            .into_outcome(args, result, execution_time)
    }

    /// Runs a query or mutation, possibly nested via `runQuery`/`runMutation`.
    async fn run_nested(
        executor: &UdfRecursiveExecutor<RT>,
        args: &DatabaseUdfArgs,
        isolate: &mut v8::Isolate,
        context_cache: &mut ContextCache,
        execution_handle: ExecutionHandle,
        mut request_state: RequestState<RT, Self>,
        timeout: &mut Timeout<RT>,
        isolate_clean: &mut bool,
        cancellation: BoxFuture<'_, ()>,
        udf_callback: Option<IsolateClient<RT>>,
    ) -> anyhow::Result<(Self, anyhow::Result<Result<PendingValue, JsError>>)> {
        scope!(let handle_scope, isolate);
        let mut context_scope;
        let (mut isolate_context, mut context_read_set) = if args.reuse_context
            && let Some((context, module_map, read_set)) = request_state
                .environment
                .take_and_validate_reused_context(context_cache)
                .await?
        {
            let v8_context = v8::Local::new(handle_scope, context);
            context_scope = v8::ContextScope::new(handle_scope, v8_context);
            (
                RequestScope::with_existing_context(
                    &mut context_scope,
                    execution_handle.clone(),
                    request_state,
                    false,
                    module_map,
                ),
                Some(read_set),
            )
        } else {
            let v8_context = context_cache.get_or_create_fresh_context(handle_scope);
            context_scope = v8::ContextScope::new(handle_scope, v8_context);
            (
                RequestScope::new(
                    &mut context_scope,
                    execution_handle.clone(),
                    request_state,
                    false,
                )?,
                None,
            )
        };
        let mut result = {
            let v8_scope = isolate_context.scope();
            let module = {
                let snoop = context_read_set.is_none() && args.reuse_context;
                if snoop {
                    RequestScope::<RT, Self>::enter(v8_scope)
                        .state_mut()?
                        .environment
                        .syscall_provider
                        .snoop_reads()?;
                }
                let initialize_result = Self::initialize_context(
                    &mut *v8_scope,
                    timeout,
                    args.reuse_context,
                    context_read_set.is_some(),
                )
                .await;
                if snoop {
                    let mut scope = RequestScope::<RT, Self>::enter(v8_scope);
                    context_read_set = scope
                        .state_mut()?
                        .environment
                        .syscall_provider
                        .finish_snoop(matches!(initialize_result, Ok(Ok(_))))
                        .await?;
                }
                initialize_result
            };
            match module {
                Ok(Ok(module)) => {
                    Self::run_inner(
                        executor,
                        &execution_handle,
                        &mut *v8_scope,
                        context_cache,
                        timeout,
                        cancellation,
                        args,
                        module,
                        udf_callback,
                    )
                    .await
                },
                Ok(Err(e)) => Ok(Err(e)),
                Err(e) => Err(e),
            }
        };

        // Perform a microtask checkpoint one last time before taking the environment
        // to ensure the microtask queue is empty. Otherwise, JS from this request may
        // leak to a subsequent one on isolate reuse.
        isolate_context.checkpoint();
        *isolate_clean = true;

        let request_state = isolate_context.take_state().context("Lost RequestState?")?;
        let this = request_state.environment;
        // Override the returned result if we hit a termination error.
        match execution_handle.pop_context(request_state.context_id)? {
            Ok(()) => (),
            Err(e) => result = Ok(Err(e)),
        }

        // Only reuse contexts if the execution was successful, in case
        // there are any promises hanging around
        if args.reuse_context
            && let Ok(Ok(_)) = result
            && let Some(read_set) = context_read_set
        {
            let module_map = isolate_context
                .take_module_map()
                .context("Lost ModuleMap?")?;
            let v8_scope = isolate_context.scope();
            let context = v8_scope.get_current_context();
            context_cache.save_context(
                CanonicalizedComponentModulePath {
                    component: this.syscall_provider.path().component,
                    module_path: this.syscall_provider.path().udf_path.module().clone(),
                },
                v8::Global::new(v8_scope, context),
                module_map,
                read_set,
            );
        }

        Ok((this, result))
    }

    #[fastrace::trace(properties = {"reusable_context": "{reusable_context}", "is_reused": "{is_reused}"})]
    async fn initialize_context<'c>(
        v8_scope: &'_ mut v8::PinScope<'c, '_>,
        timeout: &mut Timeout<RT>,
        reusable_context: bool,
        is_reused: bool,
    ) -> anyhow::Result<Result<v8::Local<'c, v8::Module>, JsError>> {
        let mut scope = RequestScope::<RT, Self>::enter(v8_scope);

        // Initialize the environment, preloading the UDF config, before executing any
        // JS.
        {
            let state = scope.state_mut()?;
            state
                .environment
                .syscall_provider
                .initialize(timeout)
                .await?;
        }

        let (udf_type, udf_path) = {
            let state = scope.state()?;
            let environment = &state.environment.syscall_provider;
            (environment.udf_type(), environment.path().udf_path.clone())
        };

        if reusable_context {
            metrics::log_reusable_context_init(udf_type, is_reused);
        }

        // Don't allow directly running a UDF within the `_deps` directory. We don't
        // really expect users to hit this unless someone is trying to exploit
        // an app written on Convex by calling directly into a compromised
        // dependency. So, consider it a system error so we can just
        // keep a watch on it.
        if udf_path.module().is_deps() {
            anyhow::bail!("Refusing to run {udf_path:?} within the '_deps' directory");
        }

        // First, load the user's module and find the specified function.
        let module_path = udf_path.module().clone();
        let Ok(module_specifier) = module_specifier_from_path(&module_path) else {
            let message = format!("Invalid module path: {module_path:?}");
            return Ok(Err(JsError::from_message(message)));
        };

        let module = match scope
            .eval_user_module(udf_type, false, &module_specifier, timeout)
            .await?
        {
            Ok(id) => id,
            Err(e) => return Ok(Err(e)),
        };

        Ok(Ok(module))
    }

    #[convex_macro::instrument_future]
    #[fastrace::trace]
    async fn run_inner(
        executor: &UdfRecursiveExecutor<RT>,
        handle: &ExecutionHandle,
        v8_scope: &mut v8::PinScope<'_, '_>,
        context_cache: &mut ContextCache,
        timeout: &mut Timeout<RT>,
        cancellation: BoxFuture<'_, ()>,
        args: &DatabaseUdfArgs,
        module: v8::Local<'_, v8::Module>,
        udf_callback: Option<IsolateClient<RT>>,
    ) -> anyhow::Result<Result<PendingValue, JsError>> {
        let mut scope = RequestScope::<RT, Self>::enter(v8_scope);

        let (rt, udf_type, path, reactor_depth) = {
            let state = scope.state()?;
            let environment = &state.environment.syscall_provider;
            (
                environment.rt().clone(),
                environment.udf_type(),
                environment.path().clone(),
                environment.reactor_depth(),
            )
        };
        let udf_path = &path.udf_path;

        let namespace = module
            .get_module_namespace()
            .to_object(&scope)
            .ok_or_else(|| anyhow!("Module namespace wasn't an object?"))?;
        let function_name = udf_path.function_name();
        let function_str: v8::Local<'_, v8::Value> = v8::String::new(&scope, function_name)
            .ok_or_else(|| anyhow!("Failed to create function name string"))?
            .into();

        if namespace.has(&scope, function_str) != Some(true) {
            let message = format!(
                "{}",
                FunctionNotFoundError::new(udf_path.function_name(), udf_path.module().as_str())
            );
            return Ok(Err(JsError::from_message(message)));
        }
        let function: v8::Local<v8::Object> = namespace
            .get(&scope, function_str)
            .ok_or_else(|| anyhow!("Did not find function in module after checking?"))?
            .try_into()?;

        // Mutations and queries are wrapped in JavaScript by a function that adds a
        // property marking it as a query or mutation UDF.
        let is_mutation_str = strings::isMutation.create(&scope)?.into();
        let mut is_mutation = false;
        if let Some(true) = function.has(&scope, is_mutation_str) {
            is_mutation = function
                .get(&scope, is_mutation_str)
                .ok_or_else(|| anyhow!("Missing `is_mutation` after explicit check"))?
                .is_true();
        }

        let is_query_str = strings::isQuery.create(&scope)?.into();
        let mut is_query = false;
        if let Some(true) = function.has(&scope, is_query_str) {
            is_query = function
                .get(&scope, is_query_str)
                .ok_or_else(|| anyhow!("Missing `is_query` after explicit check"))?
                .is_true();
        }
        let invoke_query_str = strings::invokeQuery.create(&scope)?.into();
        let invoke_mutation_str = strings::invokeMutation.create(&scope)?.into();

        let invoke_str = match (udf_type, is_query, is_mutation) {
            (UdfType::Query, true, false) => invoke_query_str,
            (UdfType::Mutation, false, true) => invoke_mutation_str,
            (_, false, false) => {
                let message = format!(
                    "Function {udf_path:?} is neither a query or mutation. Did you forget to wrap \
                     it with `query` or `mutation`?"
                );
                return Ok(Err(JsError::from_message(message)));
            },
            (UdfType::Query, false, true) => {
                let message = format!(
                    "Function {udf_path:?} is registered as a mutation but is being run as a \
                     query."
                );
                return Ok(Err(JsError::from_message(message)));
            },
            (UdfType::Mutation, true, false) => {
                let message = format!(
                    "Function {udf_path:?} is registered as a query but is being run as a \
                     mutation."
                );
                return Ok(Err(JsError::from_message(message)));
            },
            _ => {
                anyhow::bail!(
                    "Unexpected function classification: {udf_type} vs. (is_query: {is_query}, \
                     is_mutation: {is_mutation})"
                );
            },
        };

        let args_str = args.udf_args.get();
        metrics::log_argument_length(args_str);
        let args_v8_str = v8::String::new(&scope, args_str)
            .ok_or_else(|| anyhow!("Failed to create argument string"))?;

        let invoke: v8::Local<v8::Function> = function
            .get(&scope, invoke_str)
            .ok_or_else(|| anyhow!("Couldn't find invoke function in {udf_path:?}"))?
            .try_into()?;

        // Switch our phase to executing right before calling into the UDF.
        {
            let state = scope.state_mut()?;
            state
                .environment
                .syscall_provider
                .begin_execution(args.rng_seed, args.unix_timestamp)?;
        }
        let global = scope.get_current_context().global(&scope);
        let promise_r =
            scope.with_try_catch(|s| invoke.call(s, global.into(), &[args_v8_str.into()]));
        // If we hit a system error within a syscall, return `Err`, even if JS thinks it
        // returned successfully. The syscall layer uses
        // `scope.terminate_execution()` when we hit a system error, which
        // unfortunately doesn't actually terminate execution immediately. So, it's
        // possible for JS after the failed syscall to keep running and return a result
        // here before checking the termination flag.
        handle.check_terminated()?;
        let promise: v8::Local<v8::Promise> = match promise_r? {
            Ok(Some(v)) => v.try_into()?,
            Ok(None) => anyhow::bail!("Successful invocation returned None"),
            Err(e) => return Ok(Err(e)),
        };
        let mut cancellation = cancellation;
        loop {
            // Advance the user's promise as far as it can go by draining the microtask
            // queue.
            scope.perform_microtask_checkpoint();
            pump_message_loop(&scope);
            scope.record_heap_stats(handle)?;
            handle.check_terminated()?;

            // Check for rejected promises still unhandled, if so terminate.
            let rejections = scope.pending_unhandled_promise_rejections_mut();
            if let Some(promise) = rejections.exceptions.keys().next().cloned() {
                let error = rejections.exceptions.remove(&promise).unwrap();

                let as_local = v8::Local::new(&scope, error);
                let err = match scope.format_traceback(as_local) {
                    Ok(e) => e,
                    Err(e) => {
                        handle.terminate_and_throw(
                            IsolateTerminationReason::SystemError(Some(e)).into(),
                        )?;
                    },
                };
                handle.terminate_and_throw(
                    ContextTerminationReason::UnhandledPromiseRejection(err).into(),
                )?;
            }

            if let v8::PromiseState::Rejected = promise.state() {
                // Stop execution immediately once we hit an error.
                break;
            }

            // If the user's promise is blocked, it must have a pending syscall.
            // Execute a batch of syscalls before reentering into JS.
            // These are executed in a batch deterministically, down to which fetches
            // hit the cache. AsyncSyscallBatch decides which syscalls can run in
            // a batch together.
            // Results are externalized to user space in FIFO order.
            let (resolvers, results) = {
                let state = scope.take_state()?;
                let mut guard = scopeguard::guard((state, &mut scope), |(state, scope)| {
                    if let Err(mut e) = scope.return_state(state) {
                        report_error_sync(&mut e);
                    }
                });
                let (ref mut state, ref mut scope) = *guard;
                let Some(p) = state.environment.pending_syscalls.pop_front() else {
                    // No syscalls or javascript to run, so we're done.
                    break;
                };
                let mut batch = AsyncSyscallBatch::new(p.name, p.args);
                let mut resolvers = vec![p.resolver];
                while let Some(p) = state.environment.pending_syscalls.front()
                    && batch.can_push(&p.name, &p.args)
                {
                    let p = state
                        .environment
                        .pending_syscalls
                        .pop_front()
                        .expect("should have a syscall");
                    batch.push(p.name, p.args)?;
                    resolvers.push(p.resolver);
                }
                // Pause the user-code UDF timeout for the duration of the syscall.
                // This works because we know that the user is blocked on some syscall,
                // so running the syscall is on us and we shouldn't count this time
                // towards the user timeout. When we allow more concurrency, we
                // may have to rework this.
                // NOTE: Even though we release the permit, the syscall does in v8.
                // It is better if we run it in tokio to avoid oversubscribing the CPU.
                // TODO: Consider running the async call from a tokio thread.
                // Even though the future would be blocking on the database most of the
                // time it still does some processing that might result in oversubscribing
                // the CPU threads dedicated to v8.
                let results = timeout
                    .with_release_permit_regainable(
                        PauseReason::DatabaseSyscall {
                            name: batch.name().to_string(),
                        },
                        async |paused_timeout| {
                            let run_udf = RunUdf {
                                rt: &rt,
                                v8_scope: scope,
                                paused_timeout,
                                context_cache,
                                execution_handle: handle,
                                executor,
                            };
                            let udf_callback = if let Some(callback) = &udf_callback {
                                Either::Left(callback)
                            } else {
                                Either::Right(run_udf)
                            };
                            select! {
                                biased;
                                _ = &mut cancellation => {
                                    log_isolate_request_cancelled();
                                    anyhow::bail!("Cancelled");
                                },
                                results = state.environment.syscall_provider
                                    .run_async_syscall_batch(batch, udf_callback)
                                => Ok(results),
                            }
                        },
                    )
                    .await?;
                (resolvers, results)
            };
            // Every syscall must have a result (which could be an error or None).
            assert_eq!(resolvers.len(), results.len());

            // Complete the syscall's promise, which will put its handlers on the microtask
            // queue.
            for (resolver, result) in resolvers.into_iter().zip(results.into_iter()) {
                scope!(let result_scope, &mut *scope);
                let result_v8 = match result {
                    Ok(v) => Ok(serde_v8::to_v8(result_scope, v)?),
                    Err(e) => match e.downcast::<UncatchableDeveloperError>() {
                        // Ends the function before any JS sees this result,
                        // which is what makes it uncatchable.
                        Ok(UncatchableDeveloperError { message }) => {
                            report_error_sync(&mut anyhow::anyhow!(
                                "UncatchableDeveloperError: {message}"
                            ));
                            handle.terminate_and_throw(
                                ContextTerminationReason::UncatchableDeveloperError(
                                    JsError::from_message(message),
                                )
                                .into(),
                            )?;
                        },
                        Err(e) => Err(e),
                    },
                };
                resolve_promise(result_scope, resolver, result_v8)?;
            }
            handle.check_terminated()?;
        }

        // Check to see if the user's promise is blocked.
        let result = match promise.state() {
            v8::PromiseState::Pending => Err(JsError::from_message(
                "Returned promise will never resolve".to_string(),
            )),
            v8::PromiseState::Fulfilled => {
                anyhow::ensure!(
                    scope.state()?.environment.pending_syscalls.is_empty(),
                    "queries and mutations should run all syscalls to completion"
                );
                let promise_result_v8 = promise.result(&scope);
                let result_v8_str: v8::Local<v8::String> = promise_result_v8.try_into()?;
                let result_str = helpers::to_rust_string(&scope, &result_v8_str)?;
                metrics::log_result_length(&result_str);
                // Unresolved commit timestamps can only exist inside a
                // mutation's transaction: in the mutation itself or in a
                // nested call within one (a nested query can read one back
                // from a pending write). A top-level query has no commit
                // timestamp to resolve to, so it cannot return one.
                let result = deserialize_udf_result_pending(&path, &result_str)?;
                if udf_type == UdfType::Mutation || reactor_depth > 0 {
                    result
                } else {
                    match result {
                        Ok(value) if value.is_pending() => Err(JsError::from_message(format!(
                            "Function {} return value invalid: queries cannot return an \
                             unresolved commit timestamp",
                            path.clone().for_logging().debug_str()
                        ))),
                        result => result,
                    }
                }
            },
            v8::PromiseState::Rejected => {
                let e = promise.result(&scope);
                Err(scope.format_traceback(e)?)
            },
        };

        Ok(result)
    }

    async fn take_and_validate_reused_context(
        &mut self,
        context_cache: &mut ContextCache,
    ) -> anyhow::Result<Option<(v8::Global<v8::Context>, ModuleMap, ContextReadSet)>> {
        let module_path = CanonicalizedComponentModulePath {
            component: self.syscall_provider.path().component,
            module_path: self.syscall_provider.path().udf_path.module().clone(),
        };
        let Some((context, module_map, read_set)) = context_cache.take_reused_context(&module_path)
        else {
            return Ok(None);
        };
        if !self
            .syscall_provider
            .validate_reused_context(&read_set)
            .await?
        {
            return Ok(None);
        }
        Ok(Some((context, module_map, read_set)))
    }
}

impl<RT: Runtime> DatabaseUdfSyscallProvider<RT> {
    pub fn emit_audit_log_line(&mut self, audit_log_line: AuditLogLine) -> anyhow::Result<()> {
        let max_heap = *AUDIT_LOG_MAX_HEAP_SIZE_BYTES;
        anyhow::ensure!(
            self.audit_log_lines.heap_size() + audit_log_line.heap_size() <= max_heap,
            ErrorMetadata::bad_request(
                "AuditLogsExceedLimits",
                "Audit logs exceed function execution limits",
            )
        );
        self.audit_log_lines.push(audit_log_line);
        Ok(())
    }

    /// Emit a warning-level log line to the developer's function logs.
    pub fn emit_warning(&mut self, message: String) -> anyhow::Result<()> {
        self.trace(LogLevel::Warn, vec![message])
    }

    pub fn emit_log_line(&mut self, log_line: LogLine) {
        let max_log_lines = *MAX_LOG_LINES_DATABASE_UDFS;
        // - 1 to reserve for the [ERROR] log line
        match self.log_lines.len().cmp(&(max_log_lines - 1)) {
            Ordering::Less => self.log_lines.push(log_line),
            Ordering::Equal => {
                drop(log_line);
                // Note: accessing the current time here is still deterministic since
                // we don't externalize the time to the function.
                let log_line = log_overflow_log_line(max_log_lines, self.rt.unix_timestamp());
                self.log_lines.push(log_line);
            },
            Ordering::Greater => (),
        }
    }

    pub fn emit_sub_function_log_lines(
        &mut self,
        path: CanonicalizedComponentFunctionPath,
        log_lines: LogLines,
    ) {
        let max_log_lines = *MAX_LOG_LINES_DATABASE_UDFS;
        // -1 to reserve for the [ERROR] log line
        if self.log_lines.len() > max_log_lines - 1 {
            // We have previously exceeded the logging limit, so skip these logs.
            return;
        }
        if self.log_lines.len() + log_lines.len() > max_log_lines - 1 {
            // We are about to exceed the logging limit, so truncate the logs.
            let allowed_length = max_log_lines - 1 - self.log_lines.len();
            self.log_lines.push(LogLine::SubFunction {
                path,
                log_lines: log_lines.truncated(allowed_length),
            });
            // Note: accessing the current time here is still deterministic since
            // we don't externalize the time to the function.
            let log_line = log_overflow_log_line(max_log_lines, self.rt.unix_timestamp());
            self.log_lines.push(log_line);
        } else {
            self.log_lines
                .push(LogLine::SubFunction { path, log_lines });
        }
    }
}

// Called when a function finishes
pub fn add_warnings_to_log_lines(
    path: &CanonicalizedComponentFunctionPath,
    arguments: &[PendingValue],
    execution_time: FunctionExecutionTime,
    execution_size: FunctionExecutionSize,
    biggest_writes: Option<BiggestDocumentWrites>,
    result: Option<&PendingValue>,
    mut trace_system_warning: impl FnMut(SystemWarning),
) -> anyhow::Result<()> {
    let udf_path = path.udf_path.clone();
    let system_udf_path = if udf_path.is_system() {
        Some(udf_path)
    } else {
        None
    };
    if let Some(warning) = approaching_limit_warning(
        pending_udf_args_size(arguments),
        *FUNCTION_MAX_ARGS_SIZE,
        "TooLargeFunctionArguments",
        || "Large size of the function arguments".to_string(),
        None,
        Some(" bytes"),
        system_udf_path.as_ref(),
    ) {
        trace_system_warning(warning);
    }
    if let Some(warning) = approaching_limit_warning(
        execution_size.read_size.total_document_count,
        *TRANSACTION_MAX_READ_SIZE_ROWS,
        "TooManyDocumentsRead",
        || "Many documents read in a single function execution".to_string(),
        Some(OVER_LIMIT_HELP),
        None,
        system_udf_path.as_ref(),
    ) {
        trace_system_warning(warning);
    }
    if let Some(warning) = approaching_limit_warning(
        execution_size.num_intervals,
        *TRANSACTION_MAX_READ_SET_INTERVALS,
        "TooManyReads",
        || "Many reads in a single function execution".to_string(),
        Some(OVER_LIMIT_HELP),
        None,
        system_udf_path.as_ref(),
    ) {
        trace_system_warning(warning);
    }
    if let Some(warning) = approaching_limit_warning(
        execution_size.read_size.total_document_size,
        *TRANSACTION_MAX_READ_SIZE_BYTES,
        "TooManyBytesRead",
        || "Many bytes read in a single function execution".to_string(),
        Some(OVER_LIMIT_HELP),
        Some(" bytes"),
        system_udf_path.as_ref(),
    ) {
        trace_system_warning(warning);
    }
    if let Some(warning) = approaching_limit_warning(
        execution_size.write_size.num_writes,
        *TRANSACTION_MAX_NUM_USER_WRITES,
        "TooManyWrites",
        || "Many writes in a single function execution".to_string(),
        None,
        None,
        system_udf_path.as_ref(),
    ) {
        trace_system_warning(warning);
    }
    if let Some(warning) = approaching_limit_warning(
        execution_size.write_size.size,
        *TRANSACTION_MAX_USER_WRITE_SIZE_BYTES,
        "TooManyBytesWritten",
        || "Many bytes written in a single function execution".to_string(),
        None,
        Some(" bytes"),
        system_udf_path.as_ref(),
    ) {
        trace_system_warning(warning);
    }
    if let Some(warning) = approaching_limit_warning(
        execution_size.scheduled_size.num_writes,
        *TRANSACTION_MAX_NUM_SCHEDULED,
        "TooManyFunctionsScheduled",
        || "Many functions scheduled by this mutation".to_string(),
        None,
        None,
        system_udf_path.as_ref(),
    ) {
        trace_system_warning(warning);
    }
    if let Some(warning) = approaching_limit_warning(
        execution_size.scheduled_size.size,
        *TRANSACTION_MAX_SCHEDULED_TOTAL_ARGUMENT_SIZE_BYTES,
        "ScheduledFunctionsArgumentsTooLarge",
        || {
            "Large total size of the arguments of scheduled functions from this mutation"
                .to_string()
        },
        None,
        Some(" bytes"),
        system_udf_path.as_ref(),
    ) {
        trace_system_warning(warning);
    }
    if let Some(warning) = scheduled_arg_size_warning(
        execution_size.scheduled_size.max_args_size,
        &system_udf_path,
    ) {
        trace_system_warning(warning);
    }
    if let Some(warning) = approaching_limit_warning(
        execution_size.file_storage_write_size.num_writes,
        *TRANSACTION_MAX_NUM_FILES_WRITTEN,
        "TooManyFilesWritten",
        || "Many files stored by this mutation".to_string(),
        None,
        None,
        system_udf_path.as_ref(),
    ) {
        trace_system_warning(warning);
    }
    if let Some(warning) = approaching_limit_warning(
        execution_size.file_storage_write_size.size,
        *TRANSACTION_MAX_FILE_WRITE_SIZE_BYTES,
        "FilesWrittenTooLarge",
        || "Large total size of the files stored by this mutation".to_string(),
        None,
        Some(" bytes"),
        system_udf_path.as_ref(),
    ) {
        trace_system_warning(warning);
    }

    if let Some(biggest_writes) = biggest_writes {
        let (max_size_document_id, max_size) = biggest_writes.max_size;
        if let Some(warning) = approaching_limit_warning(
            max_size,
            MAX_USER_SIZE,
            VALUE_TOO_LARGE_SHORT_MSG,
            || format!("Large document written with ID \"{max_size_document_id}\""),
            None,
            Some(" bytes"),
            system_udf_path.as_ref(),
        ) {
            trace_system_warning(warning);
        }
        let (max_nesting_document_id, max_nesting) = biggest_writes.max_nesting;
        if let Some(warning) = approaching_limit_warning(
            max_nesting,
            MAX_DOCUMENT_NESTING,
            "TooNested",
            || format!("Deeply nested document written with ID \"{max_nesting_document_id}\""),
            None,
            Some(" levels"),
            system_udf_path.as_ref(),
        ) {
            trace_system_warning(warning);
        }
    }

    if let Some(result) = result
        && let Some(warning) = approaching_limit_warning(
            result.size(),
            *FUNCTION_MAX_RESULT_SIZE,
            "TooLargeFunctionResult",
            || "Large size of the function return value".to_string(),
            None,
            Some(" bytes"),
            system_udf_path.as_ref(),
        )
    {
        trace_system_warning(warning);
    };
    if let Some(warning) = approaching_duration_limit_warning(
        execution_time.elapsed,
        execution_time.limit,
        "UserTimeout",
        "Function execution took a long time",
        system_udf_path.as_ref(),
    )? {
        trace_system_warning(warning);
    }
    Ok(())
}

impl<RT: Runtime> ValidateContext for DatabaseUdfSyscallProvider<RT> {
    #[fastrace::trace]
    async fn validate_reused_context(&mut self, read_set: &ContextReadSet) -> anyhow::Result<bool> {
        let mut reusable = scopeguard::guard(false, |reusable| {
            LocalSpan::add_property(|| ("reuse_success", reusable.as_label()));
        });
        let tx = self.phase.tx_mut()?;
        for (namespace, tablet_index_name, table_name, intervals, hash) in &read_set.range_hashes {
            let tablet = *tablet_index_name.table();
            if !tx.table_mapping().tablet_id_exists(tablet) {
                return Ok(false);
            }
            let (new_namespace, _, new_table_name) =
                tx.table_mapping().get_table_metadata(tablet)?;
            anyhow::ensure!(namespace == new_namespace, "{tablet} changed namespace?");
            anyhow::ensure!(table_name == new_table_name, "{tablet} changed name?");
            let Some(new_hash) = tx
                .hash_index_interval_no_deps(tablet_index_name, table_name, intervals)
                .await?
            else {
                return Ok(false);
            };
            if new_hash != *hash {
                return Ok(false);
            }
        }
        *reusable = true;
        // All hashes match, so make sure to merge the saved read set into `tx`
        tx.apply_reads(read_set.read_set.clone());
        Ok(true)
    }
}

#[fastrace::trace]
async fn capture_context_read_set<RT: Runtime>(
    read_set: TransactionReadSet,
    tx: &mut Transaction<RT>,
) -> anyhow::Result<Option<ContextReadSet>> {
    anyhow::ensure!(
        read_set.read_set().iter_search().count() == 0,
        "searches can't be done during init"
    );
    let mut range_hashes = vec![];
    for (tablet_index_name, reads) in read_set.read_set().iter_indexed() {
        let &(namespace, _table_number, ref table_name) = tx
            .table_mapping()
            .get_table_metadata(*tablet_index_name.table())?;
        anyhow::ensure!(
            table_name.is_system(),
            "context init read non-system table {table_name}?"
        );
        let table_name = table_name.clone();
        let Some(hash) = tx
            .hash_index_interval_no_deps(tablet_index_name, &table_name, &reads.intervals)
            .await?
        else {
            return Ok(None);
        };
        range_hashes.push((
            namespace,
            tablet_index_name.clone(),
            table_name,
            reads.intervals.clone(),
            hash,
        ));
    }
    Ok(Some(ContextReadSet {
        read_set,
        range_hashes,
    }))
}
