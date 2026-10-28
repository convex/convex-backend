use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    sync::{
        Arc,
        Weak,
    },
    time::Duration,
};

use anyhow::Context;
use async_trait::async_trait;
use common::{
    auth::AuthConfig,
    bootstrap_model::components::definition::ComponentDefinitionMetadata,
    components::{
        ComponentDefinitionPath,
        ComponentName,
        Resource,
    },
    errors::JsError,
    execution_context::ExecutionContext,
    http::fetch::FetchClient,
    knobs::{
        FUNRUN_ISOLATE_ACTIVE_THREADS,
        SUBFUNCTIONS_IN_SAME_ISOLATE,
    },
    log_lines::LogLine,
    persistence::{
        PersistenceReader,
        RepeatablePersistence,
    },
    runtime::{
        Runtime,
        SpawnHandle,
        UnixTimestamp,
    },
    schemas::DatabaseSchema,
    types::{
        ConvexOrigin,
        DeploymentMetadata,
        IndexId,
        RepeatableTimestamp,
        UdfRuntime,
        UdfType,
    },
};
use database::{
    shutdown_error,
    Database,
    TextIndexManagerSnapshot,
};
use errors::ErrorMetadata;
use futures::{
    select_biased,
    FutureExt,
    StreamExt,
};
use isolate::{
    isolate_worker::FunctionRunnerIsolateWorker,
    ConcurrencyLimiter,
    IsolateConfig,
};
use keybroker::{
    FunctionRunnerKeyBroker,
    Identity,
};
use model::{
    config::types::ModuleConfig,
    environment_variables::types::{
        EnvVarName,
        EnvVarValue,
    },
    modules::module_versions::{
        AnalyzedModule,
        ModuleSource,
        SourceMap,
    },
    udf_config::types::UdfConfig,
};
use parking_lot::RwLock;
use sync_types::{
    CanonicalizedModulePath,
    Timestamp,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use udf::{
    ActionCallbacks,
    EvaluateAppDefinitionsResult,
    FunctionOutcome,
    HttpActionResponseStreamer,
};
use usage_tracking::FunctionUsageStats;
use value::identifier::Identifier;

use super::FunctionRunner;
use crate::{
    server::{
        validate_run_function_result,
        DeploymentStorage,
        FunctionMetadata,
        FunctionRunnerCore,
        HttpActionMetadata,
        RunRequestArgs,
    },
    FunctionFinalTransaction,
    FunctionWrites,
};

pub struct InProcessFunctionRunner<RT: Runtime> {
    server: FunctionRunnerCore<RT, DeploymentStorage>,
    persistence_reader: Arc<dyn PersistenceReader>,

    // Static information about the backend.
    deployment: DeploymentMetadata,
    key_broker: FunctionRunnerKeyBroker,
    convex_origin: ConvexOrigin,
    database: Database<RT>,
    // Use Weak reference to avoid reference cycle between InProcessFunctionRunner
    // and ApplicationFunctionRunner.
    action_callbacks: Arc<RwLock<Option<Weak<dyn ActionCallbacks>>>>,
    fetch_client: Arc<dyn FetchClient>,
    _concurrency_logger: Box<dyn SpawnHandle>,
}

// We gather prometheus stats every 30 seconds, so we should make sure we log
// active permits more frequently than that.
const ACTIVE_CONCURRENCY_PERMITS_LOG_FREQUENCY: Duration = Duration::from_secs(10);

impl<RT: Runtime> InProcessFunctionRunner<RT> {
    pub fn new(
        deployment: DeploymentMetadata,
        keybroker: FunctionRunnerKeyBroker,
        convex_origin: ConvexOrigin,
        rt: RT,
        persistence_reader: Arc<dyn PersistenceReader>,
        storage: DeploymentStorage,
        database: Database<RT>,
        fetch_client: Arc<dyn FetchClient>,
    ) -> anyhow::Result<Self> {
        // InProcessFunctionRunner is single tenant and thus can use the full capacity.
        let max_percent_per_client = 100;
        let concurrency_limiter = if *FUNRUN_ISOLATE_ACTIVE_THREADS > 0 {
            ConcurrencyLimiter::new(*FUNRUN_ISOLATE_ACTIVE_THREADS)
        } else {
            ConcurrencyLimiter::unlimited()
        };
        let concurrency_logger = rt.spawn(
            "concurrency_logger",
            concurrency_limiter.go_log(rt.clone(), ACTIVE_CONCURRENCY_PERMITS_LOG_FREQUENCY),
        );
        let isolate_config = IsolateConfig::new("funrun", concurrency_limiter);
        let isolate_worker = FunctionRunnerIsolateWorker::new(rt.clone(), isolate_config);
        let server = FunctionRunnerCore::new(rt, storage, max_percent_per_client, isolate_worker)?;
        Ok(Self {
            server,
            persistence_reader,
            deployment,
            key_broker: keybroker,
            convex_origin,
            database,
            action_callbacks: Arc::new(RwLock::new(None)),
            fetch_client,
            _concurrency_logger: concurrency_logger,
        })
    }

    async fn run_http_action(
        &self,
        request_metadata: RunRequestArgs,
        mut http_action_metadata: HttpActionMetadata,
    ) -> anyhow::Result<(
        Option<FunctionFinalTransaction>,
        FunctionOutcome,
        FunctionUsageStats,
    )> {
        // Mimic `FunrunClient::process_message_stream` behavior of forwarding
        // the response_streamer, and detecting cancellation.
        let (inner_response_sender, inner_response_receiver) = mpsc::unbounded_channel();
        let inner_response_streamer = HttpActionResponseStreamer::new(inner_response_sender);
        let mut outer_response_streamer = std::mem::replace(
            &mut http_action_metadata.http_response_streamer,
            inner_response_streamer,
        );
        let mut inner_response_stream =
            UnboundedReceiverStream::new(inner_response_receiver).fuse();
        let mut run_function_fut = Box::pin(self.server.run_function_no_retention_check(
            request_metadata,
            None,
            Some(http_action_metadata),
        ))
        .fuse();
        loop {
            select_biased! {
                result = &mut run_function_fut => {
                    // Flush inner_response_stream into outer_response_streamer.
                    while let Some(part) = inner_response_stream.next().await {
                        if outer_response_streamer.send_part(part)?.is_err() {
                            anyhow::bail!(ErrorMetadata::client_disconnect());
                        }
                    }
                    return result;
                },
                _ = outer_response_streamer.sender.closed().fuse() => {
                    // The streamer above us has disconnected, so stop running
                    // the function and throw an error.
                    drop(run_function_fut);
                    anyhow::bail!(ErrorMetadata::client_disconnect());
                },
                // select_next_some waits until there's a new part to send.
                // If inner_response_stream is closed, this branch doesn't run
                // and we continue waiting on the other branches.
                // This behavior (of continuing to allow the function to be
                // cancelled even after its inner_response_stream is closed)
                // isn't very important, since the function has finished running
                // user code. But it's defensive against the isolate changing
                // its behavior in the future, and it matches FunrunClient
                // behavior.
                part = inner_response_stream.select_next_some() => {
                    // Forward a response part.
                    // If outer_response_streamer is disconnected,
                    // continue and the next loop iteration will detect
                    // it is closed.
                    let _ = outer_response_streamer.send_part(part)?;
                },
            }
        }
    }
}

#[async_trait]
impl<RT: Runtime> FunctionRunner<RT> for InProcessFunctionRunner<RT> {
    #[fastrace::trace]
    async fn run_function(
        &self,
        udf_type: UdfType,
        _udf_runtime: Option<UdfRuntime>,
        identity: Identity,
        ts: RepeatableTimestamp,
        existing_writes: FunctionWrites,
        log_line_sender: Option<mpsc::UnboundedSender<LogLine>>,
        function_metadata: Option<FunctionMetadata>,
        http_action_metadata: Option<HttpActionMetadata>,
        default_system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
        in_memory_index_last_modified: BTreeMap<IndexId, Timestamp>,
        context: ExecutionContext,
    ) -> anyhow::Result<(
        Option<FunctionFinalTransaction>,
        FunctionOutcome,
        FunctionUsageStats,
    )> {
        let pause_client = self.database.runtime().pause_client();
        pause_client.wait("run_function").await;

        let snapshot = self.database.snapshot(ts)?;
        let table_count_snapshot = Arc::new(snapshot.table_counts);
        let text_index_snapshot = Arc::new(TextIndexManagerSnapshot::new(
            snapshot.index_registry,
            snapshot.text_indexes,
            self.database.searcher.clone(),
            self.database.search_storage.clone(),
        ));
        let action_callbacks = self
            .action_callbacks
            .read()
            .clone()
            .context("Action callbacks not set")?
            .upgrade()
            .context(shutdown_error())?;

        let repeatable_persistence = RepeatablePersistence::new(
            self.persistence_reader.clone(),
            ts,
            self.database.retention_validator(),
        );
        let index_reader = Arc::new(repeatable_persistence.read_snapshot(ts)?);
        let request_metadata = RunRequestArgs {
            key_broker: self.key_broker.clone(),
            index_reader,
            convex_origin: self.convex_origin.clone(),
            bootstrap_metadata: self.database.bootstrap_metadata.clone(),
            table_count_snapshot,
            text_index_snapshot,
            action_callbacks,
            fetch_client: self.fetch_client.clone(),
            log_line_sender,
            function_started_sender: None,
            udf_type,
            identity,
            existing_writes,
            default_system_env_vars,
            in_memory_index_last_modified,
            context,
            subfunctions_in_same_isolate: *SUBFUNCTIONS_IN_SAME_ISOLATE,
            deployment: self.deployment.clone(),
        };

        // NOTE: We run the function without checking retention until after the
        // function execution. It is important that we do not surface any errors
        // or results until after we call `validate_run_function_result` below.
        let result = match udf_type {
            UdfType::Query | UdfType::Mutation | UdfType::Action => {
                self.server
                    .run_function_no_retention_check(request_metadata, function_metadata, None)
                    .await
            },
            UdfType::HttpAction => {
                self.run_http_action(
                    request_metadata,
                    http_action_metadata.context("Http action metadata not set")?,
                )
                .await
            },
        };
        validate_run_function_result(udf_type, *ts, self.database.retention_validator()).await?;
        result
    }

    #[fastrace::trace]
    async fn analyze(
        &self,
        udf_config: UdfConfig,
        modules: BTreeMap<CanonicalizedModulePath, ModuleConfig>,
        environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
    ) -> anyhow::Result<Result<BTreeMap<CanonicalizedModulePath, AnalyzedModule>, JsError>> {
        self.server
            .analyze(
                udf_config,
                modules,
                environment_variables,
                self.deployment.name.clone(),
            )
            .await
    }

    #[fastrace::trace]
    async fn evaluate_app_definitions(
        &self,
        app_definition: ModuleConfig,
        component_definitions: BTreeMap<ComponentDefinitionPath, ModuleConfig>,
        dependency_graph: BTreeSet<(ComponentDefinitionPath, ComponentDefinitionPath)>,
        user_environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
        system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
    ) -> anyhow::Result<EvaluateAppDefinitionsResult> {
        self.server
            .evaluate_app_definitions(
                app_definition,
                component_definitions,
                dependency_graph,
                user_environment_variables,
                system_env_vars,
                self.deployment.name.clone(),
            )
            .await
    }

    #[fastrace::trace]
    async fn evaluate_component_initializer(
        &self,
        evaluated_definitions: BTreeMap<ComponentDefinitionPath, ComponentDefinitionMetadata>,
        path: ComponentDefinitionPath,
        definition: ModuleConfig,
        args: BTreeMap<Identifier, Resource>,
        name: ComponentName,
    ) -> anyhow::Result<BTreeMap<Identifier, Resource>> {
        self.server
            .evaluate_component_initializer(
                evaluated_definitions,
                path,
                definition,
                args,
                name,
                self.deployment.name.clone(),
            )
            .await
    }

    #[fastrace::trace]
    async fn evaluate_schema(
        &self,
        schema_bundle: ModuleSource,
        source_map: Option<SourceMap>,
        rng_seed: [u8; 32],
        unix_timestamp: UnixTimestamp,
    ) -> anyhow::Result<DatabaseSchema> {
        self.server
            .evaluate_schema(
                schema_bundle,
                source_map,
                rng_seed,
                unix_timestamp,
                self.deployment.name.clone(),
            )
            .await
    }

    #[fastrace::trace]
    async fn evaluate_auth_config(
        &self,
        auth_config_bundle: ModuleSource,
        source_map: Option<SourceMap>,
        environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
        explanation: &str,
    ) -> anyhow::Result<AuthConfig> {
        self.server
            .evaluate_auth_config(
                auth_config_bundle,
                source_map,
                environment_variables,
                explanation,
                self.deployment.name.clone(),
            )
            .await
    }

    /// This fn should be called on startup. All `run_function` calls will fail
    /// if actions callbacks are not set.
    fn set_action_callbacks(&self, action_callbacks: Arc<dyn ActionCallbacks>) {
        *self.action_callbacks.write() = Some(Arc::downgrade(&action_callbacks));
    }
}
