use common::sync::spsc;
use deno_core::{
    serde_v8,
    v8::{
        self,
    },
};

use super::{
    stream::listen_to_stream,
    V8OpProvider,
};
use crate::environment::AsyncOpRequest;

pub fn async_op_storage_store<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    args: v8::FunctionCallbackArguments,
    resolver: v8::Global<v8::PromiseResolver>,
) -> anyhow::Result<()> {
    let stream_id = serde_v8::from_v8(&mut provider.scope(), args.get(1))?;
    let body_receiver = match stream_id {
        Some(stream_id) => listen_to_stream(provider, stream_id)?,
        None => spsc::unbounded_channel().1,
    };
    let content_type: Option<String> = serde_v8::from_v8(&mut provider.scope(), args.get(2))?;
    let content_type = content_type.filter(|ct| !ct.is_empty());
    let content_length = serde_v8::from_v8(&mut provider.scope(), args.get(3))?;
    let digest = serde_v8::from_v8(&mut provider.scope(), args.get(4))?;

    provider.start_async_op(
        AsyncOpRequest::StorageStore {
            body_stream: body_receiver,
            content_type,
            content_length,
            digest,
        },
        resolver,
    )
}

pub fn async_op_storage_get<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    args: v8::FunctionCallbackArguments,
    resolver: v8::Global<v8::PromiseResolver>,
) -> anyhow::Result<()> {
    let storage_id = serde_v8::from_v8(&mut provider.scope(), args.get(1))?;
    let stream_id = provider.create_stream()?;
    provider.start_async_op(
        AsyncOpRequest::StorageGet {
            storage_id,
            stream_id,
        },
        resolver,
    )
}
