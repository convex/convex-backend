#![allow(non_snake_case)]

use anyhow::Context;
use common::{
    bootstrap_model::components::handles::FunctionHandle,
    components::{
        ComponentId,
        Reference,
    },
    runtime::{
        Runtime,
        UnixTimestamp,
    },
    try_anyhow,
    types::{
        AttributedCaller,
        AI_GATEWAY_URL,
    },
};
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use keybroker::Identity;
use model::{
    components::auth::propagate_component_auth,
    file_storage::FileStorageId,
};
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::{
    json,
    Value as JsonValue,
};
use udf::helpers::UdfArgsJson;
use value::{
    id_v6::DeveloperDocumentId,
    JsonPackedValue,
};
use vector::{
    VectorSearchJson,
    VectorSearchRequest,
};

use super::task_executor::TaskExecutor;
use crate::{
    environment::helpers::{
        remove_rejected_before_execution,
        with_argument_error,
        ArgName,
    },
    metrics::async_syscall_timer,
};

impl<RT: Runtime> TaskExecutor<RT> {
    pub fn component_id(&self) -> ComponentId {
        self.component_id
    }

    #[fastrace::trace(properties = {"name": "{name}"})]
    pub async fn run_async_syscall(&self, name: String, args: JsonValue) -> anyhow::Result<String> {
        let start = self.rt.monotonic_now();
        let timer = async_syscall_timer(&name);
        let result: anyhow::Result<_> = try_anyhow!({
            match &name[..] {
                "1.0/actions/query" => self.async_syscall_actions_runQuery(args).await?.into(),
                "1.0/actions/mutation" => {
                    self.async_syscall_actions_runMutation(args).await?.into()
                },
                "1.0/actions/action" => self.async_syscall_actions_runAction(args).await?.into(),
                "1.0/createServiceToken" => {
                    self.async_syscall_createServiceToken(args).await?.into()
                },
                "1.0/getServiceUrl" => self.async_syscall_getServiceUrl(args)?.into(),
                "1.0/actions/schedule" => self.async_syscall_schedule(args).await?.into(),
                "1.0/actions/cancel_job" => self.async_syscall_cancel_job(args).await?.into(),
                "1.0/actions/vectorSearch" => self.async_syscall_vectorSearch(args).await?.into(),
                "1.0/getFunctionMetadata" => self.async_syscall_getFunctionMetadata()?.into(),
                "1.0/getDeploymentMetadata" => self.async_syscall_getDeploymentMetadata()?.into(),
                "1.0/getRequestMetadata" => self.async_syscall_getRequestMetadata()?.into(),
                "1.0/getUserIdentity" => self.async_syscall_getUserIdentity(args).await?.into(),
                "1.0/storageDelete" => self.async_syscall_storageDelete(args).await?.into(),
                "1.0/storageGetMetadata" => {
                    self.async_syscall_storageGetMetadata(args).await?.into()
                },
                "1.0/storageGenerateUploadUrl" => self
                    .async_syscall_storageGenerateUploadUrl(args)
                    .await?
                    .into(),
                "1.0/storageGetUrl" => self.async_syscall_storageGetUrl(args).await?.into(),
                "1.0/createFunctionHandle" => {
                    self.async_syscall_createFunctionHandle(args).await?.into()
                },
                "1.0/auditLog" => {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "AuditLogNotSupportedInAction",
                        "Audit logging is not yet supported in actions",
                    ));
                },
                _ => {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "UnknownAsyncOperation",
                        format!("Unknown async operation {name}")
                    ));
                },
            }
        });
        self.syscall_trace
            .lock()
            .log_async_syscall(name, start.elapsed(), result.is_ok());
        match &result {
            Ok(_) => timer.finish(),
            Err(e) => timer.finish_with(e.metric_status_label_value()),
        };
        struct JsonString(String);
        impl From<JsonPackedValue> for JsonString {
            fn from(v: JsonPackedValue) -> Self {
                Self(v.as_str().to_owned())
            }
        }
        impl From<JsonValue> for JsonString {
            fn from(v: JsonValue) -> Self {
                Self(v.to_string())
            }
        }
        result.map(|JsonString(s)| s)
    }

    #[convex_macro::instrument_future]
    async fn async_syscall_actions_runQuery(
        &self,
        args: JsonValue,
    ) -> anyhow::Result<JsonPackedValue> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RunQueryArgs {
            name: Option<String>,
            reference: Option<String>,
            function_handle: Option<String>,
            args: UdfArgsJson,
        }
        let RunQueryArgs {
            name,
            reference,
            function_handle,
            args,
        } = with_argument_error("runQuery", || Ok(serde_json::from_value(args)?))?;
        let function_path = match function_handle {
            Some(function_handle) => {
                let handle: FunctionHandle =
                    with_argument_error("runQuery", || function_handle.parse())?;
                self.action_callbacks
                    .lookup_function_handle(self.identity.clone(), handle)
                    .await?
            },
            None => {
                let reference = parse_name_or_reference("runQuery", name, reference)?;
                self.resolve_function(&reference)?
            },
        };
        let value = self
            .action_callbacks
            .execute_query(
                propagate_component_auth(
                    &self.identity,
                    self.component_id,
                    function_path.component.is_root(),
                ),
                function_path,
                args.into_serialized_args()?,
                self.context.clone(),
            )
            .await
            .map_err(remove_rejected_before_execution)?
            .result?;
        Ok(value)
    }

    #[convex_macro::instrument_future]
    async fn async_syscall_createServiceToken(&self, args: JsonValue) -> anyhow::Result<JsonValue> {
        // Claims are chosen entirely by the backend per service; the caller only
        // names the service.
        let CreateServiceTokenArgs {
            service: Service::AiGateway,
        } = with_argument_error("createServiceToken", || Ok(serde_json::from_value(args)?))?;
        // Every value here comes from the executor's own state, so a function
        // cannot misattribute its spend by lying about who it is.
        let component_path = self.component_path.clone();
        let caller = match self.http_action_route.get() {
            Some(route) => AttributedCaller::HttpAction {
                component_path,
                route: route.clone(),
            },
            None => AttributedCaller::Action {
                component_path,
                udf_path: self.udf_path.clone(),
            },
        };
        let action_callbacks = self.action_callbacks.clone();
        let identity = self.identity.clone();
        let request_id = self.context.request_id.clone();
        let token = self
            .service_token
            .get_or_mint(
                || self.rt.system_time(),
                || async move {
                    action_callbacks
                        .create_ai_gateway_token(identity, caller, request_id)
                        .await
                },
            )
            .await?;
        Ok(JsonValue::String(token))
    }

    fn async_syscall_getServiceUrl(&self, args: JsonValue) -> anyhow::Result<JsonValue> {
        let GetServiceUrlArgs {
            service: Service::AiGateway,
        } = with_argument_error("getServiceUrl", || Ok(serde_json::from_value(args)?))?;
        Ok(JsonValue::String(AI_GATEWAY_URL.to_owned()))
    }

    #[convex_macro::instrument_future]
    async fn async_syscall_actions_runMutation(
        &self,
        args: JsonValue,
    ) -> anyhow::Result<JsonPackedValue> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RunMutationArgs {
            name: Option<String>,
            reference: Option<String>,
            function_handle: Option<String>,
            args: UdfArgsJson,
        }
        let RunMutationArgs {
            name,
            reference,
            function_handle,
            args,
        } = with_argument_error("runMutation", || Ok(serde_json::from_value(args)?))?;
        let function_path = match function_handle {
            Some(function_handle) => {
                let handle: FunctionHandle =
                    with_argument_error("runMutation", || function_handle.parse())?;
                self.action_callbacks
                    .lookup_function_handle(self.identity.clone(), handle)
                    .await?
            },
            None => {
                let reference = parse_name_or_reference("runMutation", name, reference)?;
                self.resolve_function(&reference)?
            },
        };
        let value = self
            .action_callbacks
            .execute_mutation(
                propagate_component_auth(
                    &self.identity,
                    self.component_id,
                    function_path.component.is_root(),
                ),
                function_path,
                args.into_serialized_args()?,
                self.context.clone(),
            )
            .await
            .map_err(remove_rejected_before_execution)?
            .result?;
        Ok(value)
    }

    #[convex_macro::instrument_future]
    async fn async_syscall_actions_runAction(
        &self,
        args: JsonValue,
    ) -> anyhow::Result<JsonPackedValue> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RunActionArgs {
            name: Option<String>,
            reference: Option<String>,
            function_handle: Option<String>,
            args: UdfArgsJson,
        }
        let RunActionArgs {
            name,
            reference,
            function_handle,
            args,
        } = with_argument_error("runAction", || Ok(serde_json::from_value(args)?))?;
        let function_path = match function_handle {
            Some(function_handle) => {
                let handle: FunctionHandle =
                    with_argument_error("runAction", || function_handle.parse())?;
                self.action_callbacks
                    .lookup_function_handle(self.identity.clone(), handle)
                    .await?
            },
            None => {
                let reference = parse_name_or_reference("runAction", name, reference)?;
                self.resolve_function(&reference)?
            },
        };
        let value = self
            .action_callbacks
            .execute_action(
                propagate_component_auth(
                    &self.identity,
                    self.component_id,
                    function_path.component.is_root(),
                ),
                function_path,
                args.into_serialized_args()?,
                self.context.clone(),
            )
            .await
            .map_err(remove_rejected_before_execution)?
            .result?;
        Ok(value)
    }

    #[convex_macro::instrument_future]
    async fn async_syscall_schedule(&self, args: JsonValue) -> anyhow::Result<JsonValue> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct ScheduleArgs {
            name: Option<String>,
            reference: Option<String>,
            function_handle: Option<String>,
            ts: f64,
            args: UdfArgsJson,
        }

        let ScheduleArgs {
            name,
            reference,
            function_handle,
            ts,
            args,
        }: ScheduleArgs = with_argument_error("scheduler", || Ok(serde_json::from_value(args)?))?;
        let path = match function_handle {
            Some(h) => {
                let handle: FunctionHandle = with_argument_error("scheduler", || h.parse())?;
                self.action_callbacks
                    .lookup_function_handle(self.identity.clone(), handle)
                    .await?
            },
            None => {
                let reference = parse_name_or_reference("scheduler", name, reference)?;
                self.resolve_function(&reference)?
            },
        };
        let scheduled_ts = with_argument_error("ts", || UnixTimestamp::from_secs_f64(ts))?;
        let virtual_id = self
            .action_callbacks
            .schedule_job(
                self.identity.clone(),
                self.component_id(),
                path,
                args.into_serialized_args()?,
                scheduled_ts,
                self.context.clone(),
            )
            .await?;

        Ok(JsonValue::from(virtual_id))
    }

    #[convex_macro::instrument_future]
    async fn async_syscall_cancel_job(&self, args: JsonValue) -> anyhow::Result<JsonValue> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct CancelJobArgs {
            id: String,
        }
        let virtual_id_v6 = with_argument_error("db.cancel_job", || {
            let args: CancelJobArgs = serde_json::from_value(args)?;
            let id = DeveloperDocumentId::decode(&args.id).context(ArgName("id"))?;
            Ok(id)
        })?;

        self.action_callbacks
            .cancel_job(self.identity.clone(), virtual_id_v6)
            .await?;
        Ok(JsonValue::Null)
    }

    #[convex_macro::instrument_future]
    async fn async_syscall_vectorSearch(&self, args: JsonValue) -> anyhow::Result<JsonValue> {
        let VectorSearchRequest { query } = serde_json::from_value(args)?;
        let component_id = self.component_id();
        let mut vector_search_query: VectorSearchJson = serde_json::from_value(query)?;
        vector_search_query.insert_component_id(component_id);

        let (results, usage_stats) = self
            .action_callbacks
            .vector_search(
                self.identity.clone(),
                serde_json::to_value(vector_search_query)?,
            )
            .await?;
        self.usage_tracker.add(usage_stats);
        let results: Vec<_> = results.into_iter().map(JsonValue::from).collect();
        Ok(json!({ "results": results }))
    }

    fn async_syscall_getFunctionMetadata(&self) -> anyhow::Result<JsonValue> {
        Ok(json!({
            "name": self.udf_path.clone().strip().to_string(),
            "componentPath": self.component_path.to_string(),
        }))
    }

    fn async_syscall_getDeploymentMetadata(&self) -> anyhow::Result<JsonValue> {
        Ok(json!({
            "name": self.deployment.name,
            "region": self.deployment.region,
            "class": self.deployment.class,
        }))
    }

    fn async_syscall_getRequestMetadata(&self) -> anyhow::Result<JsonValue> {
        let metadata = &self.context.request_metadata;
        // The top-level scheduled function and all of its descendants (e.g. a
        // mutation called by a scheduled action) report the scheduled function's
        // id, since `parent_scheduled_job` is propagated down the call tree. It
        // is `None` when the function was not scheduled.
        let scheduled_function_id = self
            .context
            .parent_scheduled_job
            .map(|(_, job_id)| job_id.encode());
        // Expose the raw auth JWT the request was authenticated with, if any. Only
        // `User` identities carry a JWT (an OIDC or custom JWT); admin keys and
        // logged-out requests have no token.
        let auth_token = match &self.identity {
            Identity::User(identity) => Some(identity.original_token.as_str()),
            _ => None,
        };
        Ok(json!({
            "ip": metadata.ip.as_ref().map(|ip| ip.as_str()),
            "userAgent": metadata.user_agent.as_ref().map(|ua| ua.as_str()),
            "requestId": self.context.request_id.as_str(),
            "scheduledFunctionId": scheduled_function_id,
            "authToken": auth_token,
        }))
    }

    #[convex_macro::instrument_future]
    async fn async_syscall_getUserIdentity(&self, _args: JsonValue) -> anyhow::Result<JsonValue> {
        self.user_identity()
    }

    async fn async_syscall_storageGenerateUploadUrl(
        &self,
        _args: JsonValue,
    ) -> anyhow::Result<JsonValue> {
        let issued_ts = self.rt.unix_timestamp();
        let component = self.component_id();
        let postUrl = self.file_storage.generate_upload_url_with_origin(
            self.convex_origin_override.lock().clone(),
            &self.key_broker,
            issued_ts,
            component,
        )?;
        Ok(serde_json::to_value(postUrl)?)
    }

    #[convex_macro::instrument_future]
    async fn async_syscall_storageGetUrl(&self, args: JsonValue) -> anyhow::Result<JsonValue> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct GetUrlArgs {
            storage_id: String,
        }
        let storage_id: FileStorageId = with_argument_error("storage.getUrl", || {
            let GetUrlArgs { storage_id } = serde_json::from_value(args)?;
            storage_id.parse().context(ArgName("storageId"))
        })?;
        let url = self
            .action_callbacks
            .storage_get_url(self.identity.clone(), self.component_id(), storage_id)
            .await?;
        Ok(url.into())
    }

    #[convex_macro::instrument_future]
    async fn async_syscall_storageDelete(&self, args: JsonValue) -> anyhow::Result<JsonValue> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct StorageDeleteArgs {
            storage_id: String,
        }
        let storage_id: FileStorageId = with_argument_error("storage.delete", || {
            let StorageDeleteArgs { storage_id } = serde_json::from_value(args)?;
            storage_id.parse().context(ArgName("storageId"))
        })?;

        self.action_callbacks
            .storage_delete(self.identity.clone(), self.component_id(), storage_id)
            .await?;

        Ok(JsonValue::Null)
    }

    #[convex_macro::instrument_future]
    async fn async_syscall_storageGetMetadata(&self, args: JsonValue) -> anyhow::Result<JsonValue> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct StorageGetMetadataArgs {
            storage_id: String,
        }
        let storage_id: FileStorageId = with_argument_error("storage.getMetadata", || {
            let StorageGetMetadataArgs { storage_id } = serde_json::from_value(args)?;
            storage_id.parse().context(ArgName("storageId"))
        })?;

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct FileMetadataJson {
            storage_id: String,
            sha256: String,
            size: i64,
            content_type: Option<String>,
        }
        let file_metadata = self
            .action_callbacks
            .storage_get_file_entry(self.identity.clone(), self.component_id(), storage_id)
            .await?
            .map(|(_, entry)| {
                // NB: `storage_key is an internal field that we shouldn't return in syscalls.
                FileMetadataJson {
                    storage_id: entry.storage_id.to_string(),
                    // TODO(CX-5533) use base64 for consistency.
                    sha256: entry.sha256.as_hex(),
                    size: entry.size,
                    content_type: entry.content_type,
                }
            });
        Ok(serde_json::to_value(file_metadata)?)
    }

    async fn async_syscall_createFunctionHandle(
        &self,
        args: JsonValue,
    ) -> anyhow::Result<JsonValue> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct CreateFunctionHandleArgs {
            name: Option<String>,
            function_handle: Option<String>,
            reference: Option<String>,
        }
        let CreateFunctionHandleArgs {
            name,
            function_handle,
            reference,
        } = with_argument_error("createFunctionHandle", || Ok(serde_json::from_value(args)?))?;
        let function_path = match function_handle {
            Some(function_handle) => {
                return Ok(serde_json::to_value(function_handle)?);
            },
            None => {
                let reference = parse_name_or_reference("createFunctionHandle", name, reference)?;
                self.resolve_function(&reference)?
            },
        };
        let handle = self
            .action_callbacks
            .create_function_handle(self.identity.clone(), function_path)
            .await?;
        Ok(serde_json::to_value(String::from(handle))?)
    }
}

/// The service a `1.0/createServiceToken` or `1.0/getServiceUrl` syscall
/// names.
#[derive(Deserialize, Debug, PartialEq, Eq)]
enum Service {
    #[serde(rename = "ai-gateway")]
    AiGateway,
}

#[derive(Deserialize)]
struct CreateServiceTokenArgs {
    service: Service,
}

#[derive(Deserialize)]
struct GetServiceUrlArgs {
    service: Service,
}

pub fn parse_name_or_reference(
    function_name: &str,
    name: Option<String>,
    reference: Option<String>,
) -> anyhow::Result<Reference> {
    with_argument_error(function_name, || match (name, reference) {
        (Some(name), _) => Ok(Reference::Function(name.parse()?)),
        (_, Some(reference)) => Ok(reference.parse()?),
        _ => anyhow::bail!("Missing required argument 'name'"),
    })
}
