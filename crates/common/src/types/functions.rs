use std::{
    fmt::{
        self,
        Debug,
    },
    str::FromStr,
};

use anyhow::Context;
use metrics::StaticMetricLabel;
use pb::common::UdfType as UdfTypeProto;
use serde::{
    Deserialize,
    Serialize,
};
use value::{
    heap_size::HeapSize,
    id_v6::DeveloperDocumentId,
};

use super::HttpActionRoute;
use crate::{
    components::{
        CanonicalizedComponentFunctionPath,
        ComponentId,
    },
    execution_context::ExecutionId,
    version::ClientVersion,
};

/// Whether a query call is a fresh invocation or a rerun of a query that has
/// produced a result before.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum QueryInvocation {
    /// A fresh invocation: an HTTP request, cron, action, scheduled function,
    /// dashboard tester, or the first time a client subscribes to a query.
    Fresh,
    /// A rerun of a query whose subscription was no longer valid.
    Invalidated,
    /// A rerun caused by the client's auth identity changing.
    IdentityChange,
}

#[derive(
    Serialize, Copy, Clone, Debug, PartialEq, Eq, Hash, Ord, PartialOrd, strum::VariantArray,
)]
#[serde(rename_all = "camelCase")]
pub enum UdfType {
    Query,
    Mutation,
    Action,
    HttpAction,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, strum::EnumString, strum::Display,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum UdfRuntime {
    V8,
    Wasm,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdfTypeJson {
    Query,
    Mutation,
    Action,
    HttpAction,
}

impl From<UdfType> for UdfTypeJson {
    fn from(udf_type: UdfType) -> Self {
        match udf_type {
            UdfType::Query => UdfTypeJson::Query,
            UdfType::Mutation => UdfTypeJson::Mutation,
            UdfType::Action => UdfTypeJson::Action,
            UdfType::HttpAction => UdfTypeJson::HttpAction,
        }
    }
}

impl From<UdfTypeJson> for UdfType {
    fn from(udf_type: UdfTypeJson) -> Self {
        match udf_type {
            UdfTypeJson::Query => UdfType::Query,
            UdfTypeJson::Mutation => UdfType::Mutation,
            UdfTypeJson::Action => UdfType::Action,
            UdfTypeJson::HttpAction => UdfType::HttpAction,
        }
    }
}

impl UdfType {
    pub fn metric_label(self) -> StaticMetricLabel {
        StaticMetricLabel::new("udf_type", self.to_lowercase_string())
    }

    pub fn to_lowercase_string(self) -> &'static str {
        match self {
            UdfType::Query => "query",
            UdfType::Mutation => "mutation",
            UdfType::Action => "action",
            UdfType::HttpAction => "http_action",
        }
    }
}

impl FromStr for UdfType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Query" | "query" => Ok(Self::Query),
            "Mutation" | "mutation" => Ok(Self::Mutation),
            "Action" | "action" => Ok(Self::Action),
            "HttpEndpoint" | "httpEndpoint" | "HttpAction" | "httpAction" | "http_action" => {
                Ok(Self::HttpAction)
            },
            _ => anyhow::bail!("Expected UdfType, got {:?}", s),
        }
    }
}

impl fmt::Display for UdfType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            UdfType::Query => "Query",
            UdfType::Mutation => "Mutation",
            UdfType::Action => "Action",
            UdfType::HttpAction => "HttpAction",
        };
        write!(f, "{s}")
    }
}

impl HeapSize for UdfType {
    fn heap_size(&self) -> usize {
        0
    }
}

impl From<UdfType> for UdfTypeProto {
    fn from(u: UdfType) -> UdfTypeProto {
        match u {
            UdfType::Query => UdfTypeProto::Query,
            UdfType::Mutation => UdfTypeProto::Mutation,
            UdfType::Action => UdfTypeProto::Action,
            UdfType::HttpAction => UdfTypeProto::HttpAction,
        }
    }
}

impl From<UdfTypeProto> for UdfType {
    fn from(u: UdfTypeProto) -> UdfType {
        match u {
            UdfTypeProto::Query => UdfType::Query,
            UdfTypeProto::Mutation => UdfType::Mutation,
            UdfTypeProto::Action => UdfType::Action,
            UdfTypeProto::HttpAction => UdfType::HttpAction,
        }
    }
}

/// A unique identifier for a UDF
#[derive(Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum UdfIdentifier {
    Function(CanonicalizedComponentFunctionPath),
    Http(HttpActionRoute),
    SystemJob(String),
}

impl UdfIdentifier {
    pub fn into_component_and_udf_path(self) -> (Option<String>, String) {
        match self {
            UdfIdentifier::Function(path) => {
                let (component_path, udf_path) = path.into_component_and_udf_path();
                (component_path.serialize(), udf_path.to_string())
            },
            UdfIdentifier::Http(_) | UdfIdentifier::SystemJob(_) => (None, self.to_string()),
        }
    }
}

impl fmt::Display for UdfIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            UdfIdentifier::Function(path) => write!(f, "{}", path.debug_str()),
            UdfIdentifier::Http(route) => write!(f, "{}", route.path),
            UdfIdentifier::SystemJob(command) => write!(f, "_system_job/{command}"),
        }
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug, Hash)]
pub enum AllowedVisibility {
    PublicOnly,
    All,
}

#[derive(Clone, Debug, PartialEq, Eq, Ord, PartialOrd)]
pub enum FunctionCaller {
    SyncWorker(ClientVersion),
    HttpApi(ClientVersion),
    /// Used by function tester in the dashboard
    Tester(ClientVersion),
    // This is a user defined http actions called externally. If the http action
    // calls other functions, their caller would be `Action`.
    HttpEndpoint,
    Cron,
    Scheduler {
        job_id: DeveloperDocumentId,
        component_id: ComponentId,
    },
    Action {
        parent_scheduled_job: Option<(ComponentId, DeveloperDocumentId)>,
        parent_execution_id: Option<ExecutionId>,
    },
}

impl FunctionCaller {
    pub fn client_version(&self) -> Option<ClientVersion> {
        match self {
            FunctionCaller::SyncWorker(c) => Some(c),
            FunctionCaller::HttpApi(c) => Some(c),
            FunctionCaller::Tester(c) => Some(c),
            FunctionCaller::HttpEndpoint
            | FunctionCaller::Cron
            | FunctionCaller::Scheduler { .. }
            | FunctionCaller::Action { .. } => None,
        }
        .cloned()
    }

    pub fn parent_scheduled_job(&self) -> Option<(ComponentId, DeveloperDocumentId)> {
        match self {
            FunctionCaller::SyncWorker(_)
            | FunctionCaller::HttpApi(_)
            | FunctionCaller::Tester(_)
            | FunctionCaller::HttpEndpoint
            | FunctionCaller::Cron => None,
            FunctionCaller::Scheduler {
                job_id,
                component_id,
            } => Some((*component_id, *job_id)),
            FunctionCaller::Action {
                parent_scheduled_job,
                ..
            } => *parent_scheduled_job,
        }
    }

    pub fn parent_execution_id(&self) -> Option<ExecutionId> {
        match self {
            FunctionCaller::SyncWorker(_)
            | FunctionCaller::HttpApi(_)
            | FunctionCaller::Tester(_)
            | FunctionCaller::HttpEndpoint
            | FunctionCaller::Cron
            | FunctionCaller::Scheduler { .. } => None,
            FunctionCaller::Action {
                parent_execution_id,
                ..
            } => *parent_execution_id,
        }
    }

    pub fn is_root(&self) -> bool {
        match self {
            FunctionCaller::SyncWorker(_)
            | FunctionCaller::HttpApi(_)
            | FunctionCaller::Tester(_)
            | FunctionCaller::HttpEndpoint
            | FunctionCaller::Cron
            | FunctionCaller::Scheduler { .. } => true,
            FunctionCaller::Action { .. } => false,
        }
    }

    pub fn run_until_completion_if_cancelled(&self) -> bool {
        // If the action is called from a web socket or http we want to continue
        // to run it even if the client goes away. However, we preserve the right
        // to interrupt actions if the backend restarts.
        match self {
            FunctionCaller::SyncWorker(_)
            | FunctionCaller::HttpApi(_)
            | FunctionCaller::HttpEndpoint
            | FunctionCaller::Tester(_) => true,
            FunctionCaller::Cron
            | FunctionCaller::Scheduler { .. }
            | FunctionCaller::Action { .. } => false,
        }
    }

    pub fn allowed_visibility(&self) -> AllowedVisibility {
        match self {
            FunctionCaller::SyncWorker(_) | FunctionCaller::HttpApi(_) => {
                AllowedVisibility::PublicOnly
            },
            // NOTE: Allowed visibility doesn't make sense in the context of an
            // user defined http action since all http actions are public, and
            // we shouldn't be checking visibility. We define this for completeness.
            FunctionCaller::HttpEndpoint => AllowedVisibility::PublicOnly,
            FunctionCaller::Tester(_)
            | FunctionCaller::Cron
            | FunctionCaller::Scheduler { .. }
            | FunctionCaller::Action { .. } => AllowedVisibility::All,
        }
    }
}

impl fmt::Display for FunctionCaller {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            FunctionCaller::SyncWorker(_) => "SyncWorker",
            FunctionCaller::HttpApi(_) => "HttpApi",
            FunctionCaller::Tester(_) => "Tester",
            FunctionCaller::HttpEndpoint => "HttpEndpoint",
            FunctionCaller::Cron => "Cron",
            FunctionCaller::Scheduler { .. } => "Scheduler",
            FunctionCaller::Action { .. } => "Action",
        };
        write!(f, "{s}")
    }
}

impl From<FunctionCaller> for pb::common::FunctionCaller {
    fn from(caller: FunctionCaller) -> Self {
        let caller = match caller {
            FunctionCaller::SyncWorker(client_version) => {
                pb::common::function_caller::Caller::SyncWorker(client_version.into())
            },
            FunctionCaller::HttpApi(client_version) => {
                pb::common::function_caller::Caller::HttpApi(client_version.into())
            },
            FunctionCaller::Tester(client_version) => {
                pb::common::function_caller::Caller::Tester(client_version.into())
            },
            FunctionCaller::HttpEndpoint => pb::common::function_caller::Caller::HttpEndpoint(()),
            FunctionCaller::Cron => pb::common::function_caller::Caller::Cron(()),
            FunctionCaller::Scheduler {
                job_id,
                component_id,
            } => {
                let caller = pb::common::SchedulerFunctionCaller {
                    job_id: Some(job_id.into()),
                    component_id: component_id.serialize_to_string(),
                };
                pb::common::function_caller::Caller::Scheduler(caller)
            },
            FunctionCaller::Action {
                parent_scheduled_job,
                parent_execution_id,
            } => {
                let (component_id, document_id) = parent_scheduled_job.unzip();
                let caller = pb::common::ActionFunctionCaller {
                    parent_scheduled_job: document_id.map(|job_id| job_id.into()),
                    component_id: component_id
                        .unwrap_or(ComponentId::Root)
                        .serialize_to_string(),
                    parent_execution_id: parent_execution_id
                        .map(|execution_id| execution_id.to_string()),
                };
                pb::common::function_caller::Caller::Action(caller)
            },
        };
        Self {
            caller: Some(caller),
        }
    }
}

impl TryFrom<pb::common::FunctionCaller> for FunctionCaller {
    type Error = anyhow::Error;

    fn try_from(msg: pb::common::FunctionCaller) -> anyhow::Result<Self> {
        let caller = match msg.caller {
            Some(pb::common::function_caller::Caller::SyncWorker(client_version)) => {
                FunctionCaller::SyncWorker(client_version.try_into()?)
            },
            Some(pb::common::function_caller::Caller::HttpApi(client_version)) => {
                FunctionCaller::HttpApi(client_version.try_into()?)
            },
            Some(pb::common::function_caller::Caller::Tester(client_version)) => {
                FunctionCaller::Tester(client_version.try_into()?)
            },
            Some(pb::common::function_caller::Caller::HttpEndpoint(())) => {
                FunctionCaller::HttpEndpoint
            },
            Some(pb::common::function_caller::Caller::Cron(())) => FunctionCaller::Cron,
            Some(pb::common::function_caller::Caller::Scheduler(caller)) => {
                let pb::common::SchedulerFunctionCaller {
                    job_id,
                    component_id,
                } = caller;
                let job_id = job_id.context("Missing `job_id` field")?.try_into()?;
                let component_id = ComponentId::deserialize_from_string(component_id.as_deref())?;
                FunctionCaller::Scheduler {
                    job_id,
                    component_id,
                }
            },
            Some(pb::common::function_caller::Caller::Action(caller)) => {
                let pb::common::ActionFunctionCaller {
                    parent_scheduled_job,
                    component_id,
                    parent_execution_id,
                } = caller;
                let parent_scheduled_job = parent_scheduled_job
                    .map(|job_id| job_id.try_into())
                    .transpose()?;
                let component_id = ComponentId::deserialize_from_string(component_id.as_deref())?;
                FunctionCaller::Action {
                    parent_scheduled_job: parent_scheduled_job.map(|job_id| (component_id, job_id)),
                    parent_execution_id: parent_execution_id
                        .map(|id| ExecutionId::from_str(&id))
                        .transpose()?,
                }
            },
            None => anyhow::bail!("Missing `caller` field"),
        };
        Ok(caller)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ModuleEnvironment {
    Isolate,
    Node,
    /// WASM module ("use rust"), actions only.
    Rust,
    /// The function doesn't exist (the argument/path are invalid/no accessible
    /// to the caller or analyze fails)
    Invalid,
}

impl FromStr for ModuleEnvironment {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let environment = match s {
            "node" => ModuleEnvironment::Node,
            "rust" => ModuleEnvironment::Rust,
            "isolate" => ModuleEnvironment::Isolate,
            "invalid" => ModuleEnvironment::Invalid,
            _ => anyhow::bail!("Invalid environment {s}"),
        };
        Ok(environment)
    }
}

impl fmt::Display for ModuleEnvironment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ModuleEnvironment::Isolate => "isolate",
            ModuleEnvironment::Node => "node",
            ModuleEnvironment::Rust => "rust",
            ModuleEnvironment::Invalid => "invalid",
        };
        write!(f, "{s}")
    }
}

impl ModuleEnvironment {
    pub fn as_sentry_tag(&self) -> &'static str {
        match self {
            // "isolate" is an internal term. Simply the default environment externally.
            ModuleEnvironment::Isolate => "default",
            ModuleEnvironment::Node => "node",
            ModuleEnvironment::Rust => "rust",
            ModuleEnvironment::Invalid => "unknown",
        }
    }
}

impl From<QueryInvocation> for pb::common::QueryInvocation {
    fn from(invocation: QueryInvocation) -> Self {
        let invocation = match invocation {
            QueryInvocation::Fresh => pb::common::query_invocation::Invocation::Fresh(()),
            QueryInvocation::Invalidated => {
                pb::common::query_invocation::Invocation::Invalidation(())
            },
            QueryInvocation::IdentityChange => {
                pb::common::query_invocation::Invocation::IdentityChange(())
            },
        };
        Self {
            invocation: Some(invocation),
        }
    }
}

impl TryFrom<pb::common::QueryInvocation> for QueryInvocation {
    type Error = anyhow::Error;

    fn try_from(msg: pb::common::QueryInvocation) -> anyhow::Result<Self> {
        let invocation = match msg.invocation {
            Some(pb::common::query_invocation::Invocation::Fresh(())) => QueryInvocation::Fresh,
            Some(pb::common::query_invocation::Invocation::Invalidation(())) => {
                QueryInvocation::Invalidated
            },
            Some(pb::common::query_invocation::Invocation::IdentityChange(())) => {
                QueryInvocation::IdentityChange
            },
            None => anyhow::bail!("Missing `invocation` field"),
        };
        Ok(invocation)
    }
}
