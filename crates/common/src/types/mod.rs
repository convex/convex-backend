//! Common types representing database identifiers.
pub use sync_types::{
    SessionId,
    SessionRequestSeqNumber,
    Timestamp,
};
use tuple_struct::{
    tuple_struct_string,
    tuple_struct_u64,
};
pub use value::{
    FieldName,
    FieldType,
    TableName,
};

mod actions;
mod admin_key;
mod ai_gateway;
mod attribution;
mod backend_info;
mod backend_state;
mod deployments;
mod environment_variables;
mod file_storage;
mod functions;
mod index;
mod maybe_value;
mod object_key;
mod region;
mod search_index_metric_labels;
mod snapshot_export;
pub mod streaming_export;
mod table;
mod timestamp;

pub use actions::{
    ActionCallbackToken,
    HttpActionRoute,
    NodeDependency,
    RoutableMethod,
    SerializedHttpActionRoute,
};
pub use admin_key::{
    format_admin_key,
    remove_type_prefix_from_admin_key,
    remove_type_prefix_from_deployment_name,
    split_admin_key,
    AdminKey,
    AdminKeyParts,
    SystemKey,
};
pub use ai_gateway::AI_GATEWAY_URL;
pub use attribution::{
    AttributedCaller,
    AttributionClaims,
};
pub use backend_info::{
    BackendInfo,
    DEFAULT_PROVISION_CONCURRENCY,
};
pub use backend_state::{
    BackendState,
    OldBackendState,
    SystemStopState,
    UsageLimitStopState,
    UserStopState,
};
pub use deployments::{
    DeploymentClass,
    DeploymentMetadata,
    DeploymentType,
};
pub use environment_variables::{
    env_var_limit_met,
    env_var_name_forbidden,
    env_var_name_not_unique,
    env_var_total_size,
    env_var_total_size_limit_met,
    EnvVarName,
    EnvVarValue,
    EnvironmentVariable,
};
pub use file_storage::StorageUuid;
pub use functions::{
    AllowedVisibility,
    FunctionCaller,
    ModuleEnvironment,
    QueryInvocation,
    UdfIdentifier,
    UdfRuntime,
    UdfType,
    UdfTypeJson,
};
pub use index::{
    DatabaseIndexUpdate,
    DatabaseIndexValue,
    GenericIndexName,
    IndexDescriptor,
    IndexDiff,
    IndexId,
    IndexName,
    IndexRef,
    IndexTableIdentifier,
    IndexWriteMode,
    PersistenceIndexId,
    PrevIndexEntry,
    StableIndexName,
    TabletIndexName,
    INDEX_BY_CREATION_TIME_DESCRIPTOR,
    INDEX_BY_ID_DESCRIPTOR,
};
pub use maybe_value::MaybeValue;
pub use object_key::{
    FullyQualifiedObjectKey,
    ObjectKey,
};
pub use region::{
    default_region,
    set_test_region_as_default,
    RegionName,
    TEST_REGION_NAME,
};
pub use search_index_metric_labels::SearchIndexMetricLabels;
pub use snapshot_export::SetExportExpirationRequest;
pub use table::TableStats;
pub use timestamp::{
    RepeatableReason,
    RepeatableTimestamp,
    WriteTimestamp,
};

// A developer using convex
tuple_struct_u64!(MemberId);
tuple_struct_u64!(TeamId);
tuple_struct_u64!(DeploymentId);

impl DeploymentId {
    /// A stable stand-in ID for a deployment big brain has no row for
    /// (self-hosted backends, statically configured conductors). Derived from
    /// the name so it survives restarts -- an ID-keyed persistence layout
    /// stores rows under it -- and kept within `u32`, the widest ID such
    /// layouts accept.
    pub fn stable_from_name(name: &str) -> Self {
        let digest = crate::sha256::Sha256::hash(name.as_bytes());
        let bytes: [u8; 4] = digest.as_ref()[..4]
            .try_into()
            .expect("a SHA-256 digest has at least four bytes");
        Self(u64::from(u32::from_be_bytes(bytes)))
    }
}
tuple_struct_u64!(ProjectId);
tuple_struct_u64!(CustomRoleId);
// The autoincrement primary key of the `authorized_devices` table in big brain.
// Stably identifies an access token (unlike the secret token string, which we
// avoid surfacing) so callers can refer to a specific token.
tuple_struct_u64!(AccessTokenId);
tuple_struct_string!(ConvexOrigin);
tuple_struct_string!(ConvexSite);

impl AccessTokenId {
    /// A sentinel value for legacy audit log rows that do not contain the
    /// access token
    pub fn unknown() -> Self {
        Self(0)
    }
}

/// A unique id for a subscription.
pub type SubscriberId = usize;

/// We cursor through logs with a monotonic f64 of milliseconds since epoch.
pub type CursorMs = f64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PersistenceVersion {
    V5,
    V6,
}

impl PersistenceVersion {
    /// When migrating to this PersistenceVersion causes index key encoding
    /// to change, return base_version + 1.
    /// After the migration is complete, bump base_version at all call-sites
    /// and return base_version here.
    pub fn index_key_version(&self, base_version: u8) -> u8 {
        match self {
            PersistenceVersion::V5 | PersistenceVersion::V6 => base_version,
        }
    }

    pub fn version(&self) -> usize {
        match self {
            PersistenceVersion::V5 => 5,
            PersistenceVersion::V6 => 6,
        }
    }
}
