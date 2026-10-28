use std::str::FromStr;

use anyhow::Result;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::Value as JsonValue;

use crate::{
    json::expression::JsonExpression,
    paths::FieldPath,
    query::{
        Expression,
        FullTableScan,
        IndexRange,
        IndexRangeExpression,
        Order,
        Query,
        QueryOperator,
        QuerySource,
        Search,
        SearchFilterExpression,
        MAX_QUERY_OPERATORS,
    },
    types::{
        IndexName,
        MaybeValue,
        TableName,
    },
    value::{
        PendingValue,
        MAX_COMMIT_TS,
    },
};

/// Parse an index range value, additionally accepting the
/// `{"$commitTs": null}` token, which resolves to `Int64(MAX_COMMIT_TS)`.
/// e.g. `q.eq("commitTs", db.vars.commitTs)` matches exactly the rows inserted
/// by the current transaction.
fn range_value(json: JsonValue) -> Result<MaybeValue> {
    if json == serde_json::json!({ "$undefined": null }) {
        return json.try_into();
    }
    let value = PendingValue::from_uncommitted_json(json)?;
    Ok(MaybeValue(Some(value.into_resolved(MAX_COMMIT_TS)?)))
}

fn try_order_from_string(order: Option<String>) -> anyhow::Result<Order> {
    match order.as_deref() {
        None | Some("asc") => Ok(Order::Asc),
        Some("desc") => Ok(Order::Desc),
        _ => Err(anyhow::anyhow!("expected \"asc\" or \"desc\"")),
    }
}

impl From<Order> for String {
    fn from(order: Order) -> Self {
        match order {
            Order::Asc => "asc".to_string(),
            Order::Desc => "desc".to_string(),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type")]
enum JsonQuerySource {
    FullTableScan(JsonFullTableScan),
    IndexRange(JsonQueryIndexRange),
    Search(JsonSearch),
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonQueryIndexRange {
    index_name: String,
    range: Vec<JsonIndexRangeExpression>,
    order: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type")]
enum JsonIndexRangeExpression {
    Eq(JsonFieldPathAndValue),
    Gt(JsonFieldPathAndValue),
    Gte(JsonFieldPathAndValue),
    Lt(JsonFieldPathAndValue),
    Lte(JsonFieldPathAndValue),
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonFieldPathAndValue {
    field_path: String,
    value: JsonValue,
}

impl TryFrom<JsonIndexRangeExpression> for IndexRangeExpression {
    type Error = anyhow::Error;

    fn try_from(json_range_expression: JsonIndexRangeExpression) -> Result<Self> {
        match json_range_expression {
            JsonIndexRangeExpression::Eq(field_and_value) => Ok(IndexRangeExpression::Eq(
                FieldPath::from_str(&field_and_value.field_path)?,
                range_value(field_and_value.value)?,
            )),
            JsonIndexRangeExpression::Gt(field_and_value) => Ok(IndexRangeExpression::Gt(
                FieldPath::from_str(&field_and_value.field_path)?,
                range_value(field_and_value.value)?,
            )),
            JsonIndexRangeExpression::Gte(field_and_value) => Ok(IndexRangeExpression::Gte(
                FieldPath::from_str(&field_and_value.field_path)?,
                range_value(field_and_value.value)?,
            )),
            JsonIndexRangeExpression::Lt(field_and_value) => Ok(IndexRangeExpression::Lt(
                FieldPath::from_str(&field_and_value.field_path)?,
                range_value(field_and_value.value)?,
            )),
            JsonIndexRangeExpression::Lte(field_and_value) => Ok(IndexRangeExpression::Lte(
                FieldPath::from_str(&field_and_value.field_path)?,
                range_value(field_and_value.value)?,
            )),
        }
    }
}

impl From<IndexRangeExpression> for JsonIndexRangeExpression {
    fn from(range_expression: IndexRangeExpression) -> Self {
        match range_expression {
            IndexRangeExpression::Eq(field_path, value) => {
                JsonIndexRangeExpression::Eq(JsonFieldPathAndValue {
                    field_path: field_path.into(),
                    value: value.into(),
                })
            },
            IndexRangeExpression::Gt(field_path, value) => {
                JsonIndexRangeExpression::Gt(JsonFieldPathAndValue {
                    field_path: field_path.into(),
                    value: value.into(),
                })
            },
            IndexRangeExpression::Gte(field_path, value) => {
                JsonIndexRangeExpression::Gte(JsonFieldPathAndValue {
                    field_path: field_path.into(),
                    value: value.into(),
                })
            },
            IndexRangeExpression::Lt(field_path, value) => {
                JsonIndexRangeExpression::Lt(JsonFieldPathAndValue {
                    field_path: field_path.into(),
                    value: value.into(),
                })
            },
            IndexRangeExpression::Lte(field_path, value) => {
                JsonIndexRangeExpression::Lte(JsonFieldPathAndValue {
                    field_path: field_path.into(),
                    value: value.into(),
                })
            },
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonFullTableScan {
    table_name: String,
    order: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonSearch {
    index_name: String,
    filters: Vec<JsonSearchFilterExpression>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type")]
enum JsonSearchFilterExpression {
    #[serde(rename_all = "camelCase")]
    Search {
        field_path: String,
        value: String,
    },
    Eq(JsonFieldPathAndValue),
}

impl TryFrom<JsonSearchFilterExpression> for SearchFilterExpression {
    type Error = anyhow::Error;

    fn try_from(json_filter_expression: JsonSearchFilterExpression) -> Result<Self> {
        match json_filter_expression {
            JsonSearchFilterExpression::Search { field_path, value } => Ok(
                SearchFilterExpression::Search(FieldPath::from_str(&field_path)?, value),
            ),
            JsonSearchFilterExpression::Eq(field_and_value) => Ok(SearchFilterExpression::Eq(
                FieldPath::from_str(&field_and_value.field_path)?,
                MaybeValue::try_from(field_and_value.value)?.0,
            )),
        }
    }
}

impl From<SearchFilterExpression> for JsonSearchFilterExpression {
    fn from(filter_expression: SearchFilterExpression) -> Self {
        match filter_expression {
            SearchFilterExpression::Search(field_path, value) => {
                JsonSearchFilterExpression::Search {
                    field_path: field_path.into(),
                    value,
                }
            },
            SearchFilterExpression::Eq(field_path, value) => {
                JsonSearchFilterExpression::Eq(JsonFieldPathAndValue {
                    field_path: field_path.into(),
                    value: MaybeValue(value).into(),
                })
            },
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonQuery {
    pub source: JsonQuerySource,
    pub operators: Vec<JsonQueryOperator>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum JsonQueryOperator {
    Filter(JsonExpression),
    Limit(usize),
}

impl TryFrom<JsonQuerySource> for QuerySource {
    type Error = anyhow::Error;

    fn try_from(value: JsonQuerySource) -> Result<Self> {
        Ok(match value {
            JsonQuerySource::FullTableScan(json_full_table_scan) => {
                QuerySource::FullTableScan(FullTableScan {
                    table_name: TableName::from_str(&json_full_table_scan.table_name)?,
                    order: try_order_from_string(json_full_table_scan.order)?,
                })
            },
            JsonQuerySource::IndexRange(json_index_range) => {
                let range_exprs: Vec<IndexRangeExpression> = json_index_range
                    .range
                    .into_iter()
                    .map(|json_range_expr| json_range_expr.try_into())
                    .collect::<anyhow::Result<Vec<_>>>()?;

                QuerySource::IndexRange(IndexRange {
                    index_name: IndexName::from_str(&json_index_range.index_name)?,
                    range: range_exprs,
                    order: try_order_from_string(json_index_range.order)?,
                })
            },
            JsonQuerySource::Search(json_search) => {
                let filter_expressions: Vec<SearchFilterExpression> = json_search
                    .filters
                    .into_iter()
                    .map(|json_filter_expression| json_filter_expression.try_into())
                    .collect::<anyhow::Result<Vec<_>>>()?;

                let index_name = IndexName::from_str(&json_search.index_name)?;
                QuerySource::Search(Search {
                    table: index_name.table().clone(),
                    index_name,
                    filters: filter_expressions,
                })
            },
        })
    }
}

impl From<QuerySource> for JsonQuerySource {
    fn from(query_source: QuerySource) -> Self {
        match query_source {
            QuerySource::FullTableScan(FullTableScan { table_name, order }) => {
                JsonQuerySource::FullTableScan(JsonFullTableScan {
                    table_name: table_name.into(),
                    order: Some(order.into()),
                })
            },
            QuerySource::IndexRange(IndexRange {
                index_name,
                range,
                order,
            }) => JsonQuerySource::IndexRange(JsonQueryIndexRange {
                index_name: index_name.to_string(),
                range: range
                    .into_iter()
                    .map(|range_expr| range_expr.into())
                    .collect(),
                order: Some(order.into()),
            }),
            QuerySource::Search(Search {
                index_name,
                filters,
                ..
            }) => JsonQuerySource::Search(JsonSearch {
                index_name: index_name.to_string(),
                filters: filters.into_iter().map(|filter| filter.into()).collect(),
            }),
        }
    }
}

impl TryFrom<JsonValue> for Query {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self> {
        let json_query: JsonQuery = serde_json::from_value(value)?;
        anyhow::ensure!(
            json_query.operators.len() <= MAX_QUERY_OPERATORS,
            "Query has too many operators: {}",
            json_query.operators.len()
        );
        Ok(Query {
            source: json_query.source.try_into()?,
            operators: json_query
                .operators
                .into_iter()
                .map(|json_op| {
                    Ok(match json_op {
                        JsonQueryOperator::Filter(json_predicate) => {
                            QueryOperator::Filter(Expression::try_from(json_predicate)?)
                        },
                        JsonQueryOperator::Limit(n) => QueryOperator::Limit(n),
                    })
                })
                .collect::<Result<Vec<QueryOperator>>>()?,
        })
    }
}

impl TryFrom<Query> for JsonValue {
    type Error = anyhow::Error;

    fn try_from(query: Query) -> Result<Self, Self::Error> {
        let json_query = JsonQuery {
            source: query.source.into(),
            operators: query
                .operators
                .into_iter()
                .map(|op| match op {
                    QueryOperator::Filter(predicate) => {
                        JsonQueryOperator::Filter(JsonExpression::from(predicate))
                    },
                    QueryOperator::Limit(n) => JsonQueryOperator::Limit(n),
                })
                .collect(),
        };
        Ok(serde_json::to_value(json_query)?)
    }
}
