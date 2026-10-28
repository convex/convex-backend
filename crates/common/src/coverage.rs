//! Test-only coverage counters for randomized tests.
//!
//! Declare counters with [`counters!`] in a module gated on
//! `any(test, feature = "testing")`, and instrument branches with [`cover!`].
//! Randomized tests check that every counter is nonzero to catch generated
//! workloads that miss required paths.
//!
//! Counters accumulate across tests in the same process, so a coverage check
//! runs in its own process when other tests reach the same paths.

/// The `cfg` is evaluated in the calling crate; increments are compiled only
/// in its `test` and `testing` builds.
#[macro_export]
macro_rules! cover {
    ($counter:path) => {
    };
}
