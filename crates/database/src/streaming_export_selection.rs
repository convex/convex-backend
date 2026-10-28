//! Allows to select what is included in a streaming export.
//!
//! Fivetran allows users to select which "schemas" (= Convex components),
//! tables and columns they want to include in a streaming export. This module
//! provides the [`StreamingExportSelection`] type, which allows to express such
//! a selection.
//!
//! For instance, here would be how to only include the `_id` and `name` fields
//! of the `users` table and nothing else:
//!
//! ```
//! # #![feature(try_blocks)]
//! # use common::{
//! #     components::ComponentPath,
//! #     document::{CreationTime, DeveloperDocument, InternalId},
//! #     pii::PII,
//! # };
//! # use maplit::btreemap;
//! # use sync_types::Timestamp;
//! # use value::{
//! #     assert_obj,
//! #     DeveloperDocumentId,
//! #     TableNumber,
//! # };
//! # use database::streaming_export_selection::{
//! #     StreamingExportColumnInclusion,
//! #     StreamingExportColumnSelection,
//! #     StreamingExportComponentSelection,
//! #     StreamingExportDocument,
//! #     StreamingExportInclusionDefault,
//! #     StreamingExportSelection,
//! #     StreamingExportTableSelection,
//! # };
//! # fn main() -> anyhow::Result<()> {
//! let selection = StreamingExportSelection {
//!     components: btreemap! {
//!         ComponentPath::root() => StreamingExportComponentSelection::Included {
//!             tables: btreemap! {
//!                 "users".parse()? => StreamingExportTableSelection::Included(
//!                     StreamingExportColumnSelection::new(
//!                         /* columns: */ btreemap! {
//!                             "_id".parse()? => StreamingExportColumnInclusion::Included,
//!                             "name".parse()? => StreamingExportColumnInclusion::Included,
//!                         },
//!                         /* other_columns: */ StreamingExportInclusionDefault::Excluded,
//!                     )?,
//!                 ),
//!             },
//!             other_tables: StreamingExportInclusionDefault::Excluded,
//!         },
//!     },
//!     other_components: StreamingExportInclusionDefault::Excluded,
//! };
//!
//! assert!(selection.is_table_included(&ComponentPath::root(), &"users".parse()?));
//! assert!(!selection.is_table_included(&ComponentPath::root(), &"groups".parse()?));
//! assert!(!selection.is_table_included(&"other_component".parse()?, &"users".parse()?));
//!
//! // This can also be used to filter documents
//! let doc_id = DeveloperDocumentId::new(TableNumber::try_from(1)?, InternalId::MIN);
//! let doc = DeveloperDocument::new(
//!     doc_id.clone(),
//!     CreationTime::try_from(Timestamp::MIN)?,
//!     assert_obj!("_id" => doc_id.encode(), "name" => "Nicolas", "age" => 23),
//! );
//!
//! let column_filter = selection.column_filter(&ComponentPath::root(), &"users".parse()?)?;
//! let filtered = column_filter.filter_document(doc)?;
//! assert_eq!(
//!     filtered,
//!     StreamingExportDocument::new(
//!         doc_id,
//!         PII(assert_obj!("_id" => doc_id.encode(), "name" => "Nicolas")),
//!     )?,
//! );
//! # Ok(())
//! # }
//! ```

use std::collections::BTreeMap;

use anyhow::{
    bail,
    Context,
};
use common::{
    components::ComponentPath,
    document::{
        DeveloperDocument,
        ID_FIELD,
    },
    pii::PII,
    types::streaming_export::selection as serialized,
};
use serde_json::Value as JsonValue;
use value::{
    export::ValueFormat,
    ConvexObject,
    DeveloperDocumentId,
    FieldName,
    Size,
    TableName,
};

pub struct StreamingExportSelection {
    /// For each listed component, defines what to do with it in the
    /// streaming export.
    pub components: BTreeMap<ComponentPath, StreamingExportComponentSelection>,

    /// Whether to include components that are not listed in `components`.
    pub other_components: StreamingExportInclusionDefault,
}

/// Defines what to do in streaming export for the components/tables/columns
/// that are not explicitly listed.
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum StreamingExportInclusionDefault {
    /// Exclude these items in streaming export.
    Excluded,
    /// Include these items in streaming export, including all of their
    /// descendants. For instance, if applied to a component, all tables
    /// and columns in that component will be included.
    Included,
}

impl Default for StreamingExportSelection {
    fn default() -> Self {
        // By default, includes the entire deployment.
        Self {
            components: BTreeMap::new(),
            other_components: StreamingExportInclusionDefault::Included,
        }
    }
}

impl StreamingExportSelection {
    /// Know whether to include a given table in the streaming export.
    pub fn is_table_included(&self, component: &ComponentPath, table: &TableName) -> bool {
        match self.components.get(component) {
            Some(component_selection) => component_selection.is_table_included(table),
            None => self.other_components == StreamingExportInclusionDefault::Included,
        }
    }

    /// Get the [`StreamingExportColumnSelection`] for a given table.
    ///
    /// This should only be called for a table that is included in the
    /// streaming export. Otherwise, an error is returned.
    pub fn column_filter(
        &self,
        component: &ComponentPath,
        table: &TableName,
    ) -> anyhow::Result<&StreamingExportColumnSelection> {
        match (self.components.get(component), self.other_components) {
            (Some(component_selection), _) => component_selection.column_filter(table),
            (None, StreamingExportInclusionDefault::Included) => Ok(&ALL_COLUMNS),
            (None, StreamingExportInclusionDefault::Excluded) => {
                anyhow::bail!("Getting column filter for an implicitly excluded component")
            },
        }
    }

}

/// What to do during streaming export for a particular component.
pub enum StreamingExportComponentSelection {
    Excluded,
    Included {
        tables: BTreeMap<TableName, StreamingExportTableSelection>,
        other_tables: StreamingExportInclusionDefault,
    },
}

impl StreamingExportComponentSelection {
    fn is_table_included(&self, table: &TableName) -> bool {
        match self {
            Self::Excluded => false,
            Self::Included {
                tables,
                other_tables,
            } => match tables.get(table) {
                Some(StreamingExportTableSelection::Excluded) => false,
                Some(StreamingExportTableSelection::Included { .. }) => true,
                None => other_tables == &StreamingExportInclusionDefault::Included,
            },
        }
    }

    fn column_filter(&self, table: &TableName) -> anyhow::Result<&StreamingExportColumnSelection> {
        Ok(match self {
            StreamingExportComponentSelection::Included {
                tables,
                other_tables,
            } => match (tables.get(table), other_tables) {
                (Some(StreamingExportTableSelection::Included(filter)), _) => filter,
                (None, StreamingExportInclusionDefault::Included) => &ALL_COLUMNS,
                _ => bail!("Getting column filter for an excluded table"),
            },
            StreamingExportComponentSelection::Excluded => {
                bail!("Getting column filter for an explicitly excluded component")
            },
        })
    }
}

/// What to do in streaming export for a particular table.
pub enum StreamingExportTableSelection {
    Excluded,
    Included(StreamingExportColumnSelection),
}

/// For a table in the streaming export, defines what to do for each of its
/// columns.
pub struct StreamingExportColumnSelection {
    columns: BTreeMap<FieldName, StreamingExportColumnInclusion>,
    other_columns: StreamingExportInclusionDefault,
}

/// Defines what to do for a particular column in the streaming export.
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum StreamingExportColumnInclusion {
    Excluded,
    Included,
}

static ALL_COLUMNS: StreamingExportColumnSelection = StreamingExportColumnSelection::all_columns();

impl StreamingExportColumnSelection {
    pub fn new(
        columns: BTreeMap<FieldName, StreamingExportColumnInclusion>,
        other_columns: StreamingExportInclusionDefault,
    ) -> anyhow::Result<Self> {
        let column_selection = Self {
            columns,
            other_columns,
        };

        anyhow::ensure!(
            column_selection.includes(&FieldName::from(ID_FIELD.clone())),
            "`_id` must be included in the column selection"
        );

        Ok(column_selection)
    }

    /// Create a [`StreamingExportColumnSelection`] that includes all columns.
    pub const fn all_columns() -> Self {
        Self {
            columns: BTreeMap::new(),
            other_columns: StreamingExportInclusionDefault::Included,
        }
    }

    /// Filter a [`DeveloperDocument`] to only include the columns that are
    /// included in this column selection.
    pub fn filter_document(
        &self,
        document: DeveloperDocument,
    ) -> anyhow::Result<StreamingExportDocument> {
        let id = document.id();
        let value = document.into_value().0;

        let filtered_value = value.filter_fields(|field| self.includes(field));

        StreamingExportDocument::new(id, PII(filtered_value))
    }

    fn includes(&self, column: &FieldName) -> bool {
        self.columns
            .get(column)
            .cloned()
            .map(|c| c == StreamingExportColumnInclusion::Included)
            .unwrap_or(self.other_columns == StreamingExportInclusionDefault::Included)
    }
}

/// Similar to [`DeveloperDocument`], but `_creationTime` is allowed to be
/// omitted.
#[derive(Eq, PartialEq, Debug)]
pub struct StreamingExportDocument {
    id: DeveloperDocumentId,
    value: PII<ConvexObject>,
}

impl StreamingExportDocument {
    pub fn new(id: DeveloperDocumentId, value: PII<ConvexObject>) -> anyhow::Result<Self> {
        // Verify that `value` contains `_id`
        anyhow::ensure!(
            value.0.get(&FieldName::from(ID_FIELD.clone()))
                == Some(
                    &id.encode()
                        .try_into()
                        .context("Can't serialize the ID as a Convex string")?
                ),
            "`_id` must be included in the value"
        );

        Ok(Self { id, value })
    }

    pub fn size(&self) -> usize {
        self.value.0.size()
    }

    pub fn export_fields(
        self,
        value_format: ValueFormat,
    ) -> anyhow::Result<BTreeMap<String, JsonValue>> {
        let map = match self.value.0.export(value_format) {
            JsonValue::Object(map) => map,
            _ => {
                return Err(anyhow::anyhow!(
                    "Unexpectedly serialized a Convex document as a non-object JSON value"
                ));
            },
        };

        Ok(map.into_iter().collect())
    }
}

impl TryFrom<serialized::Selection> for StreamingExportSelection {
    type Error = anyhow::Error;

    fn try_from(
        serialized::Selection {
            components,
            other_components,
        }: serialized::Selection,
    ) -> Result<Self, Self::Error> {
        Ok(Self {
            components: components
                .into_iter()
                .map(|(k, v)| -> anyhow::Result<_> { Ok((k.parse()?, v.try_into()?)) })
                .try_collect()?,
            other_components: other_components.into(),
        })
    }
}

impl From<serialized::ColumnSelection> for StreamingExportColumnInclusion {
    fn from(value: serialized::ColumnSelection) -> Self {
        match value {
            serialized::ColumnSelection::Excluded => Self::Excluded,
            serialized::ColumnSelection::Included => Self::Included,
        }
    }
}

impl TryFrom<serialized::ComponentSelection> for StreamingExportComponentSelection {
    type Error = anyhow::Error;

    fn try_from(value: serialized::ComponentSelection) -> Result<Self, Self::Error> {
        Ok(match value {
            serialized::ComponentSelection::Excluded(_) => Self::Excluded,
            serialized::ComponentSelection::Included {
                tables,
                other_tables,
            } => Self::Included {
                tables: tables
                    .into_iter()
                    .map(|(k, v)| -> anyhow::Result<_> { Ok((k.parse()?, v.try_into()?)) })
                    .try_collect()?,
                other_tables: other_tables.into(),
            },
        })
    }
}

impl TryFrom<serialized::TableSelection> for StreamingExportTableSelection {
    type Error = anyhow::Error;

    fn try_from(value: serialized::TableSelection) -> Result<Self, Self::Error> {
        Ok(match value {
            serialized::TableSelection::Excluded(_) => Self::Excluded,
            serialized::TableSelection::Included {
                columns,
                other_columns,
            } => {
                // `new` (rather than a struct literal) so deserialized
                // selections can't bypass the `_id`-must-be-included check.
                let column_selection = StreamingExportColumnSelection::new(
                    columns
                        .into_iter()
                        .map(|(k, v)| -> anyhow::Result<_> { Ok((k.parse()?, v.into())) })
                        .try_collect()?,
                    other_columns.into(),
                )?;

                Self::Included(column_selection)
            },
        })
    }
}

impl From<serialized::InclusionDefault> for StreamingExportInclusionDefault {
    fn from(value: serialized::InclusionDefault) -> Self {
        match value {
            serialized::InclusionDefault::Excluded => Self::Excluded,
            serialized::InclusionDefault::Included => Self::Included,
        }
    }
}
