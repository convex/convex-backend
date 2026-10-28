//! Tunable limits and parameters for searchlight.
//!
//! Every knob here should have a comment explaining what it's for and the
//! upper/lower bounds if applicable so an oncall engineer can adjust these
//! safely for searchlight if needed.
//!
//! When running locally, these knobs can all be overridden with an environment
//! variable.

use std::{
    num::NonZeroU64,
    sync::LazyLock,
};

use cmd_util::env::env_config;

/// The maximum number of compactions we can run concurrently on one
/// searchlight instance. Each compaction takes 4 cores, so this should
/// always be less than the number of cores on the machine / 4 to reserve CPU
/// for searches.
///
/// The queue size for compactions is set to QUEUE_SIZE_MULTIPLIER * this
/// number, so this knob also determines the maximum queue length.
pub static MAX_CONCURRENT_SEGMENT_COMPACTIONS: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_CONCURRENT_SEGMENT_COMPACTIONS", 3));

/// The maximum number of segments we can fetch in parallel across all
/// searches and compactions.
///
/// NOTE: You must consider the cache timeout, the maximum segment size and
/// the serial disk write speed of searchlight before changing this number.
/// If you set this number too high, we will not be able to download
/// segments fast enough and will have congestion collapse.
///
/// A rough way to calculate the maximum value for this knob is to determine the
/// amount of time it takes to download N segments at their maximum size:
///
/// max segment size (~3.2 GiB) * concurrent fetches / max write throughput
/// speed (~600 MiB/s)
///
/// Then compare that to the cache timeout seconds (120s) and ensure that the
/// time to fetch segments is well under the timeout. If we exceed the timeout,
/// then we'll have congestion collapse because we will fail to make progress
/// downloading segments.
///
/// The queue size for fetches is set to QUEUE_SIZE_MULTIPLIER * this number, so
/// this knob also determines the maximum queue length.
pub static MAX_CONCURRENT_SEGMENT_FETCHES: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_CONCURRENT_SEGMENT_FETCHES", 8));

/// The maximum number of concurrent vector searches we'll run at once,
/// based on a very rough estimate of memory used per search.
///
/// The queue size for searches is set to QUEUE_SIZE_MULTIPLIER * this number,
/// so this knob also determines the maximum queue length.
pub static MAX_CONCURRENT_VECTOR_SEARCHES: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_CONCURRENT_VECTOR_SEARCHES", 20));

/// A generic multiplier applied to concurrencly limits for most pools in
/// searchlight to figure out the queue size.
pub static QUEUE_SIZE_MULTIPLIER: LazyLock<usize> =
    LazyLock::new(|| env_config("QUEUE_SIZE_MULTIPLIER", 20));

/// Fraction (0.0 - 1.0) of the total archive cache a single index or
/// deployment must exceed before we emit the archive usage gauges. Set lower to
/// see more metrics and higher to suppress noisy low-volume entries.
pub static ARCHIVE_METRIC_EMIT_THRESHOLD_FRACTION: LazyLock<f64> =
    LazyLock::new(|| env_config("ARCHIVE_METRIC_EMIT_THRESHOLD_FRACTION", 0.05));

/// The maximum number of qdrant Segments (each backed by a RocksDB
/// instance) that we'll keep in memory in the LRU at once.
/// See https://www.notion.so/convex-dev/Vector-Search-Scaling-Issues-0e7c2dde6ea241af828c89a77c593f64?pvs=4#2b1852e44b734362a1b05b6dec62b744
/// for where this default value comes from. The actual value here may be some
/// multiple of the value in the doc depending on the amount of memory in the
/// instance type we're currently using for searchlight.
pub static MAX_VECTOR_LRU_SIZE: LazyLock<u64> =
    LazyLock::new(|| env_config("MAX_VECTOR_LRU_ENTRIES", 120));

/// The maximum number of segments we're allowed to prefetch at one time in a
/// given searchlight node.
pub static MAX_CONCURRENT_VECTOR_SEGMENT_PREFETCHES: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_CONCURRENT_VECTOR_PREFETCHES", 2));
/// The maximum number of text segments (each backed by a single-segment tantiy
/// index ) that we'll keep in memory in the LRU at once.
pub static MAX_TEXT_LRU_ENTRIES: LazyLock<u64> =
    LazyLock::new(|| env_config("MAX_TEXT_LRU_ENTRIES", 120));

/// The maximum number of concurrent text searches we'll run at once,
/// based on a very rough estimate of memory used per search.
///
/// The queue size for searches is set to QUEUE_SIZE_MULTIPLIER * this number,
/// so this knob also determines the maximum queue length.
pub static MAX_CONCURRENT_TEXT_SEARCHES: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_CONCURRENT_TEXT_SEARCHES", 20));

/// The size in bytes of the archive disk cache the in-process searcher keeps
/// under its local storage path. It bounds the bytes of unpacked text and
/// vector segments retained on disk; a working set larger than this re-fetches
/// and re-extracts segments as they cycle through the cache, so raise it to
/// trade disk for fetch traffic.
pub static MAX_ARCHIVE_CACHE_SIZE_BYTES: LazyLock<NonZeroU64> = LazyLock::new(|| {
    env_config(
        "MAX_ARCHIVE_CACHE_SIZE_BYTES",
        NonZeroU64::new(bytesize::mib(500u64)).unwrap(),
    )
});
