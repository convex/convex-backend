use common::{
    errors::FrameData,
    log_lines::LogLevel,
};

use super::{
    metrics,
    V8OpProvider,
};
use crate::error::source_mapped_stack;

#[convex_macro::v8_op]
pub fn op_console_message<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    level: String,
    messages: Vec<String>,
) -> anyhow::Result<()> {
    for message in messages.iter() {
        tracing::trace!("console message: {:?}", message);
        metrics::log_log_line(message);
    }
    provider.trace(level.parse()?, messages)?;
    Ok(())
}

#[convex_macro::v8_op]
pub fn op_console_trace<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    mut messages: Vec<String>,
    frame_data: Vec<FrameData>,
) -> anyhow::Result<()> {
    // A leading newline puts the frames under the message, as a browser does.
    let stack = source_mapped_stack(frame_data, |s| provider.lookup_source_map(s));
    messages.push(format!("\n{stack}"));
    for message in messages.iter() {
        tracing::trace!("console trace: {:?}", message);
    }
    provider.trace(LogLevel::Log, messages)?;
    Ok(())
}

#[convex_macro::v8_op]
pub fn op_console_time_start<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    label: String,
) -> anyhow::Result<()> {
    if provider.console_timers()?.contains_key(&label) {
        provider.trace(
            LogLevel::Warn,
            vec![format!("Timer '{label}' already exists")],
        )?;
    } else {
        let timestamp = provider.unix_timestamp_non_deterministic()?;
        provider.console_timers()?.insert(label, timestamp);
    };
    Ok(())
}

#[convex_macro::v8_op]
pub fn op_console_time_log<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    label: String,
    extra_messages: Vec<String>,
) -> anyhow::Result<()> {
    let now = provider
        .unix_timestamp_non_deterministic()?
        .as_ms_since_epoch()?;
    match provider.console_timers()?.get(&label) {
        None => {
            provider.trace(
                LogLevel::Warn,
                vec![format!("Timer '{label}' does not exist")],
            )?;
        },
        Some(time) => {
            let duration = now - time.as_ms_since_epoch()?;
            let mut messages = vec![format!("{label}: {duration}ms")];
            messages.extend(extra_messages.into_iter());
            provider.trace(LogLevel::Info, messages)?;
        },
    };
    Ok(())
}

#[convex_macro::v8_op]
pub fn op_console_time_end<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    label: String,
) -> anyhow::Result<()> {
    match provider.console_timers()?.remove(&label) {
        None => {
            provider.trace(
                LogLevel::Warn,
                vec![format!("Timer '{label}' does not exist")],
            )?;
        },
        Some(time) => {
            let now = provider
                .unix_timestamp_non_deterministic()?
                .as_ms_since_epoch()?;
            let duration = now - time.as_ms_since_epoch()?;
            provider.trace(LogLevel::Info, vec![format!("{label}: {duration}ms")])?;
        },
    };
    Ok(())
}
