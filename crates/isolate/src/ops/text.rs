use std::{
    cell::RefCell,
    rc::Rc,
};

use anyhow::Context as _;
use deno_core::{
    v8,
    ToJsBuffer,
};
use encoding_rs::{
    CoderResult,
    Decoder,
    DecoderResult,
    Encoding,
};
use serde::{
    Deserialize,
    Serialize,
};
use serde_bytes::ByteBuf;
use serde_json::{
    json,
    Value as JsonValue,
};
use slab::Slab;

use super::V8OpProvider;
use crate::{
    convert_v8::{
        FromV8,
        ToV8,
        TypeError,
    },
    strings,
};

#[derive(Default)]
struct TextDecoderStore {
    decoders: Slab<(v8::Weak<v8::Object>, Rc<RefCell<TextDecoderResource>>)>,
}

pub(crate) struct TextDecoderResource {
    decoder: Decoder,
    fatal: bool,
}

fn get_text_decoder_template<'s>(
    scope: &mut v8::PinScope<'s, '_>,
) -> anyhow::Result<v8::Local<'s, v8::FunctionTemplate>> {
    let s = strings::TextDecoder.create(scope)?;
    let private = v8::Private::for_api(scope, Some(s));
    let obj: v8::Local<'_, v8::Object> = scope
        .get_current_context()
        .global(scope)
        .get_private(scope, private)
        .context("missing TextDecoder private")?
        .try_cast()?;
    let template: v8::Local<'_, v8::FunctionTemplate> = obj
        .get_internal_field(scope, 0)
        .context("internal field missing")?
        .try_cast()?;
    Ok(template)
}

// Looks up a Rust TextDecoderResource in the TextDecoderStore based on the
// passed-in resource object.
impl FromV8 for TextDecoderResource {
    type Output = Rc<RefCell<Self>>;

    fn from_v8<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        input: v8::Local<'s, v8::Value>,
    ) -> anyhow::Result<Self::Output> {
        let template = get_text_decoder_template(scope)?;
        let constructor = template
            .get_function(scope)
            .context("get TextDecoder constructor")?;
        let input = input.try_cast::<v8::Object>()?;
        anyhow::ensure!(
            input.instance_of(scope, constructor.into()) == Some(true),
            TypeError::new("not a TextDecoder resource")
        );
        let (id, ok) = input
            .get_internal_field(scope, 0)
            .context("missing internal field")?
            .try_cast::<v8::BigInt>()?
            .u64_value();
        anyhow::ensure!(ok);
        let (weak, resource) = scope
            .get_slot::<TextDecoderStore>()
            .context("missing TextDecoderStore")?
            .decoders
            .get(id as usize)
            .context("dangling TextDecoder")?;
        anyhow::ensure!(*weak == input, "TextDecoder id reused");
        Ok(resource.clone())
    }
}

// Converts a freshly created TextDecoderResource into a new JS object and
// records it in the TextDecoderStore so that it can be passed back into
// decoding ops.
impl ToV8 for TextDecoderResource {
    fn to_v8<'s>(
        self,
        scope: &mut v8::PinScope<'s, '_>,
    ) -> anyhow::Result<v8::Local<'s, v8::Value>> {
        let template = get_text_decoder_template(scope)?;
        let object = template
            .instance_template(scope)
            .new_instance(scope)
            .context("failed to create instance")?;
        anyhow::ensure!(object.internal_field_count() == 1);
        let mut store: TextDecoderStore = scope.remove_slot().unwrap_or_default();
        let entry = store.decoders.vacant_entry();
        let id = entry.key();
        assert!(object.set_internal_field(0, v8::BigInt::new_from_u64(scope, id as u64).into()));
        // Install a finalizer so that we garbage collect the Rust data together
        // with the JS object.
        // This does not need to be a "guaranteed finalizer" because the data is
        // also dropped when the Isolate is destroyed.
        let weak = v8::Weak::with_finalizer(
            scope,
            object,
            Box::new(move |isolate| {
                if let Some(store) = isolate.get_slot_mut::<TextDecoderStore>() {
                    store.decoders.remove(id);
                }
            }),
        );
        entry.insert((weak, Rc::new(RefCell::new(self))));
        scope.set_slot(store);
        Ok(object.into())
    }
}

#[convex_macro::v8_op]
pub fn op_text_encoder_encode<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    text: String,
) -> anyhow::Result<ToJsBuffer> {
    Ok(text.into_bytes().into())
}

#[convex_macro::v8_op]
pub fn op_atob<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    encoded: String,
) -> anyhow::Result<JsonValue> {
    let mut encoded = encoded;
    // https://infra.spec.whatwg.org/#forgiving-base64
    encoded.retain(|c| !c.is_ascii_whitespace());
    // Per forgiving-base64 we need to allow trailing bits.
    // This is a bit *too* forgiving since this version of base64 allows
    // improper padding like in "39=", whereas the specification says that
    // should be an error.
    let bytes = match base64::decode_config(
        encoded,
        base64::STANDARD_NO_PAD.decode_allow_trailing_bits(true),
    ) {
        Ok(bytes) => bytes,
        Err(err) => return Ok(json!({ "error": err.to_string() })),
    };

    let decoded: String = bytes.into_iter().map(char::from).collect();
    Ok(json!({ "decoded": decoded }))
}

#[convex_macro::v8_op]
pub fn op_btoa<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    text: String,
) -> anyhow::Result<JsonValue> {
    let mut bytes = vec![];
    for char in text.chars() {
        if char as usize > u8::MAX as usize {
            return Ok(
                json!({ "error": "The string to be encoded contains characters outside of the Latin1 range." }),
            );
        }
        bytes.push(char as u8);
    }
    let encoded = base64::encode(&bytes);
    Ok(json!({ "encoded": encoded }))
}

#[convex_macro::v8_op]
pub fn op_text_encoder_encode_into<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    input: String,
    space: f64,
) -> anyhow::Result<TextEncodeIntoRetval> {
    let dest_size = space as usize;

    let mut utf16_code_points_read: usize = 0;
    let mut bytes_written: usize = 0;
    for c in input.chars() {
        if bytes_written + c.len_utf8() > dest_size {
            break;
        }
        utf16_code_points_read += c.len_utf16();
        bytes_written += c.len_utf8();
    }
    let bytes = input[0..bytes_written].to_string().into_bytes();
    Ok(TextEncodeIntoRetval {
        bytes: bytes.into(),
        read: utf16_code_points_read,
        written: bytes_written,
    })
}

#[convex_macro::v8_op]
pub fn op_text_encoder_decode_single<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    args: TextDecodeArgs,
) -> anyhow::Result<JsonValue> {
    let Some(encoding) = Encoding::for_label(args.encoding.as_bytes()) else {
        return Ok(
            json!({ "errorRangeError": format!("The encoding label provided ('{}') is invalid.", args.encoding) }),
        );
    };

    let data = args.bytes.to_vec();

    let mut decoder = if args.ignore_bom {
        encoding.new_decoder_without_bom_handling()
    } else {
        encoding.new_decoder_with_bom_removal()
    };

    let Some(max_buffer_length) = decoder.max_utf8_buffer_length(data.len()) else {
        return Ok(json!({ "error": "Value too large to decode" }));
    };
    let mut output = vec![0; max_buffer_length];

    if args.fatal {
        let (result, _, written) =
            decoder.decode_to_utf8_without_replacement(data.as_ref(), &mut output, true);
        match result {
            DecoderResult::InputEmpty => {
                output.truncate(written);
                let text = std::str::from_utf8(&output).expect("decoded utf8 not valid");
                Ok(json!({ "text": text }))
            },
            DecoderResult::OutputFull => Ok(json!({ "error": "Provided buffer too small" })),
            DecoderResult::Malformed(..) => Ok(json!({ "error": "The encoded data is not valid" })),
        }
    } else {
        let (result, _, written, _) = decoder.decode_to_utf8(data.as_ref(), &mut output, true);
        match result {
            CoderResult::InputEmpty => {
                output.truncate(written);
                let text = std::str::from_utf8(&output).expect("decoded utf8 not valid");
                Ok(json!({ "text": text }))
            },
            CoderResult::OutputFull => Ok(json!({ "error": "Provided buffer too small" })),
        }
    }
}

#[convex_macro::v8_op]
pub fn op_text_encoder_new_decoder<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    encoding: String,
    fatal: bool,
    ignore_bom: bool,
) -> anyhow::Result<TextDecoderResource> {
    let Some(encoding) = Encoding::for_label(encoding.as_bytes()) else {
        // The TextDecoder constructor validates the label via
        // `textEncoder/normalizeLabel` before a decoder is created.
        anyhow::bail!("The encoding label provided ('{encoding}') is invalid.");
    };

    let decoder = if ignore_bom {
        encoding.new_decoder_without_bom_handling()
    } else {
        encoding.new_decoder_with_bom_removal()
    };

    Ok(TextDecoderResource { decoder, fatal })
}

#[convex_macro::v8_op]
pub fn op_text_encoder_decode<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    data: ByteBuf,
    resource: TextDecoderResource,
    stream: bool,
) -> anyhow::Result<JsonValue> {
    let mut resource = resource.borrow_mut();
    let fatal = resource.fatal;
    let decoder = &mut resource.decoder;

    let Some(max_buffer_length) = decoder.max_utf8_buffer_length(data.len()) else {
        return Ok(json!({ "error": "Value too large to decode" }));
    };

    let mut output = vec![0; max_buffer_length];

    if fatal {
        let (result, _, written) =
            decoder.decode_to_utf8_without_replacement(data.as_ref(), &mut output, !stream);
        match result {
            DecoderResult::InputEmpty => {
                output.truncate(written);
                let text = std::str::from_utf8(&output).expect("decoded utf8 not valid");
                Ok(json!({ "text": text }))
            },
            DecoderResult::OutputFull => Ok(json!({ "error": "Provided buffer too small" })),
            DecoderResult::Malformed(..) => Ok(json!({ "error": "The encoded data is not valid" })),
        }
    } else {
        let (result, _, written, _) = decoder.decode_to_utf8(data.as_ref(), &mut output, !stream);
        match result {
            CoderResult::InputEmpty => {
                output.truncate(written);
                let text = std::str::from_utf8(&output).expect("decoded utf8 not valid");
                Ok(json!({ "text": text }))
            },
            CoderResult::OutputFull => Ok(json!({ "error": "Provided buffer too small" })),
        }
    }
}

#[convex_macro::v8_op]
pub fn op_text_encoder_normalize_label<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    label: String,
) -> anyhow::Result<JsonValue> {
    let Some(encoding) = Encoding::for_label_no_replacement(label.as_bytes()) else {
        return Ok(
            json!({ "error": format!("The encoding label provided ('{}') is invalid.", label) }),
        );
    };
    Ok(json!({"label": encoding.name().to_lowercase()}))
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct TextDecodeArgs {
    bytes: ByteBuf,
    encoding: String,
    fatal: bool,

    #[serde(rename = "ignoreBOM")]
    ignore_bom: bool,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct TextEncodeIntoRetval {
    bytes: ToJsBuffer,
    read: usize,
    written: usize,
}
