use std::{
    cmp,
    collections::{
        BTreeMap,
        BTreeSet,
    },
    fmt::{
        Debug,
        Formatter,
    },
};

use common::{
    components::ComponentId,
    json::JsonExpression,
    query::Expression,
    types::{
        GenericIndexName,
        IndexName,
        MaybeValue,
        WriteTimestamp,
    },
};
use errors::ErrorMetadata;
use pb::searchlight as proto;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::{
    json,
    Value as JsonValue,
};
use value::{
    id_v6::DeveloperDocumentId,
    ConvexValue,
    FieldPath,
    InternalId,
    NamespacedTableMapping,
    Size,
    TableName,
    TableNamespace,
    TableNumber,
    TabletId,
};

use crate::IndexedVector;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VectorSearchRequest {
    pub query: JsonValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VectorSearch {
    pub index_name: IndexName,
    pub component_id: ComponentId,
    pub limit: Option<u32>,
    pub vector: Vec<f32>,
    pub expressions: BTreeSet<VectorSearchExpression>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VectorSearchExpression {
    Eq(FieldPath, Option<ConvexValue>),
    In(FieldPath, BTreeSet<Option<ConvexValue>>),
}

impl VectorSearchExpression {
    /// Vector filters use a subset of the `Expression` syntax -- `q.or` and
    /// `q.eq`.
    ///
    /// We massage these into a list of Vec<VectorSearchExpression> (or error if
    /// this is impossible). As an intermediate step, we create a map from
    /// FieldPath to a Vec of Values so we can create
    /// `VectorSearchExpression::In` or `VectorSearchExpression::Eq`
    /// accordingly.
    fn assemble_filter_map(
        expression: Expression,
    ) -> anyhow::Result<BTreeMap<FieldPath, BTreeSet<Option<ConvexValue>>>> {
        match expression {
            Expression::Eq(left, right) => {
                if let (Expression::Field(field_path), Expression::Literal(value)) = (*left, *right)
                {
                    let mut field_map = BTreeMap::new();
                    let mut values = BTreeSet::new();
                    values.insert(value.0);
                    field_map.insert(field_path, values);
                    Ok(field_map)
                } else {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "InvalidVectorSearchFilter",
                        "`q.eq` must take a field path as its first argument and a value as its \
                         second"
                    ))
                }
            },
            Expression::Or(expressions) => {
                let mut full_field_map = BTreeMap::new();
                for e in expressions {
                    let field_map = Self::assemble_filter_map(e)?;
                    for (key, values) in field_map {
                        let merged_values = full_field_map.entry(key).or_insert(BTreeSet::new());
                        merged_values.extend(values);
                    }
                }
                Ok(full_field_map)
            },
            Expression::Literal(_)
            | Expression::Neq(..)
            | Expression::Lt(..)
            | Expression::Lte(..)
            | Expression::Gt(..)
            | Expression::Gte(..)
            | Expression::Add(..)
            | Expression::Sub(..)
            | Expression::Mul(..)
            | Expression::Div(..)
            | Expression::Mod(..)
            | Expression::Neg(_)
            | Expression::And(_)
            | Expression::Not(_)
            | Expression::Field(_) => {
                anyhow::bail!(ErrorMetadata::bad_request(
                    "InvalidVectorSearchFilter",
                    "Filters should be a combination of `q.eq` and `q.or`."
                ))
            },
        }
    }

    fn from_expression(expression: Expression) -> anyhow::Result<BTreeSet<Self>> {
        let field_map = Self::assemble_filter_map(expression)?;
        Ok(Self::from_field_map(field_map))
    }

    fn from_field_map(
        field_map: BTreeMap<FieldPath, BTreeSet<Option<ConvexValue>>>,
    ) -> BTreeSet<Self> {
        let mut filters = BTreeSet::new();
        for (key, values) in field_map {
            if values.len() == 1 {
                filters.insert(VectorSearchExpression::Eq(
                    key,
                    values
                        .iter()
                        .next()
                        .expect("Set does not have a single element")
                        .clone(),
                ));
            } else {
                filters.insert(VectorSearchExpression::In(key, values));
            }
        }
        filters
    }

    fn to_expression(filter_expressions: BTreeSet<Self>) -> Expression {
        let mut expressions = vec![];
        for filter in filter_expressions {
            match filter {
                VectorSearchExpression::Eq(field_path, value) => expressions.push(Expression::Eq(
                    Box::new(Expression::Field(field_path)),
                    Box::new(Expression::Literal(MaybeValue(value))),
                )),
                VectorSearchExpression::In(field_path, values) => {
                    for value in values {
                        expressions.push(Expression::Eq(
                            Box::new(Expression::Field(field_path.clone())),
                            Box::new(Expression::Literal(MaybeValue(value))),
                        ))
                    }
                },
            }
        }
        Expression::Or(expressions)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VectorSearchJson {
    index_name: String,
    component_id: Option<String>,
    limit: Option<u32>,
    vector: Vec<f32>,
    expressions: Option<JsonExpression>,
}

impl VectorSearchJson {
    /// Inject the component_id into the [VectorSearchJson]. This is a hack to
    /// allow us to use the same encoding/decoding across backend callbacks,
    /// action callbacks, and the syscall, but we don't necessarily know the
    /// component id until executing inside v8.
    pub fn insert_component_id(&mut self, component_id: ComponentId) {
        self.component_id = component_id.serialize_to_string();
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
enum VectorSearchExpressionJson {
    Eq {
        path: String,
        value: JsonValue,
    },
    In {
        path: String,
        values: Vec<JsonValue>,
    },
}

impl TryFrom<JsonValue> for VectorSearch {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        let search: VectorSearchJson = serde_json::from_value(value)?;
        let index_name: GenericIndexName<TableName> = search.index_name.parse()?;
        let component_id = ComponentId::deserialize_from_string(search.component_id.as_deref())?;
        let expressions = search
            .expressions
            .map_or(anyhow::Ok(BTreeSet::new()), |e| {
                let expression: Expression = e.try_into()?;
                VectorSearchExpression::from_expression(expression)
            })?;

        let result = Self {
            index_name,
            component_id,
            expressions,
            limit: search.limit,
            vector: search.vector,
        };
        Ok(result)
    }
}

impl TryFrom<VectorSearch> for JsonValue {
    type Error = anyhow::Error;

    fn try_from(value: VectorSearch) -> Result<Self, Self::Error> {
        let expression_json = if !value.expressions.is_empty() {
            let expression = VectorSearchExpression::to_expression(value.expressions);
            Some(expression.into())
        } else {
            None
        };

        let search = VectorSearchJson {
            index_name: format!("{}", value.index_name),
            component_id: value.component_id.serialize_to_string(),
            expressions: expression_json,
            limit: value.limit,
            vector: value.vector,
        };
        Ok(serde_json::to_value(search)?)
    }
}

impl TryFrom<VectorSearchExpression> for VectorSearchExpressionJson {
    type Error = anyhow::Error;

    fn try_from(value: VectorSearchExpression) -> Result<Self, Self::Error> {
        let result = match value {
            VectorSearchExpression::Eq(path, value) => VectorSearchExpressionJson::Eq {
                path: path.into(),
                value: MaybeValue(value).to_internal_json(),
            },
            VectorSearchExpression::In(path, values) => VectorSearchExpressionJson::In {
                path: path.into(),
                values: values
                    .into_iter()
                    .map(|v| MaybeValue(v).to_internal_json())
                    .collect(),
            },
        };
        Ok(result)
    }
}

impl TryFrom<VectorSearchExpressionJson> for VectorSearchExpression {
    type Error = anyhow::Error;

    fn try_from(value: VectorSearchExpressionJson) -> Result<Self, Self::Error> {
        let result = match value {
            VectorSearchExpressionJson::Eq { path, value } => {
                VectorSearchExpression::Eq(path.parse()?, MaybeValue::try_from(value)?.0)
            },
            VectorSearchExpressionJson::In { path, values } => VectorSearchExpression::In(
                path.parse()?,
                values
                    .into_iter()
                    .map(|v| anyhow::Ok(MaybeValue::try_from(v)?.0))
                    .try_collect()?,
            ),
        };
        Ok(result)
    }
}

impl VectorSearch {
    pub fn resolve(
        self,
        table_mapping: &NamespacedTableMapping,
    ) -> anyhow::Result<InternalVectorSearch> {
        anyhow::ensure!(
            table_mapping.namespace() == TableNamespace::from(self.component_id),
            format!(
                "Component id {:?} does not match the table namespace {:?}",
                self.component_id,
                table_mapping.namespace()
            )
        );
        let original_table_name = self.index_name.table().clone();
        let index_name = self
            .index_name
            .to_resolved(table_mapping.name_to_tablet())?;
        let result = InternalVectorSearch {
            index_name,
            vector: self.vector,
            limit: self.limit,
            expressions: self.expressions.into_iter().collect(),
            original_table_name,
        };
        Ok(result)
    }
}

pub struct InternalVectorSearch {
    pub index_name: GenericIndexName<TabletId>,
    pub limit: Option<u32>,
    pub vector: Vec<f32>,
    pub expressions: Vec<VectorSearchExpression>,
    pub original_table_name: TableName,
}

impl InternalVectorSearch {
    pub fn printable_index_name(&self) -> anyhow::Result<IndexName> {
        IndexName::new(
            self.original_table_name.clone(),
            self.index_name.descriptor().clone(),
        )
    }
}

#[derive(Clone)]
pub struct CompiledVectorSearch {
    pub vector: IndexedVector,
    pub limit: u32,
    pub filter_conditions: BTreeMap<FieldPath, CompiledVectorFilter>,
}

impl Debug for CompiledVectorSearch {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CompiledVectorSearch {{ vector_size: {}, limit: {}, filter_conditions: {:?} }}",
            self.vector.len(),
            self.limit,
            self.filter_conditions,
        )
    }
}

#[derive(Clone, Debug)]
pub enum CompiledVectorFilter {
    Eq(Vec<u8>),
    In(Vec<Vec<u8>>),
}

#[derive(Clone, Debug)]
pub struct VectorSearchQueryResult {
    pub score: f32,
    pub id: InternalId,
    pub ts: WriteTimestamp,
}

impl Ord for VectorSearchQueryResult {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.score
            .total_cmp(&other.score)
            .then(self.id.cmp(&other.id))
            .then(self.ts.cmp(&other.ts))
    }
}

impl PartialOrd for VectorSearchQueryResult {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for VectorSearchQueryResult {}

impl PartialEq for VectorSearchQueryResult {
    fn eq(&self, other: &Self) -> bool {
        self.score.total_cmp(&other.score).is_eq() && self.id == other.id && self.ts == other.ts
    }
}

impl VectorSearchQueryResult {
    pub fn to_public(self, table_number: TableNumber) -> PublicVectorSearchQueryResult {
        PublicVectorSearchQueryResult {
            id: DeveloperDocumentId::new(table_number, self.id),
            score: self.score,
        }
    }
}

impl From<CompiledVectorSearch> for proto::CompiledVectorQuery {
    fn from(value: CompiledVectorSearch) -> Self {
        Self {
            vector: value.vector.into(),
            limit: value.limit,
            filter_conditions: value
                .filter_conditions
                .into_iter()
                .map(
                    |(field_path, filter)| proto::CompiledVectorQueryFilterCondition {
                        path: Some(field_path.into()),
                        filter: Some(filter.into()),
                    },
                )
                .collect(),
        }
    }
}

impl TryFrom<proto::CompiledVectorQuery> for CompiledVectorSearch {
    type Error = anyhow::Error;

    fn try_from(value: proto::CompiledVectorQuery) -> Result<Self, Self::Error> {
        let filter_conditions = value
            .filter_conditions
            .into_iter()
            .map(|condition| {
                let path: FieldPath = condition
                    .path
                    .ok_or_else(|| anyhow::anyhow!("Path is not set"))?
                    .try_into()?;
                let filter: CompiledVectorFilter = condition
                    .filter
                    .ok_or_else(|| anyhow::anyhow!("Filter is not set"))?
                    .try_into()?;
                Ok((path, filter))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            vector: value.vector.try_into()?,
            limit: value.limit,
            filter_conditions: filter_conditions.into_iter().collect(),
        })
    }
}

impl From<CompiledVectorFilter> for proto::compiled_vector_query_filter_condition::Filter {
    fn from(value: CompiledVectorFilter) -> Self {
        match value {
            CompiledVectorFilter::Eq(value) => Self::EqCondition(value),
            CompiledVectorFilter::In(values) => {
                Self::InCondition(proto::CompiledVectorQueryFilterInCondition {
                    eq_conditions: values,
                })
            },
        }
    }
}

impl TryFrom<proto::compiled_vector_query_filter_condition::Filter> for CompiledVectorFilter {
    type Error = anyhow::Error;

    fn try_from(
        value: proto::compiled_vector_query_filter_condition::Filter,
    ) -> Result<Self, Self::Error> {
        match value {
            proto::compiled_vector_query_filter_condition::Filter::EqCondition(value) => {
                Ok(Self::Eq(value))
            },
            proto::compiled_vector_query_filter_condition::Filter::InCondition(value) => {
                Ok(Self::In(value.eq_conditions))
            },
        }
    }
}

impl From<VectorSearchQueryResult> for proto::VectorQueryResult {
    fn from(value: VectorSearchQueryResult) -> Self {
        Self {
            score: value.score,
            internal_id: value.id.into(),
            ts: match value.ts {
                WriteTimestamp::Committed(ts) => Some(u64::from(ts)),
                WriteTimestamp::Pending => None,
            },
        }
    }
}

impl TryFrom<proto::VectorQueryResult> for VectorSearchQueryResult {
    type Error = anyhow::Error;

    fn try_from(value: proto::VectorQueryResult) -> anyhow::Result<Self> {
        let result = Self {
            score: value.score,
            id: value.internal_id.try_into()?,
            ts: match value.ts {
                Some(ts) => WriteTimestamp::Committed(ts.try_into()?),
                None => WriteTimestamp::Pending,
            },
        };
        Ok(result)
    }
}

#[derive(Clone, Debug)]
pub struct PublicVectorSearchQueryResult {
    pub score: f32,
    pub id: DeveloperDocumentId,
}

impl Size for PublicVectorSearchQueryResult {
    fn size(&self) -> usize {
        self.id.size() + std::mem::size_of::<f32>()
    }

    fn nesting(&self) -> usize {
        0
    }
}

impl Eq for PublicVectorSearchQueryResult {}

impl PartialEq for PublicVectorSearchQueryResult {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.score.total_cmp(&other.score).is_eq()
    }
}

impl From<PublicVectorSearchQueryResult> for JsonValue {
    fn from(value: PublicVectorSearchQueryResult) -> Self {
        json!({
            "_id": String::from(value.id),
            "_score": value.score,
        })
    }
}

impl Ord for PublicVectorSearchQueryResult {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.score
            .total_cmp(&other.score)
            .then(self.id.cmp(&other.id))
    }
}

impl PartialOrd for PublicVectorSearchQueryResult {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}
