use anyhow::anyhow;
use serde_json::Value as JsonValue;
use value::TableMappingValue;

use super::OpProvider;

#[convex_macro::op]
pub fn op_get_table_mapping<P: OpProvider>(provider: &mut P) -> anyhow::Result<JsonValue> {
    let mapping: TableMappingValue = provider.get_all_table_mappings()?.to_value(true);
    serde_json::to_value(mapping).map_err(|_| anyhow!("Couldn’t serialize the table mapping"))
}
