// Copyright 2018-2025 the Deno authors. All rights reserved. MIT license.
// https://github.com/denoland/deno_core/blob/main/core/ops_builtin_v8.rs

use deno_core::v8::{
    self,
    tc_scope,
    ValueDeserializerHelper,
    ValueSerializerHelper,
};
use errors::ErrorMetadata;

use super::V8OpProvider;

// NOTE: not using `v8_op` macro because we want to handle serde ourselves.
pub fn op_structured_clone<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue,
) -> anyhow::Result<()> {
    let value = args.get(1);
    // `None` means the serializer already threw a `TypeError` into the
    // isolate, and returning lets that exception reach the caller intact.
    let Some(data) = op_serialize(&mut provider.scope(), value)? else {
        return Ok(());
    };
    let value = op_deserialize(&mut provider.scope(), data)?;
    rv.set(value);
    Ok(())
}

// The following is copied from `deno_core/core/ops_builtin_v8.rs`
// With simplifications to remove unused options.
struct SerializeDeserialize {
    host_object_brand: Option<v8::Global<v8::Symbol>>,
}

impl v8::ValueSerializerImpl for SerializeDeserialize {
    #[allow(unused_variables)]
    fn throw_data_clone_error<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        message: v8::Local<'s, v8::String>,
    ) {
        let error = v8::Exception::type_error(scope, message);
        scope.throw_exception(error);
    }

    fn get_shared_array_buffer_id<'s>(
        &self,
        _scope: &mut v8::PinScope<'s, '_>,
        _shared_array_buffer: v8::Local<'s, v8::SharedArrayBuffer>,
    ) -> Option<u32> {
        None
    }

    fn get_wasm_module_transfer_id(
        &self,
        _scope: &mut v8::PinScope<'_, '_>,
        _module: v8::Local<v8::WasmModuleObject>,
    ) -> Option<u32> {
        None
    }

    fn has_custom_host_object(&self, _isolate: &v8::Isolate) -> bool {
        true
    }

    fn is_host_object<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        object: v8::Local<'s, v8::Object>,
    ) -> Option<bool> {
        if let Some(symbol) = &self.host_object_brand {
            let key = v8::Local::new(scope, symbol);
            object.has_own_property(scope, key.into())
        } else {
            Some(false)
        }
    }

    fn write_host_object<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        _object: v8::Local<'s, v8::Object>,
        _value_serializer: &dyn v8::ValueSerializerHelper,
    ) -> Option<bool> {
        let message = v8::String::new(scope, "Unsupported object type").unwrap();
        self.throw_data_clone_error(scope, message);
        None
    }
}

impl v8::ValueDeserializerImpl for SerializeDeserialize {
    fn get_shared_array_buffer_from_id<'s>(
        &self,
        _scope: &mut v8::PinScope<'s, '_>,
        _transfer_id: u32,
    ) -> Option<v8::Local<'s, v8::SharedArrayBuffer>> {
        None
    }

    fn get_wasm_module_from_id<'s>(
        &self,
        _scope: &mut v8::PinScope<'s, '_>,
        _clone_id: u32,
    ) -> Option<v8::Local<'s, v8::WasmModuleObject>> {
        None
    }

    fn read_host_object<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        _value_deserializer: &dyn v8::ValueDeserializerHelper,
    ) -> Option<v8::Local<'s, v8::Object>> {
        let message: v8::Local<v8::String> =
            v8::String::new(scope, "Failed to deserialize host object").unwrap();
        let error = v8::Exception::error(scope, message);
        scope.throw_exception(error);
        None
    }
}

pub fn op_serialize(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
) -> anyhow::Result<Option<Vec<u8>>> {
    let key = v8::String::new(scope, "Deno.core.hostObject").unwrap();
    let symbol = v8::Symbol::for_key(scope, key);
    let host_object_brand = Some(v8::Global::new(scope, symbol));

    let serialize_deserialize = Box::new(SerializeDeserialize { host_object_brand });
    let value_serializer = v8::ValueSerializer::new(scope, serialize_deserialize);
    value_serializer.write_header();

    tc_scope!(let scope, scope);
    let ret = value_serializer.write_value(scope.get_current_context(), value);
    if scope.has_caught() || scope.has_terminated() {
        scope.rethrow();
        Ok(None)
    } else if let Some(true) = ret {
        let vector = value_serializer.release();
        Ok(Some(vector))
    } else {
        // TODO: incorrect error type, should be TypeError
        Err(ErrorMetadata::bad_request("SerializeFailed", "Failed to serialize response").into())
    }
}

pub fn op_deserialize<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    data: Vec<u8>,
) -> anyhow::Result<v8::Local<'s, v8::Value>> {
    let serialize_deserialize = Box::new(SerializeDeserialize {
        host_object_brand: None,
    });
    let value_deserializer = v8::ValueDeserializer::new(scope, serialize_deserialize, &data);
    let parsed_header = value_deserializer
        .read_header(scope.get_current_context())
        .unwrap_or_default();
    if !parsed_header {
        return Err(
            // TODO: incorrect error type, should be RangeError
            ErrorMetadata::bad_request("DeserializeFailed", "could not deserialize value").into(),
        );
    }

    let value = value_deserializer.read_value(scope.get_current_context());
    match value {
        Some(deserialized) => Ok(deserialized),
        None => Err(
            // TODO: incorrect error type, should be RangeError
            ErrorMetadata::bad_request("DeserializeFailed", "could not deserialize value").into(),
        ),
    }
}
