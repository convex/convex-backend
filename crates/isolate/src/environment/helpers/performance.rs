use std::time::Duration;

use anyhow::Context;
use common::runtime::{
    Runtime,
    UnixTimestamp,
};
use errors::ErrorMetadata;
use tokio::time::Instant;

/// Returned when a module lacks an import-phase timestamp (either a system
/// module or a super old module without a UdfConfig)
pub fn performance_unsupported() -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "PerformanceUnsupported",
        "The Performance API is not supported in this module",
    )
}

/// Used to implement the JavaScript Performance API.
pub enum PerformanceApi {
    Importing(ImportingPerformanceApi),
    Executing(ExecutingPerformanceApi),
}

impl PerformanceApi {
    pub fn new(time_origin: UnixTimestamp) -> Self {
        PerformanceApi::Importing(ImportingPerformanceApi { time_origin })
    }

    /// Transition from the importing state to the executing state,
    pub fn begin_execution<RT: Runtime>(
        &mut self,
        rt: &RT,
        execution_timestamp: UnixTimestamp,
    ) -> anyhow::Result<()> {
        let PerformanceApi::Importing(importing) = self else {
            anyhow::bail!("performance API has already begun executing");
        };
        let time_origin = importing.time_origin;
        let origin_offset = execution_timestamp
            .checked_sub(time_origin)
            .context("execution timestamp should never precede the import-phase time origin")?;
        *self = PerformanceApi::Executing(ExecutingPerformanceApi {
            time_origin,
            execution_origin_offset: origin_offset,
            monotonic_start: rt.monotonic_now(),
        });
        Ok(())
    }

    pub fn time_origin(&self) -> UnixTimestamp {
        match self {
            PerformanceApi::Importing(api) => api.time_origin(),
            PerformanceApi::Executing(api) => api.time_origin(),
        }
    }
}

/// Performance API state during the import phase.
pub struct ImportingPerformanceApi {
    /// The base system time (used to implement [`performance.timeOrigin`](https://developer.mozilla.org/en-US/docs/Web/API/Performance/timeOrigin)).
    time_origin: UnixTimestamp,
}

impl ImportingPerformanceApi {
    pub fn time_origin(&self) -> UnixTimestamp {
        self.time_origin
    }

    pub fn now(&self) -> Duration {
        Duration::ZERO
    }
}

/// Performance API state during the execution phase.
pub struct ExecutingPerformanceApi {
    time_origin: UnixTimestamp,
    /// The value of `now()` at the start of execution: `execution_timestamp -
    /// time_origin`
    execution_origin_offset: Duration,
    monotonic_start: Instant,
}

impl ExecutingPerformanceApi {
    pub fn time_origin(&self) -> UnixTimestamp {
        self.time_origin
    }

    /// Fixed `now()` used by queries for determinism.
    pub fn now_fixed(&self) -> Duration {
        self.execution_origin_offset
    }

    /// Incrementing `now()` used by mutations and actions.
    pub fn now_incrementing<RT: Runtime>(&self, rt: &RT) -> Duration {
        self.execution_origin_offset + rt.monotonic_now().duration_since(self.monotonic_start)
    }
}
