use std::collections::BTreeMap;

use common::bootstrap_model::index::database_index::IndexedFields;
use const_format::formatcp;
use errors::ErrorMetadata;
use model::fivetran_import::constants::{
    METADATA_CONVEX_FIELD_NAME,
    SOFT_DELETE_CONVEX_FIELD_NAME,
};
use serde::Deserialize;
use serde_json::Value as JsonValue;
use value::{
    ConvexObject,
    ConvexValue,
    FieldName,
    FieldPath,
    IdentifierFieldName,
    TableName,
};

use crate::valid_identifier::{
    prefix_field,
    ValidIdentifier,
    IDENTIFIER_PREFIX,
};

pub const DUPLICATE_FIELD_LIMIT: usize = 3;

/// Field name for CDC deletes. See Airbyte docs: https://docs.airbyte.com/understanding-airbyte/cdc#syncing
/// When this field is present, it represents a deleted record.
const CDC_DELETED_FIELD: FieldName =
    FieldName::const_new(formatcp!("{IDENTIFIER_PREFIX}ab_cdc_deleted_at"));

/// Airbyte fields that are related to CDC are prefixed with `_ab_cdc`
static CDC_PREFIX: &str = formatcp!("{IDENTIFIER_PREFIX}ab_cdc");

#[derive(Clone, Debug, PartialEq)]
pub struct AirbyteRecord {
    table_name: TableName,
    deleted: bool,
    record: ConvexObject,
}

impl AirbyteRecord {
    pub fn table_name(&self) -> &TableName {
        &self.table_name
    }

    pub fn deleted(&self) -> bool {
        self.deleted
    }

    pub fn into_object(self) -> ConvexObject {
        self.record
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
/// Message interface for Airbyte streaming import records. Do not modify
/// without considering backwards compatibility.
pub struct AirbyteRecordMessage {
    table_name: String,
    data: JsonValue,
}

/// Change field names in a JSON object to be valid identifiers
fn valid_json(v: JsonValue) -> anyhow::Result<JsonValue> {
    let r = match v {
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) => v,
        JsonValue::Array(arr) => arr
            .into_iter()
            .map(valid_json)
            .collect::<anyhow::Result<_>>()?,
        JsonValue::Object(map) => {
            let map_clone = map.clone();
            let map = map
                .into_iter()
                .map(|(mut field, value)| {
                    let valid_identifier = field.parse::<ValidIdentifier<FieldName>>()?;
                    let new_field = valid_identifier.0.to_string();
                    let mut modified = new_field != field;
                    field = new_field;
                    for _ in 0..DUPLICATE_FIELD_LIMIT {
                        if modified != map_clone.get(&field).is_some() {
                            return Ok((field, valid_json(value)?));
                        }
                        field = prefix_field(&field);
                        modified = true;
                    }
                    Err(anyhow::anyhow!(
                        "Too many duplicate field names found for modified field {field}"
                    ))
                })
                .collect::<anyhow::Result<_>>()?;
            JsonValue::Object(map)
        },
    };
    Ok(r)
}

impl TryFrom<AirbyteRecordMessage> for AirbyteRecord {
    type Error = anyhow::Error;

    fn try_from(msg: AirbyteRecordMessage) -> anyhow::Result<AirbyteRecord> {
        let table_name = msg.table_name.parse::<ValidIdentifier<TableName>>()?.0;
        let object: ConvexObject = valid_json(msg.data)?.try_into()?;
        let deleted = match object.get(&CDC_DELETED_FIELD) {
            Some(ts) => ts != &ConvexValue::Null,
            None => false,
        };
        // Filter out CDC prefixed fields because they should not be exposed to
        // developers and collide with system field space (fields prefixed with
        // underscore are system fields in Convex).
        let fields_and_values: BTreeMap<FieldName, ConvexValue> = object
            .into_iter()
            .filter(|(field_name, _value)| !field_name.starts_with(CDC_PREFIX))
            .collect();
        let record: ConvexObject = fields_and_values.try_into()?;
        Ok(Self {
            table_name,
            deleted,
            record,
        })
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AirbyteStream {
    primary_key: Option<Vec<Vec<String>>>,
    #[expect(dead_code)]
    json_schema: JsonValue,
}

#[derive(Clone, Debug)]
pub struct PrimaryKey(IndexedFields);

impl TryFrom<Vec<Vec<String>>> for PrimaryKey {
    type Error = anyhow::Error;

    fn try_from(v: Vec<Vec<String>>) -> anyhow::Result<PrimaryKey> {
        let field_paths = v
            .into_iter()
            .map(|fields| {
                let field_names = fields
                    .into_iter()
                    .map(|f| f.parse::<IdentifierFieldName>())
                    .collect::<anyhow::Result<_>>()?;
                let field_path = FieldPath::new(field_names)?;
                Ok(field_path)
            })
            .collect::<anyhow::Result<Vec<FieldPath>>>()?;
        let index_fields = field_paths.try_into()?;
        Ok(PrimaryKey(index_fields))
    }
}

impl PrimaryKey {
    pub fn into_indexed_fields(self) -> IndexedFields {
        self.0
    }
}

#[derive(Debug)]
pub enum ValidatedAirbyteStream {
    Append,
    Dedup(PrimaryKey),
}

impl TryFrom<AirbyteStream> for ValidatedAirbyteStream {
    type Error = anyhow::Error;

    fn try_from(
        AirbyteStream {
            primary_key,
            json_schema: _,
        }: AirbyteStream,
    ) -> anyhow::Result<Self> {
        // TODO(emma): Validate schema
        match primary_key {
            None => Ok(ValidatedAirbyteStream::Append),
            Some(p) => {
                anyhow::ensure!(
                    !p.is_empty(),
                    ErrorMetadata::bad_request("EmptyPrimaryKey", "Primary keys cannot be empty")
                );
                Ok(ValidatedAirbyteStream::Dedup(p.try_into()?))
            },
        }
    }
}

pub fn mark_as_soft_deleted(object: ConvexObject) -> anyhow::Result<ConvexObject> {
    let metadata_key = FieldName::from(METADATA_CONVEX_FIELD_NAME.clone());

    let mut new_value: BTreeMap<FieldName, ConvexValue> = object.into();
    let metadata_object = match new_value.remove(&metadata_key) {
        Some(ConvexValue::Object(object)) => object,
        _ => ConvexObject::empty(),
    };

    new_value.insert(
        metadata_key,
        ConvexValue::Object(
            metadata_object.insert(SOFT_DELETE_CONVEX_FIELD_NAME.into(), true.into())?,
        ),
    );
    new_value.try_into()
}
