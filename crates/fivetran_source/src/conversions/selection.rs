//! Conversions between Fivetran's selection format and the deployment-agnostic
//! streaming export [`Selection`] type.

use common::types::streaming_export::selection::{
    ColumnSelection,
    ComponentSelection,
    ExcludedTag,
    InclusionDefault,
    Selection,
    TableSelection,
};
use fivetran_common::fivetran_sdk::{
    selection::Selection as FivetranSelection,
    SchemaSelection as FivetranSchemaSelection,
    Selection as FivetranRootSelection,
    TableSelection as FivetranTableSelection,
    TablesWithSchema as FivetranSelectionWithSchema,
};

/// The name of the Fivetran schema that we use for the Convex tables in the
/// root component (i.e. the “database name” users see for the Convex root
/// component)
pub const DEFAULT_FIVETRAN_SCHEMA_NAME: &str = "app";

pub fn selection_from_fivetran(value: Option<FivetranRootSelection>) -> anyhow::Result<Selection> {
    selection_from_fivetran_selection(value.and_then(|val| val.selection))
}

fn selection_from_fivetran_selection(
    value: Option<FivetranSelection>,
) -> anyhow::Result<Selection> {
    match value {
        None => Ok(Selection::default()),
        Some(FivetranSelection::WithSchema(with_schema)) => {
            Ok(selection_from_with_schema(with_schema))
        },
        Some(FivetranSelection::WithoutSchema(_)) => {
            anyhow::bail!("Fivetran unexpectedly sent a selection setting without a schema.")
        },
    }
}

fn selection_from_with_schema(value: FivetranSelectionWithSchema) -> Selection {
    Selection {
        components: value
            .schemas
            .into_iter()
            .map(|schema| {
                (
                    if schema.schema_name == DEFAULT_FIVETRAN_SCHEMA_NAME {
                        String::from("")
                    } else {
                        schema.schema_name.clone()
                    },
                    component_selection_from_schema(schema),
                )
            })
            .collect(),
        other_components: if value.include_new_schemas {
            InclusionDefault::Included
        } else {
            InclusionDefault::Excluded
        },
    }
}

fn component_selection_from_schema(value: FivetranSchemaSelection) -> ComponentSelection {
    if !value.included {
        ComponentSelection::Excluded(ExcludedTag::Excluded)
    } else {
        ComponentSelection::Included {
            tables: value
                .tables
                .into_iter()
                .map(|table| {
                    (
                        table.table_name.clone(),
                        table_selection_from_fivetran(table),
                    )
                })
                .collect(),
            other_tables: if value.include_new_tables {
                InclusionDefault::Included
            } else {
                InclusionDefault::Excluded
            },
        }
    }
}

fn table_selection_from_fivetran(value: FivetranTableSelection) -> TableSelection {
    if !value.included {
        TableSelection::Excluded(ExcludedTag::Excluded)
    } else {
        TableSelection::Included {
            columns: value
                .columns
                .into_iter()
                .map(|(name, included)| {
                    (
                        name,
                        if included {
                            ColumnSelection::Included
                        } else {
                            ColumnSelection::Excluded
                        },
                    )
                })
                .collect(),
            other_columns: if value.include_new_columns {
                InclusionDefault::Included
            } else {
                InclusionDefault::Excluded
            },
        }
    }
}
