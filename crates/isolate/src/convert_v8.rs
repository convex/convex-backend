use std::{
    backtrace::Backtrace,
    borrow::Cow,
    fmt,
};

use anyhow::Context as _;
use deno_core::{
    serde_v8,
    v8,
};
use derive_more::FromStr;
use errors::ErrorMetadata;
use serde::{
    Deserialize,
    Serialize,
};
use strum::IntoStaticStr;
use value::heap_size::HeapSize;

use crate::strings;

pub(crate) trait FromV8 {
    type Output: Sized;
    fn from_v8<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        input: v8::Local<'s, v8::Value>,
    ) -> anyhow::Result<Self::Output>;
}

impl<T: for<'de> Deserialize<'de>> FromV8 for T {
    type Output = Self;

    fn from_v8<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        input: v8::Local<'s, v8::Value>,
    ) -> anyhow::Result<Self> {
        serde_v8::from_v8(scope, input)
            .map_err(|e| ErrorMetadata::bad_request("InvalidArgument", e.to_string()).into())
    }
}

pub(crate) trait ToV8: Sized {
    fn to_v8<'s>(
        self,
        scope: &mut v8::PinScope<'s, '_>,
    ) -> anyhow::Result<v8::Local<'s, v8::Value>>;
}

impl<T: Serialize> ToV8 for T {
    fn to_v8<'a>(
        self,
        scope: &mut v8::PinScope<'a, '_>,
    ) -> anyhow::Result<v8::Local<'a, v8::Value>> {
        Ok(serde_v8::to_v8(scope, self)?)
    }
}

#[derive(Copy, Clone, Debug, IntoStaticStr, FromStr)]
#[allow(dead_code)]
#[allow(clippy::enum_variant_names)]
pub enum DOMExceptionName {
    IndexSizeError,
    HierarchyRequestError,
    WrongDocumentError,
    InvalidCharacterError,
    NoModificationAllowedError,
    NotFoundError,
    NotSupportedError,
    InUseAttributeError,
    InvalidStateError,
    SyntaxError,
    InvalidModificationError,
    NamespaceError,
    InvalidAccessError,
    TypeMismatchError,
    SecurityError,
    NetworkError,
    AbortError,
    URLMismatchError,
    QuotaExceededError,
    TimeoutError,
    InvalidNodeTypeError,
    DataCloneError,
    EncodingError,
    NotReadableError,
    UnknownError,
    ConstraintError,
    DataError,
    TransactionInactiveError,
    ReadOnlyError,
    VersionError,
    OperationError,
    NotAllowedError,
    OptOutError,
}

#[derive(Debug, Clone)]
pub enum JsException {
    DOMException(DOMException),
    TypeError(TypeError),
}

impl HeapSize for JsException {
    fn heap_size(&self) -> usize {
        match self {
            Self::DOMException(DOMException { message, name: _ }) => message.heap_size(),
            Self::TypeError(TypeError { message }) => message.heap_size(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DOMException {
    pub message: Cow<'static, str>,
    pub name: DOMExceptionName,
}

impl DOMException {
    pub(crate) fn new(message: impl Into<Cow<'static, str>>, name: DOMExceptionName) -> Self {
        Self {
            message: message.into(),
            name,
        }
    }
}

impl ToV8 for DOMException {
    fn to_v8<'s>(
        self,
        scope: &mut v8::PinScope<'s, '_>,
    ) -> anyhow::Result<v8::Local<'s, v8::Value>> {
        // TODO: do not read off of the global object, which is mutable by JS
        let dom_exception_str = strings::DOMException.create(scope)?;
        let global = scope.get_current_context().global(scope);
        let dom_exception_class = global
            .get(scope, dom_exception_str.into())
            .context("missing DOMException")?
            .try_cast::<v8::Function>()
            .context("DOMException isn't v8::Function")?;
        let message = v8::String::new(scope, &self.message).context("failed to create string")?;
        let name = v8::String::new_from_utf8(
            scope,
            <&str>::from(self.name).as_bytes(),
            v8::NewStringType::Internalized,
        )
        .context("failed to create string")?;
        Ok(dom_exception_class
            .new_instance(scope, &[message.into(), name.into()])
            .context("failed to create DOMException")?
            .into())
    }
}

#[derive(Debug, Clone)]
pub struct TypeError {
    pub message: Cow<'static, str>,
}

impl TypeError {
    pub(crate) fn new(message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            message: message.into(),
        }
    }
}
impl ToV8 for TypeError {
    fn to_v8<'s>(
        self,
        scope: &mut v8::PinScope<'s, '_>,
    ) -> anyhow::Result<v8::Local<'s, v8::Value>> {
        let message = v8::String::new(scope, &self.message).context("failed to create string")?;
        Ok(v8::Exception::type_error(scope, message))
    }
}

impl From<DOMException> for anyhow::Error {
    fn from(e: DOMException) -> Self {
        anyhow::Error::new(JsException::DOMException(e))
    }
}

impl From<TypeError> for anyhow::Error {
    fn from(e: TypeError) -> Self {
        anyhow::Error::new(JsException::TypeError(e))
    }
}

impl From<webcrypto::DOMExceptionName> for DOMExceptionName {
    fn from(name: webcrypto::DOMExceptionName) -> Self {
        match name {
            webcrypto::DOMExceptionName::NotSupportedError => DOMExceptionName::NotSupportedError,
            webcrypto::DOMExceptionName::SyntaxError => DOMExceptionName::SyntaxError,
            webcrypto::DOMExceptionName::InvalidAccessError => DOMExceptionName::InvalidAccessError,
            webcrypto::DOMExceptionName::DataError => DOMExceptionName::DataError,
            webcrypto::DOMExceptionName::OperationError => DOMExceptionName::OperationError,
        }
    }
}

impl fmt::Display for JsException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsException::DOMException(e) => write!(f, "{:?}: {}", e.name, e.message),
            JsException::TypeError(e) => write!(f, "TypeError: {}", e.message),
        }
    }
}

impl std::error::Error for JsException {
    // Turn off native backtraces for JsExceptions as these are never meant to
    // be reported, but rather forwarded to V8
    fn provide<'a>(&'a self, request: &mut std::error::Request<'a>) {
        static NO_BACKTRACE: Backtrace = Backtrace::disabled();
        request.provide_ref(&NO_BACKTRACE);
    }
}

impl ToV8 for JsException {
    fn to_v8<'s>(
        self,
        scope: &mut v8::PinScope<'s, '_>,
    ) -> anyhow::Result<v8::Local<'s, v8::Value>> {
        match self {
            JsException::DOMException(e) => e.to_v8(scope),
            JsException::TypeError(e) => e.to_v8(scope),
        }
    }
}

/// Converts to an ArrayBuffer in JS, unlike [`serde_v8::ToJsBuffer`] which
/// becomes a Uint8Array
pub(crate) struct ArrayBuffer(pub Vec<u8>);

impl ToV8 for ArrayBuffer {
    fn to_v8<'s>(
        self,
        scope: &mut v8::PinScope<'s, '_>,
    ) -> anyhow::Result<v8::Local<'s, v8::Value>> {
        let backing_store = v8::ArrayBuffer::new_backing_store_from_vec(self.0).make_shared();
        let array_buffer = v8::ArrayBuffer::with_backing_store(scope, &backing_store);
        Ok(array_buffer.into())
    }
}
