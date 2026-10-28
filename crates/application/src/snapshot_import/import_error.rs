use std::sync::LazyLock;

use common::knobs::TRANSACTION_MAX_USER_WRITE_SIZE_BYTES;
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use humansize::{
    FormatSize,
    BINARY,
};
use strum::AsRefStr;
use value::TableName;

static IMPORT_SIZE_LIMIT: LazyLock<String> =
    LazyLock::new(|| (*TRANSACTION_MAX_USER_WRITE_SIZE_BYTES.format_size(BINARY)).to_string());

#[derive(AsRefStr, Debug, thiserror::Error)]
pub enum ImportError {
    #[error(
        "Table {0} already exists. Please choose a new table name or use replace/append modes."
    )]
    TableExists(TableName),

    #[error("{0:?} isn't a valid table name: {1}")]
    InvalidName(String, anyhow::Error),

    #[error("Import wasn't valid UTF8: {0}")]
    NotUtf8(std::io::Error),

    #[error("UTF-8 BOM is not supported. Please save your file without BOM.")]
    Utf8BomNotSupported,

    #[error(
        "Import is too large for JSON ({0} bytes > maximum {limit}). Consider converting data to JSONLines",
        limit=*IMPORT_SIZE_LIMIT
    )]
    JsonArrayTooLarge(usize),

    #[error("JSONL line {line_number} exceeds the maximum length of {limit} bytes")]
    JsonLineTooLarge { line_number: u64, limit: usize },

    #[error("CSV file doesn't have headers")]
    CsvMissingHeaders,

    #[error("CSV record {record_number} exceeds the maximum length of {limit} bytes")]
    CsvRecordTooLarge { record_number: u64, limit: usize },

    #[error("CSV header {0:?} isn't a valid field name: {1}")]
    CsvInvalidHeader(String, anyhow::Error),

    #[error("Failed to parse CSV row {0}: {1}")]
    CsvInvalidRow(u64, csv_async::Error),

    #[error("CSV row {0} doesn't have all of the fields in the header")]
    CsvRowMissingFields(u64),

    #[error("Row {0} wasn't valid JSON: {1}")]
    JsonInvalidRow(u64, serde_json::Error),

    #[error("Row {0} wasn't a valid Convex value: {1}")]
    InvalidConvexValue(u64, anyhow::Error),

    #[error("Row {0} wasn't an object")]
    NotAnObject(u64),

    #[error("Not a JSON array")]
    NotJsonArray,

    #[error("Not valid JSON: {0}")]
    NotJson(serde_json::Error),
}

impl ImportError {
    pub fn error_metadata(&self) -> ErrorMetadata {
        ErrorMetadata::bad_request(self.as_ref().to_string(), self.to_string())
    }
}

pub fn wrap_import_err(e: anyhow::Error) -> anyhow::Error {
    let e = e.wrap_error_message(|msg| format!("Hit an error while importing:\n{msg}"));
    if let Some(import_err) = e.downcast_ref::<ImportError>() {
        let error_metadata = import_err.error_metadata();
        e.context(error_metadata)
    } else {
        e
    }
}
