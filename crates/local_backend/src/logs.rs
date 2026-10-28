use std::time::Duration;

use anyhow::Context;
use application::function_log::{
    FunctionExecution,
    FunctionExecutionPart,
    UdfParams,
};
use axum::response::IntoResponse;
use common::{
    http::{
        extract::{
            Json,
            MtState,
            Query,
        },
        ExtractClientVersion,
        HttpResponseError,
    },
    log_streaming::{
        FunctionExecutionJson,
        StreamFunctionLogs,
        StreamUdfExecutionQueryArgs,
        StreamUdfExecutionResponse,
    },
    version::ClientType,
    RequestId,
};
use errors::ErrorMetadata;
use futures::FutureExt;
use serde_json::Value as JsonValue;

use crate::{
    authentication::ExtractIdentity,
    LocalAppState,
};

pub async fn stream_udf_execution(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Query(query_args): Query<StreamUdfExecutionQueryArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let function_log = st.application.function_log(&identity)?;
    let entries_future = function_log.stream(query_args.cursor);
    let mut zombify_rx = st.zombify_rx.clone();
    futures::select_biased! {
        entries_result = entries_future.fuse() => {
            let (log_entries, new_cursor) = entries_result;
            let entries = log_entries
                .into_iter()
                .map(|e| execution_to_json(e, false))
                .try_collect()?;
            let response = StreamUdfExecutionResponse {
                entries,
                new_cursor,
            };
            Ok(Json(response))
        },
        _ = tokio::time::sleep(Duration::from_secs(60)).fuse() => {
            let response = StreamUdfExecutionResponse {
                entries: vec![],
                new_cursor: query_args.cursor,
            };
            Ok(Json(response))
        },
        _ = zombify_rx.recv().fuse() => {
            // Return an error so the client reconnects after we come back up.
            Err(anyhow::anyhow!(ErrorMetadata::operational_internal_server_error()).context("Shutting down long poll request").into())
        },
    }
}

// Streams log lines + function completion events.
// Log lines can either appear in the completion (mutations, queries) or as
// separate messages (actions, HTTP actions), but will only appear once.
//
// If (session_id, client_request_counter) is provided, the results will be
// filtered to events from the root execution of the corresponding request.
pub async fn stream_function_logs(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractClientVersion(client_version): ExtractClientVersion,
    Query(query_args): Query<StreamFunctionLogs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let function_log = st.application.function_log(&identity)?;
    let entries_future = function_log.stream_parts(query_args.cursor);
    let mut zombify_rx = st.zombify_rx.clone();
    let request_id = match (query_args.session_id, query_args.client_request_counter) {
        (Some(session_id), Some(client_request_counter)) => Some(RequestId::new_for_ws_session(
            session_id.parse().context("Invalid session ID")?,
            client_request_counter,
        )),
        _ => None,
    };
    // As of writing, this endpoint is only used by the CLI and dashboard, both of
    // which support either unstructured `string` log lines or structured log
    // lines.
    let supports_structured_log_lines = match client_version.client() {
        ClientType::CLI => true,
        ClientType::Dashboard => true,
        ClientType::NPM
        | ClientType::Actions
        | ClientType::Python
        | ClientType::Rust
        | ClientType::CreateConvex
        | ClientType::StreamingImport
        | ClientType::AirbyteExport
        | ClientType::FivetranImport
        | ClientType::FivetranExport
        | ClientType::Swift
        | ClientType::Kotlin
        | ClientType::Unrecognized(_) => false,
    };
    futures::select_biased! {
        entries_result = entries_future.fuse() => {
            let (log_entries, new_cursor) = entries_result;
            let entries = log_entries
                .into_iter()
                .filter(|e| {
                    let Some(request_id_filter) = request_id.as_ref() else {
                        return true
                    };
                    match e {
                        FunctionExecutionPart::Completion(c) => {
                            &c.context.request_id == request_id_filter && c.context.is_root()
                        },
                        FunctionExecutionPart::Progress(c) => {
                            &c.event_source.context.request_id == request_id_filter
                                && c.event_source.context.is_root()
                        }
                    }
                })
                .map(|e| {
                    let json = match e {
                        FunctionExecutionPart::Completion(c) => {
                            execution_to_json(c, supports_structured_log_lines)?
                        },
                        FunctionExecutionPart::Progress(c) => {
                            FunctionExecutionJson::Progress {
                                udf_type: c.event_source.udf_type.into(),
                                component_path: c.event_source.component_path.serialize(),
                                identifier: c.event_source.udf_path,
                                timestamp: c.function_start_timestamp.as_secs_f64(),
                                log_lines: c.log_lines.to_jsons(
                                    supports_structured_log_lines,
                                    false,
                                )?,
                                request_id: c.event_source.context.request_id.to_string(),
                                execution_id: c.event_source.context.execution_id.to_string()
                            }
                        }
                    };

                    Ok(json)
                })
                .collect::<anyhow::Result<_>>()?;
            let response = StreamUdfExecutionResponse {
                entries,
                new_cursor,
            };
            Ok(Json(response))
        },
        _ = tokio::time::sleep(Duration::from_secs(60)).fuse() => {
            let response = StreamUdfExecutionResponse {
                entries: vec![],
                new_cursor: query_args.cursor,
            };
            Ok(Json(response))
        },
        _ = zombify_rx.recv().fuse() => {
            // Return an error so the client reconnects after we come back up.
            Err(anyhow::anyhow!(ErrorMetadata::operational_internal_server_error()).context("Shutting down long poll request").into())
        },
    }
}

fn usage_stats_to_json(
    stats: &usage_tracking::AggregatedFunctionUsageStats,
    memory_used_mb: u64,
) -> common::log_streaming::UsageStatsJson {
    common::log_streaming::UsageStatsJson {
        database_read_bytes: stats.database_read_bytes,
        database_write_bytes: stats.database_write_bytes,
        database_io_read_bytes: stats.database_io_read_bytes,
        database_io_write_bytes: stats.database_io_write_bytes,
        database_read_documents: stats.database_read_documents,
        database_write_documents: stats.database_write_documents,
        database_write_index_rows: stats.database_write_index_rows,
        storage_read_bytes: stats.storage_read_bytes,
        storage_write_bytes: stats.storage_write_bytes,
        vector_index_read_bytes: stats.vector_index_read_bytes,
        vector_index_write_bytes: stats.vector_index_write_bytes,
        text_index_query_bytes: stats.text_index_query_bytes,
        text_index_write_query_bytes: stats.text_index_write_query_bytes,
        vector_index_read_query_bytes: stats.vector_index_read_query_bytes,
        vector_index_write_query_bytes: stats.vector_index_write_query_bytes,
        network_egress_bytes: stats.network_egress_bytes,
        memory_used_mb,
    }
}

fn execution_to_json(
    execution: FunctionExecution,
    supports_structured_log_lines: bool,
) -> anyhow::Result<FunctionExecutionJson> {
    let usage_stats_json = usage_stats_to_json(&execution.usage_stats, execution.memory_used_mb);
    let occ_info_json = execution
        .occ_info
        .map(common::log_streaming::OccInfoJson::from);
    let will_retry = execution.will_retry;
    let identity_type = execution.identity.tag().value.to_string();
    let environment = execution.environment.to_string();
    let execution_timestamp = execution.execution_timestamp.as_secs_f64();
    let user_execution_time = execution.user_execution_time.map(|d| d.as_secs_f64());
    let json = match execution.params {
        UdfParams::Function { error, identifier } => {
            let component_path = identifier.component.serialize();
            let identifier: String = identifier.udf_path.strip().into();
            FunctionExecutionJson::Completion {
                udf_type: execution.udf_type.into(),
                component_path,
                identifier,
                log_lines: execution
                    .log_lines
                    .to_jsons(supports_structured_log_lines, false)?,
                timestamp: execution.unix_timestamp.as_secs_f64(),
                cached_result: execution.cached_result,
                execution_time: execution.execution_time,
                user_execution_time,
                success: None,
                error: error.map(|e| e.to_string()),
                request_id: execution.context.request_id.to_string(),
                caller: execution.caller.to_string(),
                parent_execution_id: execution
                    .caller
                    .parent_execution_id()
                    .map(|id| id.to_string()),
                execution_id: execution.context.execution_id.to_string(),
                usage_stats: usage_stats_json,
                return_bytes: execution.return_bytes.map(|bytes| bytes as f64),
                occ_info: occ_info_json,
                will_retry,
                execution_timestamp,
                identity_type,
                environment,
            }
        },
        UdfParams::Http { result, identifier } => {
            let identifier: String = identifier.to_string();
            let (success, error) = match result {
                Ok(v) => (Some(JsonValue::from(v)), None),
                Err(e) => (None, Some(e)),
            };
            FunctionExecutionJson::Completion {
                udf_type: execution.udf_type.into(),
                component_path: None,
                identifier,
                log_lines: execution
                    .log_lines
                    .to_jsons(supports_structured_log_lines, false)?,
                timestamp: execution.unix_timestamp.as_secs_f64(),
                cached_result: execution.cached_result,
                execution_time: execution.execution_time,
                user_execution_time,
                caller: execution.caller.to_string(),
                parent_execution_id: execution
                    .caller
                    .parent_execution_id()
                    .map(|id| id.to_string()),
                success,
                error: error.map(|e| e.to_string()),
                request_id: execution.context.request_id.to_string(),
                execution_id: execution.context.execution_id.to_string(),
                usage_stats: usage_stats_json,
                return_bytes: None, // Not supported in HTTP actions
                occ_info: occ_info_json,
                will_retry,
                execution_timestamp,
                identity_type,
                environment,
            }
        },
    };
    Ok(json)
}
