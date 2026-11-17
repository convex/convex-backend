//! Types specific to *managed* streaming export, where Convex drives the sync
//! itself and writes pages to a destination the customer points OLAP tools at,
//! rather than the customer pulling pages from the data sync API.

use usage_tracking::FunctionUsageStats;

use crate::SyncStatus;

/// A destination a managed streaming export writes its pages to.
pub enum SyncDestination {
    /// A bucket the customer owns, with credentials they supply.
    ByoAwsBucket {
        bucket: String,
        region: String,
        /// Key prefix within the bucket, without a leading or trailing slash.
        prefix: Option<String>,
        /// Set for S3-compatible services (MinIO, Cloudflare R2); `None` uses
        /// the AWS endpoint for `region`.
        endpoint_url: Option<String>,
        credentials: ByoAwsCredentials,
    },
    /// A Convex-owned bucket, whose location the writer derives itself.
    Managed,
}

/// A role in the customer's account that Porter assumes, with the export's
/// ExternalId, to write to their bucket.
pub struct ByoAwsCredentials {
    pub role_arn: String,
    pub external_id: String,
}

/// The on-disk format written to the destination.
pub enum SyncFormat {
    Iceberg,
}

/// One page of a managed streaming export, as written to the destination.
pub struct SyncPage {
    /// Cursor to persist and resume from next time.
    pub cursor: String,
    pub status: SyncStatus,
    pub num_documents: u64,
    pub usage: FunctionUsageStats,
}
