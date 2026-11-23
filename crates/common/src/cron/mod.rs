use anyhow::Context;
use chrono::{
    TimeZone,
    Utc,
};
use saffron::Cron;
use sync_types::Timestamp;

/// Returns the first matching time strictly after `ts`.
pub fn next_occurrence_after(cron: &Cron, ts: Timestamp) -> anyhow::Result<Timestamp> {
    let occurrence_utc = cron
        .next_after(Utc.timestamp_nanos(ts.into()))
        .context("Could not compute next timestamp for cron")?;
    occurrence_utc
        .timestamp_nanos_opt()
        .context("Unable to get nanos from UTC")?
        .try_into()
}
