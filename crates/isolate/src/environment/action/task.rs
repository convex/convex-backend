//! Our action runtime runs "tasks" asynchronously, which either be
//! async syscalls or async ops.

use common::{
    fastrace_helpers::EncodedSpan,
    runtime::UnixTimestamp,
};
use deno_core::{
    serde_v8,
    v8,
    ToJsBuffer,
};
use serde::Serialize;
use serde_json::Value as JsonValue;
use value::{
    heap_size::HeapSize,
    id_v6::DeveloperDocumentId,
};

use crate::{
    environment::{
        helpers::syscall_error::{
            syscall_description_for_error,
            syscall_name_for_error,
        },
        AsyncOpRequest,
    },
    http::HttpResponseV8,
};

pub struct TaskRequest {
    pub task_id: TaskId,
    pub variant: TaskRequestEnum,
    pub parent_trace: EncodedSpan,
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug, derive_more::Display)]
pub struct TaskId(pub usize);

impl TaskId {
    pub fn increment(&mut self) -> Self {
        let task_id = *self;
        self.0 += 1;
        task_id
    }
}

pub enum TaskRequestEnum {
    AsyncSyscall { name: String, args: JsonValue },
    AsyncOp(AsyncOpRequest),
}

impl TaskRequestEnum {
    pub fn to_type(&self) -> TaskType {
        match self {
            TaskRequestEnum::AsyncSyscall { name, .. } => TaskType::Syscall(name.clone()),
            TaskRequestEnum::AsyncOp(AsyncOpRequest::Fetch { .. }) => TaskType::Fetch,
            TaskRequestEnum::AsyncOp(AsyncOpRequest::ParseMultiPart { .. }) => {
                TaskType::ParseMultiPart
            },
            TaskRequestEnum::AsyncOp(AsyncOpRequest::Sleep { .. }) => TaskType::Sleep,
            TaskRequestEnum::AsyncOp(AsyncOpRequest::StorageStore { .. }) => TaskType::StorageStore,
            TaskRequestEnum::AsyncOp(AsyncOpRequest::StorageGet { .. }) => TaskType::StorageGet,
            TaskRequestEnum::AsyncOp(AsyncOpRequest::SendStream { .. }) => TaskType::SendStream,
        }
    }

    pub fn name_for_error(&self) -> &'static str {
        match self {
            TaskRequestEnum::AsyncSyscall { name, .. } => syscall_name_for_error(name),
            TaskRequestEnum::AsyncOp(op) => op.name_for_error(),
        }
    }

    pub fn description_for_error(&self) -> String {
        match self {
            TaskRequestEnum::AsyncSyscall { name, .. } => syscall_description_for_error(name),
            TaskRequestEnum::AsyncOp(op) => op.description_for_error(),
        }
    }
}

pub enum TaskType {
    Syscall(String),
    Fetch,
    ParseMultiPart,
    Sleep,
    StorageStore,
    StorageGet,
    SendStream,
}

fn syscall_display_name(syscall: &str) -> String {
    syscall
        .replacen("1.0/", "", 1)
        .replacen("actions/", "", 1)
        .replacen("httpEndpoint/", "", 1)
        .replacen("shallowMerge", "patch", 1)
        .replacen("queryPage", "paginate", 1)
}

impl TaskType {
    pub fn name_when_dangling(&self) -> String {
        match self {
            TaskType::Syscall(syscall) => syscall_display_name(syscall),
            TaskType::Fetch => "fetch".to_string(),
            TaskType::ParseMultiPart => "formData".to_string(),
            TaskType::StorageStore => "storage.store".to_string(),
            TaskType::StorageGet => "storage.get".to_string(),
            TaskType::SendStream => "ReadableStream".to_string(),
            // Sleeps cannot actually be dangling, but we handle it just in case.
            TaskType::Sleep => "setTimeout".to_string(),
        }
    }
}

pub enum TaskResponse {
    TaskDone {
        task_id: TaskId,
        variant: anyhow::Result<TaskResponseEnum>,
    },
    StreamExtend {
        stream_id: uuid::Uuid,
        chunk: anyhow::Result<Option<bytes::Bytes>>,
    },
}

#[derive(Clone, Debug)]
pub enum TaskResponseEnum {
    Syscall(String),
    Fetch(HttpResponseV8),
    ParseMultiPart(Vec<FormPart>),
    Sleep(UnixTimestamp),
    StorageStore(DeveloperDocumentId),
    StorageGet(Option<FileResponse>),
}

impl HeapSize for TaskResponseEnum {
    fn heap_size(&self) -> usize {
        match self {
            Self::Syscall(result) => result.heap_size(),
            Self::Fetch(response) => response.heap_size(),
            Self::ParseMultiPart(parts) => parts.iter().map(HeapSize::heap_size).sum(),
            Self::Sleep(_) | Self::StorageStore(_) => 0,
            Self::StorageGet(file) => file.heap_size(),
        }
    }
}

impl TaskResponseEnum {
    pub fn into_v8<'s, 'i>(
        self,
        scope: &mut v8::PinScope<'s, 'i>,
    ) -> anyhow::Result<v8::Local<'s, v8::Value>> {
        let value_v8 = match self {
            Self::Fetch(response) => serde_v8::to_v8(scope, response)?,
            Self::Syscall(s) => serde_v8::to_v8(scope, s)?,
            Self::ParseMultiPart(parts) => serde_v8::to_v8(scope, parts)?,
            Self::Sleep(_) => serde_v8::to_v8(scope, ())?,
            Self::StorageStore(storage_id) => serde_v8::to_v8(scope, storage_id.to_string())?,
            Self::StorageGet(file_response) => serde_v8::to_v8(scope, file_response)?,
        };
        Ok(value_v8)
    }
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileResponse {
    pub body_stream_id: uuid::Uuid,
    pub content_length: u64,
    pub content_type: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormPart {
    pub name: String,
    pub file: Option<FormPartFile>,
    pub text: Option<String>,
}

impl HeapSize for FormPart {
    fn heap_size(&self) -> usize {
        self.name.heap_size() + self.file.heap_size() + self.text.heap_size()
    }
}

impl HeapSize for FormPartFile {
    fn heap_size(&self) -> usize {
        self.content_type.heap_size() + self.data.heap_size() + self.file_name.heap_size()
    }
}

impl HeapSize for FileResponse {
    fn heap_size(&self) -> usize {
        self.content_type.heap_size()
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormPartFile {
    pub content_type: Option<String>,
    #[serde(serialize_with = "serialize_to_js_buffer")]
    pub data: bytes::Bytes,
    pub file_name: Option<String>,
}

/// Sends `data` to JS as a `Uint8Array`.
fn serialize_to_js_buffer<S: serde::Serializer>(
    data: &bytes::Bytes,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    ToJsBuffer::from(data.to_vec()).serialize(serializer)
}
