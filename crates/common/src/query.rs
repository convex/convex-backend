//! Types for querying a database.

use std::{
    collections::BTreeMap,
    fmt::Display,
    io::Write,
    ops::{
        Bound,
        Deref,
    },
};

use derive_more::{
    From,
    Into,
};
use errors::ErrorMetadata;
use itertools::{
    Either,
    Itertools,
};
use pb::convex_cursor::{
    cursor::Position as PositionProto,
    IndexKey as IndexKeyProto,
};
use serde::Serialize;
use serde_json::Value as JsonValue;
use sha2::{
    Digest,
    Sha256,
};
use value::{
    heap_size::HeapSize,
    id_v6::DeveloperDocumentId,
    utils::display_sequence,
    val,
    ConvexObject,
    ConvexValue,
    TabletId,
};

use crate::{
    bootstrap_model::index::database_index::IndexedFields,
    document::ID_FIELD_PATH,
    index::IndexKeyBytes,
    interval::{
        BinaryKey,
        End,
        Interval,
        StartIncluded,
    },
    paths::FieldPath,
    types::{
        GenericIndexName,
        IndexName,
        MaybeValue,
        TableName,
        TabletIndexName,
    },
    value::{
        sha256::Sha256 as CommonSha256,
        values_to_bytes,
    },
};
/// Serialized cursor representation for sending to clients.
pub type SerializedCursor = String;

/// A hash of the query that's included in cursors.
pub type QueryFingerprint = Vec<u8>;

/// A `CursorPosition` is a position within query results used to implement
/// `paginate()`.
#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum CursorPosition {
    After(IndexKeyBytes),
    End,
}

impl HeapSize for CursorPosition {
    fn heap_size(&self) -> usize {
        match self {
            CursorPosition::After(bytes) => bytes.heap_size(),
            CursorPosition::End => 0,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Cursor {
    pub position: CursorPosition,

    /// Hashed representation of the query this cursor refers to.
    pub query_fingerprint: QueryFingerprint,
}

impl From<Cursor> for pb::convex_cursor::Cursor {
    fn from(
        Cursor {
            position,
            query_fingerprint,
        }: Cursor,
    ) -> Self {
        let position = match position {
            CursorPosition::End => PositionProto::End(()),
            CursorPosition::After(ref key) => PositionProto::After(IndexKeyProto {
                values: key.clone().0,
            }),
        };
        Self {
            position: Some(position),
            query_fingerprint: Some(query_fingerprint),
        }
    }
}

impl TryFrom<pb::convex_cursor::Cursor> for Cursor {
    type Error = anyhow::Error;

    fn try_from(
        pb::convex_cursor::Cursor {
            position,
            query_fingerprint,
        }: pb::convex_cursor::Cursor,
    ) -> anyhow::Result<Self> {
        let position = position.ok_or_else(|| anyhow::anyhow!("Cursor is missing position"))?;
        let position = match position {
            pb::convex_cursor::cursor::Position::After(index_key) => {
                CursorPosition::After(IndexKeyBytes(index_key.values))
            },
            pb::convex_cursor::cursor::Position::End(()) => CursorPosition::End,
        };
        Ok(Self {
            position,
            query_fingerprint: query_fingerprint
                .ok_or_else(|| anyhow::anyhow!("Missing query_fingerprint"))?,
        })
    }
}

impl HeapSize for Cursor {
    fn heap_size(&self) -> usize {
        self.position.heap_size() + self.query_fingerprint.heap_size()
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq, PartialOrd, Ord, Debug)]
/// The order to scan a range.
pub enum Order {
    /// Ascending order, e.g. 1, 2, 3.
    Asc,
    /// Descending order, e.g. 3, 2, 1.
    Desc,
}

impl Order {
    /// Apply an ordering to an iterator, reversing it if `self == Order::Desc`.
    pub fn apply<T>(
        &self,
        iter: impl DoubleEndedIterator<Item = T>,
    ) -> impl DoubleEndedIterator<Item = T> {
        match self {
            Order::Asc => Either::Left(iter),
            Order::Desc => Either::Right(iter.rev()),
        }
    }
}

/// A range of an index to query.
#[derive(Clone, Debug, PartialEq)]
pub struct IndexRange {
    /// The index being scanned.
    pub index_name: IndexName,

    /// The range of the index to scan.
    /// These expressions must be in index order, with the `Eq` expressions
    /// preceding the others (which matches what an index can actually do
    /// efficiently).
    pub range: Vec<IndexRangeExpression>,
    /// The order to scan in.
    pub order: Order,
}

impl IndexRange {
    pub fn compile(self, indexed_fields: IndexedFields) -> anyhow::Result<Interval> {
        let index_name = self.index_name.clone();
        let SplitIndexRange {
            equalities,
            inequality,
        } = self.split()?;

        // Check that some permutation of the equality field paths + the (optional)
        // inequality field path is a prefix of the indexed paths.
        let index_rank: BTreeMap<_, _> = indexed_fields
            .iter_with_id()
            .enumerate()
            .map(|(i, field_name)| (field_name, i))
            .collect();
        anyhow::ensure!(
            index_rank.len() == indexed_fields.iter_with_id().count(),
            "{index_name} has duplicate fields?"
        );

        let mut equalities: Vec<_> = equalities
            .into_iter()
            .map(|(field, value)| -> anyhow::Result<_> {
                if let Some(rank) = index_rank.get(&field) {
                    Ok((field, value, *rank))
                } else {
                    anyhow::bail!(field_not_in_index_error(
                        &index_name,
                        &field,
                        &indexed_fields
                    ))
                }
            })
            .try_collect()?;
        equalities.sort_by_key(|(_, _, rank)| *rank);

        if let Some(ref inequality) = inequality
            && !index_rank.contains_key(&inequality.field_path)
        {
            anyhow::bail!(field_not_in_index_error(
                &index_name,
                &inequality.field_path,
                &indexed_fields,
            ))
        }

        let used_paths: Vec<_> = equalities
            .iter()
            .map(|(field_path, ..)| field_path.clone())
            .chain(
                inequality
                    .as_ref()
                    .map(|inequality| inequality.field_path.clone()),
            )
            .collect();

        let query_fields = QueryFields(used_paths.clone());

        let mut fields_iter = indexed_fields.iter_with_id();
        for field_path in used_paths {
            let matching_field = fields_iter.next().ok_or_else(|| {
                invalid_index_range(&index_name, &indexed_fields, &query_fields, &field_path)
            })?;
            if field_path != *matching_field {
                anyhow::bail!(invalid_index_range(
                    &index_name,
                    &indexed_fields,
                    &query_fields,
                    &field_path,
                ));
            }
        }

        // Now that we know the index expression is compatible with the index, turn it
        // into an interval.
        let prefix: Vec<_> = equalities.into_iter().map(|(_, v, _)| v.0).collect();

        let (start, end) = match inequality {
            Some(IndexInequality {
                field_path: _,
                start,
                end,
            }) => (start, end),
            None => (Bound::Unbounded, Bound::Unbounded),
        };

        let start = match start {
            Bound::Unbounded => BinaryKey::from(values_to_bytes(&prefix)),
            Bound::Included(value) => {
                let mut bound = prefix.clone();
                bound.push(value.0);
                BinaryKey::from(values_to_bytes(&bound))
            },
            Bound::Excluded(value) => {
                let mut bound = prefix.clone();
                bound.push(value.0);
                BinaryKey::from(values_to_bytes(&bound))
                    .increment()
                    .ok_or_else(|| anyhow::anyhow!("{bound:?} should have an increment"))?
            },
        };
        let end = match end {
            Bound::Unbounded => {
                let key = BinaryKey::from(values_to_bytes(&prefix));
                if prefix.len() == indexed_fields.iter_with_id().count() {
                    // Special case: if all fields including the implicit _id field are specified,
                    // we can use a tighter bound
                    End::included(&key)
                } else {
                    End::after_prefix(&key)
                }
            },
            Bound::Included(value) => {
                let mut bound = prefix;
                bound.push(value.0);
                let key = BinaryKey::from(values_to_bytes(&bound));
                if bound.len() == indexed_fields.iter_with_id().count() {
                    End::included(&key)
                } else {
                    End::after_prefix(&key)
                }
            },
            Bound::Excluded(value) => {
                let mut bound = prefix;
                bound.push(value.0);
                End::Excluded(BinaryKey::from(values_to_bytes(&bound)))
            },
        };
        Ok(Interval {
            start: StartIncluded(start),
            end,
        })
    }

    fn split(self) -> anyhow::Result<SplitIndexRange> {
        let mut equalities = BTreeMap::new();

        let mut inequality_field_path: Option<FieldPath> = None;
        let mut inequality_start = Bound::Unbounded;
        let mut inequality_end = Bound::Unbounded;

        for expr in self.range {
            let (field_path, value, is_less, is_equals) = match expr {
                IndexRangeExpression::Eq(field_path, value) => {
                    if let Some(old_value) = equalities.insert(field_path.clone(), value) {
                        let error =
                            already_defined_bound_error("equality", &field_path, "==", &old_value);
                        anyhow::bail!(error);
                    }
                    continue;
                },
                IndexRangeExpression::Gt(field_path, value) => (field_path, value, false, false),
                IndexRangeExpression::Gte(field_path, value) => (field_path, value, false, true),
                IndexRangeExpression::Lt(field_path, value) => (field_path, value, true, false),
                IndexRangeExpression::Lte(field_path, value) => (field_path, value, true, true),
            };

            // Check that we're defining the bound for the first time.
            let destination = if is_less {
                &mut inequality_end
            } else {
                &mut inequality_start
            };
            if *destination != Bound::Unbounded {
                let bound_type = if is_less { "upper" } else { "lower" };
                let comparator = match (is_less, is_equals) {
                    (false, false) => ">",
                    (false, true) => ">=",
                    (true, false) => "<",
                    (true, true) => "<=",
                };
                let error =
                    already_defined_bound_error(bound_type, &field_path, comparator, &value);
                anyhow::bail!(error);
            }
            // Check that all of the inequalities are for the same field path.
            if let Some(ref first_path) = inequality_field_path
                && first_path != &field_path
            {
                anyhow::bail!(bounds_on_multiple_fields_error(
                    &self.index_name,
                    first_path,
                    &field_path,
                ));
            };
            inequality_field_path = Some(field_path);

            *destination = if is_equals {
                Bound::Included(value)
            } else {
                Bound::Excluded(value)
            };
        }
        if let Some(ref inequality_field_path) = inequality_field_path
            && let Some(equality_value) = equalities.get(inequality_field_path)
        {
            let error = already_defined_bound_error(
                "inequality",
                inequality_field_path,
                "==",
                equality_value,
            );
            anyhow::bail!(error);
        }

        let inequality = inequality_field_path.map(|field_path| IndexInequality {
            field_path,
            start: inequality_start,
            end: inequality_end,
        });
        let result = SplitIndexRange {
            equalities,
            inequality,
        };
        Ok(result)
    }
}

// Helper struct for the intermediate state of `IndexRange::compile`. We want to
// turn a user-specified list of index range expressions into a set of equality
// constraints and then a single inequality at the end.
struct SplitIndexRange {
    equalities: BTreeMap<FieldPath, MaybeValue>,
    inequality: Option<IndexInequality>,
}

struct IndexInequality {
    field_path: FieldPath,
    start: Bound<MaybeValue>,
    end: Bound<MaybeValue>,
}

/// A wrapper to pretty print the fields in a query for error messages.
#[derive(Clone, Debug)]
struct QueryFields(Vec<FieldPath>);

impl Display for QueryFields {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        display_sequence(f, ["[", "]"], self.0.iter())
    }
}

fn already_defined_bound_error(
    bound_type: &str,
    field_path: &FieldPath,
    comparator: &str,
    value: &MaybeValue,
) -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "AlreadyDefinedBound",
        format!(
            "Already defined {bound_type} bound in index range. Can't add {field_path:?} \
             {comparator} {value}."
        ),
    )
}

fn bounds_on_multiple_fields_error(
    index_name: &IndexName,
    first_field_path: &FieldPath,
    second_field_path: &FieldPath,
) -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "BoundsOnMultipleFields",
        format!("Upper and lower bounds in `range` can only be applied to a single index \
    field. This query against index {index_name} attempted to set a range \
    bound on both {first_field_path:?} and {second_field_path:?}. Consider using \
    `filter` instead. See https://docs.convex.dev/using/indexes for more info."),
    )
}

fn invalid_index_range(
    name: &IndexName,
    indexed_fields: &IndexedFields,
    query_fields: &QueryFields,
    field_path: &FieldPath,
) -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "InvalidIndexRange",
        format!(
            "Tried to query index {name} but the query didn't use the index fields in order.\n\
             \
             Index fields: {indexed_fields}\n\
             Query fields: {query_fields}\n\
             First incorrect field: {field_path}\n\
             \
             For more information see https://docs.convex.dev/using/indexes."
        ),
    )
}

fn field_not_in_index_error(
    index_name: &IndexName,
    field_path: &FieldPath,
    indexed_fields: &IndexedFields,
) -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "FieldNotInIndex",
        format!("The index range included a comparison with {field_path:?}, but {index_name} with fields {indexed_fields} doesn't index this field. For more information see https://docs.convex.dev/using/indexes."),
    )
}

/// A restriction on the range of an index to query.
/// These are expressed as operators on the index fields.
#[derive(Clone, Debug, PartialEq)]
pub enum IndexRangeExpression {
    Eq(FieldPath, MaybeValue),
    Gt(FieldPath, MaybeValue),
    Gte(FieldPath, MaybeValue),
    Lt(FieldPath, MaybeValue),
    Lte(FieldPath, MaybeValue),
}

/// A table to scan
#[derive(Clone, Debug, PartialEq)]
pub struct FullTableScan {
    /// The name of the table to scan
    pub table_name: TableName,

    /// The order to scan in.
    pub order: Order,
}

/// Version of full-text search to use
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum SearchVersion {
    V1,
    /// Prototype, experimental, don't use in production!
    V2,
}

/// A query against a search index.
///
/// Results are returned in relevancy order based on how well they match
/// the search filter.
#[derive(Clone, Debug, PartialEq)]
pub struct Search {
    /// The search index being queried.
    pub index_name: IndexName,
    pub table: TableName,

    /// The filters to apply within the search index.
    ///
    /// This must include exactly one `Search` expression against the
    /// index's `searchField` and any number of `Eq` expressions comparing
    /// the index's `filterFields`.
    pub filters: Vec<SearchFilterExpression>,
}

impl Search {
    pub fn to_internal(self, tablet_index_name: TabletIndexName) -> anyhow::Result<InternalSearch> {
        Ok(InternalSearch {
            index_name: tablet_index_name,
            table_name: self.table,
            filters: self
                .filters
                .into_iter()
                .map(|f| f.to_internal())
                .collect::<anyhow::Result<Vec<InternalSearchFilterExpression>>>()?,
        })
    }

    pub fn is_empty(&self) -> anyhow::Result<bool> {
        for filter in &self.filters {
            match filter {
                SearchFilterExpression::Search(_field, s) => return Ok(s.is_empty()),
                SearchFilterExpression::Eq(..) => {},
            }
        }
        anyhow::bail!(ErrorMetadata::bad_request(
            "MissingSearchFilterError",
            format!(
                "Search query against {} does not contain any search filters. You must include a \
                 search filter like `q.search(\"field\", searchText)`.",
                self.index_name,
            )
        ))
    }
}

/// While `Search` is constructed and used at the query layer using TableNames,
/// `InternalSearch` is used within transaction and searchlight and uses
/// TableIds.
#[derive(Clone, Debug, PartialEq)]
pub struct InternalSearch {
    /// The search index being queried.
    pub index_name: GenericIndexName<TabletId>,
    pub table_name: TableName,

    /// The filters to apply within the search index.
    ///
    /// This must include exactly one `Search` expression against the
    /// index's `searchField` and any number of `Eq` expressions comparing
    /// the index's `filterFields`.
    pub filters: Vec<InternalSearchFilterExpression>,
}

impl InternalSearch {
    pub fn printable_index_name(&self) -> anyhow::Result<IndexName> {
        IndexName::new(
            self.table_name.clone(),
            self.index_name.descriptor().clone(),
        )
    }
}

/// Filter field values under this size are stored as bytes. Otherwise
/// we hash them down to 32 bytes.
const MAX_FILTER_FIELD_LENGTH: usize = 32;
const UNDEFINED_TAG: u8 = 0x1;

/// A bytes representation of a value in a document that we filter on with a
/// must clause.
#[derive(Debug, Clone, PartialEq, Eq, From, Into)]
pub struct FilterValue(Vec<u8>);

impl FilterValue {
    pub fn from_search_value(value: Option<&ConvexValue>) -> Self {
        let sort_key = match value {
            Some(value) => value.sort_key(),
            None => vec![UNDEFINED_TAG],
        };
        if sort_key.len() < MAX_FILTER_FIELD_LENGTH {
            Self(sort_key)
        } else {
            let hashed_value = CommonSha256::hash(&sort_key);
            Self(Vec::<u8>::from(*hashed_value))
        }
    }
}

impl Deref for FilterValue {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl HeapSize for FilterValue {
    fn heap_size(&self) -> usize {
        self.0.heap_size()
    }
}

pub fn search_value_to_bytes(value: Option<&ConvexValue>) -> Vec<u8> {
    FilterValue::from_search_value(value).into()
}

/// Filters to apply while querying a search index.
#[derive(Clone, Debug, PartialEq)]
pub enum SearchFilterExpression {
    Search(FieldPath, String),
    Eq(FieldPath, Option<ConvexValue>),
}

/// Filters to apply while querying a search index.
#[derive(Clone, Debug, PartialEq)]
pub enum InternalSearchFilterExpression {
    Search(FieldPath, String),
    Eq(FieldPath, FilterValue),
}

impl SearchFilterExpression {
    pub fn to_internal(self) -> anyhow::Result<InternalSearchFilterExpression> {
        let expression = match self {
            Self::Search(field, s) => InternalSearchFilterExpression::Search(field, s),
            Self::Eq(field, v) => InternalSearchFilterExpression::Eq(
                field,
                FilterValue::from_search_value(v.as_ref()),
            ),
        };
        Ok(expression)
    }
}

/// The first step of any query is a QuerySource. This defines how the initial
/// row set should be read out of the database, before applying any operators.
#[derive(Clone, Debug, PartialEq)]
pub enum QuerySource {
    /// Scan the entirety of the given table.
    FullTableScan(FullTableScan),
    /// Scan a range of an index.
    IndexRange(IndexRange),
    /// Perform a full text search.
    Search(Search),
}

/// An Expression evaluates to a Value.
///
/// If you add a new expression type, don't forget to add it to the proptest
/// strategy below!
#[derive(Eq, PartialEq, Clone, Debug)]
pub enum Expression {
    /// `l == r`
    Eq(Box<Expression>, Box<Expression>),
    /// `l != r`
    Neq(Box<Expression>, Box<Expression>),
    /// `l < r`
    Lt(Box<Expression>, Box<Expression>),
    /// `l <= r`
    Lte(Box<Expression>, Box<Expression>),
    /// `l > r`
    Gt(Box<Expression>, Box<Expression>),
    /// `l >= r`
    Gte(Box<Expression>, Box<Expression>),
    /// `l + r`
    Add(Box<Expression>, Box<Expression>),
    /// `l - r`
    Sub(Box<Expression>, Box<Expression>),
    /// `l * r`
    Mul(Box<Expression>, Box<Expression>),
    /// `l / r`
    Div(Box<Expression>, Box<Expression>),
    /// `l % r`
    Mod(Box<Expression>, Box<Expression>),
    /// `-x`
    Neg(Box<Expression>),
    /// `a && b && ...`
    And(Vec<Expression>),
    /// `a || b || ...`
    Or(Vec<Expression>),
    /// `!x`
    Not(Box<Expression>),
    /// Evaluates to the named field on the environment Value.
    Field(FieldPath),
    /// A literal value.
    Literal(MaybeValue),
}

fn binary_arithmetic<I, F>(
    name: &'static str,
    environ: &ConvexObject,
    l_expr: &Expression,
    r_expr: &Expression,
    do_ints: I,
    do_floats: F,
) -> anyhow::Result<ConvexValue>
where
    I: FnOnce(i64, i64) -> Option<i64>,
    F: FnOnce(f64, f64) -> f64,
{
    let l = l_expr.eval(environ)?;
    let r = r_expr.eval(environ)?;

    let result = match (&l.0, &r.0) {
        (Some(ConvexValue::Int64(l)), Some(ConvexValue::Int64(r))) => {
            let result = do_ints(*l, *r).ok_or_else(|| {
                let message = if *r == 0 {
                    format!("Cannot {name} {l} by zero")
                } else {
                    format!("Cannot {name} {l} and {r}: the result is out of range for Int64")
                };
                ErrorMetadata::bad_request("EvalError", message)
            })?;
            val!(result)
        },
        (Some(ConvexValue::Float64(l)), Some(ConvexValue::Float64(r))) => {
            val!(do_floats(*l, *r))
        },
        (..) => {
            anyhow::bail!(ErrorMetadata::bad_request(
                "EvalError",
                format!(
                    "Cannot {name} {l} (type {}) and {r} (type {})",
                    l.type_name(),
                    r.type_name()
                ),
            ))
        },
    };
    Ok(result)
}

impl Expression {
    /// Evaluate the expression and return the result. Expression::Fields are
    /// evaluated on `environ`.
    pub fn eval(&self, environ: &ConvexObject) -> anyhow::Result<MaybeValue> {
        // Convert input value into a value that compares with ==, !=, >, <, etc. in
        // the same order as they would be compared in an index.
        let comparable_value = |v: MaybeValue| v.0;
        let result = match self {
            // Field expressions and literals are the two places where `undefined` values
            // originate. Until we migrate our index keys, field expressions use the old behavior
            // that maps missing fields to `Value::Null`.
            Expression::Field(field_path) => {
                return Ok(MaybeValue(environ.get_path(field_path).cloned()));
            },
            Expression::Literal(v) => return Ok(v.clone()),

            // Comparison operations need to operate on `ConvexValue`, not `Value`, so they match
            // the ordering in our index keys, which store table IDs.
            Expression::Eq(l_expr, r_expr) => {
                let l = comparable_value(l_expr.eval(environ)?);
                let r = comparable_value(r_expr.eval(environ)?);
                ConvexValue::from(l == r)
            },
            Expression::Neq(l_expr, r_expr) => {
                let l = comparable_value(l_expr.eval(environ)?);
                let r = comparable_value(r_expr.eval(environ)?);
                ConvexValue::from(l != r)
            },
            Expression::Lt(l_expr, r_expr) => {
                let l = comparable_value(l_expr.eval(environ)?);
                let r = comparable_value(r_expr.eval(environ)?);
                ConvexValue::from(l < r)
            },
            Expression::Lte(l_expr, r_expr) => {
                let l = comparable_value(l_expr.eval(environ)?);
                let r = comparable_value(r_expr.eval(environ)?);
                ConvexValue::from(l <= r)
            },
            Expression::Gt(l_expr, r_expr) => {
                let l = comparable_value(l_expr.eval(environ)?);
                let r = comparable_value(r_expr.eval(environ)?);
                ConvexValue::from(l > r)
            },
            Expression::Gte(l_expr, r_expr) => {
                let l = comparable_value(l_expr.eval(environ)?);
                let r = comparable_value(r_expr.eval(environ)?);
                ConvexValue::from(l >= r)
            },
            // Arithmetic operations only work on Int64 and Float64, so we don't have to worry about
            // mapping those from table names to table IDs.
            Expression::Add(l_expr, r_expr) => {
                binary_arithmetic("add", environ, l_expr, r_expr, i64::checked_add, |l, r| {
                    l + r
                })?
            },
            Expression::Sub(l_expr, r_expr) => binary_arithmetic(
                "subtract",
                environ,
                l_expr,
                r_expr,
                i64::checked_sub,
                |l, r| l - r,
            )?,
            Expression::Mul(l_expr, r_expr) => binary_arithmetic(
                "multiply",
                environ,
                l_expr,
                r_expr,
                i64::checked_mul,
                |l, r| l * r,
            )?,
            Expression::Div(l_expr, r_expr) => binary_arithmetic(
                "divide",
                environ,
                l_expr,
                r_expr,
                i64::checked_div,
                |l, r| l / r,
            )?,
            Expression::Mod(l_expr, r_expr) => {
                binary_arithmetic("mod", environ, l_expr, r_expr, i64::checked_rem, |l, r| {
                    l % r
                })?
            },
            Expression::Neg(x_expr) => {
                let x = x_expr.eval(environ)?;
                match &x.0 {
                    Some(ConvexValue::Int64(x)) => {
                        let result = x.checked_neg().ok_or_else(|| {
                            ErrorMetadata::bad_request(
                                "EvalError",
                                format!("Cannot negate {x}: the result is out of range for Int64"),
                            )
                        })?;
                        ConvexValue::from(result)
                    },
                    Some(ConvexValue::Float64(x)) => ConvexValue::from(-*x),
                    _ => anyhow::bail!(ErrorMetadata::bad_request(
                        "EvalError",
                        format!("Cannot negate {x} (type {})", x.type_name()),
                    )),
                }
            },
            // Similarly, boolean operations only work on booleans, which don't contain table IDs.
            Expression::And(vs) => {
                for v in vs {
                    if !v.eval(environ)?.into_boolean()? {
                        return Ok(ConvexValue::from(false).into());
                    }
                }
                ConvexValue::from(true)
            },
            Expression::Or(vs) => {
                for v in vs {
                    if v.eval(environ)?.into_boolean()? {
                        return Ok(ConvexValue::from(true).into());
                    }
                }
                ConvexValue::from(false)
            },
            Expression::Not(x_expr) => ConvexValue::from(!x_expr.eval(environ)?.into_boolean()?),
        };
        Ok(result.into())
    }

    /// Shorthand for "field == literal".
    pub fn field_eq_literal(field: FieldPath, literal: ConvexValue) -> Self {
        Expression::Eq(
            Box::new(Expression::Field(field)),
            Box::new(Expression::Literal(literal.into())),
        )
    }

    /// Helper for creating an `And` variant.
    pub fn and(left: Expression, right: Expression) -> Self {
        Expression::And(vec![left, right])
    }
}

/// Queries are lazy iterations, QueryOperators take and produce a stream of
/// Values.
#[derive(Clone, Debug, PartialEq)]
pub enum QueryOperator {
    /// Return only the values for which this expression returns true.
    Filter(Expression),
    /// Return the first n results.
    Limit(usize),
}

/// The maximum number of `QueryOperator`s allowed on a single query.
/// This is only enforced for queries deserialized from JSON as we assume other
/// queries come from the system.
///
/// N.B.: this value is replicated in `query_impl.ts` in the `convex` npm
/// package.
pub const MAX_QUERY_OPERATORS: usize = 256;

/// A query, represented as a source and a chain of operators to apply as a lazy
/// iteration.
#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    /// The original source to fetch values from the database.
    pub source: QuerySource,
    /// The list of operators to apply in order.
    pub operators: Vec<QueryOperator>,
}

impl Query {
    /// Create a query starting with a table scan as the query source.
    pub fn full_table_scan(table_name: TableName, order: Order) -> Self {
        Self {
            source: QuerySource::FullTableScan(FullTableScan { table_name, order }),
            operators: vec![],
        }
    }

    /// Create a query starting with an index range as the query source.
    pub fn index_range(index_range: IndexRange) -> Self {
        Self {
            source: QuerySource::IndexRange(index_range),
            operators: vec![],
        }
    }

    pub fn get(table_name: TableName, id: DeveloperDocumentId) -> Self {
        Self::index_range(IndexRange {
            index_name: IndexName::by_id(table_name),
            range: vec![IndexRangeExpression::Eq(
                ID_FIELD_PATH.clone(),
                MaybeValue(Some(ConvexValue::from(id))),
            )],
            order: Order::Asc,
        })
    }

    pub fn search(search: Search) -> Self {
        Self {
            source: QuerySource::Search(search),
            operators: vec![],
        }
    }

    /// Add a filter predicate to a query.
    pub fn filter(mut self, expression: Expression) -> Self {
        self.operators.push(QueryOperator::Filter(expression));
        self
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.operators.push(QueryOperator::Limit(limit));
        self
    }

    pub fn fingerprint(&self, indexed_fields: &IndexedFields) -> anyhow::Result<QueryFingerprint> {
        #[derive(Serialize)]
        struct QueryFingerprintJson {
            query: JsonValue,
            indexed_fields: Vec<String>,
        }
        let fingerprint_json = QueryFingerprintJson {
            query: JsonValue::try_from(self.clone())?,
            indexed_fields: indexed_fields
                .iter()
                .map(|field| String::from(field.clone()))
                .collect(),
        };

        // Hash a JSON object of our query plus its indexed fields so the fingerprint
        // changes if any of these change.
        let vec = serde_json::to_vec(&fingerprint_json)?;
        let mut hasher = Sha256::new();
        hasher.write_all(&vec)?;
        Ok(hasher.finalize().to_vec())
    }
}
