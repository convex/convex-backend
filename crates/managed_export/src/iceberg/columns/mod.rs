//! Typed columns: one Iceberg column per top-level document field, after the
//! metadata columns every table has. A new table's columns come from its
//! enforced validator ([`schema_seeds`]) or else its shape ([`seeds`]); later
//! fields come from the pages that carry them ([`new_columns`]). A value that
//! does not fit its column goes to `_rescued`, so the shape of the data never
//! fails a page.

use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    sync::Arc,
};

use anyhow::{
    bail,
    Context,
    Result,
};
use arrow_iceberg::{
    builder::{
        BooleanBuilder,
        Float64Builder,
        Int64Builder,
        LargeBinaryBuilder,
        StringBuilder,
        TimestampMicrosecondBuilder,
    },
    ArrayRef,
    RecordBatch,
};
use common::{
    schemas::{
        validator::{
            LiteralValidator,
            Validator,
        },
        DocumentSchema,
    },
    shapes::reduced::ReducedShape,
};
pub use iceberg::transaction::AddColumn;
use iceberg::{
    arrow::{
        schema_to_arrow_schema,
        UTC_TIME_ZONE,
    },
    metadata_columns,
    spec::{
        NestedField,
        PrimitiveType,
        Schema,
        Type,
    },
    table::Table,
    transaction::{
        ApplyTransactionAction,
        Transaction,
    },
    Catalog,
};
use serde_json::Value as JsonValue;
use value::{
    export::ValueFormat,
    identifier::is_valid_identifier,
    ConvexValue,
    TableName,
    TableNumber,
    MAX_OBJECT_FIELDS,
};

use super::Change;

pub(crate) const ID_COLUMN: &str = "_id";
pub(crate) const TS_COLUMN: &str = "_ts";
// Iceberg reserves `_deleted`; see `RESERVED_COLUMNS`.
pub(crate) const DELETED_COLUMN: &str = "_is_deleted";
/// The document's `_creationTime`, as an Iceberg `timestamptz` so readers get
/// a timestamp instead of epoch milliseconds in a `double`.
pub(crate) const CREATION_TIME_COLUMN: &str = "_creationTime";
pub(crate) const RESCUED_COLUMN: &str = "_rescued";
const METADATA_COLUMNS: [&str; 5] = [
    ID_COLUMN,
    TS_COLUMN,
    DELETED_COLUMN,
    CREATION_TIME_COLUMN,
    RESCUED_COLUMN,
];
/// The largest magnitude an `int64` keeps exactly in a `double` column.
const EXACT_IN_DOUBLE: u64 = 1 << 53;
/// Names Iceberg readers resolve to their own metadata columns before the
/// table's, so a document field with one of these names is rescued. Iceberg
/// also reserves `file_path` and `pos`, but only inside delete files.
const RESERVED_COLUMNS: [&str; 10] = [
    metadata_columns::RESERVED_COL_NAME_FILE,
    metadata_columns::RESERVED_COL_NAME_POS,
    metadata_columns::RESERVED_COL_NAME_DELETED,
    metadata_columns::RESERVED_COL_NAME_SPEC_ID,
    metadata_columns::RESERVED_COL_NAME_PARTITION,
    metadata_columns::RESERVED_COL_NAME_CHANGE_TYPE,
    metadata_columns::RESERVED_COL_NAME_CHANGE_ORDINAL,
    metadata_columns::RESERVED_COL_NAME_COMMIT_SNAPSHOT_ID,
    metadata_columns::RESERVED_COL_NAME_ROW_ID,
    metadata_columns::RESERVED_COL_NAME_LAST_UPDATED_SEQUENCE_NUMBER,
];
/// Doc of a string column that holds JSON: arrays, objects, and fields whose
/// values have more than one kind. Every value fits such a column as its JSON
/// encoding, while a plain string column takes strings only.
pub(crate) const JSON_DOC: &str = "convex:json";
/// Doc prefix of a string column that holds Ids of one table, which the shape
/// knows and a page's values do not. `convex:id:users` points at `users`.
pub(crate) const ID_DOC_PREFIX: &str = "convex:id:";
/// The lossless, readable encoding that the data sync API and snapshot
/// exports use: int64 is a JSON number, bytes are `{"$bytes": ...}`.
const JSON_FORMAT: ValueFormat = ValueFormat::ConvexExportJSON;

/// The schema of a table before its first document adds field columns.
pub(crate) fn metadata_schema() -> Result<Schema> {
    Ok(Schema::builder()
        .with_fields(vec![
            NestedField::required(1, ID_COLUMN, Type::Primitive(PrimitiveType::String)).into(),
            NestedField::required(2, TS_COLUMN, Type::Primitive(PrimitiveType::Long)).into(),
            NestedField::required(3, DELETED_COLUMN, Type::Primitive(PrimitiveType::Boolean))
                .into(),
            // Optional because a tombstone has no document.
            NestedField::optional(
                4,
                CREATION_TIME_COLUMN,
                Type::Primitive(PrimitiveType::Timestamptz),
            )
            .into(),
            NestedField::optional(5, RESCUED_COLUMN, Type::Primitive(PrimitiveType::String))
                .with_doc(JSON_DOC)
                .into(),
        ])
        .build()?)
}

/// The column kinds the export writes. Strings and JSON share the Iceberg
/// `string` type and differ by doc.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    String,
    Double,
    Long,
    Boolean,
    Binary,
    Json,
}

impl Kind {
    /// `None` for null, which fits every column.
    fn of(value: &ConvexValue) -> Option<Self> {
        Some(match value {
            ConvexValue::Null => return None,
            ConvexValue::String(_) => Self::String,
            ConvexValue::Float64(_) => Self::Double,
            ConvexValue::Int64(_) => Self::Long,
            ConvexValue::Boolean(_) => Self::Boolean,
            ConvexValue::Bytes(_) => Self::Binary,
            ConvexValue::Array(_) | ConvexValue::Object(_) => Self::Json,
        })
    }

    fn of_field(field: &NestedField) -> Result<Self> {
        Ok(match &*field.field_type {
            Type::Primitive(PrimitiveType::String) if field.doc.as_deref() == Some(JSON_DOC) => {
                Self::Json
            },
            Type::Primitive(PrimitiveType::String) => Self::String,
            Type::Primitive(PrimitiveType::Double) => Self::Double,
            Type::Primitive(PrimitiveType::Long) => Self::Long,
            Type::Primitive(PrimitiveType::Boolean) => Self::Boolean,
            Type::Primitive(PrimitiveType::Binary) => Self::Binary,
            other => bail!("Column {} has unsupported type {other}", field.name),
        })
    }

    fn column(self, name: &str, doc: Option<&str>) -> AddColumn {
        let primitive = match self {
            Self::String | Self::Json => PrimitiveType::String,
            Self::Double => PrimitiveType::Double,
            Self::Long => PrimitiveType::Long,
            Self::Boolean => PrimitiveType::Boolean,
            Self::Binary => PrimitiveType::Binary,
        };
        let column = AddColumn::builder()
            .name(name)
            .field_type(Type::Primitive(primitive));
        match (self, doc) {
            (Self::Json, _) => column.doc(JSON_DOC).build(),
            (_, Some(doc)) => column.doc(doc).build(),
            (_, None) => column.build(),
        }
    }

    /// The kind of the values `validator` accepts. `None` for null, which
    /// decides no column, and for a union, whose members [`flatten`] yields.
    fn of_validator(validator: &Validator) -> Option<Self> {
        Some(match validator {
            Validator::Null | Validator::Union(_) => return None,
            // A function handle is a string.
            Validator::Id(_) | Validator::String | Validator::Function(_) => Self::String,
            Validator::Float64 => Self::Double,
            // A committed `CommitTs` is an `Int64`.
            Validator::Int64 | Validator::CommitTs => Self::Long,
            Validator::Boolean => Self::Boolean,
            Validator::Bytes => Self::Binary,
            Validator::Literal(literal) => match literal {
                LiteralValidator::String(_) => Self::String,
                LiteralValidator::Float64(_) => Self::Double,
                LiteralValidator::Int64(_) => Self::Long,
                LiteralValidator::Boolean(_) => Self::Boolean,
            },
            Validator::Array(_) | Validator::Object(_) | Validator::Record(..) | Validator::Any => {
                Self::Json
            },
        })
    }

    /// The kinds a shape's values have. Nothing for null, never and unknown,
    /// which decide no column.
    fn of_shape(shape: &ReducedShape, kinds: &mut BTreeSet<Kind>) {
        kinds.insert(match shape {
            ReducedShape::Unknown | ReducedShape::Never | ReducedShape::Null => return,
            ReducedShape::Union(members) => {
                for member in members {
                    Self::of_shape(member, kinds);
                }
                return;
            },
            ReducedShape::Id(_) | ReducedShape::String => Self::String,
            ReducedShape::Int64 => Self::Long,
            ReducedShape::Float64(_) => Self::Double,
            ReducedShape::Boolean => Self::Boolean,
            ReducedShape::Bytes => Self::Binary,
            ReducedShape::Object(_) | ReducedShape::Array(_) | ReducedShape::Record { .. } => {
                Self::Json
            },
        });
    }
}

/// Where a document field's value lands.
enum Target<'a> {
    Column(&'a str),
    /// No column yet; [`new_columns`] adds one unless the table is at the cap.
    New,
    Rescue,
}

/// Whether a document field may have its own column. Glue, Athena and
/// BigQuery reject names that are not plain identifiers, and the metadata and
/// Iceberg-reserved names are taken.
fn accepts_name(name: &str) -> bool {
    is_valid_identifier(name)
        && !METADATA_COLUMNS.contains(&name)
        && !RESERVED_COLUMNS.contains(&name)
}

/// Where a non-null `value` of field `name` lands. Glue, Athena and BigQuery
/// treat column names as case-insensitive, so a field that differs from a
/// column only by case is rescued. A `double` column takes an `int64` it can
/// hold exactly.
fn target<'a>(schema: &'a Schema, name: &str, value: &ConvexValue) -> Result<Target<'a>> {
    if !accepts_name(name) {
        return Ok(Target::Rescue);
    }
    Ok(match schema.field_by_name_case_insensitive(name) {
        None => Target::New,
        Some(field) if field.name != name => Target::Rescue,
        Some(field) => match (Kind::of_field(field)?, value) {
            (Kind::Json, _) => Target::Column(&field.name),
            (Kind::Double, ConvexValue::Int64(n)) if n.unsigned_abs() <= EXACT_IN_DOUBLE => {
                Target::Column(&field.name)
            },
            (column, _) if Some(column) == Kind::of(value) => Target::Column(&field.name),
            _ => Target::Rescue,
        },
    })
}

/// Adds the columns `changes` need to `table` and returns the table that has
/// them.
pub(crate) async fn add_columns(
    catalog: &dyn Catalog,
    table: &Table,
    changes: &[Change],
) -> Result<Table> {
    let columns = new_columns(table.metadata().current_schema(), changes)?;
    commit_columns(catalog, table, columns).await
}

/// Adds `columns` to `table` and returns the table that has them. Iceberg
/// assigns the new field IDs at commit, and the Parquet writer matches
/// columns by ID, so this commits before any data is written.
pub(crate) async fn commit_columns(
    catalog: &dyn Catalog,
    table: &Table,
    columns: Vec<AddColumn>,
) -> Result<Table> {
    if columns.is_empty() {
        return Ok(table.clone());
    }
    let tx = Transaction::new(table);
    let mut action = tx.update_schema();
    for column in columns {
        action = action.add_column(column);
    }
    Ok(action.apply(tx)?.commit(catalog).await?)
}

/// The columns `schema` lacks for the fields in `changes`. Null values decide
/// nothing.
pub(crate) fn new_columns(schema: &Schema, changes: &[Change]) -> Result<Vec<AddColumn>> {
    let mut kinds: BTreeMap<&str, BTreeSet<Kind>> = BTreeMap::new();
    for document in changes.iter().filter_map(|c| c.document.as_ref()) {
        for (name, value) in document.iter() {
            let name: &str = name;
            let Some(kind) = Kind::of(value) else {
                continue;
            };
            if matches!(target(schema, name, value)?, Target::New) {
                kinds.entry(name).or_default().insert(kind);
            }
        }
    }
    Ok(choose(schema, kinds, &BTreeMap::new()))
}

/// The columns a new table starts with, from its shape. The shape is a sound
/// supertype of every live document, so a column typed from it fits the whole
/// backfill, where a column typed from the first page fits only that sample.
/// `table_name` resolves the table an Id field points at, for its doc. Fields
/// the shape does not resolve are left to [`new_columns`].
pub fn seeds(
    shape: &ReducedShape,
    table_name: impl Fn(TableNumber) -> Option<TableName>,
) -> Result<Vec<AddColumn>> {
    let fields = match shape {
        ReducedShape::Object(fields) => Some(fields),
        // One document with too many fields or a key that is not an identifier
        // makes the table a union of an object and a record. The object still
        // describes every other document.
        ReducedShape::Union(members) => members.iter().find_map(|member| match member {
            ReducedShape::Object(fields) => Some(fields),
            _ => None,
        }),
        _ => None,
    };
    let Some(fields) = fields else {
        return Ok(vec![]);
    };
    let mut kinds: BTreeMap<&str, BTreeSet<Kind>> = BTreeMap::new();
    let mut docs: BTreeMap<&str, String> = BTreeMap::new();
    for (name, field) in fields {
        let name: &str = name;
        if !accepts_name(name) {
            continue;
        }
        let mut field_kinds = BTreeSet::new();
        Kind::of_shape(&field.shape, &mut field_kinds);
        if field_kinds.is_empty() {
            continue;
        }
        if let Some(table) = id_target(&field.shape).and_then(&table_name) {
            docs.insert(name, format!("{ID_DOC_PREFIX}{table}"));
        }
        kinds.insert(name, field_kinds);
    }
    Ok(choose(&metadata_schema()?, kinds, &docs))
}

/// The columns a new table starts with, from its enforced validator, or `None`
/// when the validator accepts any document. The validator covers every value
/// the table may hold, where the shape covers only the live documents: a
/// union field gets a JSON column before its second kind arrives, and an
/// optional field gets a column before any document sets it.
pub fn schema_seeds(document: &DocumentSchema) -> Result<Option<Vec<AddColumn>>> {
    let DocumentSchema::Union(objects) = document else {
        return Ok(None);
    };
    let mut members: BTreeMap<&str, Vec<&Validator>> = BTreeMap::new();
    for object in objects {
        for (name, field) in &object.0 {
            let name: &str = name;
            if accepts_name(name) {
                flatten(&field.validator, members.entry(name).or_default());
            }
        }
    }
    let mut kinds: BTreeMap<&str, BTreeSet<Kind>> = BTreeMap::new();
    let mut docs: BTreeMap<&str, String> = BTreeMap::new();
    for (name, members) in members {
        let field_kinds: BTreeSet<Kind> = members
            .iter()
            .filter_map(|v| Kind::of_validator(v))
            .collect();
        if field_kinds.is_empty() {
            continue;
        }
        let mut non_null = members.iter().filter(|v| !matches!(v, Validator::Null));
        if let Some(Validator::Id(table)) = non_null.next()
            && non_null.all(|v| matches!(v, Validator::Id(t) if t == table))
        {
            docs.insert(name, format!("{ID_DOC_PREFIX}{table}"));
        }
        kinds.insert(name, field_kinds);
    }
    Ok(Some(choose(&metadata_schema()?, kinds, &docs)))
}

/// The non-union validators `validator` is a union of.
fn flatten<'a>(validator: &'a Validator, out: &mut Vec<&'a Validator>) {
    match validator {
        Validator::Union(members) => {
            for member in members {
                flatten(member, out);
            }
        },
        other => out.push(other),
    }
}

/// The table a shape's Ids point at, when every non-null value is such an Id.
fn id_target(shape: &ReducedShape) -> Option<TableNumber> {
    match shape {
        ReducedShape::Id(table) => Some(*table),
        ReducedShape::Union(members) => {
            let mut values = members.iter().filter(|m| **m != ReducedShape::Null);
            match (values.next(), values.next()) {
                (Some(ReducedShape::Id(table)), None) => Some(*table),
                _ => None,
            }
        },
        _ => None,
    }
}

/// One column per field in `kinds` that `schema` has room for. A field whose
/// values all have one kind gets that kind; one with both `int64` and
/// `float64` gets a `double` column, as Fivetran does; one with any other mix
/// gets a JSON column. Seeded and page-inferred tables type a field the same
/// way because both come through here.
fn choose(
    schema: &Schema,
    kinds: BTreeMap<&str, BTreeSet<Kind>>,
    docs: &BTreeMap<&str, String>,
) -> Vec<AddColumn> {
    let fields = schema.as_struct().fields();
    // The cap counts field columns, after the metadata columns. A document has
    // at most `MAX_OBJECT_FIELDS` top-level fields, so a table whose documents
    // share a schema never fills up. A table that uses fields as map keys
    // does, and its further fields go to `_rescued`.
    let mut room =
        MAX_OBJECT_FIELDS.saturating_sub(fields.len().saturating_sub(METADATA_COLUMNS.len()));
    let mut taken: BTreeSet<String> = fields.iter().map(|f| f.name.to_lowercase()).collect();
    let mut columns = vec![];
    for (name, kinds) in kinds {
        // Two new fields that differ only by case get one column, for the
        // name that sorts first.
        if room == 0 || !taken.insert(name.to_lowercase()) {
            continue;
        }
        let kind = match kinds.len() {
            1 => *kinds.first().expect("one kind"),
            _ if kinds == BTreeSet::from([Kind::Double, Kind::Long]) => Kind::Double,
            _ => Kind::Json,
        };
        columns.push(kind.column(name, docs.get(name).map(String::as_str)));
        room -= 1;
    }
    columns
}

/// `changes` as one Arrow batch with `schema`'s columns. A tombstone fills
/// only the metadata columns.
pub(crate) fn batch(schema: &Schema, changes: &[Change]) -> Result<RecordBatch> {
    let fields = schema.as_struct().fields();
    let mut ids = StringBuilder::new();
    let mut ts = Int64Builder::new();
    let mut deleted = BooleanBuilder::new();
    let mut created = TimestampMicrosecondBuilder::new().with_timezone(UTC_TIME_ZONE);
    let mut rescued = StringBuilder::new();
    let mut builders: BTreeMap<&str, Builder> = fields
        .iter()
        .filter(|f| !METADATA_COLUMNS.contains(&f.name.as_str()))
        .map(|f| Ok((f.name.as_str(), Builder::new(Kind::of_field(f)?))))
        .collect::<Result<_>>()?;
    for change in changes {
        ids.append_value(&change.id);
        ts.append_value(i64::try_from(change.ts)?);
        deleted.append_value(change.document.is_none());
        let mut placed: BTreeMap<&str, &ConvexValue> = BTreeMap::new();
        let mut misfits = serde_json::Map::new();
        let mut creation_ms = None;
        if let Some(document) = &change.document {
            for (name, value) in document.iter() {
                let name: &str = name;
                if name == ID_COLUMN {
                    continue;
                }
                if name == CREATION_TIME_COLUMN {
                    // Convex stores every document with a finite float
                    // `_creationTime`, so anything else is a backend bug.
                    let ConvexValue::Float64(ms) = value else {
                        bail!("Document {} has a non-float _creationTime", change.id);
                    };
                    creation_ms = Some(*ms);
                    continue;
                }
                if Kind::of(value).is_none() {
                    // A null fits any column. A field with no column keeps
                    // its null in `_rescued`, so `{"f": null}` and `{}` stay
                    // distinct.
                    if schema.field_by_name(name).is_none() {
                        misfits.insert(name.to_owned(), JsonValue::Null);
                    }
                    continue;
                }
                match target(schema, name, value)? {
                    Target::Column(column) => {
                        placed.insert(column, value);
                    },
                    Target::New | Target::Rescue => {
                        misfits.insert(name.to_owned(), value.clone().export(JSON_FORMAT));
                    },
                }
            }
        }
        created.append_option(creation_ms.map(|ms| (ms * 1000.0).round() as i64));
        rescued
            .append_option((!misfits.is_empty()).then(|| JsonValue::Object(misfits).to_string()));
        for (name, builder) in &mut builders {
            builder.append(placed.get(name).copied())?;
        }
    }
    let mut arrays: Vec<ArrayRef> = Vec::with_capacity(fields.len());
    for field in fields {
        arrays.push(match field.name.as_str() {
            ID_COLUMN => Arc::new(ids.finish()),
            TS_COLUMN => Arc::new(ts.finish()),
            DELETED_COLUMN => Arc::new(deleted.finish()),
            CREATION_TIME_COLUMN => Arc::new(created.finish()),
            RESCUED_COLUMN => Arc::new(rescued.finish()),
            name => builders
                .remove(name)
                .with_context(|| format!("Column {name} has no builder"))?
                .finish(),
        });
    }
    Ok(RecordBatch::try_new(
        Arc::new(schema_to_arrow_schema(schema)?),
        arrays,
    )?)
}

enum Builder {
    String(StringBuilder),
    Double(Float64Builder),
    Long(Int64Builder),
    Boolean(BooleanBuilder),
    Binary(LargeBinaryBuilder),
    Json(StringBuilder),
}

impl Builder {
    fn new(kind: Kind) -> Self {
        match kind {
            Kind::String => Self::String(StringBuilder::new()),
            Kind::Double => Self::Double(Float64Builder::new()),
            Kind::Long => Self::Long(Int64Builder::new()),
            Kind::Boolean => Self::Boolean(BooleanBuilder::new()),
            Kind::Binary => Self::Binary(LargeBinaryBuilder::new()),
            Kind::Json => Self::Json(StringBuilder::new()),
        }
    }

    /// `value` has already been matched to this column's kind by [`target`].
    fn append(&mut self, value: Option<&ConvexValue>) -> Result<()> {
        match (self, value) {
            (Self::String(b), Some(ConvexValue::String(s))) => b.append_value(&**s),
            (Self::Double(b), Some(ConvexValue::Float64(f))) => b.append_value(*f),
            (Self::Double(b), Some(ConvexValue::Int64(n))) => b.append_value(*n as f64),
            (Self::Long(b), Some(ConvexValue::Int64(i))) => b.append_value(*i),
            (Self::Boolean(b), Some(ConvexValue::Boolean(v))) => b.append_value(*v),
            (Self::Binary(b), Some(ConvexValue::Bytes(v))) => b.append_value(&**v),
            (Self::Json(b), Some(value)) => {
                b.append_value(serde_json::to_string(&value.clone().export(JSON_FORMAT))?)
            },
            (Self::String(b) | Self::Json(b), None) => b.append_null(),
            (Self::Double(b), None) => b.append_null(),
            (Self::Long(b), None) => b.append_null(),
            (Self::Boolean(b), None) => b.append_null(),
            (Self::Binary(b), None) => b.append_null(),
            (_, Some(value)) => bail!("A {} does not fit its column", value.type_name()),
        }
        Ok(())
    }

    fn finish(self) -> ArrayRef {
        match self {
            Self::String(mut b) | Self::Json(mut b) => Arc::new(b.finish()),
            Self::Double(mut b) => Arc::new(b.finish()),
            Self::Long(mut b) => Arc::new(b.finish()),
            Self::Boolean(mut b) => Arc::new(b.finish()),
            Self::Binary(mut b) => Arc::new(b.finish()),
        }
    }
}
