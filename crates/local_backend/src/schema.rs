use anyhow::Context;
use application::deploy_config::ModuleJson;
use axum::{
    debug_handler,
    extract::State,
    response::IntoResponse,
};
use common::{
    bootstrap_model::{
        index::{
            database_index::{
                DatabaseIndexSpec,
                DatabaseIndexState,
            },
            text_index::{
                TextIndexSpec,
                TextIndexState,
            },
            vector_index::{
                VectorIndexSpec,
                VectorIndexState,
            },
            IndexConfig,
            IndexMetadata,
        },
        schema::{
            invalid_schema_id,
            parse_schema_id,
            SchemaMetadata,
            SchemaState,
        },
    },
    execution_context::RequestMetadata,
    http::{
        extract::{
            Json,
            MtState,
            Path,
        },
        ExtractRequestMetadata,
        HttpResponseError,
    },
    types::IndexDiff,
};
use database::{
    IndexModel,
    SchemaModel,
};
use errors::ErrorMetadata;
use roles::RequireDeploymentOp;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::{
    json,
    Value as JsonValue,
};
use value::{
    ConvexValue,
    ResolvedDocumentId,
    TableName,
    TableNamespace,
};

use crate::{
    admin::must_be_admin_from_key,
    authentication::ExtractIdentity,
    LocalAppState,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BackfillResponse {
    state: String,
}

// When updating this, please keep `IndexMetadata`
// in `npm-packages/convex/src/cli/lib/indexes.ts` in sync
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexMetadataResponse {
    table: String,
    name: String,
    // Either:
    // - an array of fields (`string[]`) for a database index
    // - `{ searchField: string, filterFields: string[] }` for a search index
    // - `{ dimensions: number, vectorField: string, filterFields: string[] }` for a vector index
    fields: JsonValue,
    backfill: BackfillResponse,
    staged: bool,
}

impl TryFrom<IndexMetadata<TableName>> for IndexMetadataResponse {
    type Error = anyhow::Error;

    fn try_from(meta: IndexMetadata<TableName>) -> Result<Self, Self::Error> {
        let table = meta.name.table().to_string();
        let name = meta.name.descriptor().to_string();
        Ok(match meta.config {
            IndexConfig::Database {
                spec: DatabaseIndexSpec { fields },
                on_disk_state,
                persistence_index_id: _,
            } => {
                let backfill_state = match on_disk_state {
                    DatabaseIndexState::Backfilling(_) => "in_progress".to_string(),
                    // TODO(CX-3851): The result of this is used to poll for state
                    // in the CLI and also for display in the dashboard. We
                    // might consider a new value that would let us
                    // differentiate between Backfilled and Enabled in the
                    // dashboard. The CLI doesn't currently care.
                    DatabaseIndexState::Enabled | DatabaseIndexState::Backfilled { .. } => {
                        "done".to_string()
                    },
                };

                IndexMetadataResponse {
                    table,
                    name,
                    fields: JsonValue::from(ConvexValue::try_from(fields)?),
                    backfill: BackfillResponse {
                        state: backfill_state,
                    },
                    staged: on_disk_state.is_staged(),
                }
            },
            IndexConfig::Text {
                on_disk_state,
                spec:
                    TextIndexSpec {
                        search_field,
                        filter_fields,
                    },
            } => {
                let backfill_state = match on_disk_state {
                    TextIndexState::Backfilling(_) => "in_progress".to_string(),
                    // TODO(CX-3851): The result of this is used to poll for state in the CLI and
                    // also for display in the dashboard. We might consider a new value that would
                    // let us differentiate between Backfilled and SnapshottedAt in the dashboard.
                    // The CLI doesn't currently care.
                    TextIndexState::SnapshottedAt(_) | TextIndexState::Backfilled { .. } => {
                        "done".to_string()
                    },
                };
                IndexMetadataResponse {
                    table,
                    name,
                    fields: json!({
                        "searchField":  String::from(search_field),
                        "filterFields": filter_fields.into_iter().map(String::from).collect::<Vec<_>>()
                    }),
                    backfill: BackfillResponse {
                        state: backfill_state,
                    },
                    staged: on_disk_state.is_staged(),
                }
            },
            IndexConfig::Vector {
                spec:
                    VectorIndexSpec {
                        dimensions,
                        vector_field,
                        filter_fields,
                    },
                on_disk_state,
            } => {
                let backfill_state = match on_disk_state {
                    VectorIndexState::Backfilling(_) => "in_progress".to_string(),
                    VectorIndexState::Backfilled { .. } | VectorIndexState::SnapshottedAt(_) => {
                        "done".to_string()
                    },
                };
                IndexMetadataResponse {
                    table,
                    name,
                    fields: json!({
                        "dimensions": u32::from(dimensions),
                        "vectorField": String::from(vector_field),
                        "filterFields": filter_fields.into_iter().map(String::from).collect::<Vec<_>>()
                    }),
                    backfill: BackfillResponse {
                        state: backfill_state,
                    },
                    staged: on_disk_state.is_staged(),
                }
            },
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareSchemaArgs {
    bundle: ModuleJson,
    pub admin_key: String,
    dry_run: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareSchemaResponse {
    added: Vec<IndexMetadataResponse>,
    dropped: Vec<IndexMetadataResponse>,
    enabled: Vec<IndexMetadataResponse>,
    disabled: Vec<IndexMetadataResponse>,
    schema_id: String,
}

impl PrepareSchemaResponse {
    fn new(diff: IndexDiff, schema_id: ResolvedDocumentId) -> anyhow::Result<Self> {
        Ok(PrepareSchemaResponse {
            added: diff
                .added
                .into_iter()
                .map(IndexMetadataResponse::try_from)
                .try_collect()?,
            dropped: diff
                .dropped
                .into_iter()
                .map(|doc| doc.into_value())
                .map(IndexMetadataResponse::try_from)
                .try_collect()?,
            enabled: diff
                .enabled
                .into_iter()
                .map(|doc| doc.into_value())
                .map(IndexMetadataResponse::try_from)
                .try_collect()?,
            disabled: diff
                .disabled
                .into_iter()
                .map(|doc| doc.into_value())
                .map(IndexMetadataResponse::try_from)
                .try_collect()?,
            schema_id: schema_id.to_string(),
        })
    }
}

#[debug_handler]
pub async fn prepare_schema(
    State(st): State<LocalAppState>,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Json(req): Json<PrepareSchemaArgs>,
) -> Result<Json<PrepareSchemaResponse>, HttpResponseError> {
    let (response, _) = prepare_schema_handler(st, request_metadata, req).await?;
    Ok(response)
}

pub async fn prepare_schema_handler(
    st: LocalAppState,
    request_metadata: RequestMetadata,
    req: PrepareSchemaArgs,
) -> Result<(Json<PrepareSchemaResponse>, bool), HttpResponseError> {
    let bundle = req.bundle.try_into()?;
    let identity = must_be_admin_from_key(
        st.application.app_auth(),
        st.instance_name.clone(),
        req.admin_key,
    )
    .await?;
    identity.require_operation(keybroker::DeploymentOp::Deploy)?;
    let schema = match st.application.evaluate_schema(bundle).await {
        Ok(m) => m,
        Err(e) => return Err(e.into()),
    };
    let schema_validation_enabled = schema.schema_validation;
    let mut tx = st.application.begin(identity.clone()).await?;

    let dry_run = req.dry_run.unwrap_or(true);

    // Table namespace is root because this endpoint is only used in non-components
    // push.
    let table_namespace = TableNamespace::root_component();
    // In dry_run we only commit the schema, to enable CLI to check if the schema is
    // valid.
    let index_diff = if dry_run {
        let mut tx = st.application.begin(identity.clone()).await?;
        IndexModel::new(&mut tx)
            .prepare_new_and_mutated_indexes(table_namespace, &schema)
            .await?
    } else {
        IndexModel::new(&mut tx)
            .prepare_new_and_mutated_indexes(table_namespace, &schema)
            .await?
    };

    let (schema_id, schema_state) = SchemaModel::new(&mut tx, table_namespace)
        .submit_pending(schema)
        .await?;
    let should_save_new_schema = match schema_state {
        SchemaState::Pending => anyhow::Ok(true),
        SchemaState::Validated | SchemaState::Active => Ok(false),
        SchemaState::Failed { .. } | SchemaState::Overwritten => Err(anyhow::anyhow!(
            "Newly inserted pending schema cannot be failed or overwritten."
        )),
    }?;

    if index_diff.is_empty() && !should_save_new_schema {
        drop(tx);
    } else {
        let audit_events = if !dry_run && !index_diff.is_empty() {
            vec![index_diff.clone().into()]
        } else {
            vec![]
        };
        st.application
            .commit_with_audit_log_events(tx, audit_events, request_metadata, "prepare_schema")
            .await?;
    }

    Ok((
        Json(PrepareSchemaResponse::new(index_diff, schema_id)?),
        schema_validation_enabled,
    ))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SchemaStateResponse {
    indexes: Vec<IndexMetadataResponse>,
    schema_state: SchemaStateJson,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "state")]
enum SchemaStateJson {
    Pending,
    Validated,
    Active,
    #[serde(rename_all = "camelCase")]
    Failed {
        error: String,
        table_name: Option<String>,
    },
    Overwritten,
}

impl From<SchemaState> for SchemaStateJson {
    fn from(state: SchemaState) -> Self {
        match state {
            SchemaState::Pending => SchemaStateJson::Pending,
            SchemaState::Validated => SchemaStateJson::Validated,
            SchemaState::Active => SchemaStateJson::Active,
            SchemaState::Failed { error, table_name } => {
                SchemaStateJson::Failed { error, table_name }
            },
            SchemaState::Overwritten => SchemaStateJson::Overwritten,
        }
    }
}

/// Gets the current state of the indexes and schema.
pub async fn schema_state(
    MtState(st): MtState<LocalAppState>,
    Path(schema_id): Path<String>,
    ExtractIdentity(identity): ExtractIdentity,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::Deploy)?;
    let mut tx = st.application.begin(identity.clone()).await?;
    // This endpoint is only used in non-components push.
    let table_namespace = TableNamespace::root_component();
    let indexes = IndexModel::new(&mut tx)
        .get_application_indexes(table_namespace)
        .await?;
    let schema_id = parse_schema_id(&schema_id, tx.table_mapping(), table_namespace)
        .context(invalid_schema_id(&schema_id))?;

    let doc = tx.get(schema_id).await?.ok_or_else(|| {
        anyhow::anyhow!(ErrorMetadata::not_found(
            "SchemaNotFound",
            format!("Schema with id {schema_id} not found"),
        ))
    })?;
    let SchemaMetadata { state, .. } = doc.into_value().into_value().try_into()?;
    Ok(Json(SchemaStateResponse {
        indexes: indexes
            .into_iter()
            .map(|idx| idx.into_value().try_into())
            .collect::<anyhow::Result<_>>()?,
        schema_state: state.into(),
    }))
}
