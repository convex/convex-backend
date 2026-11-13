//! Entry point for actions run through [`IsolateClient`]: `"use rust"` modules
//! go to the wasm runtime, everything else to a V8 worker.

use std::sync::Arc;

use common::{
    execution_context::ExecutionContext,
    http::fetch::FetchClient,
    log_lines::LogLine,
    runtime::Runtime,
    types::ModuleEnvironment,
};
use database::Transaction;
use common::document::ParsedDocument;
use model::modules::{
    types::ModuleMetadata,
    ModuleModel,
};
use tokio::sync::{
    mpsc,
    oneshot,
};
use udf::{
    validation::ValidatedPathAndArgs,
    ActionCallbacks,
    ActionOutcome,
};

use crate::{
    client::{
        ActionRequest,
        ActionRequestParams,
        EnvironmentData,
    },
    IsolateClient,
};

impl<RT: Runtime> IsolateClient<RT> {
    /// Like [`IsolateClient::execute_action`], but looks up the function's
    /// module environment first and runs `"use rust"` modules on the wasm
    /// runtime instead of a V8 worker.
    #[fastrace::trace]
    pub async fn execute_action_or_rust(
        &self,
        path_and_args: ValidatedPathAndArgs,
        mut transaction: Transaction<RT>,
        action_callbacks: Arc<dyn ActionCallbacks>,
        fetch_client: Arc<dyn FetchClient>,
        log_line_sender: mpsc::UnboundedSender<LogLine>,
        context: ExecutionContext,
        environment_data: EnvironmentData<RT>,
        instance_name: String,
        function_started_sender: Option<oneshot::Sender<()>>,
    ) -> anyhow::Result<ActionOutcome> {
        // `"use rust"` modules run on the wasm runtime, not a V8 worker.
        let metadata = ModuleModel::new(&mut transaction)
            .get_metadata_for_function_by_id(path_and_args.path())
            .await?;
        if let Some(metadata) = metadata.filter(|m| m.environment == ModuleEnvironment::Rust) {
            self.execute_rust_action(
                metadata,
                path_and_args,
                transaction,
                action_callbacks,
                fetch_client,
                log_line_sender,
                context,
                environment_data,
                function_started_sender,
            )
            .await
        } else {
            self.execute_action(
                path_and_args,
                transaction,
                action_callbacks,
                fetch_client,
                log_line_sender,
                context,
                environment_data,
                instance_name,
                function_started_sender,
            )
            .await
        }
    }

    /// Execute a `"use rust"` action. Same inputs and outcome as
    /// [`IsolateClient::execute_action`], but runs on the wasm runtime, not a
    /// V8 worker.
    #[fastrace::trace]
    async fn execute_rust_action(
        &self,
        metadata: Arc<ParsedDocument<ModuleMetadata>>,
        path_and_args: ValidatedPathAndArgs,
        transaction: Transaction<RT>,
        action_callbacks: Arc<dyn ActionCallbacks>,
        fetch_client: Arc<dyn FetchClient>,
        log_line_sender: mpsc::UnboundedSender<LogLine>,
        context: ExecutionContext,
        environment_data: EnvironmentData<RT>,
        function_started_sender: Option<oneshot::Sender<()>>,
    ) -> anyhow::Result<ActionOutcome> {
        let request = ActionRequest {
            params: ActionRequestParams { path_and_args },
            identity: transaction.identity().clone(),
            transaction,
            context,
            environment_data,
        };
        self.rust_runtime
            .execute_action(
                metadata,
                request,
                action_callbacks,
                fetch_client,
                log_line_sender,
                function_started_sender,
            )
            .await
    }
}
