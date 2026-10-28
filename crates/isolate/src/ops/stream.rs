use std::collections::BTreeMap;

use common::runtime::Runtime;
use deno_core::{
    serde_v8,
    v8::{
        self,
        scope,
    },
    ToJsBuffer,
};
use serde::Serialize;
use serde_bytes::ByteBuf;
use uuid::Uuid;

use super::V8OpProvider;
use crate::{
    environment::{
        helpers::resolve_promise,
        V8IsolateEnvironment,
    },
    execution_scope::ExecutionScope,
    request_scope::StreamListener,
};

pub fn async_op_stream_read_part<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    args: v8::FunctionCallbackArguments,
    resolver: v8::Global<v8::PromiseResolver>,
) -> anyhow::Result<()> {
    let stream_id = serde_v8::from_v8(&mut provider.scope(), args.get(1))?;
    provider.new_stream_listener(stream_id, StreamListener::JsPromise(resolver))
}

#[convex_macro::v8_op]
pub fn op_stream_create<'b, P: V8OpProvider<'b>>(provider: &mut P) -> anyhow::Result<Uuid> {
    provider.create_stream()
}

#[convex_macro::v8_op]
pub fn op_stream_extend<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    id: Uuid,
    bytes: Option<ByteBuf>,
    new_done: bool,
) -> anyhow::Result<()> {
    provider.extend_stream(id, bytes.map(|b| b.into_vec().into()), new_done)
}

impl<'a, 's: 'a, 'i: 'a, RT: Runtime, E: V8IsolateEnvironment<RT>>
    ExecutionScope<'a, 's, 'i, RT, E>
{
    pub fn error_stream(&mut self, id: uuid::Uuid, error: anyhow::Error) -> anyhow::Result<()> {
        let state = self.state_mut()?;
        state.streams.insert(id, Err(error));
        self.update_stream_listeners()
    }

    /// Call this when a stream has a new chunk or there is a new stream
    /// listener, to potentially notify the listeners.
    pub fn update_stream_listeners(&mut self) -> anyhow::Result<()> {
        #[derive(Serialize, Debug)]
        #[serde(rename_all = "camelCase")]
        struct JsStreamChunk {
            done: bool,
            value: Option<ToJsBuffer>,
        }
        loop {
            let state = self.state_mut()?;
            let mut ready = BTreeMap::new();
            for stream_id in state.stream_listeners.keys() {
                let chunk = state.streams.mutate(
                    stream_id,
                    |stream| -> anyhow::Result<Result<(Option<bytes::Bytes>, bool), ()>> {
                        let stream = stream
                            .ok_or_else(|| anyhow::anyhow!("listening on nonexistent stream"))?;
                        let result = match stream {
                            Ok(stream) => Ok((stream.parts.pop_front(), stream.done)),
                            Err(_) => Err(()),
                        };
                        Ok(result)
                    },
                )?;
                match chunk {
                    Err(_) => {
                        ready.insert(
                            *stream_id,
                            Err(state.streams.remove(stream_id).unwrap().unwrap_err()),
                        );
                    },
                    Ok((chunk, stream_done)) => {
                        if let Some(chunk) = chunk {
                            ready.insert(*stream_id, Ok(Some(chunk)));
                        } else if stream_done {
                            ready.insert(*stream_id, Ok(None));
                        }
                    },
                }
            }
            if ready.is_empty() {
                // Nothing to notify -- all caught up.
                return Ok(());
            }
            for (stream_id, update) in ready {
                if let Some(listener) = self.state_mut()?.stream_listeners.remove(&stream_id) {
                    match listener {
                        StreamListener::JsPromise(resolver) => {
                            scope!(let scope, &mut **self);
                            let result = match update {
                                Ok(update) => Ok(serde_v8::to_v8(
                                    scope,
                                    JsStreamChunk {
                                        done: update.is_none(),
                                        value: update.map(|chunk| chunk.to_vec().into()),
                                    },
                                )?),
                                Err(e) => Err(e),
                            };
                            resolve_promise(scope, resolver, result)?;
                        },
                        StreamListener::RustStream(mut stream) => match update {
                            Ok(None) => drop(stream),
                            Ok(Some(bytes)) => {
                                let _ = stream.send(Ok(bytes));
                                self.state_mut()?
                                    .stream_listeners
                                    .insert(stream_id, StreamListener::RustStream(stream));
                            },
                            Err(e) => {
                                let _ = stream.send(Err(e));
                                drop(stream);
                            },
                        },
                    }
                }
            }
        }
    }
}
