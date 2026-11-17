//! Compares sampled V5 reads with the V6 replica: index scans, previous
//! revisions, and revision pairs. Only metadata is compared, never document
//! values.
use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    fmt::Debug,
    mem,
    pin::Pin,
    sync::Arc,
    task::{
        ready,
        Context,
        Poll,
    },
};

use async_trait::async_trait;
use common::{
    index::{
        IndexKey,
        IndexKeyBytes,
    },
    interval::Interval,
    persistence::{
        DocumentLogEntry,
        DocumentPrevTsQuery,
        DocumentRevisionStream,
        DocumentStream,
        IndexStream,
        LatestDocument,
        PersistenceGlobalKey,
        PersistenceReader,
        PersistenceTableSize,
        RetentionValidator,
        TimestampRange,
    },
    persistence_helpers::RevisionPair,
    query::Order,
    runtime::Runtime,
    types::{
        IndexRef,
        PersistenceVersion,
        Timestamp,
    },
    value::{
        InternalDocumentId,
        TabletId,
    },
};
use errors::ErrorMetadataAnyhowExt as _;
use futures::{
    stream::BoxStream,
    Stream,
    StreamExt as _,
    TryStreamExt as _,
};
use serde_json::Value as JsonValue;
use tokio::sync::OwnedSemaphorePermit;

use super::{
    in_flight::InFlightCalls,
    metrics::{
        self,
        ComparedRead,
        ComparisonOutcome,
    },
    replica::ReplicaWriter,
};

pub(crate) struct ComparingV5Reader<RT: Runtime> {
    v5: Arc<dyn PersistenceReader>,
    replica: Arc<ReplicaWriter<RT>>,
    in_flight: Arc<InFlightCalls>,
}

impl<RT: Runtime> ComparingV5Reader<RT> {
    pub(crate) fn new(
        v5: Arc<dyn PersistenceReader>,
        replica: Arc<ReplicaWriter<RT>>,
        in_flight: Arc<InFlightCalls>,
    ) -> Self {
        Self {
            v5,
            replica,
            in_flight,
        }
    }
}

#[async_trait]
impl<RT: Runtime> PersistenceReader for ComparingV5Reader<RT> {
    fn load_documents(
        &self,
        range: TimestampRange,
        order: Order,
        page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> DocumentStream<'_> {
        self.v5
            .load_documents(range, order, page_size, retention_validator)
    }

    fn load_documents_from_table(
        &self,
        tablet_id: TabletId,
        range: TimestampRange,
        order: Order,
        page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> DocumentStream<'_> {
        self.v5
            .load_documents_from_table(tablet_id, range, order, page_size, retention_validator)
    }

    fn load_revision_pairs(
        &self,
        tablet_id: Option<TabletId>,
        range: TimestampRange,
        order: Order,
        page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> DocumentRevisionStream<'_> {
        let stream = self.v5.load_revision_pairs(
            tablet_id,
            range,
            order,
            page_size,
            retention_validator.clone(),
        );
        let Some(comparison) = Comparison::try_sample(
            &self.replica,
            &self.in_flight,
            ReadRequest::RevisionPairs(RevisionPairsRequest {
                tablet_id,
                range,
                order,
                page_size,
                retention_validator,
            }),
        ) else {
            return stream;
        };
        let stream = CapturingStream::new(
            stream,
            comparison,
            self.replica.settings.max_comparison_entries,
            RevisionPairMetadata::new,
            ReadResult::RevisionPairs,
        )
        .boxed();
        stream
    }

    async fn previous_revisions(
        &self,
        ids: BTreeSet<(InternalDocumentId, Timestamp)>,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> anyhow::Result<BTreeMap<(InternalDocumentId, Timestamp), DocumentLogEntry>> {
        // The replica re-reads a bounded prefix of the request.
        let compared_ids: BTreeSet<_> = ids
            .iter()
            .take(self.replica.settings.max_comparison_entries)
            .copied()
            .collect();
        let comparison = if compared_ids.is_empty() {
            None
        } else {
            Comparison::try_sample(
                &self.replica,
                &self.in_flight,
                ReadRequest::PreviousRevisions(PreviousRevisionsRequest {
                    ids: compared_ids.clone(),
                    retention_validator: retention_validator.clone(),
                }),
            )
        };
        let result = self.v5.previous_revisions(ids, retention_validator).await;
        if let Some(comparison) = comparison {
            match &result {
                Ok(revisions) => comparison
                    .spawn_comparison(previous_revisions_metadata(&compared_ids, revisions), true),
                Err(_) => comparison.cancel_on_source_error(),
            }
        }
        result
    }

    async fn previous_revisions_of_documents(
        &self,
        ids: BTreeSet<DocumentPrevTsQuery>,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> anyhow::Result<BTreeMap<DocumentPrevTsQuery, DocumentLogEntry>> {
        self.v5
            .previous_revisions_of_documents(ids, retention_validator)
            .await
    }

    fn index_scan(
        &self,
        index: IndexRef,
        tablet_id: TabletId,
        read_timestamp: Timestamp,
        range: &Interval,
        order: Order,
        size_hint: usize,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> IndexStream<'_> {
        let stream = self.v5.index_scan(
            index,
            tablet_id,
            read_timestamp,
            range,
            order,
            size_hint,
            retention_validator.clone(),
        );
        let Some(comparison) = Comparison::try_sample(
            &self.replica,
            &self.in_flight,
            ReadRequest::IndexScan(IndexScanRequest {
                index,
                tablet_id,
                read_timestamp,
                range: range.clone(),
                order,
                retention_validator,
            }),
        ) else {
            return stream;
        };
        let stream = CapturingStream::new(
            stream,
            comparison,
            self.replica.settings.max_comparison_entries,
            index_entry_metadata,
            ReadResult::IndexScan,
        )
        .boxed();
        stream
    }

    async fn get_persistence_global(
        &self,
        key: PersistenceGlobalKey,
    ) -> anyhow::Result<Option<JsonValue>> {
        self.v5.get_persistence_global(key).await
    }

    async fn index_get(
        &self,
        index: IndexRef,
        tablet_id: TabletId,
        read_timestamp: Timestamp,
        key: IndexKey,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> anyhow::Result<Option<LatestDocument>> {
        self.v5
            .index_get(index, tablet_id, read_timestamp, key, retention_validator)
            .await
    }

    async fn max_ts(&self) -> anyhow::Result<Option<Timestamp>> {
        self.v5.max_ts().await
    }

    fn version(&self) -> PersistenceVersion {
        self.v5.version()
    }

    fn needs_index_retention_deletes(&self) -> bool {
        self.v5.needs_index_retention_deletes()
    }

    async fn table_size_stats(&self) -> anyhow::Result<Vec<PersistenceTableSize>> {
        self.v5.table_size_stats().await
    }
}

struct IndexScanRequest {
    index: IndexRef,
    tablet_id: TabletId,
    read_timestamp: Timestamp,
    range: Interval,
    order: Order,
    retention_validator: Arc<dyn RetentionValidator>,
}

struct PreviousRevisionsRequest {
    ids: BTreeSet<(InternalDocumentId, Timestamp)>,
    retention_validator: Arc<dyn RetentionValidator>,
}

struct RevisionPairsRequest {
    tablet_id: Option<TabletId>,
    range: TimestampRange,
    order: Order,
    page_size: u32,
    retention_validator: Arc<dyn RetentionValidator>,
}

/// A sampled V5 read, re-run on the replica with the same arguments.
enum ReadRequest {
    IndexScan(IndexScanRequest),
    PreviousRevisions(PreviousRevisionsRequest),
    RevisionPairs(RevisionPairsRequest),
}

impl ReadRequest {
    fn kind(&self) -> ComparedRead {
        match self {
            Self::IndexScan(_) => ComparedRead::IndexScan,
            Self::PreviousRevisions(_) => ComparedRead::PreviousRevisions,
            Self::RevisionPairs(_) => ComparedRead::RevisionPairs,
        }
    }

    /// V6 reads an index only through its persistence index ID.
    fn eligible(&self) -> bool {
        match self {
            Self::IndexScan(scan) => scan.index.persistence_index_id().is_some(),
            Self::PreviousRevisions(_) | Self::RevisionPairs(_) => true,
        }
    }

    /// Streams read one entry past a complete source result to detect a longer
    /// replica result; a captured prefix establishes only its own length.
    async fn read_replica(
        &self,
        reader: &dyn PersistenceReader,
        source: &ReadResult,
        source_exhausted: bool,
    ) -> anyhow::Result<ReadResult> {
        let limit = source.len() + usize::from(source_exhausted);
        match self {
            Self::IndexScan(scan) => Ok(ReadResult::IndexScan(
                reader
                    .index_scan(
                        scan.index,
                        scan.tablet_id,
                        scan.read_timestamp,
                        &scan.range,
                        scan.order,
                        limit,
                        scan.retention_validator.clone(),
                    )
                    .take(limit)
                    .map_ok(|entry| index_entry_metadata(&entry))
                    .try_collect()
                    .await?,
            )),
            Self::PreviousRevisions(request) => {
                let revisions = reader
                    .previous_revisions(request.ids.clone(), request.retention_validator.clone())
                    .await?;
                Ok(previous_revisions_metadata(&request.ids, &revisions))
            },
            Self::RevisionPairs(request) => Ok(ReadResult::RevisionPairs(
                reader
                    .load_revision_pairs(
                        request.tablet_id,
                        request.range,
                        request.order,
                        request.page_size,
                        request.retention_validator.clone(),
                    )
                    .take(limit)
                    .map_ok(|pair| RevisionPairMetadata::new(&pair))
                    .try_collect()
                    .await?,
            )),
        }
    }

    /// Checked after reading the replica, as the source checks after its own
    /// read, so a snapshot retention has passed compares as expired.
    async fn validate_snapshot(&self) -> anyhow::Result<()> {
        match self {
            Self::IndexScan(scan) => {
                scan.retention_validator
                    .validate_snapshot(scan.read_timestamp)
                    .await
            },
            Self::PreviousRevisions(request) => match request.ids.iter().map(|(_, ts)| *ts).min() {
                Some(min_ts) => {
                    request
                        .retention_validator
                        .validate_document_snapshot(min_ts)
                        .await
                },
                None => Ok(()),
            },
            Self::RevisionPairs(request) => {
                request
                    .retention_validator
                    .validate_document_snapshot(request.range.min_timestamp_inclusive())
                    .await
            },
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::IndexScan(scan) => {
                format!("index scan of {:?} at {}", scan.index, scan.read_timestamp)
            },
            Self::PreviousRevisions(request) => {
                format!("previous revisions of {} documents", request.ids.len())
            },
            Self::RevisionPairs(request) => format!(
                "revision pairs of {:?} in {:?} {:?}",
                request.tablet_id, request.range, request.order
            ),
        }
    }
}

#[derive(Debug, PartialEq)]
struct IndexEntryMetadata {
    key: IndexKeyBytes,
    ts: Timestamp,
    id: InternalDocumentId,
    prev_ts: Option<Timestamp>,
}

fn index_entry_metadata((key, document): &(IndexKeyBytes, LatestDocument)) -> IndexEntryMetadata {
    IndexEntryMetadata {
        key: key.clone(),
        ts: document.ts,
        id: document.value.id_with_table_id(),
        prev_ts: document.prev_ts,
    }
}

/// A document log entry without its value.
#[derive(Debug, PartialEq)]
struct RevisionMetadata {
    ts: Timestamp,
    tombstone: bool,
    prev_ts: Option<Timestamp>,
}

impl RevisionMetadata {
    fn new(entry: &DocumentLogEntry) -> Self {
        Self {
            ts: entry.ts,
            tombstone: entry.value.is_none(),
            prev_ts: entry.prev_ts,
        }
    }
}

/// A revision pair without document values. Whether the predecessor's value
/// is still stored depends on document retention timing on each side, so only
/// its timestamp is compared.
#[derive(Debug, PartialEq)]
struct RevisionPairMetadata {
    id: InternalDocumentId,
    ts: Timestamp,
    tombstone: bool,
    prev_ts: Option<Timestamp>,
}

impl RevisionPairMetadata {
    fn new(pair: &RevisionPair) -> Self {
        Self {
            id: pair.id,
            ts: pair.rev.ts,
            tombstone: pair.rev.document.is_none(),
            prev_ts: pair.prev_rev.as_ref().map(|prev| prev.ts),
        }
    }
}

fn previous_revisions_metadata(
    ids: &BTreeSet<(InternalDocumentId, Timestamp)>,
    revisions: &BTreeMap<(InternalDocumentId, Timestamp), DocumentLogEntry>,
) -> ReadResult {
    ReadResult::PreviousRevisions(
        ids.iter()
            .filter_map(|key| {
                revisions
                    .get(key)
                    .map(|entry| (*key, RevisionMetadata::new(entry)))
            })
            .collect(),
    )
}

#[derive(Debug, PartialEq)]
enum ReadResult {
    IndexScan(Vec<IndexEntryMetadata>),
    PreviousRevisions(BTreeMap<(InternalDocumentId, Timestamp), RevisionMetadata>),
    RevisionPairs(Vec<RevisionPairMetadata>),
}

impl ReadResult {
    fn len(&self) -> usize {
        match self {
            Self::IndexScan(entries) => entries.len(),
            Self::PreviousRevisions(revisions) => revisions.len(),
            Self::RevisionPairs(pairs) => pairs.len(),
        }
    }

    /// The first entry on which the source and replica results differ.
    fn first_difference(&self, replica: &Self) -> String {
        match (self, replica) {
            (Self::IndexScan(source), Self::IndexScan(replica)) => {
                first_differing_entry(source, replica)
            },
            (Self::RevisionPairs(source), Self::RevisionPairs(replica)) => {
                first_differing_entry(source, replica)
            },
            (Self::PreviousRevisions(source), Self::PreviousRevisions(replica)) => {
                match source
                    .keys()
                    .chain(replica.keys())
                    .find(|&key| source.get(key) != replica.get(key))
                {
                    Some(key) => format!(
                        "{key:?}: V5 {:?}, V6 {:?}",
                        source.get(key),
                        replica.get(key)
                    ),
                    None => "no differing entry".to_owned(),
                }
            },
            _ => "results of different reads".to_owned(),
        }
    }
}

fn first_differing_entry<T: PartialEq + Debug>(source: &[T], replica: &[T]) -> String {
    let position = source
        .iter()
        .zip(replica)
        .position(|(source_entry, replica_entry)| source_entry != replica_entry)
        .unwrap_or(source.len().min(replica.len()));
    format!(
        "entry {position}: V5 {:?}, V6 {:?}",
        source.get(position),
        replica.get(position)
    )
}

/// A sampled read whose result is re-run on the replica once replay has
/// caught up with every write the source read could have observed.
struct Comparison<RT: Runtime> {
    replica: Arc<ReplicaWriter<RT>>,
    in_flight: Arc<InFlightCalls>,
    request: ReadRequest,
    // Held from sampling through outcome logging to bound capture buffers and
    // let tests wait for recorded outcomes.
    _permit: OwnedSemaphorePermit,
}

impl<RT: Runtime> Comparison<RT> {
    fn try_sample(
        replica: &Arc<ReplicaWriter<RT>>,
        in_flight: &Arc<InFlightCalls>,
        request: ReadRequest,
    ) -> Option<Self> {
        if rand::random::<f64>() >= replica.settings.comparison_sample_rate {
            return None;
        }
        let log = |outcome| metrics::log_comparison(&replica.cluster_name, request.kind(), outcome);
        log(ComparisonOutcome::Sampled);
        if !replica.is_active() || !request.eligible() {
            log(ComparisonOutcome::Ineligible);
            return None;
        }
        let Ok(permit) = replica.comparison_permits.clone().try_acquire_owned() else {
            log(ComparisonOutcome::Dropped);
            return None;
        };
        Some(Self {
            replica: replica.clone(),
            in_flight: in_flight.clone(),
            request,
            _permit: permit,
        })
    }

    fn log(&self, outcome: ComparisonOutcome) {
        metrics::log_comparison(&self.replica.cluster_name, self.request.kind(), outcome);
    }

    fn cancel_on_source_error(self) {
        self.log(ComparisonOutcome::ServingError);
    }

    fn spawn_comparison(self, source: ReadResult, source_exhausted: bool) {
        // Streams may be dropped outside a Tokio runtime during shutdown.
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        // Calls begun after the read ended cannot have been visible to it.
        let calls_through = self.in_flight.latest();
        common::runtime::tokio_spawn("v6_replica_comparison", async move {
            let outcome = self
                .compare_with_replica(&source, source_exhausted, calls_through)
                .await;
            self.log(outcome);
        });
    }

    async fn compare_with_replica(
        &self,
        source: &ReadResult,
        source_exhausted: bool,
        calls_through: Option<u64>,
    ) -> ComparisonOutcome {
        let deadline = tokio::time::Instant::now() + self.replica.settings.apply_wait;
        // V5 commits can be visible before their replay is queued. Once every
        // call begun before the read ended has finished, each write the read
        // observed has queued replay or retired the replica.
        if let Some(calls_through) = calls_through
            && !self
                .in_flight
                .wait_finished_through(calls_through, deadline)
                .await
        {
            return ComparisonOutcome::Invalidated;
        }
        if !self.replica.wait_for_prior_replay(Some(deadline)).await {
            return ComparisonOutcome::Invalidated;
        }
        let Ok(v6) = self.replica.v6() else {
            return ComparisonOutcome::Invalidated;
        };
        let reader = v6.reader();
        let replica_result = {
            let Ok(Ok(_read_permit)) =
                tokio::time::timeout_at(deadline, self.replica.replica_read_permits.acquire())
                    .await
            else {
                return ComparisonOutcome::Dropped;
            };
            self.request
                .read_replica(reader.as_ref(), source, source_exhausted)
                .await
        };
        if self.request.validate_snapshot().await.is_err() {
            return ComparisonOutcome::Expired;
        }
        let replica_result = match replica_result {
            Ok(replica_result) => replica_result,
            Err(error) if error.is_out_of_retention() => return ComparisonOutcome::Expired,
            Err(error) => {
                tracing::warn!("V6 replica comparison could not read the replica: {error:#}");
                return ComparisonOutcome::Error;
            },
        };
        if replica_result == *source {
            return ComparisonOutcome::Match;
        }
        // Retirement can delete data during the read; confirm ownership before
        // reporting cleanup as a mismatch.
        match self.replica.is_active_owner().await {
            Ok(true) => {},
            Ok(false) => return ComparisonOutcome::Invalidated,
            Err(error) => {
                tracing::warn!("V6 replica comparison could not read replica state: {error:#}");
                return ComparisonOutcome::Error;
            },
        }
        let kind: &'static str = self.request.kind().into();
        tracing::error!(
            instance_name = %self.replica.instance_name,
            "V6 replica {kind} mismatch on {}, {} ({} V5 entries, {} V6, complete: \
             {source_exhausted})",
            self.request.describe(),
            source.first_difference(&replica_result),
            source.len(),
            replica_result.len(),
        );
        ComparisonOutcome::Mismatch
    }
}

/// A capped or dropped stream supplies a prefix; reaching EOF also establishes
/// the full result length.
struct Capture<RT: Runtime, M> {
    comparison: Option<Comparison<RT>>,
    entries: Vec<M>,
    max_entries: usize,
    into_result: fn(Vec<M>) -> ReadResult,
}

impl<RT: Runtime, M> Capture<RT, M> {
    fn capture_entry(&mut self, entry: M) {
        if self.comparison.is_none() {
            return;
        }
        self.entries.push(entry);
        if self.entries.len() >= self.max_entries {
            self.start_comparison(false);
        }
    }

    fn cancel_on_source_error(&mut self) {
        if let Some(comparison) = self.comparison.take() {
            comparison.cancel_on_source_error();
        }
    }

    fn start_comparison(&mut self, source_exhausted: bool) {
        if let Some(comparison) = self.comparison.take() {
            comparison.spawn_comparison(
                (self.into_result)(mem::take(&mut self.entries)),
                source_exhausted,
            );
        }
    }
}

impl<RT: Runtime, M> Drop for Capture<RT, M> {
    fn drop(&mut self) {
        if !self.entries.is_empty() {
            self.start_comparison(false);
        }
    }
}

struct CapturingStream<'a, RT: Runtime, T, M> {
    inner: BoxStream<'a, anyhow::Result<T>>,
    capture: Capture<RT, M>,
    metadata: fn(&T) -> M,
}

impl<'a, RT: Runtime, T, M> CapturingStream<'a, RT, T, M> {
    fn new(
        inner: BoxStream<'a, anyhow::Result<T>>,
        comparison: Comparison<RT>,
        max_entries: usize,
        metadata: fn(&T) -> M,
        into_result: fn(Vec<M>) -> ReadResult,
    ) -> Self {
        Self {
            inner,
            capture: Capture {
                comparison: Some(comparison),
                entries: Vec::new(),
                max_entries,
                into_result,
            },
            metadata,
        }
    }
}

impl<RT: Runtime, T, M: Unpin> Stream for CapturingStream<'_, RT, T, M> {
    type Item = anyhow::Result<T>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        let item = ready!(this.inner.poll_next_unpin(cx));
        match &item {
            Some(Ok(entry)) => this.capture.capture_entry((this.metadata)(entry)),
            Some(Err(_)) => this.capture.cancel_on_source_error(),
            None => this.capture.start_comparison(true),
        }
        Poll::Ready(item)
    }
}
