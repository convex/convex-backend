use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    sync::Arc,
};

use anyhow::Context;
use application::{
    airbyte_import::{
        AirbyteRecordMessage,
        AirbyteStream,
        PrimaryKey,
    },
    valid_identifier::ValidIdentifier,
};
use axum::response::IntoResponse;
use chrono::{
    DateTime,
    Utc,
};
use common::{
    self,
    components::ComponentPath,
    http::{
        extract::{
            Json,
            MtState,
        },
        ExtractRequestMetadata,
        HttpResponseError,
    },
    schemas::json::{
        DatabaseSchemaJson,
        TableDefinitionJson,
    },
};
use errors::ErrorMetadata;
use http::StatusCode;
use model::{
    fivetran_import::{
        BatchWriteRow,
        DeleteType,
    },
    snapshot_imports::types::ImportRequestor,
};
use roles::RequireDeploymentOp;
use serde::{
    Deserialize,
    Serialize,
};
use usage_tracking::FunctionUsageTracker;
use value::{
    identifier::IDENTIFIER_REQUIREMENTS,
    TableName,
    TableNamespace,
};

use crate::{
    authentication::ExtractIdentity,
    LocalAppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AirbyteImportArgs {
    tables: BTreeMap<String, AirbyteStream>,
    messages: Vec<AirbyteRecordMessage>,
}

pub async fn import_airbyte_records(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Json(AirbyteImportArgs { tables, messages }): Json<AirbyteImportArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ImportBackups)?;

    let usage = FunctionUsageTracker::new();

    let records = messages
        .into_iter()
        .map(|msg| msg.try_into())
        .collect::<anyhow::Result<_>>()?;
    let tables = tables
        .into_iter()
        .map(|(k, v)| Ok((k.parse::<ValidIdentifier<TableName>>()?.0, v.try_into()?)))
        .collect::<anyhow::Result<_>>()?;

    st.application
        .import_airbyte_records(&identity, records, tables, usage.clone())
        .await?;

    // Not tracking streaming_import
    drop(usage);

    Ok(StatusCode::OK)
}

pub async fn apply_fivetran_operations(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Json(rows): Json<Vec<BatchWriteRow>>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ImportBackups)?;

    let usage = FunctionUsageTracker::new();

    st.application
        .apply_fivetran_operations(&identity, rows, usage.clone())
        .await?;

    // Not tracking streaming_import
    drop(usage);

    Ok(StatusCode::OK)
}

pub async fn get_schema(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ImportBackups)?;
    let schema = st
        .application
        .get_schema(TableNamespace::root_component(), &identity)
        .await?;
    Ok(Json(match schema {
        None => None,
        Some(schema) => Some(DatabaseSchemaJson::try_from(Arc::unwrap_or_clone(schema))?),
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTableArgs {
    table_definition: TableDefinitionJson,
}

pub async fn fivetran_create_table(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Json(CreateTableArgs { table_definition }): Json<CreateTableArgs>,
) -> Result<StatusCode, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ImportBackups)?;
    let table_definition = table_definition.try_into()?;
    st.application
        .fivetran_create_table(&identity, table_definition)
        .await?;

    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearTableArgs {
    table_names: Vec<String>,
}

pub async fn clear_tables(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Json(ClearTableArgs { table_names }): Json<ClearTableArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ImportBackups)?;

    let usage = FunctionUsageTracker::new();

    let table_names = table_names
        .into_iter()
        .map(|t| {
            Ok((
                ComponentPath::root(),
                t.parse::<ValidIdentifier<TableName>>()?.0,
            ))
        })
        .collect::<anyhow::Result<_>>()?;

    let _num_deleted = st
        .application
        .clear_tables(
            &identity,
            request_metadata,
            table_names,
            ImportRequestor::StreamingImport,
            usage.clone(),
        )
        .await?;

    // Not tracking streaming_import
    drop(usage);

    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TruncateTableArgs {
    table_name: String,
    delete_before: Option<DateTime<Utc>>,
    delete_type: DeleteType,
}

/// Truncates the given table.
///
/// This function call corresponds to the `TruncateRequest` request sent by
/// the Fivetran destination connector.
///
/// It supports truncate requests for all rows (delete_before == None) and
/// for the rows that have a fivetran.synced value smaller than
/// `delete_before`. The deletion can either be a hard delete, or a soft
/// delete, in which case the `fivetran.deleted` attribute is set to true.
///
/// In any case, we use the index that the user created on `fivetran.synced`
/// (and `fivetran.deleted` when using soft deletes) to efficiently find the
/// rows to delete.
pub async fn fivetran_truncate_table(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Json(TruncateTableArgs {
        table_name,
        delete_before,
        delete_type,
    }): Json<TruncateTableArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ImportBackups)?;

    let usage = FunctionUsageTracker::new();

    let table_name = table_name.parse::<ValidIdentifier<TableName>>()?;

    // Full table hard deletes can be done through the optimized implementation
    if delete_before.is_none() && delete_type == DeleteType::HardDelete {
        st.application
            .clear_tables(
                &identity,
                request_metadata,
                vec![(ComponentPath::root(), table_name.0)],
                ImportRequestor::StreamingImport,
                usage.clone(),
            )
            .await?;

        // Not tracking streaming_import
        drop(usage);

        return Ok(StatusCode::OK);
    }

    st.application
        .fivetran_truncate(
            &identity,
            table_name.0,
            delete_before,
            delete_type,
            usage.clone(),
        )
        .await?;

    // Not tracking streaming_import
    drop(usage);

    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceTableArgs {
    _table_names: BTreeMap<String, String>,
}

pub async fn replace_tables(
    MtState(_st): MtState<LocalAppState>,
    ExtractIdentity(_identity): ExtractIdentity,
    Json(ReplaceTableArgs { _table_names: _ }): Json<ReplaceTableArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    // Disabled until we figure out how to make schema validation and indexes
    // work with overwrites.
    Err::<(), _>(
        anyhow::anyhow!(ErrorMetadata::bad_request(
            "OverwriteNotSupported",
            "Overwrite sync mode is not supported. Please use a different sync mode."
        ))
        .into(),
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddIndexesArgs {
    indexes: BTreeMap<String, Vec<Vec<String>>>,
}

pub async fn add_primary_key_indexes(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Json(AddIndexesArgs { indexes }): Json<AddIndexesArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ImportBackups)?;
    let indexes: BTreeMap<TableName, PrimaryKey> = indexes
        .into_iter()
        .map(|(stream, primary_key)| {
            let table_name = stream.parse::<ValidIdentifier<TableName>>()?.0;
            let primary_key =
                PrimaryKey::try_from(primary_key.clone()).context(ErrorMetadata::bad_request(
                    "InvalidPrimaryKey",
                    format!("Invalid primary key: {IDENTIFIER_REQUIREMENTS} {primary_key:?}."),
                ))?;
            Ok((table_name, primary_key))
        })
        .collect::<anyhow::Result<_>>()?;

    st.application
        .add_primary_key_indexes(&identity, indexes)
        .await?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexesReadyArgs {
    tables: BTreeSet<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexesReadyResponse {
    indexes_ready: bool,
}

pub async fn primary_key_indexes_ready(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Json(IndexesReadyArgs { tables }): Json<IndexesReadyArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ImportBackups)?;
    let table_names = tables
        .into_iter()
        .map(|t| Ok(t.parse::<ValidIdentifier<TableName>>()?.0))
        .collect::<anyhow::Result<BTreeSet<_>>>()?;
    let indexes_ready = st
        .application
        .primary_key_indexes_ready(identity, table_names)
        .await?;
    Ok(Json(IndexesReadyResponse { indexes_ready }))
}
