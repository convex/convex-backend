//! This module contains the implementation of both synchronous and
//! async ops. Unlike syscalls, these functions are present in *every*
//! environment, but the environment may decide not to implement their
//! functionality, causing a runtime error.

pub mod call;
mod console;
mod crypto;
mod database;
mod environment_variables;
mod errors;
mod http;
mod random;
mod storage;
mod stream;
mod structured_clone;
mod subtle_crypto;
mod text;
mod time;
mod validate_args;
mod validate_returns;

use std::{
    collections::BTreeMap,
    time::Duration,
};

use ::errors::ErrorMetadata;
use anyhow::anyhow;
use bytes::Bytes;
use call::{
    OpCall,
    V8Call,
    WasmCall,
};
use common::{
    log_lines::LogLevel,
    runtime::{
        Runtime,
        UnixTimestamp,
    },
    types::{
        EnvVarName,
        EnvVarValue,
    },
};
use deno_core::{
    v8,
    ModuleSpecifier,
};
use rand_chacha::ChaCha12Rng;
use sourcemap::SourceMap;
use structured_clone::op_structured_clone;
use uuid::Uuid;
use validate_returns::op_validate_returns_call;
use value::{
    heap_size::WithHeapSize,
    NamespacedTableMapping,
};

pub(crate) use self::errors::uncatchable_developer_error;
use self::{
    console::{
        op_console_message,
        op_console_time_end,
        op_console_time_log,
        op_console_time_start,
        op_console_trace,
    },
    crypto::{
        op_crypto_get_random_values_call,
        op_crypto_random_uuid_call,
    },
    database::op_get_table_mapping_call,
    environment_variables::op_environment_variables_get_call,
    errors::{
        op_error_stack,
        op_throw_uncatchable_developer_error,
    },
    http::{
        async_op_fetch,
        async_op_parse_multi_part,
        op_headers_get_mime_type,
        op_headers_normalize_name,
        op_url_get_url_info,
        op_url_get_url_search_param_pairs,
        op_url_stringify_url_search_params,
        op_url_update_url_info,
    },
    random::op_random_call,
    storage::{
        async_op_storage_get,
        async_op_storage_store,
    },
    stream::{
        async_op_stream_read_part,
        op_stream_create,
        op_stream_extend,
    },
    text::{
        op_atob,
        op_btoa,
        op_text_encoder_decode,
        op_text_encoder_decode_single,
        op_text_encoder_encode,
        op_text_encoder_encode_into,
        op_text_encoder_new_decoder,
        op_text_encoder_normalize_label,
    },
    time::{
        async_op_sleep,
        op_now_call,
        op_performance_now_call,
        op_performance_time_origin_call,
        sleep_request,
    },
    validate_args::op_validate_args_call,
};
use crate::{
    environment::{
        crypto_rng::CryptoRng,
        AsyncOpRequest,
        OpProvider,
        V8IsolateEnvironment,
    },
    execution_scope::ExecutionScope,
    helpers::to_rust_string,
    metrics,
    request_scope::{
        ReadableStream,
        StreamListener,
    },
};

/// [`OpProvider`] plus V8 itself, for an op that handles V8 values directly or
/// owns state living in the isolate's request scope. Only [`run_v8_op`] can
/// reach these.
pub trait V8OpProvider<'b>: OpProvider {
    fn scope(&mut self) -> v8::PinScope<'_, 'b>;
    fn lookup_source_map(
        &mut self,
        specifier: &ModuleSpecifier,
    ) -> anyhow::Result<Option<SourceMap>>;
    fn console_timers(
        &mut self,
    ) -> anyhow::Result<&mut WithHeapSize<BTreeMap<String, UnixTimestamp>>>;
    fn unix_timestamp_non_deterministic(&mut self) -> anyhow::Result<UnixTimestamp>;

    fn start_async_op(
        &mut self,
        request: AsyncOpRequest,
        resolver: v8::Global<v8::PromiseResolver>,
    ) -> anyhow::Result<()>;

    fn create_stream(&mut self) -> anyhow::Result<Uuid>;
    fn extend_stream(
        &mut self,
        id: Uuid,
        bytes: Option<Bytes>,
        new_done: bool,
    ) -> anyhow::Result<()>;
    fn new_stream_listener(
        &mut self,
        stream_id: Uuid,
        listener: StreamListener,
    ) -> anyhow::Result<()>;
}

impl<'a, 's: 'a, 'i, RT: Runtime, E: V8IsolateEnvironment<RT>> OpProvider
    for ExecutionScope<'a, 's, 'i, RT, E>
{
    fn rng(&mut self) -> anyhow::Result<&mut ChaCha12Rng> {
        let state = self.state_mut()?;
        state.environment.syscall_provider().rng()
    }

    fn crypto_rng(&mut self) -> anyhow::Result<CryptoRng> {
        let state = self.state_mut()?;
        state.environment.syscall_provider().crypto_rng()
    }

    fn trace(&mut self, level: LogLevel, messages: Vec<String>) -> anyhow::Result<()> {
        let state = self.state_mut()?;
        state
            .environment
            .syscall_provider()
            .trace(level, messages)?;
        Ok(())
    }

    fn unix_timestamp(&mut self) -> anyhow::Result<UnixTimestamp> {
        let state = self.state_mut()?;
        state.environment.syscall_provider().unix_timestamp()
    }

    fn performance_now(&mut self) -> anyhow::Result<Duration> {
        let state = self.state_mut()?;
        state.environment.syscall_provider().performance_now()
    }

    fn performance_time_origin(&mut self) -> anyhow::Result<UnixTimestamp> {
        let state = self.state_mut()?;
        state
            .environment
            .syscall_provider()
            .performance_time_origin()
    }

    fn get_environment_variable(
        &mut self,
        name: EnvVarName,
    ) -> anyhow::Result<Option<EnvVarValue>> {
        let state = self.state_mut()?;
        state
            .environment
            .syscall_provider()
            .get_environment_variable(name)
    }

    fn get_all_table_mappings(&mut self) -> anyhow::Result<NamespacedTableMapping> {
        let state = self.state_mut()?;
        state
            .environment
            .syscall_provider()
            .get_all_table_mappings()
    }
}

impl<'a, 's: 'a, 'i, RT: Runtime, E: V8IsolateEnvironment<RT>> V8OpProvider<'i>
    for ExecutionScope<'a, 's, 'i, RT, E>
{
    fn lookup_source_map(
        &mut self,
        specifier: &ModuleSpecifier,
    ) -> anyhow::Result<Option<SourceMap>> {
        ExecutionScope::lookup_source_map(self, specifier)
    }

    fn scope(&mut self) -> v8::PinScope<'_, 'i> {
        self.as_mut_ref()
    }

    fn console_timers(
        &mut self,
    ) -> anyhow::Result<&mut WithHeapSize<BTreeMap<String, UnixTimestamp>>> {
        let state = self.state_mut()?;
        Ok(&mut state.console_timers)
    }

    fn unix_timestamp_non_deterministic(&mut self) -> anyhow::Result<UnixTimestamp> {
        let state = self.state_mut()?;
        Ok(state.unix_timestamp_non_deterministic())
    }

    fn start_async_op(
        &mut self,
        request: AsyncOpRequest,
        resolver: v8::Global<v8::PromiseResolver>,
    ) -> anyhow::Result<()> {
        let state = self.state_mut()?;
        state.environment.start_async_op(request, resolver)
    }

    fn create_stream(&mut self) -> anyhow::Result<Uuid> {
        self.state_mut()?.create_stream()
    }

    fn extend_stream(
        &mut self,
        id: Uuid,
        bytes: Option<Bytes>,
        new_done: bool,
    ) -> anyhow::Result<()> {
        let state = self.state_mut()?;
        if let Some(bytes) = &bytes
            && let Some(request_stream_state) = state.request_stream_state.as_mut()
            && request_stream_state.stream_id() == id
        {
            request_stream_state.track_bytes_read(bytes.len());
        }
        state.streams.mutate(&id, |stream| -> anyhow::Result<()> {
            let Some(Ok(ReadableStream { parts, done })) = stream else {
                anyhow::bail!("unrecognized stream id {id}");
            };
            if *done {
                anyhow::bail!("stream {id} is already done");
            }
            if let Some(bytes) = bytes {
                parts.push_back(bytes);
            }
            if new_done {
                *done = true;
            }
            Ok(())
        })?;
        self.update_stream_listeners()?;
        Ok(())
    }

    fn new_stream_listener(
        &mut self,
        stream_id: Uuid,
        listener: StreamListener,
    ) -> anyhow::Result<()> {
        if self
            .state_mut()?
            .stream_listeners
            .insert(stream_id, listener)
            .is_some()
        {
            anyhow::bail!("cannot read from the same stream twice");
        }
        self.update_stream_listeners()
    }
}

/// The op table, and the dispatchers generated from it.
///
/// Every op is named once. `dual` ops carry the [`OpCall`]-generic wrapper
/// [`macro@convex_macro::op`] emits, so one arm serves both runtimes; `v8` ops
/// handle V8 values themselves and only `run_v8_op` can call them. Listing an
/// op in the wrong section is a compile error rather than a runtime surprise,
/// and neither runtime can quietly fall behind the other.
macro_rules! op_table {
    (
        dual: { $($dual_name:literal => $dual_op:ident,)* },
        v8: { $($v8_name:literal => $v8_op:path,)* },
    ) => {
        fn dispatch<P: OpProvider, M: OpCall<P>>(
            provider: &mut P,
            op_name: &str,
            op_call: M,
        ) -> anyhow::Result<M::Output> {
            match op_name {
                $($dual_name => ::paste::paste!([<$dual_op _call>])(provider, op_call),)*
                _ => {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "UnknownOperation",
                        format!("Unknown operation {op_name}")
                    ));
                },
            }
        }

        fn is_v8_only(op_name: &str) -> bool {
            matches!(op_name, $($v8_name)|*)
        }

        pub fn run_v8_op<'b, P: V8OpProvider<'b>>(
            provider: &mut P,
            args: v8::FunctionCallbackArguments,
            rv: v8::ReturnValue,
        ) -> anyhow::Result<()> {
            if args.length() < 1 {
                // This must be a bug in our `udf-runtime` code, not a developer error.
                anyhow::bail!("op(op_name, ...) takes at least one argument");
            }
            let op_name: v8::Local<v8::String> = args.get(0).try_into()?;
            let op_name = to_rust_string(&provider.scope(), &op_name)?;

            let timer = metrics::op_timer(&op_name);
            match &op_name[..] {
                $($v8_name => $v8_op(provider, args, rv)?,)*
                _ => dispatch(provider, &op_name, V8Call::new(args, rv))?,
            }
            timer.finish();
            Ok(())
        }

        /// `run_v8_op` for a runtime with no V8: `args` is the text of the
        /// positional argument array `performOp` sends, and the answer comes
        /// back as text rather than through a `v8::ReturnValue`. What that text
        /// is encoded as is [`WasmCall`]'s business, not an op's.
        ///
        /// An op this cannot serve reports itself as unimplemented rather than
        /// unknown, because the name is one the V8 runtime does answer.
        pub fn run_wasm_op<P: OpProvider>(
            provider: &mut P,
            op_name: &str,
            args: &str,
        ) -> anyhow::Result<String> {
            anyhow::ensure!(
                !is_v8_only(op_name),
                ErrorMetadata::bad_request(
                    "OperationNotImplemented",
                    format!("The op `{op_name}` is not implemented in the wasm runtime yet")
                )
            );
            let timer = metrics::op_timer(op_name);
            let value = dispatch(provider, op_name, WasmCall::new(args)?)?;
            timer.finish();
            Ok(value)
        }
    };
}

op_table! {
    dual: {
        "random" => op_random,
        "now" => op_now,
        "performance_now" => op_performance_now,
        "performance_time_origin" => op_performance_time_origin,
        "crypto/randomUUID" => op_crypto_random_uuid,
        "crypto/getRandomValues" => op_crypto_get_random_values,
        "environmentVariables/get" => op_environment_variables_get,
        "getTableMapping" => op_get_table_mapping,
        "validateArgs" => op_validate_args,
        "validateReturns" => op_validate_returns,
    },
    v8: {
        "throwUncatchableDeveloperError" => op_throw_uncatchable_developer_error,
        "console/message" => op_console_message,
        "console/trace" => op_console_trace,
        "console/timeStart" => op_console_time_start,
        "console/timeLog" => op_console_time_log,
        "console/timeEnd" => op_console_time_end,
        // The wasm runtime answers this one from `WasmSyscallHost`, which is
        // where its module source maps live.
        "error/stack" => op_error_stack,
        "url/getUrlInfo" => op_url_get_url_info,
        "url/getUrlSearchParamPairs" => op_url_get_url_search_param_pairs,
        "url/stringifyUrlSearchParams" => op_url_stringify_url_search_params,
        "url/updateUrlInfo" => op_url_update_url_info,
        "headers/getMimeType" => op_headers_get_mime_type,
        "headers/normalizeName" => op_headers_normalize_name,
        "stream/create" => op_stream_create,
        "stream/extend" => op_stream_extend,
        "textEncoder/encode" => op_text_encoder_encode,
        "textEncoder/encodeInto" => op_text_encoder_encode_into,
        "textEncoder/decodeSingle" => op_text_encoder_decode_single,
        "textEncoder/decode" => op_text_encoder_decode,
        "textEncoder/newDecoder" => op_text_encoder_new_decoder,
        "textEncoder/normalizeLabel" => op_text_encoder_normalize_label,
        "atob" => op_atob,
        "btoa" => op_btoa,
        "structuredClone" => op_structured_clone,
        "crypto/subtle/decrypt" => subtle_crypto::op_crypto_subtle_decrypt,
        "crypto/subtle/deriveBits" => subtle_crypto::op_crypto_subtle_derive_bits,
        "crypto/subtle/deriveKey" => subtle_crypto::op_crypto_subtle_derive_key,
        "crypto/subtle/digest" => subtle_crypto::op_crypto_subtle_digest,
        "crypto/subtle/encrypt" => subtle_crypto::op_crypto_subtle_encrypt,
        "crypto/subtle/exportKey" => subtle_crypto::op_crypto_subtle_export_key,
        "crypto/subtle/generateKey" => subtle_crypto::op_crypto_subtle_generate_key,
        "crypto/subtle/importKey" => subtle_crypto::op_crypto_subtle_import_key,
        "crypto/subtle/sign" => subtle_crypto::op_crypto_subtle_sign,
        "crypto/subtle/unwrapKey" => subtle_crypto::op_crypto_subtle_unwrap_key,
        "crypto/subtle/verify" => subtle_crypto::op_crypto_subtle_verify,
        "crypto/subtle/wrapKey" => subtle_crypto::op_crypto_subtle_wrap_key,
    },
}

/// `start_async_op`, but with JSON inputs. `args` is the text of the positional
/// argument array `performAsyncOp` sends. Returns the parsed request.
pub fn wasm_async_op_request<P: OpProvider>(
    provider: &mut P,
    op_name: &str,
    args: &str,
) -> anyhow::Result<AsyncOpRequest> {
    match op_name {
        "sleep" => {
            // JSON.stringify converts NaN/Infinity to `null`; convert that back to NaN
            let (name, millis): (String, Option<f64>) = serde_json::from_str(args)?;
            sleep_request(provider, name, millis.unwrap_or(f64::NAN))
        },
        "fetch" | "form/parseMultiPart" | "storage/store" | "storage/get" | "stream/readPart" => {
            anyhow::bail!(ErrorMetadata::bad_request(
                "OperationNotImplemented",
                format!("The async op `{op_name}` is not implemented in the wasm runtime yet")
            ))
        },
        _ => anyhow::bail!(ErrorMetadata::bad_request(
            "UnknownAsyncOperation",
            format!("Unknown async operation {op_name}")
        )),
    }
}

pub fn start_async_op<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue,
) -> anyhow::Result<()> {
    if args.length() < 1 {
        anyhow::bail!("asyncOp(op, ...args) takes at least one argument");
    }
    let scope = provider.scope();
    let op_name: v8::Local<v8::String> = args.get(0).try_into()?;
    let op_name = to_rust_string(&scope, &op_name)?;

    let resolver = v8::PromiseResolver::new(&scope)
        .ok_or_else(|| anyhow!("Failed to create PromiseResolver"))?;
    let resolver = v8::Global::new(&scope, resolver);

    match &op_name[..] {
        "fetch" => async_op_fetch(provider, args, resolver.clone())?,
        "form/parseMultiPart" => async_op_parse_multi_part(provider, args, resolver.clone())?,
        "sleep" => async_op_sleep(provider, args, resolver.clone())?,
        "storage/store" => async_op_storage_store(provider, args, resolver.clone())?,
        "storage/get" => async_op_storage_get(provider, args, resolver.clone())?,
        "stream/readPart" => async_op_stream_read_part(provider, args, resolver.clone())?,
        _ => {
            anyhow::bail!(ErrorMetadata::bad_request(
                "UnknownAsyncOperation",
                format!("Unknown async operation {op_name}")
            ));
        },
    };

    // TODO: ideally we should not need to clone `resolver`, but
    // `V8OpProvider::scope` returns a scope with a restricted lifetime
    let scope = provider.scope();
    let promise = v8::Local::new(&scope, resolver).get_promise(&scope);
    rv.set(promise.into());
    Ok(())
}
