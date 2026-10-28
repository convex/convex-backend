use std::collections::VecDeque;

use common::{
    knobs::{
        MAX_BYTES_WRITTEN_PER_SECOND,
        MAX_ROWS_WRITTEN_PER_SECOND,
        PROPOSED_MAX_BYTES_WRITTEN_PER_SECOND,
        WRITE_THROUGHPUT_WINDOW,
    },
    types::Timestamp,
};

use crate::metrics::{
    log_write_throughput,
    log_write_throughput_limit_exceeded,
    log_write_throughput_limit_would_be_exceeded,
    log_write_throughput_rows,
    log_write_throughput_rows_limit_exceeded,
};

/// What a commit wrote to persistence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WriteVolume {
    /// Serialized size of the document and index rows.
    pub bytes: u64,
    /// Number of document and index rows.
    pub rows: u64,
}

/// A write throughput limit that a deployment can exceed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteThroughputLimit {
    Bytes,
    Rows,
}

/// Tracks write throughput and enforces rate limits on database writes.
///
/// This limiter maintains a sliding window of write events and their volume,
/// allowing efficient O(1) throughput checks by keeping a running total.
pub struct WriteThroughputLimiter {
    /// Volume written in transactions that committed at each timestamp.
    writes: VecDeque<(Timestamp, WriteVolume)>,
    /// Running total of `writes` for O(1) throughput checks.
    total_in_window: WriteVolume,
    /// Maximum volume that can be written in the window. `u64::MAX` rows means
    /// unlimited.
    max_in_window: WriteVolume,
    /// Proposed maximum bytes that can be written in the window. Exceeding it
    /// only logs a metric.
    proposed_max_bytes_in_window: u64,
}

impl WriteThroughputLimiter {
    pub fn new() -> Self {
        let window_ms = WRITE_THROUGHPUT_WINDOW.as_millis() as u64;
        Self {
            writes: VecDeque::new(),
            total_in_window: WriteVolume::default(),
            max_in_window: WriteVolume {
                bytes: (*MAX_BYTES_WRITTEN_PER_SECOND).saturating_mul(window_ms) / 1000,
                rows: MAX_ROWS_WRITTEN_PER_SECOND.map_or(u64::MAX, |rows_per_second| {
                    rows_per_second.saturating_mul(window_ms) / 1000
                }),
            },
            proposed_max_bytes_in_window: (*PROPOSED_MAX_BYTES_WRITTEN_PER_SECOND)
                .saturating_mul(window_ms)
                / 1000,
        }
    }

    pub fn record_write(&mut self, ts: Timestamp, volume: WriteVolume) {
        // Clean up old write events outside the write throughput window
        while let Some((event_ts, _)) = self.writes.front() {
            if (ts - *event_ts) > *WRITE_THROUGHPUT_WINDOW {
                if let Some((_, WriteVolume { bytes, rows })) = self.writes.pop_front() {
                    self.total_in_window.bytes = self.total_in_window.bytes.saturating_sub(bytes);
                    self.total_in_window.rows = self.total_in_window.rows.saturating_sub(rows);
                }
            } else {
                break;
            }
        }

        // Track new write event for throughput limiting
        self.writes.push_back((ts, volume));
        self.total_in_window.bytes += volume.bytes;
        self.total_in_window.rows += volume.rows;
    }

    /// The running total as of `current_ts`. Old entries are only evicted on
    /// writes, so once the newest write has left the window, nothing written
    /// is in it. While writes are still arriving, this may include older
    /// writes that have since left the window.
    fn total_at(&self, current_ts: Timestamp) -> WriteVolume {
        let newest_write_expired = self.writes.back().is_some_and(|(event_ts, _)| {
            current_ts >= *event_ts && (current_ts - *event_ts) > *WRITE_THROUGHPUT_WINDOW
        });
        if newest_write_expired {
            WriteVolume::default()
        } else {
            self.total_in_window
        }
    }

    /// Returns the limit exceeded by writes in the window ending at
    /// `current_ts`, if any.
    pub fn exceeded_limit(&self, current_ts: Timestamp) -> Option<WriteThroughputLimit> {
        let total_in_window = self.total_at(current_ts);
        log_write_throughput(total_in_window.bytes);
        log_write_throughput_rows(total_in_window.rows);
        let in_window = if total_in_window.bytes > self.max_in_window.bytes
            || total_in_window.rows > self.max_in_window.rows
        {
            // N.B. Check the actual volume written in the window relative to
            // the current ts passed in, because old entries are only evicted
            // on writes.
            self.writes
                .iter()
                .filter(|(event_ts, _)| {
                    current_ts < *event_ts || (current_ts - *event_ts) <= *WRITE_THROUGHPUT_WINDOW
                })
                .fold(WriteVolume::default(), |total, (_, volume)| WriteVolume {
                    bytes: total.bytes + volume.bytes,
                    rows: total.rows + volume.rows,
                })
        } else {
            total_in_window
        };
        if in_window.bytes > self.max_in_window.bytes {
            log_write_throughput_limit_exceeded();
            return Some(WriteThroughputLimit::Bytes);
        }
        if in_window.rows > self.max_in_window.rows {
            log_write_throughput_rows_limit_exceeded();
            return Some(WriteThroughputLimit::Rows);
        }
        if in_window.bytes > self.proposed_max_bytes_in_window {
            log_write_throughput_limit_would_be_exceeded();
        }
        None
    }
}
