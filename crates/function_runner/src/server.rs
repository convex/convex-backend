use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    fmt::Debug,
    sync::Arc,
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
    document::udf_unix_timestamp,
    errors::JsError,
    execution_context::ExecutionContext,
    http::{
        fetch::FetchClient,
        RoutedHttpPath,
    },
    knobs::MAX_ISOLATE_WORKERS,
    log_lines::LogLine,
    persistence::RetentionValidator,
    query_journal::QueryJournal,
    runtime::{
        Runtime,
        UnixTimestamp,
    },
    schemas::DatabaseSchema,
    types::{
        ConvexOrigin,
        DeploymentMetadata,
        IndexId,
        ModuleEnvironment,
        UdfType,
    },
};
use database::{
    BootstrapMetadata,
    TableCountSnapshot,
    Transaction,
    TransactionTextSnapshot,
};
use file_storage::TransactionalFileStorage;
use futures::FutureExt;
use indexing::index_reader::IndexReader;
use isolate::{
    client::{
        EnvironmentData,
        IsolateWorker,
    },
    IsolateClient,
};
use keybroker::{
    FunctionRunnerKeyBroker,
    Identity,
};
use model::{
    components::auth::propagate_component_auth,
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
    source_packages::{
        types::SourcePackage,
        upload_download::download_package,
    },
    udf_config::types::UdfConfig,
};
use rand::Rng;
use storage::{
    Storage,
    StorageUseCase,
};
use sync_types::{
    CanonicalizedModulePath,
    Timestamp,
};
use tokio::sync::{
    mpsc,
    oneshot,
};
use udf::{
    validation::{
        ValidatedHttpPath,
        ValidatedPathAndArgs,
    },
    ActionCallbacks,
    EvaluateAppDefinitionsResult,
    FunctionOutcome,
    HttpActionRequest as HttpActionRequestInner,
    HttpActionResponseStreamer,
};
use usage_tracking::{
    FunctionUsageStats,
    FunctionUsageTracker,
};
use value::identifier::Identifier;

use super::in_memory_indexes::InMemoryIndexCache;
use crate::{
    module_cache::{
        CodeCache,
        FunctionRunnerModuleLoader,
        ModuleCache,
    },
    FunctionFinalTransaction,
    FunctionWrites,
};

pub struct RunRequestArgs {
    pub key_broker: FunctionRunnerKeyBroker,
    pub index_reader: Arc<dyn IndexReader>,
    pub convex_origin: ConvexOrigin,
    pub bootstrap_metadata: BootstrapMetadata,
    pub table_count_snapshot: Arc<dyn TableCountSnapshot>,
    pub text_index_snapshot: Arc<dyn TransactionTextSnapshot>,
    pub action_callbacks: Arc<dyn ActionCallbacks>,
    pub fetch_client: Arc<dyn FetchClient>,
    pub log_line_sender: Option<mpsc::UnboundedSender<LogLine>>,
    pub function_started_sender: Option<oneshot::Sender<()>>,
    pub udf_type: UdfType,
    pub identity: Identity,
    pub existing_writes: FunctionWrites,
    pub default_system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
    pub in_memory_index_last_modified: BTreeMap<IndexId, Timestamp>,
    pub context: ExecutionContext,
    pub subfunctions_in_same_isolate: bool,
    pub deployment: DeploymentMetadata,
}

#[derive(Clone)]
pub struct FunctionMetadata {
    pub path_and_args: ValidatedPathAndArgs,
    pub journal: QueryJournal,
}

pub struct HttpActionMetadata {
    pub http_response_streamer: HttpActionResponseStreamer,
    pub http_module_path: ValidatedHttpPath,
    pub routed_path: RoutedHttpPath,
    pub http_request: HttpActionRequestInner,
}

#[async_trait]
pub trait StorageForDeployment<RT: Runtime>: Debug + Clone + Send + Sync + 'static {
    /// Gets a storage impl for a deployment. Agnostic to what kind of storage -
    /// local or s3, or how it was loaded (e.g. passed directly within backend,
    /// loaded from a transaction created in Funrun)
    async fn storage_for_deployment(
        &self,
        transaction: &mut Transaction<RT>,
        use_case: StorageUseCase,
    ) -> anyhow::Result<Arc<dyn Storage>>;

    /// Reads a source package's modules out of `modules_storage`.
    async fn download_package(
        &self,
        modules_storage: Arc<dyn Storage>,
        source_package: &SourcePackage,
    ) -> anyhow::Result<BTreeMap<CanonicalizedModulePath, ModuleConfig>> {
        download_package(modules_storage, source_package).await
    }
}

#[derive(Clone, Debug)]
pub struct DeploymentStorage {
    pub files_storage: Arc<dyn Storage>,
    pub modules_storage: Arc<dyn Storage>,
}

#[async_trait]
impl<RT: Runtime> StorageForDeployment<RT> for DeploymentStorage {
    async fn storage_for_deployment(
        &self,
        _transaction: &mut Transaction<RT>,
        use_case: StorageUseCase,
    ) -> anyhow::Result<Arc<dyn Storage>> {
        match use_case {
            StorageUseCase::Files => Ok(self.files_storage.clone()),
            StorageUseCase::Modules => Ok(self.modules_storage.clone()),
            _ => anyhow::bail!("function runner storage does not support {use_case}"),
        }
    }
}

pub struct FunctionRunnerCore<RT: Runtime, S: StorageForDeployment<RT>> {
    rt: RT,
    storage: S,
    index_cache: InMemoryIndexCache<RT>,
    module_cache: ModuleCache<RT>,
    code_cache: CodeCache,
    isolate_client: IsolateClient<RT>,
}

impl<RT: Runtime, S: StorageForDeployment<RT>> Clone for FunctionRunnerCore<RT, S> {
    fn clone(&self) -> Self {
        Self {
            rt: self.rt.clone(),
            storage: self.storage.clone(),
            index_cache: self.index_cache.clone(),
            module_cache: self.module_cache.clone(),
            code_cache: self.code_cache.clone(),
            isolate_client: self.isolate_client.clone(),
        }
    }
}

#[fastrace::trace]
pub async fn validate_run_function_result(
    udf_type: UdfType,
    ts: Timestamp,
    retention_validator: Arc<dyn RetentionValidator>,
) -> anyhow::Result<()> {
    match udf_type {
        // Since queries and mutations have no side effects, we perform the
        // retention check here, when validating the result.
        UdfType::Query | UdfType::Mutation => retention_validator
            .validate_snapshot(ts)
            .await
            .context("Function runner retention check changed"),
        // Since Actions can have side effects, we have to validate their
        // retention while we run them. We can't perform an additional check
        // here since actions can run longer than the retention.
        UdfType::Action | UdfType::HttpAction => Ok(()),
    }
}

impl<RT: Runtime, S: StorageForDeployment<RT>> FunctionRunnerCore<RT, S> {
    pub fn new<W: IsolateWorker<RT>>(
        rt: RT,
        storage: S,
        max_percent_per_client: usize,
        isolate_worker: W,
    ) -> anyhow::Result<Self> {
        let max_isolate_workers = *MAX_ISOLATE_WORKERS;
        let isolate_client = IsolateClient::new(
            rt.clone(),
            max_percent_per_client,
            max_isolate_workers,
            isolate_worker,
        )?;
        let index_cache = InMemoryIndexCache::new(rt.clone());
        let module_cache = ModuleCache::new(rt.clone());
        let code_cache = CodeCache::new();

        Ok(Self {
            rt,
            storage,
            index_cache,
            module_cache,
            code_cache,
            isolate_client,
        })
    }

    pub fn active_isolate_workers(&self) -> usize {
        self.isolate_client.active_workers()
    }

    pub fn max_isolate_workers(&self) -> usize {
        self.isolate_client.max_workers()
    }

    pub async fn shutdown(&self) -> anyhow::Result<()> {
        self.isolate_client.shutdown().await
    }

    // Runs a function given the information for the backend as well as arguments
    // to the function itself.
    // NOTE: The caller of this is responsible of checking retention by calling
    // `validate_function_runner_result`. If the retention check fails, we should
    // ignore any results or errors returned by this method.
    #[fastrace::trace]
    pub async fn run_function_no_retention_check(
        &self,
        run_request_args: RunRequestArgs,
        function_metadata: Option<FunctionMetadata>,
        http_action_metadata: Option<HttpActionMetadata>,
    ) -> anyhow::Result<(
        Option<FunctionFinalTransaction>,
        FunctionOutcome,
        FunctionUsageStats,
    )> {
        self.run_function_no_retention_check_inner(
            run_request_args,
            function_metadata,
            http_action_metadata,
        )
        .boxed()
        .await
    }

    pub async fn run_function_no_retention_check_inner(
        &self,
        RunRequestArgs {
            key_broker,
            index_reader,
            convex_origin,
            bootstrap_metadata,
            table_count_snapshot,
            text_index_snapshot,
            action_callbacks,
            fetch_client,
            log_line_sender,
            function_started_sender,
            udf_type,
            identity,
            existing_writes,
            default_system_env_vars,
            in_memory_index_last_modified,
            context,
            subfunctions_in_same_isolate,
            deployment,
        }: RunRequestArgs,
        function_metadata: Option<FunctionMetadata>,
        http_action_metadata: Option<HttpActionMetadata>,
    ) -> anyhow::Result<(
        Option<FunctionFinalTransaction>,
        FunctionOutcome,
        FunctionUsageStats,
    )> {
        let deployment_name = deployment.name.clone();
        let usage_tracker = FunctionUsageTracker::new();
        let mut transaction = self
            .index_cache
            .begin_tx(
                identity.clone(),
                existing_writes,
                index_reader,
                deployment_name.clone(),
                in_memory_index_last_modified,
                bootstrap_metadata,
                table_count_snapshot,
                text_index_snapshot,
                usage_tracker.clone(),
            )
            .await?;
        let storage = self
            .storage
            .storage_for_deployment(&mut transaction, StorageUseCase::Files)
            .await?;
        let file_storage = TransactionalFileStorage::new(self.rt.clone(), storage, convex_origin);
        let modules_storage = self
            .storage
            .storage_for_deployment(&mut transaction, StorageUseCase::Modules)
            .await?;

        let environment_data = EnvironmentData {
            key_broker,
            default_system_env_vars,
            file_storage: file_storage.clone(),
            module_loader: Arc::new(FunctionRunnerModuleLoader {
                deployment_name: deployment_name.clone(),
                cache: self.module_cache.clone(),
                code_cache: self.code_cache.clone(),
                modules_storage,
                storage: self.storage.clone(),
            }),
            deployment,
        };

        match udf_type {
            UdfType::Query | UdfType::Mutation => {
                let FunctionMetadata {
                    path_and_args,
                    journal,
                } = function_metadata.context("Missing function metadata for query or mutation")?;
                // Initialize the UDF's RNG from some high-quality entropy. As with
                // `unix_timestamp` below, the UDF is only deterministic modulo this
                // system-generated input.
                let rng_seed = self.rt.rng().random();
                let unix_timestamp = udf_unix_timestamp(transaction.next_creation_time());
                let (mut tx, outcome) = self
                    .isolate_client
                    .execute_udf(
                        udf_type,
                        path_and_args,
                        transaction,
                        journal,
                        context,
                        environment_data,
                        rng_seed,
                        unix_timestamp,
                        0,
                        deployment_name,
                        function_started_sender,
                        subfunctions_in_same_isolate,
                    )
                    .await?;
                if matches!(&outcome, FunctionOutcome::Mutation(o) if o.result.is_err()) {
                    tx.discard_pending_file_uploads()?;
                } else {
                    file_storage
                        .upload_pending_files(&mut tx, &usage_tracker)
                        .await?;
                }
                Ok((
                    Some(tx.try_into()?),
                    outcome,
                    usage_tracker.gather_user_stats(),
                ))
            },
            UdfType::Action => {
                let FunctionMetadata { path_and_args, .. } =
                    function_metadata.context("Missing function metadata for action")?;
                let log_line_sender =
                    log_line_sender.context("Missing log line sender for action")?;
                let outcome = self
                    .isolate_client
                    .execute_action(
                        path_and_args,
                        transaction,
                        action_callbacks,
                        fetch_client,
                        log_line_sender,
                        context,
                        environment_data,
                        deployment_name,
                        function_started_sender,
                    )
                    .await?;
                Ok((
                    None,
                    FunctionOutcome::Action(outcome),
                    usage_tracker.gather_user_stats(),
                ))
            },
            UdfType::HttpAction => {
                let HttpActionMetadata {
                    http_response_streamer,
                    http_module_path,
                    routed_path,
                    http_request,
                } = http_action_metadata.context("Missing http action metadata")?;
                let log_line_sender =
                    log_line_sender.context("Missing log line sender for http action")?;
                // Set the proper identity for component HTTP actions. Note that for HTTP,
                // the component is both the caller and the callee.
                let component_id = http_module_path.path().component;
                let identity =
                    propagate_component_auth(&identity, component_id, component_id.is_root());
                let outcome = self
                    .isolate_client
                    .execute_http_action(
                        http_module_path,
                        routed_path,
                        http_request,
                        identity,
                        action_callbacks,
                        fetch_client,
                        log_line_sender,
                        http_response_streamer,
                        transaction,
                        context,
                        environment_data,
                        deployment_name,
                        function_started_sender,
                    )
                    .await?;
                Ok((
                    None,
                    FunctionOutcome::HttpAction(outcome),
                    usage_tracker.gather_user_stats(),
                ))
            },
        }
    }

    pub async fn analyze(
        &self,
        udf_config: UdfConfig,
        modules: BTreeMap<CanonicalizedModulePath, ModuleConfig>,
        environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
        deployment_name: String,
    ) -> anyhow::Result<Result<BTreeMap<CanonicalizedModulePath, AnalyzedModule>, JsError>> {
        anyhow::ensure!(
            modules
                .values()
                .all(|m| m.environment == ModuleEnvironment::Isolate),
            "Can only analyze Isolate modules"
        );

        self.isolate_client
            .analyze(udf_config, modules, environment_variables, deployment_name)
            .await
    }

    #[fastrace::trace]
    pub async fn evaluate_app_definitions(
        &self,
        app_definition: ModuleConfig,
        component_definitions: BTreeMap<ComponentDefinitionPath, ModuleConfig>,
        dependency_graph: BTreeSet<(ComponentDefinitionPath, ComponentDefinitionPath)>,
        user_environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
        system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
        deployment_name: String,
    ) -> anyhow::Result<EvaluateAppDefinitionsResult> {
        anyhow::ensure!(
            app_definition.environment == ModuleEnvironment::Isolate,
            "Can only evaluate Isolate modules"
        );
        anyhow::ensure!(
            component_definitions
                .values()
                .all(|m| m.environment == ModuleEnvironment::Isolate),
            "Can only evaluate Isolate modules"
        );

        self.isolate_client
            .evaluate_app_definitions(
                app_definition,
                component_definitions,
                dependency_graph,
                user_environment_variables,
                system_env_vars,
                deployment_name,
            )
            .await
    }

    #[fastrace::trace]
    pub async fn evaluate_component_initializer(
        &self,
        evaluated_definitions: BTreeMap<ComponentDefinitionPath, ComponentDefinitionMetadata>,
        path: ComponentDefinitionPath,
        definition: ModuleConfig,
        args: BTreeMap<Identifier, Resource>,
        name: ComponentName,
        deployment_name: String,
    ) -> anyhow::Result<BTreeMap<Identifier, Resource>> {
        self.isolate_client
            .evaluate_component_initializer(
                evaluated_definitions,
                path,
                definition,
                args,
                name,
                deployment_name,
            )
            .await
    }

    #[fastrace::trace]
    pub async fn evaluate_schema(
        &self,
        schema_bundle: ModuleSource,
        source_map: Option<SourceMap>,
        rng_seed: [u8; 32],
        unix_timestamp: UnixTimestamp,
        deployment_name: String,
    ) -> anyhow::Result<DatabaseSchema> {
        self.isolate_client
            .evaluate_schema(
                schema_bundle,
                source_map,
                rng_seed,
                unix_timestamp,
                deployment_name,
            )
            .await
    }

    #[fastrace::trace]
    pub async fn evaluate_auth_config(
        &self,
        auth_config_bundle: ModuleSource,
        source_map: Option<SourceMap>,
        environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
        explanation: &str,
        deployment_name: String,
    ) -> anyhow::Result<AuthConfig> {
        self.isolate_client
            .evaluate_auth_config(
                auth_config_bundle,
                source_map,
                environment_variables,
                explanation,
                deployment_name,
            )
            .await
    }
}
