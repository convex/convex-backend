use std::borrow::Cow;

use serde::{
    Deserialize,
    Serialize,
};
use strum::IntoStaticStr;

#[derive(Copy, Clone, Debug, IntoStaticStr, Serialize, Deserialize)]
pub enum DOMExceptionName {
    NotSupportedError,
    SyntaxError,
    InvalidAccessError,
    DataError,
    OperationError,
}

/// Errors raised by WebCrypto operations.
#[derive(Debug)]
pub enum Error {
    /// A JS `DOMException` with the given name and message.
    Dom {
        name: DOMExceptionName,
        message: Cow<'static, str>,
    },
    /// A JS `TypeError`.
    Type(Cow<'static, str>),
    /// An algorithm/operation combination this crate does not implement.
    NotImplemented {
        operation: &'static str,
        algorithm: &'static str,
    },
    /// An internal error (or an error passed through from a callback).
    Other(anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn dom(message: impl Into<Cow<'static, str>>, name: DOMExceptionName) -> Self {
        Error::Dom {
            name,
            message: message.into(),
        }
    }

    pub fn type_error(message: impl Into<Cow<'static, str>>) -> Self {
        Error::Type(message.into())
    }
}

impl<E> From<E> for Error
where
    anyhow::Error: From<E>,
{
    fn from(e: E) -> Self {
        Error::Other(e.into())
    }
}

macro_rules! ensure {
    ($cond:expr, $err:expr $(,)?) => {
        if !$cond {
            return Err($err);
        }
    };
}
pub(crate) use ensure;
