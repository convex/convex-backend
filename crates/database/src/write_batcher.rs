use std::{
    sync::Arc,
    time::Duration,
};

use anyhow::Context as _;
use common::{
    backoff::Backoff,
    errors::{
        is_db_deadlock_error,
        is_transient_db_error,
        recapture_stacktrace_noreport,
        report_error,
    },
    knobs::{
        COMMITTER_BATCH_WRITE_THRESHOLD,
        COMMITTER_MAX_COMMIT_DELAY,
        COMMITTER_MAX_CONCURRENT_WRITE_BATCHES,
        COMMITTER_MAX_WRITE_BATCH_BYTES,
        COMMITTER_MAX_WRITE_BATCH_DOCUMENTS,
        INITIAL_PERSISTENCE_WRITES_BACKOFF,
        MAX_PERSISTENCE_WRITES_BACKOFF,
    },
    persistence::{
        ConflictStrategy,
        DocumentLogEntry,
        Persistence,
        PersistenceIndexEntry,
    },
    runtime::{
        tokio_spawn,
        JoinSet,
        Runtime,
    },
};
use futures::{
    pin_mut,
    select_biased,
    FutureExt,
};
use tokio::sync::{
    mpsc,
    oneshot,
};
use tokio_util::task::AbortOnDropHandle;

use crate::metrics;

/// One commit's rows, waiting to be written to persistence as part of a batch.
struct WriteRequest {
    documents: Vec<DocumentLogEntry>,
    indexes: Vec<PersistenceIndexEntry>,
    size_bytes: u64,
    ack: oneshot::Sender<anyhow::Result<()>>,
}

#[derive(Clone, Copy)]
pub struct WriteBatcherConfig {
    /// Document rows at which a batch stops accepting further commits.
    pub max_documents: usize,
    /// Serialized batch size at which a batch stops accepting further commits.
    pub max_bytes: u64,
    /// How long to hold a partially-filled batch open for commits that are
    /// still arriving.
    pub max_delay: Duration,
    /// How many batched writes the batcher keeps in flight at once.
    pub max_concurrent_writes: usize,
    /// Writes in flight, counting the one about to start, at which commits
    /// begin being combined.
    pub batch_write_threshold: usize,
}

impl WriteBatcherConfig {
    pub(crate) fn from_knobs() -> Self {
        let max_concurrent_writes = *COMMITTER_MAX_CONCURRENT_WRITE_BATCHES;
        Self {
            max_documents: *COMMITTER_MAX_WRITE_BATCH_DOCUMENTS,
            max_bytes: *COMMITTER_MAX_WRITE_BATCH_BYTES,
            max_delay: *COMMITTER_MAX_COMMIT_DELAY,
            max_concurrent_writes,
            batch_write_threshold: (*COMMITTER_BATCH_WRITE_THRESHOLD)
                .clamp(1, max_concurrent_writes),
        }
    }
}

/// Combines persistence writes from independent commits into batched
/// `Persistence::write` calls.
///
/// A commit's rows are never split across `Persistence::write` calls.
#[derive(Clone)]
pub struct WriteBatcher {
    tx: mpsc::UnboundedSender<WriteRequest>,
}

impl WriteBatcher {
    pub fn start<RT: Runtime>(
        runtime: RT,
        persistence: Arc<dyn Persistence>,
        config: WriteBatcherConfig,
    ) -> (Self, AbortOnDropHandle<anyhow::Result<()>>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let handle = AbortOnDropHandle::new(tokio_spawn(
            "committer_write_batcher",
            batch_writer_loop(runtime, persistence, config, rx),
        ));
        (Self { tx }, handle)
    }

    pub async fn write(
        &self,
        documents: Vec<DocumentLogEntry>,
        indexes: Vec<PersistenceIndexEntry>,
        size_bytes: u64,
    ) -> anyhow::Result<()> {
        let (ack, ack_rx) = oneshot::channel();
        self.tx
            .send(WriteRequest {
                documents,
                indexes,
                size_bytes,
                ack,
            })
            .map_err(|_| anyhow::anyhow!("committer write batcher has shut down"))?;
        ack_rx
            .await
            .context("committer write batcher has shut down")?
    }
}

struct Batch {
    documents: Vec<DocumentLogEntry>,
    indexes: Vec<PersistenceIndexEntry>,
    size_bytes: u64,
    acks: Vec<oneshot::Sender<anyhow::Result<()>>>,
}

impl Batch {
    fn new(request: WriteRequest) -> Self {
        let mut batch = Self {
            documents: Vec::new(),
            indexes: Vec::new(),
            size_bytes: 0,
            acks: Vec::new(),
        };
        batch.push(request);
        batch
    }

    fn push(&mut self, request: WriteRequest) {
        self.documents.extend(request.documents);
        self.indexes.extend(request.indexes);
        self.size_bytes += request.size_bytes;
        self.acks.push(request.ack);
    }

    fn is_full(&self, config: &WriteBatcherConfig) -> bool {
        self.documents.len() >= config.max_documents || self.size_bytes >= config.max_bytes
    }
}

async fn batch_writer_loop<RT: Runtime>(
    runtime: RT,
    persistence: Arc<dyn Persistence>,
    config: WriteBatcherConfig,
    mut rx: mpsc::UnboundedReceiver<WriteRequest>,
) -> anyhow::Result<()> {
    let mut write_batch_futures = JoinSet::new();

    loop {
        while let Some(result) = write_batch_futures.try_join_next() {
            result?;
        }
        if write_batch_futures.len() >= config.max_concurrent_writes
            && let Some(result) = write_batch_futures.join_next().await
        {
            result?;
        }
        let Some(first) = rx.recv().await else {
            // The committer has shut down.
            return Ok(());
        };
        let mut batch = Batch::new(first);
        // Combine commits only once enough writes are in flight.
        if write_batch_futures.len() + 1 >= config.batch_write_threshold {
            let mut deadline = runtime.wait(config.max_delay);
            while !batch.is_full(&config) {
                let next_request = rx.recv().fuse();
                pin_mut!(next_request);
                select_biased! {
                    request = next_request => match request {
                        Some(request) => batch.push(request),
                        None => break,
                    },
                    _ = deadline => break,
                }
            }
        }
        write_batch_futures.spawn(
            "committer_write_batch",
            write_batch(runtime.clone(), persistence.clone(), batch),
        );
    }
}

async fn write_batch<RT: Runtime>(runtime: RT, persistence: Arc<dyn Persistence>, batch: Batch) {
    let Batch {
        documents,
        indexes,
        size_bytes,
        acks,
    } = batch;
    metrics::log_write_batch(acks.len(), documents.len(), size_bytes);
    let mut backoff = Backoff::new(
        *INITIAL_PERSISTENCE_WRITES_BACKOFF,
        *MAX_PERSISTENCE_WRITES_BACKOFF,
    );
    let result = loop {
        let timer = metrics::commit_persistence_write_timer();
        match persistence
            .write(&documents, &indexes, ConflictStrategy::Error)
            .await
        {
            Ok(()) => {
                timer.finish();
                break Ok(());
            },
            Err(mut e) => {
                // MySQL rolls back the deadlock victim's entire transaction, so
                // retrying the batch with the same timestamps is safe.
                if !is_transient_db_error(&e) && !is_db_deadlock_error(&e) {
                    break Err(e);
                }
                let delay = backoff.fail(&mut runtime.rng());
                tracing::error!("Failed to write to persistence, retrying");
                report_error(&mut e).await;
                runtime.wait(delay).await;
            },
        }
    };
    match result {
        Ok(()) => {
            for ack in acks {
                let _ = ack.send(Ok(()));
            }
        },
        Err(e) => {
            for ack in acks {
                let _ = ack.send(Err(recapture_stacktrace_noreport(&e)));
            }
        },
    }
}
