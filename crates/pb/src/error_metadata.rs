use std::{
    error::Error as _,
    fmt::Display,
};

use anyhow::Context;
use errors::{
    ErrorCode,
    ErrorMetadata,
    INTERNAL_SERVER_ERROR,
    INTERNAL_SERVER_ERROR_MSG,
};
use prost::Message;

use crate::errors::{
    ErrorCode as ErrorCodeProto,
    ErrorMetadata as ErrorMetadataProto,
    OccInfo as OccInfoProto,
    StatusDetails as StatusDetailsProto,
};

impl From<ErrorCode> for ErrorCodeProto {
    fn from(code: ErrorCode) -> Self {
        match code {
            ErrorCode::BadRequest => ErrorCodeProto::BadRequest,
            ErrorCode::Conflict => ErrorCodeProto::Conflict,
            ErrorCode::Unauthenticated => ErrorCodeProto::Unauthenticated,
            ErrorCode::AuthUpdateFailed => ErrorCodeProto::AuthUpdateFailed,
            ErrorCode::Forbidden => ErrorCodeProto::Forbidden,
            ErrorCode::NotFound => ErrorCodeProto::TransientNotFound,
            ErrorCode::ClientDisconnect => ErrorCodeProto::ClientDisconnect,
            ErrorCode::RateLimited => ErrorCodeProto::RateLimited,
            ErrorCode::Overloaded => ErrorCodeProto::Overloaded,
            ErrorCode::FeatureTemporarilyUnavailable => {
                ErrorCodeProto::FeatureTemporarilyUnavailable
            },
            ErrorCode::RejectedBeforeExecution => ErrorCodeProto::RejectedBeforeExecution,
            ErrorCode::TooEarly => ErrorCodeProto::TooEarly,
            ErrorCode::OCC { .. } => ErrorCodeProto::Occ,
            ErrorCode::PaginationLimit => ErrorCodeProto::PaginationLimit,
            ErrorCode::OutOfRetention => ErrorCodeProto::OutOfRetention,
            ErrorCode::OperationalInternalServerError => {
                ErrorCodeProto::OperationalInternalServerError
            },
            ErrorCode::MisdirectedRequest => ErrorCodeProto::MisdirectedRequest,
        }
    }
}

impl ErrorCodeProto {
    fn into_rust_type(self, occ_info: OccInfoProto) -> ErrorCode {
        match self {
            ErrorCodeProto::BadRequest => ErrorCode::BadRequest,
            ErrorCodeProto::Conflict => ErrorCode::Conflict,
            ErrorCodeProto::Unauthenticated => ErrorCode::Unauthenticated,
            ErrorCodeProto::AuthUpdateFailed => ErrorCode::AuthUpdateFailed,
            ErrorCodeProto::Forbidden => ErrorCode::Forbidden,
            ErrorCodeProto::TransientNotFound => ErrorCode::NotFound,
            ErrorCodeProto::ClientDisconnect => ErrorCode::ClientDisconnect,
            ErrorCodeProto::RateLimited => ErrorCode::RateLimited,
            ErrorCodeProto::Overloaded => ErrorCode::Overloaded,
            ErrorCodeProto::FeatureTemporarilyUnavailable => {
                ErrorCode::FeatureTemporarilyUnavailable
            },
            ErrorCodeProto::RejectedBeforeExecution => ErrorCode::RejectedBeforeExecution,
            ErrorCodeProto::TooEarly => ErrorCode::TooEarly,
            ErrorCodeProto::Occ => ErrorCode::OCC {
                table_name: occ_info.table_name,
                document_id: occ_info.document_id,
                write_source: occ_info.write_source,
                component_path: occ_info.component_path,
                is_system: occ_info.is_system,
                write_ts: occ_info.write_ts,
            },
            ErrorCodeProto::PaginationLimit => ErrorCode::PaginationLimit,
            ErrorCodeProto::OutOfRetention => ErrorCode::OutOfRetention,
            ErrorCodeProto::OperationalInternalServerError => {
                ErrorCode::OperationalInternalServerError
            },
            ErrorCodeProto::MisdirectedRequest => ErrorCode::MisdirectedRequest,
        }
    }
}

impl From<ErrorMetadata> for ErrorMetadataProto {
    fn from(metadata: ErrorMetadata) -> Self {
        ErrorMetadataProto {
            code: ErrorCodeProto::from(metadata.code.clone()).into(),
            short_msg: Some(metadata.short_msg.to_string()),
            msg: Some(truncate(&metadata.msg)),
            occ_info: match metadata.code {
                ErrorCode::OCC {
                    table_name,
                    document_id,
                    write_source,
                    component_path,
                    is_system,
                    write_ts,
                } => Some(OccInfoProto {
                    table_name,
                    document_id,
                    write_source,
                    component_path,
                    is_system,
                    write_ts,
                }),
                _ => None,
            },
            source: metadata.source,
        }
    }
}

/// gRPC error messages are encoded in HTTP2 trailers, which are size limited
/// limit error messages at 4K to stay on the safe side
fn truncate(msg: &str) -> String {
    const MAX_ERROR_LENGTH: usize = 4096;
    if msg.len() <= MAX_ERROR_LENGTH {
        return msg.to_owned();
    }
    format!("{}...", &msg[..msg.floor_char_boundary(MAX_ERROR_LENGTH)])
}

impl TryFrom<ErrorMetadataProto> for ErrorMetadata {
    type Error = anyhow::Error;

    fn try_from(metadata: ErrorMetadataProto) -> anyhow::Result<Self> {
        let code = ErrorCodeProto::try_from(metadata.code)?
            .into_rust_type(metadata.occ_info.unwrap_or_default());
        let short_msg = metadata.short_msg.context("Missing `short_msg` field")?;
        let msg = metadata.msg.context("Missing `msg` field")?;
        Ok(Self {
            code,
            short_msg: short_msg.into(),
            msg: msg.into(),
            source: metadata.source,
        })
    }
}

fn has_connection_reset_source(status: &tonic::Status) -> bool {
    let mut source = status.source();
    while let Some(error) = source {
        if error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::ConnectionReset)
        {
            return true;
        }
        source = error.source();
    }
    false
}

pub trait ErrorMetadataStatusExt {
    fn from_anyhow(error: anyhow::Error) -> Self;
    fn into_anyhow(self) -> anyhow::Error;
    fn context<C>(self, context: C) -> anyhow::Error
    where
        C: Display + Send + Sync + 'static;
}

impl ErrorMetadataStatusExt for tonic::Status {
    fn from_anyhow(error: anyhow::Error) -> Self {
        let message = truncate(&format!("{error:#}"));
        if let Some(metadata) = error.downcast_ref::<ErrorMetadata>().cloned() {
            let code: tonic::Code = metadata.code.grpc_status_code();
            let details = StatusDetailsProto {
                error_metadata: Some(metadata.into()),
            };
            tonic::Status::with_details(code, message, details.encode_to_vec().into())
        } else {
            tonic::Status::internal(message)
        }
    }

    fn into_anyhow(self) -> anyhow::Error {
        let code = self.code();
        let is_connection_reset = has_connection_reset_source(&self);
        let details = match StatusDetailsProto::decode(self.details()) {
            Ok(details) => details,
            Err(err) => {
                return anyhow::anyhow!("Failed to decode StatusDetails proto: {}", err);
            },
        };
        let message = self.message().to_string();
        let mut source_chain = String::new();
        let mut source = self.source();
        while let Some(s) = source {
            source_chain.push_str(&format!(": {s}"));
            source = s.source();
        }
        let mut error: anyhow::Error =
            anyhow::anyhow!("status: {code:?}, message: {message:?}{source_chain}");
        if let Some(error_metadata) = details.error_metadata {
            let error_metadata = match ErrorMetadata::try_from(error_metadata) {
                Ok(error_metadata) => error_metadata,
                Err(err) => return err.context("Failed to parse ErrorMetadata proto"),
            };
            error = error.context(error_metadata)
        } else if code == tonic::Code::ResourceExhausted
            || (code == tonic::Code::Unknown && is_connection_reset)
        {
            // A push can cut off an active connection, which Tonic reports as a
            // source-backed `Unknown` rather than `Unavailable`.
            error = error.context(ErrorMetadata::overloaded(
                INTERNAL_SERVER_ERROR,
                INTERNAL_SERVER_ERROR_MSG,
            ));
        }
        error
    }

    fn context<C>(self, context: C) -> anyhow::Error
    where
        C: Display + Send + Sync + 'static,
    {
        self.into_anyhow().context(context)
    }
}
