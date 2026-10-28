use std::{
    collections::BTreeMap,
    time::Duration,
};

use pb::outcome::{
    SyscallStats as SyscallStatsProto,
    SyscallTrace as SyscallTraceProto,
};
use serde_json::{
    json,
    Value as JsonValue,
};
use value::heap_size::{
    HeapSize,
    WithHeapSize,
};

use crate::SyscallStats;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyscallTrace {
    pub async_syscalls: WithHeapSize<BTreeMap<String, SyscallStats>>,
}

impl HeapSize for SyscallTrace {
    fn heap_size(&self) -> usize {
        self.async_syscalls.heap_size()
    }
}

impl From<BTreeMap<String, SyscallStats>> for SyscallTrace {
    fn from(async_syscalls: BTreeMap<String, SyscallStats>) -> Self {
        Self {
            async_syscalls: async_syscalls.into(),
        }
    }
}

impl TryFrom<SyscallTrace> for SyscallTraceProto {
    type Error = anyhow::Error;

    fn try_from(SyscallTrace { async_syscalls }: SyscallTrace) -> anyhow::Result<Self> {
        Ok(Self {
            async_syscalls: async_syscalls
                .into_iter()
                .map(|(name, stats)| {
                    anyhow::Ok::<(String, SyscallStatsProto)>((name, stats.try_into()?))
                })
                .try_collect()?,
        })
    }
}

impl TryFrom<SyscallTraceProto> for SyscallTrace {
    type Error = anyhow::Error;

    fn try_from(SyscallTraceProto { async_syscalls }: SyscallTraceProto) -> anyhow::Result<Self> {
        Ok(Self {
            async_syscalls: async_syscalls
                .into_iter()
                .map(|(name, stats)| {
                    anyhow::Ok::<(String, SyscallStats)>((name, stats.try_into()?))
                })
                .try_collect()?,
        })
    }
}

impl SyscallTrace {
    pub fn new() -> Self {
        Self {
            async_syscalls: WithHeapSize::default(),
        }
    }

    pub fn log_async_syscall(&mut self, name: String, duration: Duration, is_success: bool) {
        self.async_syscalls.mutate_entry_or_default(name, |stats| {
            stats.invocations += 1;
            if !is_success {
                stats.errors += 1;
            }
            stats.total_duration += duration;
        });
    }

    pub fn merge(&mut self, other: &Self) {
        for (name, syscall) in &other.async_syscalls {
            self.async_syscalls
                .mutate_entry_or_default(name.clone(), |s| s.merge(syscall));
        }
    }
}

impl From<SyscallTrace> for JsonValue {
    fn from(value: SyscallTrace) -> Self {
        json!({
            "asyncSyscalls": value
                .async_syscalls
                .into_iter()
                .map(|(k, v)| (k, JsonValue::from(v)))
                .collect::<serde_json::Map<_, _>>(),
        })
    }
}
