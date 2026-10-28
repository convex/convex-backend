use std::{
    collections::BTreeSet,
    sync::{
        atomic::{
            AtomicU64,
            Ordering,
        },
        Arc,
    },
};

use common::log_streaming::{
    LogEvent,
    LogEventFormatVersion,
    LogTopic,
    StructuredLogEvent,
};

/// Shared counter for total log stream egress bytes across all sinks.
/// Sinks atomically increment this; a periodic task drains it and emits
/// a `LogStreamEgress` event.
pub type EgressCounter = Arc<AtomicU64>;

/// Decides which log events a sink receives.
#[derive(Debug, Clone)]
pub enum SinkFilter {
    /// Exception sinks (Sentry, PostHog Error Tracking): receive only
    /// `exception` events and nothing else.
    OnlyExceptions,
    /// `subscribed_topics == None` means the sink is subscribed to all
    /// non-exception topics. When `Some`, only the listed topics are delivered.
    General {
        subscribed_topics: Option<BTreeSet<LogTopic>>,
    },
}

impl SinkFilter {
    /// Build a `General` filter for a sink, applying topic subscriptions only
    /// for V2 log streams. V1 (legacy) streams ignore `subscribed_topics` and
    /// fall back to receiving all non-exception topics.
    pub fn for_version(
        version: LogEventFormatVersion,
        subscribed_topics: Option<BTreeSet<LogTopic>>,
    ) -> Self {
        let subscribed_topics = match version {
            LogEventFormatVersion::V2 => subscribed_topics,
            LogEventFormatVersion::V1 => None,
        };
        SinkFilter::General { subscribed_topics }
    }

    pub fn allows(&self, event: &LogEvent) -> bool {
        let topic = event.event.topic();
        match self {
            SinkFilter::OnlyExceptions => topic == LogTopic::Exception,
            SinkFilter::General { subscribed_topics } => {
                // `verification` is the connection test and always passes;
                if topic == LogTopic::Verification {
                    return true;
                }
                // `exception` is only ever routed to exception sinks.
                if topic == LogTopic::Exception {
                    return false;
                }
                match subscribed_topics {
                    // Subscribe-all. `custom_audit` is opt-in (it's gated behind
                    // an entitlement), so it's excluded from the default even
                    // though every other topic, including future ones, is
                    // included.
                    None => topic != LogTopic::CustomAudit,
                    Some(topics) => topics.contains(&topic),
                }
            },
        }
    }
}

pub fn build_event_batches(
    events: Vec<Arc<LogEvent>>,
    batch_size: usize,
    filter: &SinkFilter,
) -> Vec<Vec<Arc<LogEvent>>> {
    events
        .into_iter()
        .filter(|ev| filter.allows(ev))
        .collect::<Vec<_>>()
        .chunks(batch_size)
        .map(|arr: &[Arc<LogEvent>]| arr.to_vec())
        .collect()
}

/// Greedily pack `items` into batches of at most `max_entries` items whose
/// serialized JSON array payload stays within `max_bytes`. `size` returns an
/// item's serialized size; the byte accounting adds the array overhead (two
/// brackets per batch, a comma separator between elements) so `max_bytes`
/// bounds the assembled payload. An item that alone exceeds `max_bytes` is
/// placed in its own batch, since a single item can't be split.
pub fn build_sized_batches<T>(
    items: &[T],
    size: impl Fn(&T) -> usize,
    max_entries: usize,
    max_bytes: usize,
) -> Vec<&[T]> {
    let mut batches: Vec<&[T]> = vec![];
    let mut batch = 0..0;
    // Start each batch at two bytes for the enclosing brackets.
    let mut batch_bytes = 2;
    for (i, item) in items.iter().enumerate() {
        let item_bytes = size(item);
        if !batch.is_empty()
            && (batch.len() >= max_entries || batch_bytes + 1 + item_bytes > max_bytes)
        {
            batches.push(&items[batch.clone()]);
            batch.start = batch.end;
            batch_bytes = 2;
        }
        if !batch.is_empty() {
            batch_bytes += 1;
        }
        batch_bytes += item_bytes;
        batch.end = i + 1;
    }
    if !batch.is_empty() {
        batches.push(&items[batch]);
    }
    batches
}

pub fn batch_has_non_egress_events(events: &[Arc<LogEvent>]) -> bool {
    events
        .iter()
        .any(|ev| !matches!(ev.event, StructuredLogEvent::LogStreamEgress { .. }))
}

/// Accumulates log sink network egress bytes. Called by sinks after each
/// successful HTTP send. The accumulated bytes are periodically drained by
/// `LogManager::egress_emission_worker` which emits both a `LogStreamEgress`
/// event and a billing `track_call`.
///
/// Note: This should only be called for non-verification requests.
pub fn track_log_sink_bandwidth(
    num_bytes_egress: u64,
    egress_counter: &EgressCounter,
    metrics_fn: impl FnOnce(u64),
) {
    metrics_fn(num_bytes_egress);
    egress_counter.fetch_add(num_bytes_egress, Ordering::Relaxed);
}
