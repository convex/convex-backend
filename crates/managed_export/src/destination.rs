//! The export's commit point: one S3 object that names every table's current
//! Iceberg metadata file and the sync cursor. Writing it with a conditional
//! PUT commits a page's data and cursor together. Glue mirrors the committed
//! state for query engines; a mirror interrupted by a crash is finished by
//! the next page.

use std::collections::{
    BTreeMap,
    HashMap,
};

use anyhow::{
    bail,
    Context,
    Result,
};
use aws_credential_types::Credentials;
use aws_sdk_glue::types::{
    Column,
    DatabaseInput,
    StorageDescriptor,
    TableInput,
};
use aws_sdk_s3::{
    config::{
        http::HttpResponse,
        BehaviorVersion,
        Region,
    },
    error::{
        ProvideErrorMetadata,
        SdkError,
    },
    operation::get_object::GetObjectOutput,
    primitives::ByteStream,
};
use errors::ErrorMetadata;
use iceberg::spec::{
    PrimitiveType,
    Schema,
    TableMetadata,
    Type,
};
use keybroker::RandomEncryptor;
use pb::managed_export as pb_state;
use prost::Message;

use crate::iceberg::{
    S3Destination,
    TableState,
};

const EXPORT_ID_PARAMETER: &str = "convex.export-id";
const PAGE_PARAMETER: &str = "convex.page";

/// Largest size for an object that Porter can read whole from the bucket.
/// This is to prevent a malicious bucket owner from replacing the object with
/// an object of any size and OOMing Porter, which every deployment shares.
/// The state and a table's metadata are a few KB per table.
const MAX_OBJECT_BYTES: i64 = 16 * (1 << 20); // 16 MiB

/// One export's progress, sealed at `<root>/_convex/exports/<export id>/state`
/// in the customer's bucket. The export ID is in the key, so a recreated
/// export starts empty instead of reading the old export's state.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportState {
    /// Number of pages committed by this export.
    pub page: u64,
    /// Base64 `SyncCursor` after page `page`. `None` before the first page.
    pub cursor: Option<String>,
    /// Every table ever created, live and retired, by Glue name.
    pub tables: BTreeMap<String, TableState>,
}

impl ExportState {
    /// A state with no pages.
    pub fn fresh() -> Self {
        Self {
            page: 0,
            cursor: None,
            tables: BTreeMap::new(),
        }
    }

    /// The number of the page after `page`. Since `page` is customer-writable,
    /// a malicious actor could cause an overflow. We check against the
    /// overflow here.
    pub fn next_page(&self) -> Result<u64> {
        self.page
            .checked_add(1)
            .context("The export state's page counter overflowed")
    }

    /// The state after one more committed page.
    pub fn next(&self, cursor: String, tables: BTreeMap<String, TableState>) -> Result<Self> {
        Ok(Self {
            page: self.next_page()?,
            cursor: Some(cursor),
            tables,
        })
    }

    /// Whether `publish` should drop a Glue table with `parameters` that the
    /// state does not list: one a Convex export wrote, and not this export at
    /// this page or later.
    fn is_stale(&self, export_id: &str, parameters: Option<&HashMap<String, String>>) -> bool {
        parameters
            .is_some_and(|p| p.contains_key(EXPORT_ID_PARAMETER) && !self.published(export_id, p))
    }

    /// Whether `export_id` published this Glue table at `page` or later.
    fn published(&self, export_id: &str, parameters: &HashMap<String, String>) -> bool {
        parameters.get(EXPORT_ID_PARAMETER).map(String::as_str) == Some(export_id)
            && parameters
                .get(PAGE_PARAMETER)
                .and_then(|page| page.parse::<u64>().ok())
                .is_some_and(|page| page >= self.page)
    }
}

impl From<ExportState> for pb_state::ExportState {
    fn from(state: ExportState) -> Self {
        Self {
            page: state.page,
            cursor: state.cursor,
            tables: state
                .tables
                .into_iter()
                .map(|(name, table)| (name, table.into()))
                .collect(),
        }
    }
}

impl TryFrom<pb_state::ExportState> for ExportState {
    type Error = anyhow::Error;

    fn try_from(state: pb_state::ExportState) -> Result<Self> {
        Ok(Self {
            page: state.page,
            cursor: state.cursor,
            tables: state
                .tables
                .into_iter()
                .map(|(name, table)| (name, table.into()))
                .collect(),
        })
    }
}

impl From<TableState> for pb_state::TableState {
    fn from(table: TableState) -> Self {
        Self {
            component: table.component,
            table: table.table,
            tablet: table.tablet,
            metadata: table.metadata,
            retired: table.retired,
        }
    }
}

impl From<pb_state::TableState> for TableState {
    fn from(table: pb_state::TableState) -> Self {
        Self {
            component: table.component,
            table: table.table,
            tablet: table.tablet,
            metadata: table.metadata,
            retired: table.retired,
        }
    }
}

/// The state object in S3 and its Glue mirror, in the destination's account
/// and region with the destination's credentials.
pub struct ExportStore {
    s3: aws_sdk_s3::Client,
    glue: aws_sdk_glue::Client,
    bucket: String,
    key: String,
    database: String,
    /// Stamped on every Glue table so `publish` can tell our tables from a
    /// deleted export's.
    export_id: String,
}

impl ExportStore {
    pub fn new(
        destination: &S3Destination,
        key: String,
        database: String,
        export_id: String,
    ) -> Self {
        let credentials = Credentials::new(
            destination.access_key_id.clone(),
            destination.secret_access_key.clone(),
            destination.session_token.clone(),
            None,
            "convex-export",
        );
        let mut s3 = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(destination.region.clone()))
            .credentials_provider(credentials.clone());
        let mut glue = aws_sdk_glue::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(destination.region.clone()))
            .credentials_provider(credentials);
        // Glue exists only in AWS, so a custom endpoint is a local mock
        // serving both S3 and Glue.
        if let Some(endpoint) = &destination.endpoint_url {
            s3 = s3.endpoint_url(endpoint).force_path_style(true);
            glue = glue.endpoint_url(endpoint);
        }
        Self {
            s3: aws_sdk_s3::Client::from_conf(s3.build()),
            glue: aws_sdk_glue::Client::from_conf(glue.build()),
            bucket: destination.bucket.clone(),
            key,
            database,
            export_id,
        }
    }

    /// Returns the state and its ETag, or `None` when no page has committed.
    /// The customer can write this object, so only bytes `encryptor` opens
    /// get parsed.
    pub async fn read(&self, encryptor: &RandomEncryptor) -> Result<Option<(ExportState, String)>> {
        let output = match self
            .s3
            .get_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .send()
            .await
        {
            Ok(output) => output,
            Err(error) if error.as_service_error().is_some_and(|e| e.is_no_such_key()) => {
                return Ok(None);
            },
            Err(error) => return Err(aws_error(error, "read the export state")),
        };
        let etag = output.e_tag.clone().context("Export state has no ETag")?;
        let bytes = collect_bounded(output, "the export state").await?;
        let state = unseal_state(encryptor, &self.key, bytes)?;
        Ok(Some((state, etag)))
    }

    /// Replaces the state only if it still has `etag`, or creates it only if
    /// it does not exist when `etag` is `None`. Another writer's commit fails
    /// this with a conflict error.
    pub async fn commit(
        &self,
        state: &ExportState,
        etag: Option<&str>,
        encryptor: &RandomEncryptor,
    ) -> Result<()> {
        let message = pb_state::ExportState::from(state.clone());
        let bytes = encryptor.encrypt_bytes(self.key.as_bytes(), message.encode_to_vec());
        let put = self
            .s3
            .put_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .content_type("application/octet-stream")
            .body(ByteStream::from(bytes));
        let put = match etag {
            Some(etag) => put.if_match(etag),
            None => put.if_none_match("*"),
        };
        match put.send().await {
            Ok(_) => Ok(()),
            // S3 answers 412 to a failed precondition and 409 when a
            // concurrent write wins.
            Err(SdkError::ServiceError(error))
                if matches!(error.raw().status().as_u16(), 409 | 412) =>
            {
                bail!(ErrorMetadata::conflict(
                    "ExportStateConflict",
                    "Another writer changed the export state",
                ))
            },
            Err(error) => Err(aws_error(error, "write the export state")),
        }
    }

    /// Makes Glue match `state`. Tables a Convex export wrote that the state
    /// does not list are dropped; anything else the customer keeps in this
    /// database is left alone. Requires one writer per
    /// database: a table already at this export's page or later is left
    /// alone, so a slow mirror cannot move Glue backward.
    pub async fn publish(&self, state: &ExportState) -> Result<()> {
        self.ensure_database().await?;
        let mut published = BTreeMap::new();
        let mut pages = self
            .glue
            .get_tables()
            .database_name(&self.database)
            .into_paginator()
            .send();
        while let Some(page) = pages.next().await {
            let page = page.map_err(|e| aws_error(e, "list the Glue tables"))?;
            for table in page.table_list() {
                published.insert(table.name().to_owned(), table.clone());
            }
        }
        for (name, table) in &state.tables {
            let location = &table.metadata;
            let Some(current) = published.remove(name) else {
                let columns = self.read_glue_columns(location).await?;
                self.glue
                    .create_table()
                    .database_name(&self.database)
                    .table_input(table_input(
                        name,
                        location,
                        None,
                        columns,
                        &self.export_id,
                        state,
                    )?)
                    .send()
                    .await
                    .map_err(|e| aws_error(e, "create a Glue table"))?;
                continue;
            };
            let parameters = current.parameters.unwrap_or_default();
            // A table no Convex export wrote is the customer's, even when its
            // name matches ours.
            anyhow::ensure!(
                parameters.contains_key(EXPORT_ID_PARAMETER),
                ErrorMetadata::bad_request(
                    "S3ExportForeignTable",
                    format!(
                        "Glue table {name} was not written by a Convex export. Remove it from the \
                         export's Glue database or recreate the export."
                    ),
                )
            );
            let previous = parameters.get("metadata_location").map(String::as_str);
            // Most pages change few tables.
            if previous == Some(location) || state.published(&self.export_id, &parameters) {
                continue;
            }
            let columns = self.read_glue_columns(location).await?;
            let mut update = self
                .glue
                .update_table()
                .database_name(&self.database)
                .table_input(table_input(
                    name,
                    location,
                    previous,
                    columns,
                    &self.export_id,
                    state,
                )?)
                .skip_archive(true);
            if let Some(version_id) = current.version_id {
                update = update.version_id(version_id);
            }
            update
                .send()
                .await
                .map_err(|e| aws_error(e, "update a Glue table"))?;
        }
        for (name, table) in published {
            if !state.is_stale(&self.export_id, table.parameters()) {
                continue;
            }
            self.glue
                .delete_table()
                .database_name(&self.database)
                .name(name)
                .send()
                .await
                .map_err(|e| aws_error(e, "drop a Glue table"))?;
        }
        Ok(())
    }

    /// The Glue columns for the table whose current metadata file is at
    /// `metadata_location`. Athena reads the Iceberg schema itself, but the
    /// Glue console, crawlers and Lake Formation show the Glue columns.
    async fn read_glue_columns(&self, metadata_location: &str) -> Result<Vec<Column>> {
        let key = metadata_location
            .strip_prefix(&format!("s3://{}/", self.bucket))
            .with_context(|| {
                format!("Metadata location {metadata_location} is outside the bucket")
            })?;
        let output = self
            .s3
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| aws_error(e, "read a table's metadata"))?;
        let bytes = collect_bounded(output, "a table's metadata").await?;
        let metadata: TableMetadata =
            serde_json::from_slice(&bytes).context("Cannot parse a table's metadata")?;
        glue_columns(metadata.current_schema())
    }

    async fn ensure_database(&self) -> Result<()> {
        let result = self
            .glue
            .create_database()
            .database_input(DatabaseInput::builder().name(&self.database).build()?)
            .send()
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(error)
                if error
                    .as_service_error()
                    .is_some_and(|e| e.is_already_exists_exception()) =>
            {
                Ok(())
            },
            Err(error) => Err(aws_error(error, "create the Glue database")),
        }
    }
}

/// AWS error codes that mean the customer's bucket, role or region is set up
/// wrong. Retrying does not fix them; the customer has to.
const DESTINATION_ERROR_CODES: &[&str] = &[
    "AccessDenied",
    "AccessDeniedException",
    "AuthorizationHeaderMalformed",
    "NoSuchBucket",
    "PermanentRedirect",
];

/// Opens and parses the state. Only Porter can have sealed it, and only for
/// this `key` (it holds the export ID), so a state copied from a deleted or
/// sibling export does not open. Anything else means: recreate the export.
fn unseal_state(encryptor: &RandomEncryptor, key: &str, bytes: Vec<u8>) -> Result<ExportState> {
    let plain = encryptor
        .decrypt_bytes(key.as_bytes(), bytes)
        .map_err(|error| {
            error.context(ErrorMetadata::bad_request(
                "S3ExportStateModified",
                "The export state in the bucket was not written by Convex or was modified. \
                 Recreate the export to start it again.",
            ))
        })?;
    pb_state::ExportState::decode(&*plain)
        .context("Cannot parse the export state")?
        .try_into()
}

/// The body of `output`, refused before any byte is read when S3 reports it
/// larger than [`MAX_OBJECT_BYTES`].
async fn collect_bounded(output: GetObjectOutput, what: &str) -> Result<Vec<u8>> {
    let length = output.content_length().context("S3 object has no length")?;
    if length > MAX_OBJECT_BYTES {
        bail!(ErrorMetadata::bad_request(
            "S3ExportObjectTooLarge",
            format!(
                "Convex cannot read {what}: it is {length} bytes, more than the \
                 {MAX_OBJECT_BYTES} byte limit"
            ),
        ));
    }
    Ok(output.body.collect().await?.to_vec())
}

/// Wraps an S3 or Glue error. An error the customer must fix in AWS becomes a
/// user error, so the export shows it instead of retrying silently.
fn aws_error<E>(error: SdkError<E, HttpResponse>, action: &str) -> anyhow::Error
where
    E: ProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
{
    let code = error.code().map(str::to_owned);
    let detail = error.message().map(str::to_owned);
    let error = anyhow::Error::new(error).context(format!("Cannot {action}"));
    match code {
        Some(code) if DESTINATION_ERROR_CODES.contains(&code.as_str()) => {
            error.context(ErrorMetadata::bad_request(
                "S3ExportDestinationError",
                format!(
                    "AWS did not let Convex {action}: {code}{}",
                    detail.map(|d| format!(" ({d})")).unwrap_or_default()
                ),
            ))
        },
        _ => error,
    }
}

/// Glue's Hive type names for the primitive types the export writes. Nested
/// values are stored as JSON strings, so nested types never appear.
fn glue_columns(schema: &Schema) -> Result<Vec<Column>> {
    schema
        .as_struct()
        .fields()
        .iter()
        .map(|field| {
            let kind = match &*field.field_type {
                Type::Primitive(PrimitiveType::String) => "string",
                Type::Primitive(PrimitiveType::Long) => "bigint",
                Type::Primitive(PrimitiveType::Double) => "double",
                Type::Primitive(PrimitiveType::Boolean) => "boolean",
                Type::Primitive(PrimitiveType::Binary) => "binary",
                Type::Primitive(PrimitiveType::Timestamptz) => "timestamp",
                other => bail!("Column {} has unsupported type {other}", field.name),
            };
            Ok(Column::builder()
                .name(&field.name)
                .r#type(kind)
                .set_comment(field.doc.clone())
                .build()?)
        })
        .collect()
}

fn table_input(
    name: &str,
    metadata_location: &str,
    previous: Option<&str>,
    columns: Vec<Column>,
    export_id: &str,
    state: &ExportState,
) -> Result<TableInput> {
    let location = metadata_location
        .rsplit_once("/metadata/")
        .map(|(table, _)| table)
        .context("Metadata location is outside the table's metadata directory")?;
    let mut parameters = HashMap::from([
        ("table_type".to_owned(), "ICEBERG".to_owned()),
        ("metadata_location".to_owned(), metadata_location.to_owned()),
        (EXPORT_ID_PARAMETER.to_owned(), export_id.to_owned()),
        (PAGE_PARAMETER.to_owned(), state.page.to_string()),
    ]);
    if let Some(previous) = previous {
        parameters.insert("previous_metadata_location".to_owned(), previous.to_owned());
    }
    Ok(TableInput::builder()
        .name(name)
        .table_type("EXTERNAL_TABLE")
        .set_parameters(Some(parameters))
        .storage_descriptor(
            StorageDescriptor::builder()
                .location(location)
                .set_columns(Some(columns))
                .build(),
        )
        .build()?)
}
