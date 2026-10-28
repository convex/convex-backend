//! The sink the [`UsageLimitWorker`](super::UsageLimitWorker) hands
//! newly-exceeded limits to.

use std::time::SystemTime;

use async_trait::async_trait;
use model::usage_limits::types::{
    UsageLimitMetric,
    UsageLimitType,
    UsageLimitWindow,
};

/// A single usage limit that was exceeded
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageLimitNotification {
    pub metric: UsageLimitMetric,
    pub window: UsageLimitWindow,
    /// Whether crossing this limit only warns or disables the deployment.
    pub limit_type: UsageLimitType,
    /// The configured limit in user-facing units (calls, GB, or GB-hours).
    pub limit: u64,
    /// When the current window resets (its exclusive end).
    pub window_reset: SystemTime,
}

/// Sink for "usage limit exceeded" notifications. Called once per limit per
/// window, after the audit event commits.
#[async_trait]
pub trait UsageLimitNotifier: Send + Sync {
    fn notify_exceeded(&self, notifications: Vec<UsageLimitNotification>);
    async fn shutdown(&self);
}

/// No-op notifier used by the open-source build and tests.
pub struct NoopUsageLimitNotifier;

#[async_trait]
impl UsageLimitNotifier for NoopUsageLimitNotifier {
    fn notify_exceeded(&self, _notifications: Vec<UsageLimitNotification>) {}

    async fn shutdown(&self) {}
}
