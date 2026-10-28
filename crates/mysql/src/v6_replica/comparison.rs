//! Compares consumed entries from sampled V5 index scans with V6.
use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
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
    Stream,
    StreamExt as _,
    TryStreamExt as _,
};
use serde_json::Value as JsonValue;
use tokio::sync::{
    OwnedSemaphorePermit,
    RwLock,
};

use super::{
    metrics::{
        self,
        ComparisonOutcome,
    },
    replica::ReplicaWriter,
};

pub(crate) struct ComparingV5Reader<RT: Runtime> {
    v5: Arc<dyn PersistenceReader>,
    replica: Arc<ReplicaWriter<RT>>,
    source_calls: Arc<RwLock<()>>,
}

impl<RT: Runtime> ComparingV5Reader<RT> {
    pub(crate) fn new(
        v5: Arc<dyn PersistenceReader>,
        replica: Arc<ReplicaWriter<RT>>,
        source_calls: Arc<RwLock<()>>,
    ) -> Self {
        Self {
            v5,
            replica,
            source_calls,
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
        self.v5
            .load_revision_pairs(tablet_id, range, order, page_size, retention_validator)
    }

    async fn previous_revisions(
        &self,
        ids: BTreeSet<(InternalDocumentId, Timestamp)>,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> anyhow::Result<BTreeMap<(InternalDocumentId, Timestamp), DocumentLogEntry>> {
        self.v5.previous_revisions(ids, retention_validator).await
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
        let Some(comparison) = IndexScanComparison::try_sample(
            &self.replica,
            &self.source_calls,
            IndexScanRequest {
                index,
                tablet_id,
                read_timestamp,
                range: range.clone(),
                order,
                retention_validator,
            },
        ) else {
            return stream;
        };
        let stream = CapturingIndexStream {
            inner: stream,
            capture: IndexScanCapture {
                max_entries: self.replica.settings.max_comparison_entries,
                comparison: Some(comparison),
                source_entries: Vec::new(),
            },
        }
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

#[derive(Debug, PartialEq)]
struct IndexEntryMetadata {
    key: IndexKeyBytes,
    ts: Timestamp,
    id: InternalDocumentId,
    prev_ts: Option<Timestamp>,
}

impl IndexEntryMetadata {
    fn new(key: IndexKeyBytes, document: &LatestDocument) -> Self {
        Self {
            key,
            ts: document.ts,
            id: document.value.id_with_table_id(),
            prev_ts: document.prev_ts,
        }
    }
}

struct IndexScanComparison<RT: Runtime> {
    replica: Arc<ReplicaWriter<RT>>,
    source_calls: Arc<RwLock<()>>,
    scan: IndexScanRequest,
    // Held from sampling through outcome logging to bound capture buffers and
    // concurrent V6 reads, and let tests wait for recorded outcomes.
    _permit: OwnedSemaphorePermit,
}

impl<RT: Runtime> IndexScanComparison<RT> {
    fn try_sample(
        replica: &Arc<ReplicaWriter<RT>>,
        source_calls: &Arc<RwLock<()>>,
        scan: IndexScanRequest,
    ) -> Option<Self> {
        if rand::random::<f64>() >= replica.settings.comparison_sample_rate {
            return None;
        }
        let log = |outcome| metrics::log_comparison(&replica.cluster_name, outcome);
        log(ComparisonOutcome::Sampled);
        if replica.is_stopped() || scan.index.persistence_index_id().is_none() {
            log(ComparisonOutcome::Ineligible);
            return None;
        }
        let Ok(permit) = replica.comparison_permits.clone().try_acquire_owned() else {
            log(ComparisonOutcome::Dropped);
            return None;
        };
        Some(Self {
            replica: replica.clone(),
            source_calls: source_calls.clone(),
            scan,
            _permit: permit,
        })
    }

    fn spawn_comparison(self, source_entries: Vec<IndexEntryMetadata>, source_exhausted: bool) {
        // Streams may be dropped outside a Tokio runtime during shutdown.
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        common::runtime::tokio_spawn("v6_replica_comparison", async move {
            let outcome = self
                .compare_with_replica(source_entries, source_exhausted)
                .await;
            metrics::log_comparison(&self.replica.cluster_name, outcome);
        });
    }

    async fn compare_with_replica(
        &self,
        source_entries: Vec<IndexEntryMetadata>,
        source_exhausted: bool,
    ) -> ComparisonOutcome {
        let scan = &self.scan;
        // V5 commits can be visible before their replay is queued. Once source
        // calls are idle, every write observed by this scan has queued replay
        // or retired the replica. Release the lock before waiting for replay.
        if self.source_calls.try_write().is_err() {
            return ComparisonOutcome::Invalidated;
        }
        if !self
            .replica
            .wait_for_prior_replay(Some(
                tokio::time::Instant::now() + self.replica.settings.apply_wait,
            ))
            .await
        {
            return ComparisonOutcome::Invalidated;
        }
        // Once V5 reaches the end, read one extra entry to detect a longer V6
        // result. A captured prefix establishes only its own length.
        let limit = source_entries.len() + usize::from(source_exhausted);
        let reader = self.replica.v6.reader();
        let replica_entries: anyhow::Result<Vec<IndexEntryMetadata>> = reader
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
            .map_ok(|(key, document)| IndexEntryMetadata::new(key, &document))
            .try_collect()
            .await;
        if scan
            .retention_validator
            .validate_snapshot(scan.read_timestamp)
            .await
            .is_err()
        {
            return ComparisonOutcome::Expired;
        }
        let replica_entries = match replica_entries {
            Ok(replica_entries) => replica_entries,
            Err(error) if error.is_out_of_retention() => return ComparisonOutcome::Expired,
            Err(error) => {
                tracing::warn!("V6 replica comparison could not read the replica: {error:#}");
                return ComparisonOutcome::Error;
            },
        };
        // Retirement can delete data during the scan; check ownership after
        // reading to avoid reporting cleanup as a mismatch.
        match self.replica.is_active_owner().await {
            Ok(true) => {},
            Ok(false) => return ComparisonOutcome::Invalidated,
            Err(error) => {
                tracing::warn!("V6 replica comparison could not read replica state: {error:#}");
                return ComparisonOutcome::Error;
            },
        }
        if replica_entries == source_entries {
            return ComparisonOutcome::Match;
        }
        let position = source_entries
            .iter()
            .zip(&replica_entries)
            .position(|(source_entry, replica_entry)| source_entry != replica_entry)
            .unwrap_or(source_entries.len().min(replica_entries.len()));
        tracing::error!(
            instance_name = %self.replica.instance_name,
            "V6 replica index scan mismatch on {:?} at {} entry {position}: V5 {:?}, V6 {:?} \
             ({} V5 entries, {} V6, complete: {source_exhausted})",
            scan.index,
            scan.read_timestamp,
            source_entries.get(position),
            replica_entries.get(position),
            source_entries.len(),
            replica_entries.len(),
        );
        ComparisonOutcome::Mismatch
    }
}

/// A capped or dropped scan supplies a prefix; reaching EOF also establishes
/// the full result length.
struct IndexScanCapture<RT: Runtime> {
    comparison: Option<IndexScanComparison<RT>>,
    source_entries: Vec<IndexEntryMetadata>,
    max_entries: usize,
}

impl<RT: Runtime> IndexScanCapture<RT> {
    fn capture_entry(&mut self, key: &IndexKeyBytes, document: &LatestDocument) {
        if self.comparison.is_none() {
            return;
        }
        self.source_entries
            .push(IndexEntryMetadata::new(key.clone(), document));
        if self.source_entries.len() >= self.max_entries {
            self.start_comparison(false);
        }
    }

    fn cancel_on_source_error(&mut self) {
        if let Some(comparison) = self.comparison.take() {
            metrics::log_comparison(
                &comparison.replica.cluster_name,
                ComparisonOutcome::ServingError,
            );
        }
    }

    fn start_comparison(&mut self, source_exhausted: bool) {
        if let Some(comparison) = self.comparison.take() {
            comparison.spawn_comparison(mem::take(&mut self.source_entries), source_exhausted);
        }
    }
}

impl<RT: Runtime> Drop for IndexScanCapture<RT> {
    fn drop(&mut self) {
        if !self.source_entries.is_empty() {
            self.start_comparison(false);
        }
    }
}

struct CapturingIndexStream<'a, RT: Runtime> {
    inner: IndexStream<'a>,
    capture: IndexScanCapture<RT>,
}

impl<RT: Runtime> Stream for CapturingIndexStream<'_, RT> {
    type Item = anyhow::Result<(IndexKeyBytes, LatestDocument)>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        let item = ready!(this.inner.poll_next_unpin(cx));
        match &item {
            Some(Ok((key, document))) => this.capture.capture_entry(key, document),
            Some(Err(_)) => this.capture.cancel_on_source_error(),
            None => this.capture.start_comparison(true),
        }
        Poll::Ready(item)
    }
}
