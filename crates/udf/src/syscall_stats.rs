use std::time::Duration;

use pb::outcome::SyscallStats as SyscallStatsProto;
use serde_json::{
    json,
    Value as JsonValue,
};
use value::heap_size::HeapSize;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SyscallStats {
    pub invocations: u32,
    pub errors: u32,
    pub total_duration: Duration,
}

impl HeapSize for SyscallStats {
    fn heap_size(&self) -> usize {
        0
    }
}

impl SyscallStats {
    pub fn merge(&mut self, other: &Self) {
        self.invocations += other.invocations;
        self.errors += other.errors;
        self.total_duration += other.total_duration;
    }
}

impl TryFrom<SyscallStats> for SyscallStatsProto {
    type Error = anyhow::Error;

    fn try_from(
        SyscallStats {
            invocations,
            errors,
            total_duration,
        }: SyscallStats,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            invocations: Some(invocations),
            errors: Some(errors),
            total_duration: Some(total_duration.try_into()?),
        })
    }
}

impl TryFrom<SyscallStatsProto> for SyscallStats {
    type Error = anyhow::Error;

    fn try_from(
        SyscallStatsProto {
            invocations,
            errors,
            total_duration,
        }: SyscallStatsProto,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            invocations: invocations.ok_or_else(|| anyhow::anyhow!("Missing invocations"))?,
            errors: errors
                .ok_or_else(|| anyhow::anyhow!("Missing errors in SyscallStats deserialization"))?,
            total_duration: total_duration
                .ok_or_else(|| anyhow::anyhow!("Missing total duration"))?
                .try_into()?,
        })
    }
}

impl From<SyscallStats> for JsonValue {
    fn from(value: SyscallStats) -> Self {
        json!({
            "invocations": value.invocations,
            "errors": value.errors,
            "totalDuration": value.total_duration.as_secs_f64(),
        })
    }
}
