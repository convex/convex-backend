use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use common::{
    errors::report_error,
    knobs::UDF_EXECUTOR_OCC_MAX_RETRIES,
    runtime::Runtime,
};
use database::Database;
use errors::ErrorMetadataAnyhowExt;
use sync_types::backoff::Backoff;

// Overloaded means indexes are not yet ready, but if we fail to load them in
// 10ish minutes, probably something has gone wrong.
const MAX_OVERLOADED_RETRIES: usize = 20;

#[async_trait]
pub(crate) trait RetriableWorker<RT: Runtime> {
    async fn work_loop(
        &mut self,
        name: &'static str,
        rt: &RT,
        db: &Database<RT>,
        backoff: &mut Backoff,
    ) -> anyhow::Result<()>;
}

pub(crate) async fn retry_loop_expect_occs_and_overloaded<RT: Runtime>(
    name: &'static str,
    runtime: RT,
    db: Database<RT>,
    initial_wait: Duration,
    initial_backoff: Duration,
    max_backoff: Duration,
    work: impl RetriableWorker<RT>,
) {
    tracing::info!("Starting {name}");
    runtime.wait(initial_wait).await;
    retry_failures_impl(
        name,
        runtime,
        db,
        MAX_OVERLOADED_RETRIES,
        initial_backoff,
        max_backoff,
        work,
    )
    .await
}

async fn retry_failures_impl<RT: Runtime>(
    name: &'static str,
    runtime: RT,
    db: Database<RT>,
    max_overloaded_errors: usize,
    initial_backoff: Duration,
    max_backoff: Duration,
    mut work: impl RetriableWorker<RT>,
) {
    let mut backoff = Backoff::new(initial_backoff, max_backoff);
    let mut occ_errors = 0;
    let mut overloaded_errors = 0;
    loop {
        if let Err(mut e) = work
            .work_loop(name, &runtime, &db, &mut backoff)
            .await
            .context(format!("{name} died"))
        {
            // Note: These aren't quite the same thing, but they're close enough for
            // the purposes of this retry loop.
            let is_overloaded = e.is_overloaded() || e.is_operational_internal_server_error();
            if e.is_occ() {
                occ_errors += 1;
                // Do not reset overloaded errors because we expect
                // overloaded to last a while during backend start and we
                // might get the occasional random OCC.
            } else if is_overloaded {
                overloaded_errors += 1;
                // Reset because we got a new failure type and we don't expect OCCs to last
                // any particular period of time.
                occ_errors = 0;
            }

            // Only report OCCs that happen repeatedly
            let expected_occ = e.is_occ() && occ_errors <= *UDF_EXECUTOR_OCC_MAX_RETRIES;
            // Overloaded means indexes are not yet ready, they should eventually become
            // ready but we can be pretty patient.
            let expected_overloaded = is_overloaded && overloaded_errors <= max_overloaded_errors;

            let expected_error = expected_occ || expected_overloaded;
            if !expected_error {
                report_error(&mut e).await;
            }
            let delay = backoff.fail(&mut runtime.rng());
            tracing::error!(
                "{name} died, num_failures: {}. Backing off for {}ms (max: {}ms), expected: {}: \
                 {e:#}",
                backoff.failures(),
                delay.as_millis(),
                max_backoff.as_millis(),
                expected_error,
            );
            runtime.wait(delay).await;
        } else {
            overloaded_errors = 0;
            occ_errors = 0;
        }
    }
}
