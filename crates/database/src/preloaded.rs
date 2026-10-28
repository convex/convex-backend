use std::{
    collections::BTreeMap,
    slice,
};

use common::{
    document::PackedDocument,
    interval::{
        BinaryKey,
        Interval,
    },
    runtime::Runtime,
    types::TabletIndexName,
};
use value::{
    values_to_bytes,
    ConvexValue,
    FieldPath,
    TableName,
};

use crate::Transaction;

pub struct PreloadedIndexRange {
    table_name: TableName,
    tablet_index_name: TabletIndexName,
    indexed_field: FieldPath,
    range: BTreeMap<Option<ConvexValue>, PackedDocument>,
}

impl PreloadedIndexRange {
    pub(crate) fn new(
        table_name: TableName,
        tablet_index_name: TabletIndexName,
        indexed_field: FieldPath,
        range: BTreeMap<Option<ConvexValue>, PackedDocument>,
    ) -> Self {
        Self {
            table_name,
            tablet_index_name,
            indexed_field,
            range,
        }
    }

    pub fn get<RT: Runtime>(
        &self,
        tx: &mut Transaction<RT>,
        key: &Option<ConvexValue>,
    ) -> anyhow::Result<Option<&PackedDocument>> {
        tx.reads.record_indexed_directly(
            self.tablet_index_name.clone(),
            vec![self.indexed_field.clone()].try_into()?,
            Interval::prefix(BinaryKey::from(values_to_bytes(slice::from_ref(key)))),
            &tx.limits,
        )?;
        let result = self.range.get(key);
        if let Some(document) = &result {
            let component_path = tx
                .component_path_for_tablet_id(*self.tablet_index_name.table())?
                .unwrap_or_default();
            tx.reads.record_read_document(
                component_path,
                self.table_name.clone(),
                document.size(),
                &tx.usage_tracker,
                &tx.virtual_system_mapping,
                &tx.limits,
            )?;
        }
        Ok(result)
    }
}
