#![feature(coroutines)]
#![feature(impl_trait_in_assoc_type)]
#![feature(try_blocks)]
#![feature(try_blocks_heterogeneous)]
#![feature(try_find)]

pub mod fast_forward;
pub mod index_meta;
mod metrics;
pub mod retriable_worker;
pub mod search_compactor;
pub mod search_flusher;
pub mod search_worker;
pub mod text_index_worker;
pub mod vector_index_worker;
pub mod writer;

use std::{
    num::NonZeroU32,
    time::Duration,
};

use common::{
    knobs::{
        SEARCH_WORKER_PAGES_PER_SECOND,
        SEARCH_WORKER_PASSIVE_PAGES_PER_SECOND,
    },
    runtime::Runtime,
    types::RepeatableTimestamp,
};
use database::TableScanCursor;
pub use fast_forward::FastForwardIndexWorker;
use rand::Rng;
pub use search_worker::SearchIndexWorkers;
pub use vector_index_worker::flusher::VectorIndexFlusher;

pub const MAX_BACKOFF: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Copy)]
pub(crate) enum BuildReason {
    Backfilling,
    TooOld,
    TooLarge,
    VersionMismatch,
}

/// There are two types of search index flushers: live flushers and backfill
/// flushers. Live flushers are responsible for flushing the in-memory search
/// index contents when they get too large or old. Backfill flushers are
/// responsible for backfilling newly added indexes or indexes on the wrong
/// version. We separate the flushers so that building backfill segments (which
/// can take a long time) does not block the in-memory index flushes. In-memory
/// flushes need to happen quickly or else writes fail when the in-memory
/// index gets too large.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FlusherType {
    LiveFlush,
    Backfill,
}
impl From<BuildReason> for FlusherType {
    fn from(reason: BuildReason) -> Self {
        match reason {
            BuildReason::Backfilling | BuildReason::VersionMismatch => FlusherType::Backfill,
            BuildReason::TooOld | BuildReason::TooLarge => FlusherType::LiveFlush,
        }
    }
}

impl BuildReason {
    pub fn read_max_pages_per_second(&self) -> NonZeroU32 {
        match self {
            // In non-blocking update pathways, use a lower limit to avoid overloading the database
            // with rebuilds.
            BuildReason::TooOld | BuildReason::VersionMismatch => {
                *SEARCH_WORKER_PASSIVE_PAGES_PER_SECOND
            },
            // If the developer is writing data, then use a higher limit to try to avoid causing
            // transient 503s for the developer's writes.
            BuildReason::Backfilling | BuildReason::TooLarge => *SEARCH_WORKER_PAGES_PER_SECOND,
        }
    }
}

// Timeout from 1/2 the target duration to 1.5 the target duration.
pub async fn timeout_with_jitter<RT: Runtime>(rt: &RT, duration: Duration) {
    let half_timer = duration / 2;
    let sleep = half_timer + duration.mul_f32(rt.rng().random::<f32>());
    rt.wait(sleep).await;
}

#[derive(Debug)]
pub struct MultiSegmentBackfillResult {
    pub new_cursor: TableScanCursor,
    pub new_ts: RepeatableTimestamp,
}
