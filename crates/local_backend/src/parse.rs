use common::components::ExportPath;
use errors::ErrorMetadata;
use sync_types::CanonicalizedUdfPath;
use value::{
    id_v6::DeveloperDocumentId,
    NamespacedTableMapping,
    ResolvedDocumentId,
    TableName,
};

pub fn parse_export_path(path: &str) -> anyhow::Result<ExportPath> {
    path.parse().map_err(|e: anyhow::Error| {
        let msg = format!("{path} is not a valid path to a Convex function. {e}");
        e.context(ErrorMetadata::bad_request(
            "BadConvexFunctionIdentifier",
            msg,
        ))
    })
}

pub fn parse_udf_path(path: &str) -> anyhow::Result<CanonicalizedUdfPath> {
    path.parse().map_err(|e: anyhow::Error| {
        let msg = format!("{path} is not a valid path to a Convex function. {e}");
        e.context(ErrorMetadata::bad_request(
            "BadConvexFunctionIdentifier",
            msg,
        ))
    })
}

pub fn invalid_id_error(table_name: &TableName) -> ErrorMetadata {
    ErrorMetadata::bad_request("InvalidId", format!("Invalid ID for table {table_name}"))
}

/// Parse a string in the format of IDv6 into a [`ResolvedDocumentId`].
pub fn parse_document_id(
    id: &str,
    table_mapping: &NamespacedTableMapping,
    table_name: &TableName,
) -> anyhow::Result<ResolvedDocumentId> {
    let id = DeveloperDocumentId::decode(id)
        .map_err(anyhow::Error::from)
        .and_then(|id| id.to_resolved(table_mapping.number_to_tablet()))
        .map_err(|e| e.context(invalid_id_error(table_name)))?;
    anyhow::ensure!(
        table_mapping.tablet_matches_name(id.tablet_id, table_name),
        invalid_id_error(table_name)
    );
    Ok(id)
}
