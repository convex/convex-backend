//! The export's commit point: one S3 object that names every table's current
//! Iceberg metadata file and the sync cursor. Writing it with a conditional
//! PUT commits a page's data and cursor together. Glue mirrors the committed
//! state for query engines; a mirror interrupted by a crash is finished by
//! the next page.

use std::collections::{
    BTreeMap,
    BTreeSet,
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
    primitives::ByteStream,
};
use errors::ErrorMetadata;
use serde::{
    Deserialize,
    Serialize,
};

use crate::iceberg::S3Destination;

const EXPORT_ID_PARAMETER: &str = "convex.export-id";
const PAGE_PARAMETER: &str = "convex.page";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportState {
    /// The `_log_sinks` document of the export that wrote this state. A
    /// different ID means the export was recreated, so the state is ignored.
    pub export_id: String,
    /// Number of pages committed by this export.
    pub page: u64,
    /// Base64 `SyncCursor` after page `page`. `None` before the first page.
    pub cursor: Option<String>,
    /// Current metadata location of each Iceberg table, by Glue table name.
    pub tables: BTreeMap<String, String>,
    /// Tables in `tables` marked retired. A page reloads every table only
    /// when the selection no longer matches this set.
    pub retired: BTreeSet<String>,
}

impl ExportState {
    pub fn fresh(export_id: &str) -> Self {
        Self {
            export_id: export_id.to_owned(),
            page: 0,
            cursor: None,
            tables: BTreeMap::new(),
            retired: BTreeSet::new(),
        }
    }

    /// The state after one more committed page.
    pub fn next(
        &self,
        cursor: String,
        tables: BTreeMap<String, String>,
        retired: BTreeSet<String>,
    ) -> Self {
        Self {
            export_id: self.export_id.clone(),
            page: self.page + 1,
            cursor: Some(cursor),
            tables,
            retired,
        }
    }

    /// Whether a Glue table with `parameters` was published by this export at
    /// page `page` or later.
    fn published(&self, parameters: &HashMap<String, String>) -> bool {
        parameters.get(EXPORT_ID_PARAMETER) == Some(&self.export_id)
            && parameters
                .get(PAGE_PARAMETER)
                .and_then(|page| page.parse::<u64>().ok())
                .is_some_and(|page| page >= self.page)
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
}

impl ExportStore {
    pub fn new(destination: &S3Destination, key: String, database: String) -> Self {
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
        }
    }

    /// Returns the state and its ETag, or `None` when no page has committed.
    pub async fn read(&self) -> Result<Option<(ExportState, String)>> {
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
        let bytes = output.body.collect().await?.into_bytes();
        let state = serde_json::from_slice(&bytes).context("Cannot parse the export state")?;
        Ok(Some((state, etag)))
    }

    /// Replaces the state only if it still has `etag`, or creates it only if
    /// it does not exist when `etag` is `None`. Another writer's commit fails
    /// this with a conflict error.
    pub async fn commit(&self, state: &ExportState, etag: Option<&str>) -> Result<()> {
        let put = self
            .s3
            .put_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .content_type("application/json")
            .body(ByteStream::from(serde_json::to_vec(state)?));
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

    /// Makes Glue match `state`. The database belongs to the deployment, so
    /// tables the state does not list are dropped. Requires one writer per
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
        for (name, location) in &state.tables {
            let Some(current) = published.remove(name) else {
                self.glue
                    .create_table()
                    .database_name(&self.database)
                    .table_input(table_input(name, location, None, state)?)
                    .send()
                    .await
                    .map_err(|e| aws_error(e, "create a Glue table"))?;
                continue;
            };
            let parameters = current.parameters.unwrap_or_default();
            let previous = parameters.get("metadata_location").map(String::as_str);
            // Most pages change few tables.
            if previous == Some(location) || state.published(&parameters) {
                continue;
            }
            let mut update = self
                .glue
                .update_table()
                .database_name(&self.database)
                .table_input(table_input(name, location, previous, state)?)
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
            if table.parameters().is_some_and(|p| state.published(p)) {
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

fn table_input(
    name: &str,
    metadata_location: &str,
    previous: Option<&str>,
    state: &ExportState,
) -> Result<TableInput> {
    let location = metadata_location
        .rsplit_once("/metadata/")
        .map(|(table, _)| table)
        .context("Metadata location is outside the table's metadata directory")?;
    let mut parameters = HashMap::from([
        ("table_type".to_owned(), "ICEBERG".to_owned()),
        ("metadata_location".to_owned(), metadata_location.to_owned()),
        (EXPORT_ID_PARAMETER.to_owned(), state.export_id.clone()),
        (PAGE_PARAMETER.to_owned(), state.page.to_string()),
    ]);
    if let Some(previous) = previous {
        parameters.insert("previous_metadata_location".to_owned(), previous.to_owned());
    }
    // The columns mirror `change_log_schema`, which never changes.
    let columns = [
        ("_id", "string"),
        ("ts", "bigint"),
        ("deleted", "boolean"),
        ("payload", "string"),
    ]
    .into_iter()
    .map(|(name, kind)| Column::builder().name(name).r#type(kind).build())
    .collect::<Result<Vec<_>, _>>()?;
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
