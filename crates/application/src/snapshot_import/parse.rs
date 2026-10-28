use std::{
    collections::BTreeMap,
    io,
    str::FromStr,
    sync::{
        Arc,
        LazyLock,
    },
};

use anyhow::Context;
use bytes::Bytes;
use common::{
    bootstrap_model::tables::TABLES_TABLE,
    components::{
        ComponentName,
        ComponentPath,
    },
    knobs::{
        SNAPSHOT_IMPORT_MAX_LINE_SIZE_BYTES,
        TRANSACTION_MAX_USER_WRITE_SIZE_BYTES,
    },
    types::{
        FieldName,
        FullyQualifiedObjectKey,
    },
};
use errors::ErrorMetadata;
use futures::{
    stream::{
        self,
        BoxStream,
    },
    AsyncReadExt,
    StreamExt,
    TryStreamExt,
};
use futures_async_stream::{
    try_stream,
    try_stream_block,
};
use model::{
    file_storage::FILE_STORAGE_VIRTUAL_TABLE,
    snapshot_imports::types::ImportFormat,
};
use regex::Regex;
use serde_json::{
    json,
    Value as JsonValue,
};
use shape_inference::{
    export_context::{
        ExportContext,
        GeneratedSchema,
    },
    ProdConfig,
    Shape,
    ShapeConfig,
};
use storage::{
    Storage,
    StorageExt,
};
use storage_zip_reader::StorageZipArchive;
use tokio::io::{
    AsyncBufRead,
    AsyncBufReadExt as _,
    AsyncRead,
    AsyncReadExt as _,
    BufReader,
};
use tokio_util::io::ReaderStream;
use value::{
    id_v6::DeveloperDocumentId,
    TableName,
};

use crate::snapshot_import::{
    import_error::ImportError,
    metrics::log_snapshot_import_found_legacy_generated_schema,
};

pub type ImportDocumentStream = BoxStream<'static, anyhow::Result<JsonValue>>;
pub type ImportStorageFileStream =
    Box<dyn FnOnce() -> BoxStream<'static, anyhow::Result<Bytes>> + Send>;
pub struct ParsedImport {
    pub generated_schemas: Vec<(ComponentPath, TableName, GeneratedSchema<ProdConfig>)>,
    pub documents: Vec<(ComponentPath, TableName, ImportDocumentStream)>,
    pub storage_files: BoxStream<
        'static,
        anyhow::Result<(ComponentPath, DeveloperDocumentId, ImportStorageFileStream)>,
    >,
}

impl ParsedImport {
    fn single_table(
        component_path: ComponentPath,
        table_name: TableName,
        documents: ImportDocumentStream,
    ) -> Self {
        Self {
            generated_schemas: vec![],
            documents: vec![(component_path, table_name, documents)],
            storage_files: stream::empty().boxed(),
        }
    }
}

static COMPONENT_NAME_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.*/)?_components/([^/]+)/$").unwrap());
static GENERATED_SCHEMA_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.*/)?([^/]+)/generated_schema\.jsonl$").unwrap());
static DOCUMENTS_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.*/)?([^/]+)/documents\.jsonl$").unwrap());
// _storage/(ID) with optional ignored prefix and extension like
// snapshot/_storage/(ID).png
static STORAGE_FILE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(.*/)?_storage/([^/.]+)(?:\.[^/]+)?$").unwrap());

fn map_zip_io_error(e: io::Error) -> anyhow::Error {
    if e.kind() == io::ErrorKind::InvalidData {
        // Content errors become InvalidData errors
        anyhow::Error::from(e).context(ErrorMetadata::bad_request("InvalidZip", "invalid zip file"))
    } else {
        // S3 errors get mapped into ErrorKind::Other
        e.into()
    }
}

fn map_csv_error(e: csv_async::Error) -> anyhow::Error {
    let pos_line = |pos: &Option<csv_async::Position>| pos.as_ref().map_or(0, |pos| pos.line());
    match e.kind() {
        csv_async::ErrorKind::Utf8 { pos, .. } => {
            ImportError::CsvInvalidRow(pos_line(pos), e).into()
        },
        csv_async::ErrorKind::UnequalLengths { pos, .. } => {
            ImportError::CsvRowMissingFields(pos_line(pos)).into()
        },
        csv_async::ErrorKind::Io(source) => {
            let metadata = source
                .get_ref()
                .and_then(|source| source.downcast_ref::<ImportError>())
                .map(ImportError::error_metadata);
            match metadata {
                Some(metadata) => anyhow::Error::from(e).context(metadata),
                None => e.into(),
            }
        },
        csv_async::ErrorKind::Seek
        // We're not using serde for CSV parsing, so these errors are unexpected
        | csv_async::ErrorKind::Serialize(_)
        | csv_async::ErrorKind::Deserialize { .. }
        => e.into(),
        _ => e.into(),
    }
}

/// Parse the imported file, returning separate streams for each table or
/// storage file.
pub async fn parse_import_file(
    format: ImportFormat,
    component_path: ComponentPath,
    storage: Arc<dyn Storage>,
    fq_object_key: FullyQualifiedObjectKey,
) -> anyhow::Result<ParsedImport> {
    let stream_body = || async {
        storage
            .get_fq_object(&fq_object_key)
            .await?
            .with_context(|| format!("Missing import object {fq_object_key:?}"))
    };
    match format {
        ImportFormat::Csv(table_name) => Ok(ParsedImport::single_table(
            component_path,
            table_name,
            parse_csv_import(stream_body().await?).boxed(),
        )),
        ImportFormat::JsonLines(table_name) => {
            let mut reader = BufReader::new(stream_body().await?.into_tokio_reader());
            Ok(ParsedImport::single_table(
                component_path,
                table_name,
                try_stream_block!({
                    let mut line = Vec::new();
                    let mut lineno = 1;
                    while let Some(line) = read_bounded_line(
                        &mut reader,
                        &mut line,
                        *SNAPSHOT_IMPORT_MAX_LINE_SIZE_BYTES,
                        lineno,
                        |e| ImportError::NotUtf8(e).into(),
                    )
                    .await?
                    {
                        // Check for UTF-8 BOM at the start of the first line
                        if lineno == 1 && line.as_bytes().starts_with(&[0xEF, 0xBB, 0xBF]) {
                            anyhow::bail!(ImportError::Utf8BomNotSupported);
                        }
                        let v: serde_json::Value = serde_json::from_str(line)
                            .map_err(|e| ImportError::JsonInvalidRow(lineno, e))?;
                        yield v;
                        lineno += 1;
                    }
                })
                .boxed(),
            ))
        },
        ImportFormat::JsonArray(table_name) => {
            let reader = stream_body().await?;
            let mut buf = Vec::new();
            let mut truncated_reader = reader
                .into_reader()
                .take((*TRANSACTION_MAX_USER_WRITE_SIZE_BYTES as u64) + 1);
            truncated_reader.read_to_end(&mut buf).await?;
            if buf.len() > *TRANSACTION_MAX_USER_WRITE_SIZE_BYTES {
                anyhow::bail!(ImportError::JsonArrayTooLarge(buf.len()));
            }
            let v: serde_json::Value = {
                // Check for UTF-8 BOM and reject it
                if buf.starts_with(&[0xEF, 0xBB, 0xBF]) {
                    anyhow::bail!(ImportError::Utf8BomNotSupported);
                }
                serde_json::from_slice(&buf).map_err(ImportError::NotJson)?
            };
            let JsonValue::Array(array) = v else {
                anyhow::bail!(ImportError::NotJsonArray)
            };
            Ok(ParsedImport::single_table(
                component_path,
                table_name,
                stream::iter(array.into_iter().map(Ok)).boxed(),
            ))
        },
        ImportFormat::Zip => {
            let base_component_path = component_path;
            let zip_reader = StorageZipArchive::open_fq(storage, fq_object_key).await?;

            let mut generated_schemas = vec![];
            let mut documents = vec![];
            for entry in zip_reader.entries() {
                if let Some((component_path, table_name)) =
                    parse_documents_jsonl_table_name(&entry.name, &base_component_path)?
                {
                    if table_name.is_system()
                        && table_name != TABLES_TABLE
                        && table_name != FILE_STORAGE_VIRTUAL_TABLE
                    {
                        tracing::info!("Skipping system table entry {}", entry.name);
                        continue;
                    }
                    let entry_reader = zip_reader.read_entry(entry.clone());
                    tracing::info!(
                        "importing zip file containing table {component_path}:{table_name}"
                    );
                    documents.push((
                        component_path,
                        table_name,
                        parse_documents_jsonl(entry_reader).boxed(),
                    ));
                } else if let Some((component_path, table_name)) = parse_table_filename(
                    &entry.name,
                    &base_component_path,
                    &GENERATED_SCHEMA_PATTERN,
                )? {
                    let entry_reader = zip_reader.read_entry(entry.clone());
                    tracing::info!("importing zip file containing generated_schema {table_name}");
                    let generated_schema =
                        parse_generated_schema(&entry.name, entry_reader).await?;

                    generated_schemas.push((component_path, table_name, generated_schema));
                }
            }
            let storage_files = try_stream_block!({
                let zip_reader = Arc::new(zip_reader);
                for entry in zip_reader.entries() {
                    if let Some((component_path, storage_id)) =
                        parse_storage_filename(&entry.name, &base_component_path)?
                    {
                        let zip_reader = zip_reader.clone();
                        let entry = entry.clone();
                        let entry_reader_stream = Box::new(move || {
                            ReaderStream::new(zip_reader.read_entry(entry))
                                .map_err(anyhow::Error::from)
                                .boxed()
                        }) as Box<_>;
                        yield (component_path, storage_id, entry_reader_stream);
                    }
                }
            });
            Ok(ParsedImport {
                generated_schemas,
                documents,
                storage_files: storage_files.boxed(),
            })
        },
    }
}

#[try_stream(ok = Vec<u8>, error = io::Error)]
async fn limit_csv_records(mut reader: impl AsyncRead + Unpin, max_record_bytes: usize) {
    // Match csv-async's default parser settings so validation observes the same
    // record boundaries and escapes before csv-async grows its record buffers.
    let mut parser = csv_core::Reader::new();
    let mut chunk = [0; 8192];
    let mut decoded = [0; 8192];
    let mut ends = [0; 128];
    let mut record_bytes = 0;
    let mut record_number = 1;
    let mut pending_cr = None;
    loop {
        let len = reader.read(&mut chunk).await?;
        let mut input = &chunk[..len];
        // An empty input flushes the final field and record at EOF.
        while !input.is_empty() || len == 0 {
            // csv-core reports a record at CR; the optional LF can arrive in
            // the next read and contributes to the preceding record's length.
            if let Some(previous_bytes) = pending_cr.take()
                && input.first() == Some(&b'\n')
            {
                if previous_bytes >= max_record_bytes {
                    Err(io::Error::other(ImportError::CsvRecordTooLarge {
                        record_number: record_number - 1,
                        limit: max_record_bytes,
                    }))?;
                }
                let _ = parser.read_record(&input[..1], &mut decoded, &mut ends);
                input = &input[1..];
                continue;
            }
            // Empty physical lines are skipped by the CSV parser.
            if record_bytes == 0 {
                let blank_bytes = input
                    .iter()
                    .take_while(|&&b| b == b'\r' || b == b'\n')
                    .count();
                if blank_bytes > 0 {
                    let _ = parser.read_record(&input[..blank_bytes], &mut decoded, &mut ends);
                    input = &input[blank_bytes..];
                    continue;
                }
            }
            let (result, consumed, ..) = parser.read_record(input, &mut decoded, &mut ends);
            let ended_with_cr = input[..consumed].last() == Some(&b'\r');
            input = &input[consumed..];
            record_bytes += consumed;
            if record_bytes > max_record_bytes {
                Err(io::Error::other(ImportError::CsvRecordTooLarge {
                    record_number,
                    limit: max_record_bytes,
                }))?;
            }
            match result {
                csv_core::ReadRecordResult::End => return Ok(()),
                csv_core::ReadRecordResult::Record => {
                    pending_cr = ended_with_cr.then_some(record_bytes);
                    record_bytes = 0;
                    record_number += 1;
                },
                csv_core::ReadRecordResult::InputEmpty => break,
                csv_core::ReadRecordResult::OutputFull
                | csv_core::ReadRecordResult::OutputEndsFull => {},
            }
        }
        yield chunk[..len].to_vec();
    }
}

#[try_stream(ok = JsonValue, error = anyhow::Error)]
async fn parse_csv_import(reader: storage::StorageGetStream) {
    let reader = limit_csv_records(
        reader.into_tokio_reader(),
        *SNAPSHOT_IMPORT_MAX_LINE_SIZE_BYTES,
    )
    .boxed()
    .into_async_read();
    let mut reader = csv_async::AsyncReader::from_reader(reader);
    if !reader.has_headers() {
        // TODO: this will never happen.
        anyhow::bail!(ImportError::CsvMissingHeaders);
    }
    let field_names = {
        let headers = reader.headers().await.map_err(map_csv_error)?;
        headers
            .iter()
            .map(|s| {
                let trimmed = s.trim_matches(' ');
                let field_name = FieldName::from_str(trimmed)
                    .map_err(|e| ImportError::CsvInvalidHeader(trimmed.to_string(), e))?;
                Ok(field_name)
            })
            .collect::<anyhow::Result<Vec<_>>>()?
    };
    let mut lineno = 0;
    let mut rows = reader.records();
    while let Some(row_r) = rows.next().await {
        lineno += 1;
        let parsed_row = row_r
            .map_err(map_csv_error)?
            .iter()
            .map(parse_csv_cell)
            .collect::<Vec<JsonValue>>();
        let mut obj = BTreeMap::new();
        if field_names.len() != parsed_row.len() {
            anyhow::bail!(ImportError::CsvRowMissingFields(lineno));
        }
        for (field_name, value) in field_names.iter().zip(parsed_row.into_iter()) {
            obj.insert(field_name.to_string(), value);
        }
        yield serde_json::to_value(obj)?;
    }
}

pub fn parse_component_path(
    mut filename: &str,
    base_component_path: &ComponentPath,
) -> anyhow::Result<ComponentPath> {
    let mut component_names = Vec::new();
    while let Some(captures) = COMPONENT_NAME_PATTERN.captures(filename) {
        filename = captures.get(1).map_or("", |c| c.as_str());
        let component_name_str = captures
            .get(2)
            .expect("regex has two capture groups")
            .as_str();
        let component_name: ComponentName = component_name_str.parse().map_err(|e| {
            ErrorMetadata::bad_request(
                "InvalidComponentName",
                format!("component name '{component_name_str}' invalid: {e}"),
            )
        })?;
        component_names.push(component_name);
    }
    component_names.reverse();
    let mut component_path = base_component_path.clone();
    for component_name in component_names {
        component_path = component_path.push(component_name);
    }
    Ok(component_path)
}

fn parse_table_filename(
    filename: &str,
    base_component_path: &ComponentPath,
    regex: &Regex,
) -> anyhow::Result<Option<(ComponentPath, TableName)>> {
    match regex.captures(filename) {
        None => Ok(None),
        Some(captures) => {
            let table_name_str = captures
                .get(2)
                .expect("regex has two capture groups")
                .as_str();
            let table_name = table_name_str.parse().map_err(|e| {
                ErrorMetadata::bad_request(
                    "InvalidTableName",
                    format!("table name '{table_name_str}' invalid: {e}"),
                )
            })?;
            let prefix = captures.get(1).map_or("", |c| c.as_str());
            let component_path = parse_component_path(prefix, base_component_path)?;
            Ok(Some((component_path, table_name)))
        },
    }
}

fn parse_storage_filename(
    filename: &str,
    base_component_path: &ComponentPath,
) -> anyhow::Result<Option<(ComponentPath, DeveloperDocumentId)>> {
    match STORAGE_FILE_PATTERN.captures(filename) {
        None => Ok(None),
        Some(captures) => {
            let storage_id_str = captures
                .get(2)
                .expect("regex has two capture groups")
                .as_str();
            if storage_id_str == "documents" {
                return Ok(None);
            }
            let storage_id = DeveloperDocumentId::decode(storage_id_str).map_err(|e| {
                ErrorMetadata::bad_request(
                    "InvalidStorageId",
                    format!("_storage id '{storage_id_str}' invalid: {e}"),
                )
            })?;
            let prefix = captures.get(1).map_or("", |c| c.as_str());
            let component_path = parse_component_path(prefix, base_component_path)?;
            Ok(Some((component_path, storage_id)))
        },
    }
}

fn parse_documents_jsonl_table_name(
    filename: &str,
    base_component_path: &ComponentPath,
) -> anyhow::Result<Option<(ComponentPath, TableName)>> {
    parse_table_filename(filename, base_component_path, &DOCUMENTS_PATTERN)
}

async fn read_bounded_line<'a>(
    reader: &mut (impl AsyncBufRead + Unpin),
    line: &'a mut Vec<u8>,
    limit: usize,
    line_number: u64,
    map_io_error: impl Fn(io::Error) -> anyhow::Error,
) -> anyhow::Result<Option<&'a str>> {
    line.clear();
    // One byte of lookahead distinguishes a line at the limit from an oversized
    // line, including when the final line has no newline.
    let bytes_read = reader
        .take((limit as u64).saturating_add(1))
        .read_until(b'\n', line)
        .await
        .map_err(&map_io_error)?;
    if bytes_read > limit {
        anyhow::bail!(ImportError::JsonLineTooLarge { line_number, limit });
    }
    if bytes_read == 0 {
        return Ok(None);
    }
    let line = std::str::from_utf8(line)
        .map_err(|e| map_io_error(io::Error::new(io::ErrorKind::InvalidData, e)))?;
    Ok(Some(line))
}

#[try_stream(ok = JsonValue, error = anyhow::Error)]
async fn parse_documents_jsonl(reader: impl AsyncRead + Unpin) {
    let mut line = Vec::new();
    let mut lineno = 1;
    let mut reader = BufReader::new(reader);
    while let Some(line) = read_bounded_line(
        &mut reader,
        &mut line,
        *SNAPSHOT_IMPORT_MAX_LINE_SIZE_BYTES,
        lineno,
        map_zip_io_error,
    )
    .await?
    {
        let v: serde_json::Value =
            serde_json::from_str(line).map_err(|e| ImportError::JsonInvalidRow(lineno, e))?;
        yield v;
        lineno += 1;
    }
}

async fn parse_generated_schema<T: ShapeConfig>(
    filename: &str,
    entry_reader: impl tokio::io::AsyncRead + Unpin,
) -> anyhow::Result<GeneratedSchema<T>> {
    let mut line = Vec::new();
    let mut lineno = 1;
    let mut entry_reader = BufReader::new(entry_reader);
    let first_line = read_bounded_line(
        &mut entry_reader,
        &mut line,
        *SNAPSHOT_IMPORT_MAX_LINE_SIZE_BYTES,
        lineno,
        |e| ImportError::NotUtf8(e).into(),
    )
    .await?
    .unwrap_or("");
    let inferred_type_json: serde_json::Value =
        serde_json::from_str(first_line).map_err(|e| ImportError::JsonInvalidRow(lineno, e))?;
    if inferred_type_json.as_str() == Some("uniform") {
        return Ok(GeneratedSchema::Uniform);
    }
    log_snapshot_import_found_legacy_generated_schema();
    let inferred_type = Shape::from_str(inferred_type_json.as_str().with_context(|| {
        ImportError::InvalidConvexValue(
            lineno,
            anyhow::anyhow!("first line of generated_schema must be a string"),
        )
    })?)
    .map_err(|e| {
        ErrorMetadata::bad_request(
            "InvalidGeneratedSchema",
            format!("cannot parse {filename}: {e:#}"),
        )
    })?;
    lineno += 1;
    let mut overrides = BTreeMap::new();
    while let Some(line) = read_bounded_line(
        &mut entry_reader,
        &mut line,
        *SNAPSHOT_IMPORT_MAX_LINE_SIZE_BYTES,
        lineno,
        |e| ImportError::NotUtf8(e).into(),
    )
    .await?
    {
        let mut v: serde_json::Value =
            serde_json::from_str(line).map_err(|e| ImportError::JsonInvalidRow(lineno, e))?;
        let o = v.as_object_mut().with_context(|| {
            ImportError::InvalidConvexValue(lineno, anyhow::anyhow!("overrides should be object"))
        })?;
        if o.len() != 1 {
            anyhow::bail!(ImportError::InvalidConvexValue(
                lineno,
                anyhow::anyhow!("override object should have one item")
            ));
        }
        let (key, value) = o.into_iter().next().context("must have one item")?;
        let export_context = ExportContext::try_from(value.clone())
            .map_err(|e| ImportError::InvalidConvexValue(lineno, e))?;
        overrides.insert(
            DeveloperDocumentId::decode(key)
                .map_err(|e| ImportError::InvalidConvexValue(lineno, e.into()))?,
            export_context,
        );

        lineno += 1;
    }
    let generated_schema = GeneratedSchema::LegacyInferred {
        inferred_shape: inferred_type,
        overrides,
    };
    Ok(generated_schema)
}

// For now, we only parse out floats and strings in CSV files.
pub fn parse_csv_cell(s: &str) -> JsonValue {
    if let Ok(r) = s.parse::<f64>() {
        return json!(r);
    }
    json!(s)
}
