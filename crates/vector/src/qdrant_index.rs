use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    fs,
    mem,
    ops::Deref,
    path::{
        Path,
        PathBuf,
    },
    str::FromStr,
    sync::{
        atomic::AtomicBool,
        Arc,
    },
    time::{
        Duration,
        Instant,
    },
};

use atomic_refcell::AtomicRefCell;
use common::{
    bootstrap_model::index::vector_index::VectorIndexSpec,
    document::ResolvedDocument,
    knobs::VECTOR_INDEX_THREADS,
    persistence::DocumentStream,
    query::search_value_to_bytes,
    types::{
        Timestamp,
        WriteTimestamp,
    },
};
use errors::ErrorMetadata;
use futures::TryStreamExt;
use pb::searchlight as proto;
use qdrant_common::types::{
    DetailsLevel,
    TelemetryDetail,
};
use qdrant_segment::{
    data_types::{
        named_vectors::NamedVectors,
        vectors::{
            VectorElementType,
            VectorRef,
        },
    },
    entry::entry_point::SegmentEntry,
    json_path::JsonPath,
    segment::Segment,
    spaces::{
        metric::Metric,
        simple::CosineMetric,
    },
    types::{
        AnyVariants,
        Condition,
        ExtendedPointId,
        FieldCondition,
        Filter,
        Match,
        MatchAny,
        MatchValue,
        PayloadFieldSchema,
        PayloadSchemaType,
        PayloadSelector,
        PayloadSelectorInclude,
        PointIdType,
        SearchParams,
        ValueVariants,
        WithPayload,
        WithVector,
        VECTOR_ELEMENT_SIZE,
    },
};
use serde_json::Value as JsonValue;
use tempfile::TempDir;
use uuid::Uuid;
use value::{
    base64,
    ConvexValue,
    FieldPath,
    InternalDocumentId,
    InternalId,
    ResolvedDocumentId,
};

use crate::{
    id_tracker::VectorMemoryIdTracker,
    incorrect_vector_filter_field_error,
    metrics::{
        self,
    },
    qdrant_segments::{
        build_disk_segment,
        create_mutable_segment,
        segment_config,
        snapshot_segment,
        VectorDiskSegmentValues,
        DEFAULT_VECTOR_NAME,
    },
    query::{
        CompiledVectorFilter,
        CompiledVectorSearch,
        InternalVectorSearch,
        VectorSearchExpression,
    },
    vector_dimensions_mismatch_error,
    IndexedVector,
    VectorSearchQueryResult,
    DEFAULT_VECTOR_LIMIT,
    MAX_FILTER_LENGTH,
    MAX_VECTOR_RESULTS,
};

pub(crate) const TIMESTAMP_FIELD: &str = "_ts";

#[derive(Clone, Debug)]
pub struct QdrantSchema {
    dimension: usize,
    vector_field: FieldPath,
    filter_fields: BTreeSet<FieldPath>,
}

#[derive(Clone, Copy, Debug)]
pub enum QdrantVectorIndexType {
    Plain,
    HNSW,
}

impl QdrantSchema {
    pub fn new(index_config: &VectorIndexSpec) -> Self {
        Self {
            dimension: u32::from(index_config.dimensions) as usize,
            vector_field: index_config.vector_field.clone(),
            filter_fields: index_config.filter_fields.clone(),
        }
    }

    pub fn index(&self, document: &ResolvedDocument) -> Option<QdrantDocument> {
        let object = document.value();
        let Some(ConvexValue::Array(array)) = object.get_path(&self.vector_field) else {
            return None;
        };
        let mut vector = Vec::with_capacity(self.dimension);
        if array.len() != self.dimension {
            tracing::debug!(
                "Ignoring mismatched vector length, expected: {}, actual: {}",
                self.dimension,
                array.len(),
            );
            return None;
        }
        for value in array.iter() {
            let ConvexValue::Float64(f) = value else {
                return None;
            };
            vector.push(*f as f32);
        }
        let vector = IndexedVector::try_from(vector).ok()?;
        let document = QdrantDocument {
            internal_id: document.internal_id(),
            vector,
            filter_fields: self
                .filter_fields
                .iter()
                .map(|f| (f.clone(), search_value_to_bytes(object.get_path(f))))
                .collect(),
        };
        Some(document)
    }

    pub fn estimate_vector_size(&self) -> usize {
        self.dimension * VECTOR_ELEMENT_SIZE
    }

    pub fn compile(&self, query: InternalVectorSearch) -> anyhow::Result<CompiledVectorSearch> {
        let timer = metrics::compile_timer();

        let index_name = query.printable_index_name()?;
        let query_vector = IndexedVector::try_from(query.vector)?;
        let query_limit = query.limit.unwrap_or(DEFAULT_VECTOR_LIMIT);
        anyhow::ensure!(
            query_limit as usize <= MAX_VECTOR_RESULTS,
            ErrorMetadata::bad_request(
                "VectorLimitTooLargeError",
                format!(
                    "Vector queries can fetch at most {} results, requested {}.",
                    MAX_VECTOR_RESULTS, query_limit as usize,
                )
            )
        );
        let mut filter_conditions = BTreeMap::new();
        // Each equality expression contributes to this, so an `In` with N elements
        // increments this by N
        let mut filter_length = 0;

        for expresion in query.expressions {
            match expresion {
                VectorSearchExpression::Eq(field_path, value) => {
                    if !self.filter_fields.contains(&field_path) {
                        anyhow::bail!(incorrect_vector_filter_field_error(
                            &index_name,
                            &field_path
                        ))
                    }
                    let value_bytes = search_value_to_bytes(value.as_ref());
                    if filter_conditions.contains_key(&field_path) {
                        anyhow::bail!("Found multiple filters for the same field?")
                    }
                    filter_conditions.insert(field_path, CompiledVectorFilter::Eq(value_bytes));
                    filter_length += 1;
                },
                VectorSearchExpression::In(field_path, values) => {
                    if !self.filter_fields.contains(&field_path) {
                        anyhow::bail!(incorrect_vector_filter_field_error(
                            &index_name,
                            &field_path
                        ))
                    }
                    let values_bytes: Vec<_> = values
                        .into_iter()
                        .map(|v| search_value_to_bytes(v.as_ref()))
                        .collect();
                    if filter_conditions.contains_key(&field_path) {
                        anyhow::bail!("Found multiple filters for the same field?")
                    }
                    filter_length += values_bytes.len();
                    filter_conditions.insert(field_path, CompiledVectorFilter::In(values_bytes));
                },
            }
        }
        anyhow::ensure!(
            filter_length <= MAX_FILTER_LENGTH,
            ErrorMetadata::bad_request(
                "TooManyElementsInVectorQueryError",
                format!(
                    "Vector query against {index_name} has too many conditions. Max: \
                     {MAX_FILTER_LENGTH} Actual: {filter_length}"
                )
            )
        );
        anyhow::ensure!(
            query_vector.len() == self.dimension,
            vector_dimensions_mismatch_error(query_vector.len() as u32, self.dimension as u32)
        );
        let result = CompiledVectorSearch {
            vector: query_vector,
            limit: query_limit,
            filter_conditions,
        };
        metrics::log_compiled_query(&result);
        timer.finish();
        Ok(result)
    }

    pub fn search(
        &self,
        segment: &Segment,
        query: CompiledVectorSearch,
        overfetch_delta: u32,
        slow_vector_query_threshold_millis: u64,
        require_exact: bool,
    ) -> anyhow::Result<Vec<VectorSearchQueryResult>> {
        let qdrant_conditions = query
            .filter_conditions
            .iter()
            .map(|(field_path, condition)| {
                let field_condition = FieldCondition::new_match(
                    encode_user_field_path(field_path)?,
                    qdrant_filter_condition(condition),
                );
                Ok(Some(Condition::Field(field_condition)))
            })
            .collect::<anyhow::Result<Option<Vec<_>>>>()?;
        let qdrant_filter = Filter {
            should: qdrant_conditions,
            min_should: None,
            must: None,
            must_not: None,
        };
        let search_params = SearchParams {
            hnsw_ef: None,
            exact: require_exact,
            quantization: None,
            indexed_only: false,
        };
        let payload_selector = PayloadSelectorInclude {
            include: vec![json_path_from_str(TIMESTAMP_FIELD)?],
        };
        let start = Instant::now();
        let qdrant_results = segment.search(
            DEFAULT_VECTOR_NAME,
            &query.vector.into(),
            &WithPayload {
                enable: true,
                payload_selector: Some(PayloadSelector::Include(payload_selector)),
            },
            &WithVector::Bool(false),
            Some(&qdrant_filter),
            (query.limit + overfetch_delta) as usize,
            Some(&search_params),
            &AtomicBool::new(false),
        )?;
        let duration = Instant::now().duration_since(start);
        if duration > Duration::from_millis(slow_vector_query_threshold_millis) {
            let detail = TelemetryDetail {
                level: DetailsLevel::Level2,
                histograms: true,
            };
            tracing::warn!(
                "Slow qdrant query, duration: {}ms, segment telemetry: {:?}",
                duration.as_millis(),
                segment.get_telemetry_data(detail),
            )
        }
        let mut results = Vec::with_capacity(qdrant_results.len());
        for qdrant_result in qdrant_results {
            let ExtendedPointId::Uuid(ref uuid) = qdrant_result.id else {
                anyhow::bail!("Received non-UUID ID from qdrant: {qdrant_result:?}");
            };
            let internal_id = InternalId::from(*uuid.as_bytes());
            let Some(ref payload) = qdrant_result.payload else {
                anyhow::bail!("Received no payload from qdrant: {qdrant_result:?}");
            };
            let Some(JsonValue::String(ts_b64)) = payload.0.get(TIMESTAMP_FIELD) else {
                anyhow::bail!("Invalid timestamp from qdrant: {qdrant_result:?}");
            };
            let ts_bytes = base64::decode_urlsafe(ts_b64)?;
            let ts = u64::from_le_bytes(ts_bytes[..].try_into()?);

            let result = VectorSearchQueryResult {
                score: qdrant_result.score,
                id: internal_id,
                ts: WriteTimestamp::Committed(ts.try_into()?),
            };
            results.push(result);
        }
        Ok(results)
    }

    pub async fn build_disk_index<T: PreviousVectorSegmentsHack>(
        &self,
        index_path: &Path,
        revision_stream: DocumentStream<'_>,
        hnsw_threshold_bytes: usize,
        previous_segments: &mut T,
    ) -> anyhow::Result<Option<VectorDiskSegmentValues>> {
        let tmpdir = TempDir::new()?;
        let memory_timer = metrics::qdrant_segment_memory_build_timer();
        // With HNSW, we need to construct a temporary index, then do a one-time
        // non-appending write to the final disk index. Use a temp dir for the
        // temporary index. Since we don't know which index type we're going to use
        // upfront, always set up the more complex directory.
        let memory_dir: PathBuf = tmpdir.path().join("memory");
        let id_tracker = Arc::new(AtomicRefCell::new(VectorMemoryIdTracker::new()));
        let mutable_config = segment_config(self.dimension, true, *VECTOR_INDEX_THREADS);
        let mut memory_segment = create_mutable_segment(
            &memory_dir,
            id_tracker.clone(),
            self.dimension,
            mutable_config,
        )?;

        let op_num = 1;
        futures::pin_mut!(revision_stream);
        while let Some(entry) = revision_stream.try_next().await? {
            let point_id = QdrantExternalId::try_from(&entry.id)?;
            if let Some(document) = entry.value {
                let Some(qdrant_doc) = self.index(&document) else {
                    tracing::trace!("Skipping an invalid doc: {:?}", document);
                    continue;
                };
                memory_segment.upsert_point(op_num, *point_id, qdrant_doc.qdrant_vector())?;
                let payload = qdrant_doc.encode_payload(entry.ts)?;
                memory_segment.set_payload(op_num, *point_id, &payload.into(), &None)?;
            } else {
                // If the document was inserted and then deleted in this batch,
                // then we might need to remove a vector we just
                // added to this segment.
                if memory_segment.delete_point(op_num, *point_id)? {
                    tracing::trace!("Delete a point");
                } else {
                    tracing::trace!("Point was neither added nor deleted!");
                }
            }

            // Updates or deletes of documents need to clear out old versions of those docs
            // in previous segments. We could theoretically skip inserts here,
            // but we can't tell which documents are strictly new vs which are
            // updates based just on the stream.
            // Similarly we could theoretically use timestamps to determine exactly which
            // segment should have a given vector so long as merging retains
            // timestamp order. However to find the tombstoned document's
            // timestamps, we'd have to load previous revisions which
            // would require extra queries and logic, so instead we just try mutating each
            // segment in memory. This removes an opportunity to verify
            // consistency, but it's faster and simpler.
            previous_segments.maybe_delete_qdrant(*point_id)?;
        }
        // We encode all of our index values as strings.
        let field_schema = Some(&PayloadFieldSchema::FieldType(PayloadSchemaType::Keyword));
        for field in self.filter_fields.iter() {
            memory_segment.create_field_index(
                op_num,
                &encode_user_field_path(field)?,
                field_schema,
            )?;
        }
        memory_timer.finish();

        // Ideally we'd not even have created the memory segment, but because vectors
        // can be added and deleted from the segment in the same pass, it's not
        // trivial to tell up front if we're going to produce a useful segment or not.
        if memory_segment.available_point_count() == 0 {
            tracing::debug!("Skipping an empty vector index for {index_path:?}",);
            return Ok(None);
        } else {
            tracing::debug!(
                "Building segment with total vectors {}",
                memory_segment.total_point_count()
            );
        }

        // Use total_point_count to include deleted vectors in this estimate. Qdrant
        // used to, but no longer actually drops the vector data so they do take
        // up space (at least until we rebuild the segment via compaction or to add an
        // HNSW index).
        let estimated_size_bytes =
            memory_segment.total_point_count() * self.dimension * VECTOR_ELEMENT_SIZE;
        let estmated_size_kb = estimated_size_bytes / 1024;
        let index_type = if estmated_size_kb >= hnsw_threshold_bytes {
            QdrantVectorIndexType::HNSW
        } else {
            QdrantVectorIndexType::Plain
        };

        let disk_timer = metrics::qdrant_segment_disk_build_timer(index_type);

        let result = match index_type {
            QdrantVectorIndexType::Plain => {
                let num_vectors = memory_segment.total_point_count() as u32;
                let num_deleted = memory_segment.deleted_point_count() as u32;
                anyhow::ensure!(
                    num_deleted + memory_segment.available_point_count() as u32 == num_vectors
                );
                Ok::<VectorDiskSegmentValues, anyhow::Error>(VectorDiskSegmentValues {
                    paths: snapshot_segment(
                        &id_tracker,
                        &memory_segment,
                        tmpdir.path(),
                        index_path,
                    )?,
                    num_vectors,
                    num_deleted,
                })
            },
            QdrantVectorIndexType::HNSW => {
                let indexing_path = tmpdir.path().join("indexing");
                fs::create_dir_all(&indexing_path)?;
                let disk_path = index_path.join("disk");
                fs::create_dir_all(&disk_path)?;
                let disk_config = segment_config(self.dimension, false, *VECTOR_INDEX_THREADS);
                build_disk_segment(&memory_segment, &indexing_path, &disk_path, disk_config)
            },
        }?;
        disk_timer.finish();

        tracing::debug!("Built a {index_type:?} vector index for {index_path:?}",);

        Ok(Some(result))
    }
}

#[derive(Clone, Debug)]
pub struct QdrantDocument {
    pub internal_id: InternalId,
    pub vector: IndexedVector,
    pub filter_fields: BTreeMap<FieldPath, Vec<u8>>,
}

impl QdrantDocument {
    pub fn qdrant_vector(&self) -> NamedVectors<'_> {
        NamedVectors::from_ref(DEFAULT_VECTOR_NAME, VectorRef::Dense(&self.vector[..]))
    }

    pub fn encode_payload(&self, ts: Timestamp) -> anyhow::Result<JsonValue> {
        let mut map = serde_json::Map::new();
        for (field_path, field_value) in &self.filter_fields {
            let mut current = &mut map;
            // The path should consist of nested json objects.
            for i in 0..field_path.fields().len() - 1 {
                let field: String = field_path.fields()[i].clone().into();
                let JsonValue::Object(inner) = current
                    .entry(field)
                    .or_insert_with(|| JsonValue::Object(serde_json::Map::new()))
                else {
                    // This means one filter field path is a prefix of another. We should
                    // prevent the developer from defining such index. Throw a system error here.
                    anyhow::bail!("Conflicting field path: {:?} {:?}", field_path, map);
                };
                current = inner;
            }
            current.insert(
                field_path.last().clone().into(),
                JsonValue::String(base64::encode_urlsafe(&field_value[..])),
            );
        }
        map.insert(
            TIMESTAMP_FIELD.to_string(),
            JsonValue::String(base64::encode_urlsafe(&u64::from(ts).to_le_bytes()[..])),
        );
        Ok(map.into())
    }

    /// Estimates size of `QdrantDocument` in bytes
    pub fn estimate_size(&self) -> usize {
        self.vector.len() * mem::size_of::<VectorElementType>()
    }
}

// NB: For cosine similarity, we need to normalize vectors before indexing them.
#[derive(Clone, Debug)]
pub struct NormalizedQdrantDocument {
    pub internal_id: InternalId,
    pub vector: Vec<f32>,
    pub filter_fields: BTreeMap<FieldPath, Vec<u8>>,
}

impl From<QdrantDocument> for NormalizedQdrantDocument {
    fn from(value: QdrantDocument) -> Self {
        let vector = Vec::from(value.vector);
        let vector = CosineMetric::preprocess(vector);
        Self {
            internal_id: value.internal_id,
            vector,
            filter_fields: value.filter_fields,
        }
    }
}

impl NormalizedQdrantDocument {
    pub fn size(&self) -> usize {
        let mut size = 0;
        size += self.vector.len() * mem::size_of::<f32>();
        size += self.filter_fields.len() * mem::size_of::<(FieldPath, Vec<u8>)>();
        for (field_path, maybe_value) in &self.filter_fields {
            size += field_path.fields().iter().map(|f| f.len()).sum::<usize>();
            size += maybe_value.len();
        }
        size
    }
}

fn encode_user_field_path(field_path: &FieldPath) -> anyhow::Result<JsonPath> {
    let key = String::from(field_path.clone());
    json_path_from_str(key.as_str())
}

fn qdrant_filter_condition(condition: &CompiledVectorFilter) -> Match {
    match condition {
        CompiledVectorFilter::Eq(value) => {
            let value_b64 = base64::encode_urlsafe(&value[..]);
            let match_value = MatchValue {
                value: ValueVariants::Keyword(value_b64),
            };
            Match::Value(match_value)
        },
        CompiledVectorFilter::In(values) => {
            let values_b64 = values
                .iter()
                .map(|v| base64::encode_urlsafe(&v[..]))
                .collect();
            let match_value = MatchAny {
                any: AnyVariants::Keywords(values_b64),
            };
            Match::Any(match_value)
        },
    }
}

impl From<QdrantSchema> for proto::VectorIndexConfig {
    fn from(value: QdrantSchema) -> Self {
        proto::VectorIndexConfig {
            dimension: value.dimension as u32,
            vector_field_path: Some(value.vector_field.into()),
            filter_fields: value.filter_fields.into_iter().map(|f| f.into()).collect(),
        }
    }
}

impl TryFrom<proto::VectorIndexConfig> for QdrantSchema {
    type Error = anyhow::Error;

    fn try_from(value: proto::VectorIndexConfig) -> Result<Self, Self::Error> {
        let vector_field = value
            .vector_field_path
            .ok_or_else(|| anyhow::anyhow!("Missing vector field path in VectorIndexConfigProto"))?
            .try_into()?;
        let filter_fields = value
            .filter_fields
            .into_iter()
            .map(|f| f.try_into())
            .collect::<Result<_, _>>()?;
        Ok(QdrantSchema {
            dimension: value.dimension as usize,
            vector_field,
            filter_fields,
        })
    }
}

/// A workaround for circular dependencies between database
/// (vector_index_worker) and qdrant.
pub trait PreviousVectorSegmentsHack {
    /// Marks the id deleted, returning a failure of an invariant was violated
    /// (this should never happen!)
    fn maybe_delete_qdrant(&mut self, external_id: ExtendedPointId) -> anyhow::Result<()>;
}

pub struct QdrantExternalId(PointIdType);

impl TryFrom<InternalId> for QdrantExternalId {
    type Error = anyhow::Error;

    fn try_from(value: InternalId) -> Result<Self, Self::Error> {
        let uuid = Uuid::from_bytes(value[..].try_into()?);
        Ok(Self(PointIdType::Uuid(uuid)))
    }
}

impl TryFrom<&InternalDocumentId> for QdrantExternalId {
    type Error = anyhow::Error;

    fn try_from(value: &InternalDocumentId) -> Result<Self, Self::Error> {
        let uuid = Uuid::from_bytes(value.internal_id()[..].try_into()?);
        Ok(Self(PointIdType::Uuid(uuid)))
    }
}

impl TryFrom<ResolvedDocumentId> for QdrantExternalId {
    type Error = anyhow::Error;

    fn try_from(value: ResolvedDocumentId) -> Result<Self, Self::Error> {
        let uuid = Uuid::from_bytes(value.internal_id()[..].try_into()?);
        Ok(Self(PointIdType::Uuid(uuid)))
    }
}

impl Deref for QdrantExternalId {
    type Target = PointIdType;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

fn json_path_from_str(s: &str) -> anyhow::Result<JsonPath> {
    match JsonPath::from_str(s) {
        Ok(path) => Ok(path),
        Err(()) => {
            anyhow::bail!("Unable to parse to JsonPath: {s}");
        },
    }
}
