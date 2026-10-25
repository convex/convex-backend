//! [`HostBackend`] that runs a guest's tasks on the shared
//! [`TaskExecutor`](crate::environment::action::task_executor::TaskExecutor),
//! i.e. the same implementation V8 actions use for `ctx.runQuery`, `fetch`,
//! storage and the scheduler.

use std::{
    collections::{
        BTreeMap,
        BTreeSet,
        HashMap,
        HashSet,
        VecDeque,
    },
    pin::Pin,
    str::FromStr,
    time::Duration,
};

use async_trait::async_trait;
use common::{
    errors::{
        report_error_sync,
        JsError,
    },
    fastrace_helpers::EncodedSpan,
    http::HttpRequestStream,
    knobs::MAX_LOG_LINES_V8_ACTIONS,
    log_lines::{
        LogLevel,
        LogLine,
    },
    runtime::{
        Runtime,
        SpawnHandle,
        UnixTimestamp,
    },
    sync::spsc,
};
use errors::ErrorMetadataAnyhowExt as _;
use http::{
    HeaderMap,
    HeaderName,
    HeaderValue,
    Method,
};
use serde_json::{
    json,
    Value as JsonValue,
};
use tokio::sync::mpsc;
use url::Url;
use value::{
    identifier::Identifier,
    ConvexValue,
};

use super::core::{
    new_chunk_event,
    HostBackend,
    PollOutcome,
};
use crate::environment::{
    action::{
        task::TaskRequest,
        TaskId,
        TaskRequestEnum,
        TaskResponse,
        TaskResponseEnum,
    },
    helpers::log_overflow_log_line,
    AsyncOpRequest,
};

static CHUNK_TRACE: std::sync::LazyLock<bool> =
    std::sync::LazyLock::new(|| std::env::var_os("CONVEX_RUST_TRACE_CHUNKS").is_some());

/// Largest binary chunk event built by coalescing adjacent chunks.
const MAX_COALESCED_CHUNK: usize = 1 << 20;

/// An event ready for the guest.
enum Ready {
    Json(String),
    Chunk(Vec<u8>),
}

enum TaskKind {
    Syscall,
    Fetch { stream: uuid::Uuid },
    Sleep,
    StorageStore,
    StorageGet { stream: uuid::Uuid },
}

pub struct ActionBackend<RT: Runtime> {
    pub rt: RT,
    pub pending_task_sender: spsc::UnboundedSender<TaskRequest>,
    pub task_responses: mpsc::UnboundedReceiver<TaskResponse>,
    pub running_tasks: Option<Box<dyn SpawnHandle>>,
    pub log_line_sender: mpsc::UnboundedSender<LogLine>,
    /// `None` for root-component functions.
    pub component_arguments: Option<BTreeMap<Identifier, ConvexValue>>,
    pub next_task_id: TaskId,
    pub total_log_lines: usize,
    kinds: HashMap<usize, TaskKind>,
    open_streams: HashSet<uuid::Uuid>,
    ready: VecDeque<Ready>,
    /// A response read while coalescing that did not belong to the chunk
    /// being built; always consumed before the channel is read again.
    stash: Option<TaskResponse>,
    /// Guest sleeps, timed here instead of on the shared executor (see `submit`).
    sleeps: BTreeSet<(UnixTimestamp, usize)>,
}

impl<RT: Runtime> ActionBackend<RT> {
    pub fn new(
        rt: RT,
        pending_task_sender: spsc::UnboundedSender<TaskRequest>,
        task_responses: mpsc::UnboundedReceiver<TaskResponse>,
        running_tasks: Box<dyn SpawnHandle>,
        log_line_sender: mpsc::UnboundedSender<LogLine>,
        component_arguments: Option<BTreeMap<Identifier, ConvexValue>>,
    ) -> Self {
        Self {
            rt,
            pending_task_sender,
            task_responses,
            running_tasks: Some(running_tasks),
            log_line_sender,
            component_arguments,
            next_task_id: TaskId(0),
            total_log_lines: 0,
            kinds: HashMap::new(),
            open_streams: HashSet::new(),
            ready: VecDeque::new(),
            stash: None,
            sleeps: BTreeSet::new(),
        }
    }

    /// Stops running tasks and drops any that haven't started (what V8 does
    /// for dangling promises at the end of an action).
    pub fn shutdown_tasks(&mut self) {
        self.pending_task_sender.close();
        if let Some(mut running_tasks) = self.running_tasks.take() {
            running_tasks.shutdown();
        }
    }

    fn outstanding(&self) -> bool {
        !self.kinds.is_empty() || !self.open_streams.is_empty()
    }

    fn parse_request(&mut self, req: &JsonValue) -> Result<(TaskRequestEnum, TaskKind), String> {
        let ty = req
            .get("type")
            .and_then(JsonValue::as_str)
            .ok_or("missing request type")?;
        let str_field = |name: &str| -> Result<&str, String> {
            req.get(name)
                .and_then(JsonValue::as_str)
                .ok_or_else(|| format!("{ty}: missing string field `{name}`"))
        };
        let body_field = || -> Result<Option<bytes::Bytes>, String> {
            match req.get("body") {
                None | Some(JsonValue::Null) => Ok(None),
                Some(JsonValue::String(b)) => base64::decode(b)
                    .map(|b| Some(bytes::Bytes::from(b)))
                    .map_err(|e| format!("{ty}: invalid base64 body: {e}")),
                Some(_) => Err(format!("{ty}: `body` must be a base64 string or null")),
            }
        };
        match ty {
            "syscall" => {
                let name = str_field("name")?.to_string();
                let args = req.get("args").cloned().unwrap_or_else(|| json!({}));
                Ok((
                    TaskRequestEnum::AsyncSyscall { name, args },
                    TaskKind::Syscall,
                ))
            },
            "fetch" => {
                let url = Url::parse(str_field("url")?).map_err(|e| format!("fetch: {e}"))?;
                let method = Method::from_str(
                    req.get("method")
                        .and_then(JsonValue::as_str)
                        .unwrap_or("GET"),
                )
                .map_err(|e| format!("fetch: bad method: {e}"))?;
                let mut headers = HeaderMap::new();
                if let Some(pairs) = req.get("headers").and_then(JsonValue::as_array) {
                    for pair in pairs {
                        let (Some(k), Some(v)) = (
                            pair.get(0).and_then(JsonValue::as_str),
                            pair.get(1).and_then(JsonValue::as_str),
                        ) else {
                            return Err("fetch: headers must be [name, value] pairs".into());
                        };
                        headers.append(
                            HeaderName::from_str(k)
                                .map_err(|e| format!("fetch: bad header name {k}: {e}"))?,
                            HeaderValue::from_str(v)
                                .map_err(|e| format!("fetch: bad header value for {k}: {e}"))?,
                        );
                    }
                }
                let body = body_field()?.map(|b| {
                    Box::pin(futures::stream::once(async move { Ok(b) }))
                        as Pin<
                            Box<
                                dyn futures::Stream<Item = anyhow::Result<bytes::Bytes>>
                                    + Sync
                                    + Send,
                            >,
                        >
                });
                let stream = uuid::Uuid::new_v4();
                let request = HttpRequestStream {
                    headers,
                    url,
                    method,
                    body,
                    // No request cancellation in v1.
                    signal: Box::pin(futures::future::pending()),
                };
                Ok((
                    TaskRequestEnum::AsyncOp(AsyncOpRequest::Fetch {
                        request,
                        response_body_stream_id: stream,
                    }),
                    TaskKind::Fetch { stream },
                ))
            },
            "sleep" => {
                let ms = req
                    .get("ms")
                    .and_then(JsonValue::as_u64)
                    .ok_or("sleep: missing integer field `ms`")?;
                Ok((
                    TaskRequestEnum::AsyncOp(AsyncOpRequest::Sleep {
                        name: "setTimeout".to_string(),
                        until: self.rt.unix_timestamp() + Duration::from_millis(ms),
                    }),
                    TaskKind::Sleep,
                ))
            },
            "storageStore" => {
                let body = body_field()?.unwrap_or_default();
                let content_type = req
                    .get("contentType")
                    .and_then(JsonValue::as_str)
                    .map(str::to_string);
                let content_length = Some(body.len().to_string());
                let (mut sender, body_stream) = spsc::unbounded_channel();
                sender
                    .send(Ok(body))
                    .map_err(|_| "storageStore: body channel closed".to_string())?;
                drop(sender);
                Ok((
                    TaskRequestEnum::AsyncOp(AsyncOpRequest::StorageStore {
                        body_stream,
                        content_type,
                        content_length,
                        digest: None,
                    }),
                    TaskKind::StorageStore,
                ))
            },
            "storageGet" => {
                let storage_id = str_field("storageId")?.to_string();
                let stream = uuid::Uuid::new_v4();
                Ok((
                    TaskRequestEnum::AsyncOp(AsyncOpRequest::StorageGet {
                        storage_id,
                        stream_id: stream,
                    }),
                    TaskKind::StorageGet { stream },
                ))
            },
            other => Err(format!("unknown request type {other:?}")),
        }
    }

    /// Translates an executor response into an ABI event (None if the response
    /// is for a task the guest did not start).
    fn translate(&mut self, response: TaskResponse) -> Option<Ready> {
        match response {
            TaskResponse::TaskDone { task_id, variant } => {
                let kind = self.kinds.remove(&task_id.0)?;
                let id = task_id.0;
                let event = match variant {
                    Ok(v) => match ok_value(v) {
                        Ok(ok) => {
                            // A `None` file / failed lookup opens no stream.
                            if matches!(kind, TaskKind::StorageGet { .. }) && ok.is_null()
                                && let TaskKind::StorageGet { stream } = kind {
                                    self.open_streams.remove(&stream);
                                }
                            json!({"type": "done", "taskId": id, "ok": ok})
                        },
                        Err(e) => {
                            self.close_stream_of(&kind);
                            json!({"type": "done", "taskId": id, "error": error_json(e)})
                        },
                    },
                    Err(e) => {
                        self.close_stream_of(&kind);
                        json!({"type": "done", "taskId": id, "error": error_json(e)})
                    },
                };
                Some(Ready::Json(event.to_string()))
            },
            TaskResponse::StreamExtend { stream_id, chunk } => {
                let event = match chunk {
                    Ok(Some(bytes)) => return Some(Ready::Chunk(self.coalesce(stream_id, bytes))),
                    Ok(None) => {
                        self.open_streams.remove(&stream_id);
                        json!({"type": "chunk", "streamId": stream_id, "end": true})
                    },
                    Err(e) => {
                        self.open_streams.remove(&stream_id);
                        json!({
                            "type": "chunk",
                            "streamId": stream_id,
                            "error": error_json(e)["message"],
                        })
                    },
                };
                Some(Ready::Json(event.to_string()))
            },
        }
    }

    /// Next executor response: the stashed one, else whatever is queued.
    fn try_next(&mut self) -> Option<TaskResponse> {
        self.stash.take().or_else(|| self.task_responses.try_recv().ok())
    }

    /// Builds one binary chunk event from `first` and the chunks of the same
    /// stream queued right behind it, up to [`MAX_COALESCED_CHUNK`].
    fn coalesce(&mut self, stream_id: uuid::Uuid, first: bytes::Bytes) -> Vec<u8> {
        let id = stream_id.to_string();
        let mut ev = new_chunk_event(&id, &first, 0);
        let header = ev.len() - first.len();
        let mut parts = 1;
        while ev.len() - header < MAX_COALESCED_CHUNK {
            match self.try_next() {
                Some(TaskResponse::StreamExtend {
                    stream_id: next,
                    chunk: Ok(Some(more)),
                }) if next == stream_id => {
                    ev.extend_from_slice(&more);
                    parts += 1;
                },
                Some(other) => {
                    self.stash = Some(other);
                    break;
                },
                None => break,
            }
        }
        if *CHUNK_TRACE {
            eprintln!("rust chunk event: {} bytes from {parts} chunk(s)", ev.len() - header);
        }
        ev
    }

    fn close_stream_of(&mut self, kind: &TaskKind) {
        if let TaskKind::Fetch { stream } | TaskKind::StorageGet { stream } = kind {
            self.open_streams.remove(stream);
        }
    }
}

impl<RT: Runtime> Drop for ActionBackend<RT> {
    fn drop(&mut self) {
        self.shutdown_tasks();
    }
}

/// The `ok` payload of a finished task, per ABI.md.
fn ok_value(v: TaskResponseEnum) -> anyhow::Result<JsonValue> {
    Ok(match v {
        TaskResponseEnum::Syscall(s) => serde_json::from_str(&s)?,
        TaskResponseEnum::Fetch(resp) => {
            // `HttpResponseV8` serializes as
            // {streamId, status, statusText, headerPairs, url}.
            let r = serde_json::to_value(&resp)?;
            json!({
                "status": r["status"],
                "statusText": r["statusText"].as_str().unwrap_or(""),
                "headers": r["headerPairs"],
                "url": r["url"].as_str().unwrap_or(""),
                "bodyStreamId": r["streamId"],
            })
        },
        TaskResponseEnum::Sleep(_) => JsonValue::Null,
        TaskResponseEnum::StorageStore(id) => JsonValue::String(id.to_string()),
        TaskResponseEnum::StorageGet(None) => JsonValue::Null,
        TaskResponseEnum::StorageGet(Some(file)) => serde_json::to_value(&file)?,
        TaskResponseEnum::ParseMultiPart(_) => {
            anyhow::bail!("formData is not supported in \"use rust\" actions")
        },
    })
}

/// `{"message": .., "data": ..?}`, built the way V8 builds the error a
/// rejected task promise carries (`resolve_promise_allow_all_errors`).
fn error_json(mut e: anyhow::Error) -> JsonValue {
    if !e.is_deterministic_user_error() {
        report_error_sync(&mut e);
    }
    let message = e.user_facing_message();
    let data = e
        .downcast_ref::<JsError>()
        .and_then(|j| j.custom_data.as_ref())
        .map(ConvexValue::to_internal_json);
    match data {
        Some(data) => json!({"message": message, "data": data}),
        None => json!({"message": message}),
    }
}

#[async_trait]
impl<RT: Runtime> HostBackend for ActionBackend<RT> {
    fn submit(&mut self, request: JsonValue) -> Result<i64, String> {
        // A rejected submit still consumes an id (NATIVE.md §2.3), so the guest and host counters agree.
        let task_id = self.next_task_id.increment();
        // Sleeps never reach the shared executor: its `run_sleep` keeps
        // setTimeout ordering by re-polling every 5 ms while an earlier sleep
        // is unresolved, which chains into ~25 ms stalls for a fan-out of
        // sleeps. A guest has no setTimeout ordering contract, so `poll`
        // times them itself.
        if request.get("type").and_then(JsonValue::as_str) == Some("sleep") {
            let ms = request
                .get("ms")
                .and_then(JsonValue::as_u64)
                .ok_or("sleep: missing integer field `ms`")?;
            let until = self.rt.unix_timestamp() + Duration::from_millis(ms);
            self.sleeps.insert((until, task_id.0));
            self.kinds.insert(task_id.0, TaskKind::Sleep);
            return i64::try_from(task_id.0).map_err(|e| e.to_string());
        }
        let (variant, kind) = self.parse_request(&request)?;
        self.pending_task_sender
            .send(TaskRequest {
                task_id,
                variant,
                parent_trace: EncodedSpan::from_parent(),
            })
            .map_err(|_| "task executor went away".to_string())?;
        if let TaskKind::Fetch { stream } | TaskKind::StorageGet { stream } = &kind {
            self.open_streams.insert(*stream);
        }
        self.kinds.insert(task_id.0, kind);
        i64::try_from(task_id.0).map_err(|e| e.to_string())
    }

    async fn poll(&mut self, block: bool) -> PollOutcome {
        loop {
            if let Some(ev) = self.ready.pop_front() {
                return match ev {
                    Ready::Json(ev) => PollOutcome::Event(ev),
                    Ready::Chunk(ev) => PollOutcome::Chunk(ev),
                };
            }
            if let Some(response) = self.try_next() {
                if let Some(ev) = self.translate(response) {
                    self.ready.push_back(ev);
                }
                continue;
            }
            if let Some(&(until, _)) = self.sleeps.first() {
                let now = self.rt.unix_timestamp();
                if until <= now {
                    let (_, id) = self.sleeps.pop_first().expect("checked non-empty");
                    if self.kinds.remove(&id).is_some() {
                        self.ready
                            .push_back(Ready::Json(
                                json!({"type": "done", "taskId": id, "ok": null}).to_string(),
                            ));
                    }
                    continue;
                }
                if !block {
                    return PollOutcome::Empty;
                }
                let wait = self.rt.wait(until - now);
                tokio::select! {
                    response = self.task_responses.recv() => match response {
                        Some(response) => {
                            if let Some(ev) = self.translate(response) {
                                self.ready.push_back(ev);
                            }
                        },
                        // Executor gone; only sleeps remain.
                        None => self.rt.wait(until - now).await,
                    },
                    _ = wait => (),
                }
                continue;
            }
            if !block {
                return PollOutcome::Empty;
            }
            if !self.outstanding() {
                return PollOutcome::NoTasks;
            }
            match self.task_responses.recv().await {
                Some(response) => {
                    if let Some(ev) = self.translate(response) {
                        self.ready.push_back(ev);
                    }
                },
                // The executor is gone; nothing will ever arrive.
                None => return PollOutcome::NoTasks,
            }
        }
    }

    fn outstanding_tasks(&self) -> usize {
        self.kinds.len()
    }

    fn sync_syscall(&mut self, op: &str, args: JsonValue) -> JsonValue {
        match op {
            "now" => {
                json!({"ok": self.rt.unix_timestamp().as_secs_f64() * 1000.0})
            },
            "componentArgument" => {
                let Some(name) = args.get("name").and_then(JsonValue::as_str) else {
                    return json!({"error": {"message": "componentArgument: missing `name`"}});
                };
                let Some(component_arguments) = &self.component_arguments else {
                    return json!({"error": {
                        "message": "Component arguments are not available within the app"
                    }});
                };
                match Identifier::from_str(name)
                    .ok()
                    .and_then(|id| component_arguments.get(&id))
                {
                    Some(v) => json!({"ok": v.to_internal_json()}),
                    None => json!({"ok": null}),
                }
            },
            _ => json!({"error": {"message": format!("unknown sync op {op}")}}),
        }
    }

    fn log(&mut self, level: i32, message: String) {
        let level = match level {
            0 => LogLevel::Debug,
            2 => LogLevel::Warn,
            3 => LogLevel::Error,
            _ => LogLevel::Log,
        };
        let max_log_lines = *MAX_LOG_LINES_V8_ACTIONS;
        // - 1 to reserve for the [ERROR] log line
        match self.total_log_lines.cmp(&(max_log_lines - 1)) {
            std::cmp::Ordering::Less => {
                let _ = self.log_line_sender.send(LogLine::new_developer_log_line(
                    level,
                    vec![message],
                    self.rt.unix_timestamp(),
                ));
                self.total_log_lines += 1;
            },
            std::cmp::Ordering::Equal => {
                let _ = self.log_line_sender.send(log_overflow_log_line(
                    max_log_lines,
                    self.rt.unix_timestamp(),
                ));
                self.total_log_lines += 1;
            },
            std::cmp::Ordering::Greater => (),
        }
    }
}
