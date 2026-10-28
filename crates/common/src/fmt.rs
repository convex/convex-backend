use std::time::Duration;

/// Units ordered from largest to smallest, mixing binary and decimal.
/// We try each and pick the one that divides most cleanly.
const BYTE_UNITS: &[(u64, &str)] = &[
    (1 << 30, "GiB"),
    (1_000_000_000, "GB"),
    (1 << 20, "MiB"),
    (1_000_000, "MB"),
    (1 << 10, "KiB"),
    (1_000, "KB"),
];

/// Format a byte count into a human-friendly string.
///
/// Picks the unit (binary or decimal) that divides most cleanly.
/// Shows one decimal place only when it divides exactly (e.g. "4.1 MiB").
/// Falls back to raw bytes when no unit divides cleanly.
///
/// ```
/// use common::fmt::format_bytes;
/// assert_eq!(format_bytes(8_388_608), "8 MiB");
/// assert_eq!(format_bytes(1534), "1534 bytes");
/// assert_eq!(format_bytes(1024), "1 KiB");
/// assert_eq!(format_bytes(1000), "1 KB");
/// assert_eq!(format_bytes(0), "0 bytes");
/// assert_eq!(format_bytes(4_718_592), "4.5 MiB");
/// ```
pub fn format_bytes(n: u64) -> String {
    if n == 0 {
        return "0 bytes".to_string();
    }
    for &(unit_size, unit_name) in BYTE_UNITS {
        if n < unit_size {
            continue;
        }
        if n % unit_size == 0 {
            return format!("{} {unit_name}", n / unit_size);
        }
        if (n * 10) % unit_size == 0 {
            let whole = n / unit_size;
            let frac = (n * 10 / unit_size) % 10;
            return format!("{whole}.{frac} {unit_name}");
        }
    }
    format!("{n} bytes")
}

/// Format a duration into a human-friendly string like "1 second" or "500ms".
///
/// ```
/// use std::time::Duration;
/// use common::fmt::format_duration;
/// assert_eq!(format_duration(Duration::from_millis(1000)), "1 second");
/// assert_eq!(format_duration(Duration::from_millis(2000)), "2 seconds");
/// assert_eq!(format_duration(Duration::from_millis(500)), "500ms");
/// assert_eq!(format_duration(Duration::from_millis(1500)), "1.5 seconds");
/// assert_eq!(format_duration(Duration::from_secs(60)), "60 seconds");
/// ```
pub fn format_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms == 0 {
        return "0ms".to_string();
    }
    if ms >= 1000 {
        if ms % 1000 == 0 {
            let secs = ms / 1000;
            if secs == 1 {
                return "1 second".to_string();
            }
            return format!("{secs} seconds");
        }
        if (ms * 10) % 1000 == 0 {
            let whole = ms / 1000;
            let frac = (ms * 10 / 1000) % 10;
            return format!("{whole}.{frac} seconds");
        }
    }
    format!("{ms}ms")
}

/// Given how long the last window spent reading from the source, writing to the
/// destination, and waiting on a rate limiter (throttle), describe whether a
/// copy phase is read-, write-, or throttle-bound.
///
/// The verdict names the largest bucket, or "balanced" when the top two are
/// within ~10% (of the total) of each other. The percentage is taken over
/// `read + write + throttle`, so unaccounted CPU or idle time never distorts
/// it. The throttle segment is omitted when there was no throttling (e.g. the
/// document-copy phase has no rate limiter).
///
/// ```
/// use std::time::Duration;
/// use common::fmt::format_read_write_balance;
/// assert_eq!(
///     format_read_write_balance(Duration::from_secs(11), Duration::from_secs(49), Duration::ZERO),
///     "write-bound 82% (read 11 seconds, write 49 seconds)",
/// );
/// assert_eq!(
///     format_read_write_balance(
///         Duration::from_secs(10),
///         Duration::from_secs(30),
///         Duration::from_secs(60),
///     ),
///     "throttle-bound 60% (read 10 seconds, write 30 seconds, throttle 60 seconds)",
/// );
/// assert_eq!(
///     format_read_write_balance(Duration::ZERO, Duration::ZERO, Duration::ZERO),
///     "no read/write activity",
/// );
/// ```
pub fn format_read_write_balance(read: Duration, write: Duration, throttle: Duration) -> String {
    let read_secs = read.as_secs_f64();
    let write_secs = write.as_secs_f64();
    let throttle_secs = throttle.as_secs_f64();
    let total = read_secs + write_secs + throttle_secs;
    if total <= 0.0 {
        return "no read/write activity".to_string();
    }
    let mut buckets = [
        ("read-bound", read_secs),
        ("write-bound", write_secs),
        ("throttle-bound", throttle_secs),
    ];
    buckets.sort_by(|a, b| b.1.total_cmp(&a.1));
    let verdict = if (buckets[0].1 - buckets[1].1) / total <= 0.10 {
        "balanced"
    } else {
        buckets[0].0
    };
    let pct = buckets[0].1 / total * 100.0;
    let segments = if throttle_secs > 0.0 {
        format!(
            "read {}, write {}, throttle {}",
            format_duration(read),
            format_duration(write),
            format_duration(throttle),
        )
    } else {
        format!(
            "read {}, write {}",
            format_duration(read),
            format_duration(write),
        )
    };
    format!("{verdict} {pct:.0}% ({segments})")
}
