use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        OnceLock,
    },
    time::Duration,
};

use common::{
    components::{
        CanonicalizedComponentFunctionPath,
        ComponentId,
        ComponentPath,
        Reference,
        Resource,
    },
    execution_context::ExecutionContext,
    fastrace_helpers::initialize_root_from_parent,
    http::fetch::FetchClient,
    knobs::MAX_CONCURRENT_ACTION_OPS,
    runtime::{
        Runtime,
        UnixTimestamp,
    },
    sync::spsc,
    types::{
        ConvexOrigin,
        DeploymentMetadata,
        HttpActionRoute,
    },
};
use errors::ErrorMetadata;
use fastrace::future::FutureExt as _;
use file_storage::TransactionalFileStorage;
use futures::{
    select_biased,
    stream::FuturesUnordered,
    FutureExt,
    StreamExt,
};
use keybroker::{
    FunctionRunnerKeyBroker,
    Identity,
};
use parking_lot::Mutex;
use serde_json::Value as JsonValue;
use sync_types::CanonicalizedUdfPath;
use tokio::sync::mpsc;
use udf::{
    ActionCallbacks,
    SyscallTrace,
};
use usage_tracking::FunctionUsageTracker;

use crate::{
    environment::{
        action::{
            service_token::ServiceTokenCache,
            task::{
                TaskId,
                TaskRequest,
                TaskRequestEnum,
                TaskResponse,
                TaskResponseEnum,
            },
            task_order::TaskOrder,
        },
        AsyncOpRequest,
    },
    metrics::log_http_action_with_unknown_identity,
    module_cache::ModuleCache,
};

/// TaskExecutor is able to execute async syscalls and ops for actions.
/// It is cheaply clonable so it can be used from multiple async coroutines
/// at the same time.
#[derive(Clone)]
pub struct TaskExecutor<RT: Runtime> {
    pub rt: RT,
    pub identity: Identity,
    pub file_storage: TransactionalFileStorage<RT>,
    pub syscall_trace: Arc<Mutex<SyscallTrace>>,
    pub action_callbacks: Arc<dyn ActionCallbacks>,
    pub fetch_client: Arc<dyn FetchClient>,
    pub _module_loader: Arc<dyn ModuleCache<RT>>,
    pub key_broker: FunctionRunnerKeyBroker,
    pub task_order: TaskOrder,
    pub task_retval_sender: mpsc::UnboundedSender<TaskResponse>,
    pub usage_tracker: FunctionUsageTracker,
    pub context: ExecutionContext,
    pub resources: Arc<Mutex<BTreeMap<Reference, Resource>>>,
    pub component_id: ComponentId,
    pub udf_path: CanonicalizedUdfPath,
    pub component_path: ComponentPath,
    pub convex_origin_override: Arc<Mutex<Option<ConvexOrigin>>>,
    pub http_action_route: Arc<OnceLock<HttpActionRoute>>,
    pub deployment: DeploymentMetadata,
    pub(crate) service_token: Arc<ServiceTokenCache>,
}

impl<RT: Runtime> TaskExecutor<RT> {
    pub async fn go(self, mut pending_tasks: spsc::UnboundedReceiver<TaskRequest>) {
        let mut running_tasks = FuturesUnordered::new();
        let mut requests_closed = false;
        loop {
            if requests_closed && running_tasks.is_empty() {
                return;
            }
            if requests_closed || self.task_order.active_task_count() >= *MAX_CONCURRENT_ACTION_OPS
            {
                // There is an op running, and we want to run it without starting
                // another request, either because there are no more requests or
                // we're at max concurrency.
                let task_id = running_tasks
                    .next()
                    .await
                    .expect("nonempty stream returned None");
                self.task_order.pop_running_task(task_id);
                continue;
            }
            select_biased! {
                task_id = running_tasks.select_next_some() => {
                    self.task_order.pop_running_task(task_id);
                },
                task_request = pending_tasks.recv().fuse() => {
                    if let Some(task_request) = task_request {
                        let root = initialize_root_from_parent("TaskExecutor::execute_task", task_request.parent_trace.clone());
                        self.task_order.push_running_task(&task_request);
                        running_tasks.push(self.clone().run_async_task(task_request).in_span(root));
                    } else {
                        requests_closed = true;
                    }
                },
            }
        }
    }

    #[fastrace::trace]
    async fn run_async_task(self, task_request: TaskRequest) -> TaskId {
        let task_id = task_request.task_id;
        let variant = match task_request.variant {
            TaskRequestEnum::AsyncSyscall { name, args } => self
                .run_async_syscall(name, args)
                .await
                .map(TaskResponseEnum::Syscall),
            TaskRequestEnum::AsyncOp(AsyncOpRequest::SendStream { stream, stream_id }) => {
                let _ = self.send_stream(stream_id, stream).await;
                return task_id;
            },
            TaskRequestEnum::AsyncOp(AsyncOpRequest::Fetch {
                request,
                response_body_stream_id: stream_id,
            }) => {
                self.run_fetch(task_id, request, stream_id).await;
                return task_id;
            },
            TaskRequestEnum::AsyncOp(AsyncOpRequest::ParseMultiPart {
                content_type,
                request_stream,
            }) => self
                .run_parse_multi_part(content_type, request_stream)
                .await
                .map(TaskResponseEnum::ParseMultiPart),
            TaskRequestEnum::AsyncOp(AsyncOpRequest::Sleep { until, .. }) => {
                self.run_sleep(until).await.map(TaskResponseEnum::Sleep)
            },
            TaskRequestEnum::AsyncOp(AsyncOpRequest::StorageStore {
                body_stream,
                content_type,
                content_length,
                digest,
            }) => self
                .run_storage_store(body_stream, content_type, content_length, digest)
                .await
                .map(TaskResponseEnum::StorageStore),
            TaskRequestEnum::AsyncOp(AsyncOpRequest::StorageGet {
                storage_id,
                stream_id,
            }) => {
                self.run_storage_get(task_id, storage_id, stream_id).await;
                return task_id;
            },
        };
        let _ = self
            .task_retval_sender
            .send(TaskResponse::TaskDone { task_id, variant });
        task_id
    }

    async fn run_sleep(&self, until: UnixTimestamp) -> anyhow::Result<UnixTimestamp> {
        self.rt.pause_client().wait("begin_run_sleep").await;
        let now = self.rt.unix_timestamp();
        if now >= until {
            return Ok(until);
        }
        self.rt.wait(until - now).await;
        while self.task_order.sleep_is_blocked(&until) {
            // Another sleep with an earlier `until` time is still running. Let it finish.
            self.rt.wait(Duration::from_millis(5)).await;
        }
        Ok(until)
    }

    pub fn user_identity(&self) -> anyhow::Result<JsonValue> {
        let user_identity = match self.identity.clone() {
            Identity::User(identity) => Some(identity.attributes),
            Identity::ActingUser(_, identity) => Some(identity),
            Identity::Unknown(Some(error_message)) => {
                log_http_action_with_unknown_identity();
                tracing::info!(
                    "Http Action called getUserIdentity with unknown identity: {}",
                    error_message.short_msg,
                );
                anyhow::bail!(error_message);
            },
            _ => None,
        };
        if let Some(user_identity) = user_identity {
            return Arc::unwrap_or_clone(user_identity).try_into();
        }
        Ok(JsonValue::Null)
    }

    pub fn resolve(&self, reference: &Reference) -> anyhow::Result<Resource> {
        let resource = {
            let resources = self.resources.lock();
            resources
                .get(reference)
                .ok_or_else(|| {
                    ErrorMetadata::bad_request(
                        "InvalidReference",
                        format!("Couldn't resolve {}", reference.evaluation_time_debug_str()),
                    )
                })?
                .clone()
        };
        Ok(resource)
    }

    pub fn resolve_function(
        &self,
        reference: &Reference,
    ) -> anyhow::Result<CanonicalizedComponentFunctionPath> {
        let resource = self.resolve(reference)?;
        match resource {
            Resource::Function(p) => Ok(p),
            Resource::ResolvedSystemUdf { .. } => {
                anyhow::bail!("actions cannot call functions by component id");
            },
            Resource::Value(v) => anyhow::bail!(ErrorMetadata::bad_request(
                "InvalidFunction",
                format!(
                    "Resolved reference {} to {v}, not a function",
                    reference.evaluation_time_debug_str()
                ),
            )),
        }
    }
}
