use std::collections::BTreeMap;

use maplit::btreemap;
use serde::{
    Deserialize,
    Serialize,
};
use utoipa::ToSchema;
use super::SelectionArg;

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, ToSchema)]
#[schema(example = json!({
    "_other": "excluded",
    "": {
        "_other": "excluded",
        "posts": {"_other": "included"},
        "users": {"_other": "included", "ssn": "excluded"},
    },
}))]
pub struct Selection {
    /// Set of components to include/exclude in sync.
    ///
    /// Mapping from the component path to the inclusion/exclusion.
    /// Use the empty string to represent the root component.
    #[serde(flatten)]
    pub components: BTreeMap<
        String, // The component name ("" for the default component)
        ComponentSelection,
    >,
    /// Whether components not explicitly listed are exported
    #[serde(rename = "_other")]
    pub other_components: InclusionDefault,
}

#[derive(Clone, Copy, Eq, PartialEq, Debug, Serialize, Deserialize, ToSchema)]
pub enum InclusionDefault {
    #[serde(alias = "excl", rename = "excluded")]
    Excluded,
    #[serde(alias = "incl", rename = "included")]
    Included,
}

impl Default for Selection {
    fn default() -> Self {
        // By default, include everything.
        Self {
            components: BTreeMap::new(),
            other_components: InclusionDefault::Included,
        }
    }
}

/// The literal string `"excluded"`, excluding the item entirely.
#[derive(Clone, Copy, Eq, PartialEq, Debug, Serialize, Deserialize, ToSchema)]
#[schema(title = "Excluded")]
pub enum ExcludedTag {
    #[serde(alias = "excl", rename = "excluded")]
    Excluded,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, ToSchema)]
#[serde(untagged)]
pub enum ComponentSelection {
    /// Export some of this component's tables.
    #[schema(title = "Included")]
    Included {
        /// Set of tables to include/exclude in sync within the component.
        ///
        /// Mapping from the table name to the inclusion/exclusion.
        #[serde(flatten)]
        tables: BTreeMap<String, TableSelection>,
        /// Whether tables not explicitly listed are exported
        #[serde(rename = "_other")]
        other_tables: InclusionDefault,
    },
    #[schema(title = "Excluded")]
    /// Exclude this component entirely.
    Excluded(ExcludedTag),
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, ToSchema)]
#[serde(untagged)]
pub enum TableSelection {
    #[schema(title = "Included")]
    Included {
        /// Set of columns to include/exclude in sync within the table.
        ///
        /// Mapping from the column name to the inclusion/exclusion. `_id`
        /// cannot be excluded.
        #[serde(flatten)]
        columns: BTreeMap<String, ColumnSelection>,
        /// Whether columns not explicitly listed are exported
        #[serde(rename = "_other")]
        other_columns: InclusionDefault,
    },
    #[schema(title = "Excluded")]
    /// Exclude this table entirely.
    Excluded(ExcludedTag),
}

impl TableSelection {
    pub fn included_with_all_columns() -> Self {
        Self::Included {
            columns: BTreeMap::new(),
            other_columns: InclusionDefault::Included,
        }
    }
}

/// Whether the column is exported.
// Serializable version of `StreamingExportColumnInclusion`.
#[derive(Clone, Copy, Eq, PartialEq, Debug, Serialize, Deserialize, ToSchema)]
pub enum ColumnSelection {
    #[serde(alias = "excl", rename = "excluded")]
    Excluded,
    #[serde(alias = "incl", rename = "included")]
    Included,
}

impl From<SelectionArg> for Selection {
    fn from(arg: SelectionArg) -> Self {
        match arg {
            SelectionArg::Exact { selection } => selection,
            SelectionArg::SingleTable {
                table_name,
                component,
            } => Self {
                components: btreemap! {
                    component.unwrap_or(String::from("")) => ComponentSelection::Included {
                        tables: btreemap! {
                            table_name => TableSelection::included_with_all_columns(),
                        },
                        other_tables: InclusionDefault::Excluded,
                    },
                },
                other_components: InclusionDefault::Excluded,
            },
            SelectionArg::SingleComponent { component } => Self {
                components: btreemap! {
                    component => ComponentSelection::Included {
                        tables: BTreeMap::new(),
                        other_tables: InclusionDefault::Included,
                    },
                },
                other_components: InclusionDefault::Excluded,
            },
            SelectionArg::Everything {} => Self {
                components: BTreeMap::new(),
                other_components: InclusionDefault::Included,
            },
        }
    }
}
