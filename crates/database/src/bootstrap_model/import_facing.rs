use anyhow::Context;
use common::{
    document::{
        CreationTime,
        ResolvedDocument,
        CREATION_TIME_FIELD,
        ID_FIELD,
    },
    runtime::Runtime,
};
use errors::ErrorMetadata;
use keybroker::DeploymentOp;
use roles::RequireDeploymentOp;
use value::{
    ConvexObject,
    ConvexValue,
    DeveloperDocumentId,
    FieldName,
    ResolvedDocumentId,
    TableMapping,
    TableName,
    TabletIdAndTableNumber,
};

use crate::{
    defaults::bootstrap_system_tables,
    SchemaModel,
    Transaction,
};

/// `ImportFacingModel` is similar to `UserFacingModel` but with a few
/// differences for insertions:
/// - the table for insertion is chosen by table id, not table name or number.
/// - we allow the insertion to choose its document ID.
/// - nonexistent tables won't be created implicitly.
/// - the _creationTime may be user-specified.
/// - only admin/system auth is allowed.
pub struct ImportFacingModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> ImportFacingModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    /// Inserts a new document as part of a snapshot import.
    #[convex_macro::instrument_future]
    pub async fn insert(
        &mut self,
        table_id: TabletIdAndTableNumber,
        table_name: &TableName,
        value: ConvexObject,
        table_mapping_for_schema: &TableMapping,
    ) -> anyhow::Result<DeveloperDocumentId> {
        if self
            .tx
            .virtual_system_mapping()
            .is_virtual_table(table_name)
        {
            anyhow::bail!(ErrorMetadata::bad_request(
                "ReadOnlyTable",
                format!("{table_name} is a read-only table"),
            ));
        }
        anyhow::ensure!(
            bootstrap_system_tables()
                .iter()
                .all(|t| t.table_name() != *table_name),
            "Cannot import into bootstrap system table {table_name}"
        );
        self.tx
            .identity
            .require_operation(DeploymentOp::ImportBackups)?;

        let id_field = FieldName::from(ID_FIELD.clone());
        let internal_id = if let Some(ConvexValue::String(s)) = value.get(&id_field) {
            let id_v6 = DeveloperDocumentId::decode(s).context(ErrorMetadata::bad_request(
                "InvalidId",
                format!("invalid _id '{s}'"),
            ))?;
            anyhow::ensure!(
                id_v6.table() == table_id.table_number,
                ErrorMetadata::bad_request(
                    "ImportConflict",
                    format!(
                        "_id {s} cannot be imported into '{table_name}' because it came from a \
                         different deployment and conflict with preexisting tables in this \
                         deployment. Try deleting preexisting tables or importing into an empty
                         deployment."
                    )
                )
            );
            id_v6.internal_id()
        } else {
            self.tx.id_generator.generate_internal()
        };
        let id = ResolvedDocumentId::new(
            table_id.tablet_id,
            DeveloperDocumentId::new(table_id.table_number, internal_id),
        );
        let namespace = self
            .tx
            .table_mapping()
            .tablet_namespace(table_id.tablet_id)?;

        let creation_time_field = FieldName::from(CREATION_TIME_FIELD.clone());
        let creation_time = if let Some(ConvexValue::Float64(f)) = value.get(&creation_time_field) {
            CreationTime::try_from(*f)?
        } else {
            self.tx.next_creation_time.increment()?
        };

        let document = ResolvedDocument::new(id, creation_time, value)?;
        SchemaModel::new(self.tx, namespace)
            .enforce_with_table_mapping(&document, &table_mapping_for_schema.namespace(namespace))
            .await?;
        self.tx
            .apply_validated_write(id, None, Some(document.into()))?;

        Ok(id.into())
    }

    #[convex_macro::instrument_future]
    pub async fn upsert(
        &mut self,
        table_id: TabletIdAndTableNumber,
        table_name: &TableName,
        value: ConvexObject,
        table_mapping_for_schema: &TableMapping,
    ) -> anyhow::Result<DeveloperDocumentId> {
        if self
            .tx
            .virtual_system_mapping()
            .is_virtual_table(table_name)
        {
            anyhow::bail!(ErrorMetadata::bad_request(
                "ReadOnlyTable",
                format!("{table_name} is a read-only table"),
            ));
        }
        anyhow::ensure!(
            bootstrap_system_tables()
                .iter()
                .all(|t| t.table_name() != *table_name),
            "Cannot import into bootstrap system table {table_name}"
        );
        self.tx
            .identity
            .require_operation(DeploymentOp::ImportBackups)?;

        let id_field = FieldName::from(ID_FIELD.clone());
        let developer_id = if let Some(ConvexValue::String(s)) = value.get(&id_field) {
            let id_v6 = DeveloperDocumentId::decode(s).context(ErrorMetadata::bad_request(
                "InvalidId",
                format!("invalid _id '{s}'"),
            ))?;
            anyhow::ensure!(
                id_v6.table() == table_id.table_number,
                ErrorMetadata::bad_request(
                    "ImportConflict",
                    format!(
                        "_id {s} cannot be imported into '{table_name}' because its IDs have a \
                         different format"
                    )
                )
            );
            id_v6
        } else {
            anyhow::bail!("upsert requires _id field");
        };
        let id = ResolvedDocumentId::new(table_id.tablet_id, developer_id);
        let namespace = self
            .tx
            .table_mapping()
            .tablet_namespace(table_id.tablet_id)?;

        let existing_doc = self.tx.get_with_ts(id).await?;

        let creation_time_field = FieldName::from(CREATION_TIME_FIELD.clone());
        let creation_time = if let Some(ConvexValue::Float64(f)) = value.get(&creation_time_field) {
            CreationTime::try_from(*f)?
        } else {
            self.tx.next_creation_time.increment()?
        };

        let document = ResolvedDocument::new(id, creation_time, value)?;
        SchemaModel::new(self.tx, namespace)
            .enforce_with_table_mapping(&document, &table_mapping_for_schema.namespace(namespace))
            .await?;
        self.tx
            .apply_validated_write(id, existing_doc, Some(document.into()))?;

        Ok(id.into())
    }

    pub async fn delete(
        &mut self,
        table_id: TabletIdAndTableNumber,
        table_name: &TableName,
        developer_id: DeveloperDocumentId,
    ) -> anyhow::Result<()> {
        if self
            .tx
            .virtual_system_mapping()
            .is_virtual_table(table_name)
        {
            anyhow::bail!(ErrorMetadata::bad_request(
                "ReadOnlyTable",
                format!("{table_name} is a read-only table"),
            ));
        }
        anyhow::ensure!(
            bootstrap_system_tables()
                .iter()
                .all(|t| t.table_name() != *table_name),
            "Cannot import into bootstrap system table {table_name}"
        );
        self.tx
            .identity
            .require_operation(DeploymentOp::ImportBackups)?;

        let id = ResolvedDocumentId::new(table_id.tablet_id, developer_id);
        let existing_doc = self.tx.get_with_ts(id).await?;

        self.tx.apply_validated_write(id, existing_doc, None)?;

        Ok(())
    }
}
