use async_trait::async_trait;
use common::{
    bootstrap_model::components::handles::FunctionHandle,
    components::{
        CanonicalizedComponentFunctionPath,
        ComponentId,
        ComponentPath,
    },
    execution_context::{
        ExecutionContext,
        RequestId,
    },
    runtime::UnixTimestamp,
    types::AttributedCaller,
};
use keybroker::Identity;
use model::file_storage::{
    types::FileStorageEntry,
    FileStorageId,
};
use serde_json::Value as JsonValue;
use sync_types::types::SerializedArgs;
use usage_tracking::FunctionUsageStats;
use value::DeveloperDocumentId;
use vector::PublicVectorSearchQueryResult;

use crate::FunctionResult;

#[async_trait]
pub trait ActionCallbacks: Send + Sync {
    /// Mints an AI gateway token for the calling function. The caller stays
    /// typed until Conductor builds the claims it signs. Node actions call
    /// `Application::mint_ai_gateway_jwt` over HTTP instead.
    ///
    /// `identity` is the identity the action runs under; it gates the mint on
    /// `DeploymentOp::UseAiGateway` for deploy keys and dashboard members.
    async fn create_ai_gateway_token(
        &self,
        identity: Identity,
        caller: AttributedCaller,
        request_id: RequestId,
    ) -> anyhow::Result<String>;

    // Executing UDFs
    async fn execute_query(
        &self,
        identity: Identity,
        path: CanonicalizedComponentFunctionPath,
        args: SerializedArgs,
        context: ExecutionContext,
    ) -> anyhow::Result<FunctionResult>;

    async fn execute_mutation(
        &self,
        identity: Identity,
        path: CanonicalizedComponentFunctionPath,
        args: SerializedArgs,
        context: ExecutionContext,
    ) -> anyhow::Result<FunctionResult>;

    async fn execute_action(
        &self,
        identity: Identity,
        path: CanonicalizedComponentFunctionPath,
        args: SerializedArgs,
        context: ExecutionContext,
    ) -> anyhow::Result<FunctionResult>;

    // Storage
    async fn storage_get_url(
        &self,
        identity: Identity,
        component: ComponentId,
        storage_id: FileStorageId,
    ) -> anyhow::Result<Option<String>>;

    async fn storage_delete(
        &self,
        identity: Identity,
        component: ComponentId,
        storage_id: FileStorageId,
    ) -> anyhow::Result<()>;

    // Used to get a file content from an action running in v8.
    async fn storage_get_file_entry(
        &self,
        identity: Identity,
        component: ComponentId,
        storage_id: FileStorageId,
    ) -> anyhow::Result<Option<(ComponentPath, FileStorageEntry)>>;

    // Used to store an already uploaded file from an action running in v8.
    async fn storage_store_file_entry(
        &self,
        identity: Identity,
        component: ComponentId,
        entry: FileStorageEntry,
    ) -> anyhow::Result<(ComponentPath, DeveloperDocumentId)>;

    // Scheduler
    async fn schedule_job(
        &self,
        identity: Identity,
        scheduling_component: ComponentId,
        scheduled_path: CanonicalizedComponentFunctionPath,
        udf_args: SerializedArgs,
        scheduled_ts: UnixTimestamp,
        context: ExecutionContext,
    ) -> anyhow::Result<DeveloperDocumentId>;

    async fn cancel_job(
        &self,
        identity: Identity,
        virtual_id: DeveloperDocumentId,
    ) -> anyhow::Result<()>;

    // Vector Search
    async fn vector_search(
        &self,
        identity: Identity,
        query: JsonValue,
    ) -> anyhow::Result<(Vec<PublicVectorSearchQueryResult>, FunctionUsageStats)>;

    // Components
    async fn lookup_function_handle(
        &self,
        identity: Identity,
        handle: FunctionHandle,
    ) -> anyhow::Result<CanonicalizedComponentFunctionPath>;
    async fn create_function_handle(
        &self,
        identity: Identity,
        path: CanonicalizedComponentFunctionPath,
    ) -> anyhow::Result<FunctionHandle>;
}
