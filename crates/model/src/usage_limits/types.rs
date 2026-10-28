use errors::ErrorMetadata;
use serde::{
    Deserialize,
    Serialize,
};
use utoipa::ToSchema;
use value::codegen_convex_serialization;

/// The user-facing unit a metric's limits and usage are expressed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum MetricUnit {
    #[serde(rename = "calls")]
    Calls,
    #[serde(rename = "GB")]
    Gb,
    #[serde(rename = "Query-GB")]
    QueryGb,
    #[serde(rename = "GB-hours")]
    GbHours,
    #[serde(rename = "dollars")]
    Dollars,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageLimitConfig {
    pub metric: UsageLimitMetric,
    pub window: UsageLimitWindow,
    pub limit_type: UsageLimitType,
    pub limit: u64,
    pub enabled: bool,
}

impl UsageLimitConfig {
    pub fn new(
        metric: UsageLimitMetric,
        window: UsageLimitWindow,
        limit_type: UsageLimitType,
        limit: u64,
        enabled: bool,
    ) -> anyhow::Result<Self> {
        let config = Self {
            metric,
            window,
            limit_type,
            limit,
            enabled,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn key(&self) -> UsageLimitKey {
        UsageLimitKey {
            metric: self.metric,
            window: self.window,
            limit_type: self.limit_type,
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.limit == 0 {
            return Err(ErrorMetadata::bad_request(
                "InvalidUsageLimit",
                "Usage limits must have a positive limit.",
            )
            .into());
        }
        Ok(())
    }

}

pub struct UsageLimitKey {
    pub metric: UsageLimitMetric,
    pub window: UsageLimitWindow,
    pub limit_type: UsageLimitType,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    strum::EnumString,
    strum::Display,
    strum::EnumIter,
    Serialize,
    Deserialize,
    ToSchema,
)]
#[strum(serialize_all = "camelCase")]
// The serde renames must match the strum camelCase output (heck
// lower-camel-cases `GB` to `Gb`, which serde's `rename_all` would not);
// `test_metric_serialized_names` pins both against each other.
pub enum UsageLimitMetric {
    #[serde(rename = "functionCalls")]
    FunctionCalls,
    #[serde(rename = "databaseIoGb")]
    DatabaseIoGB,
    #[serde(rename = "dataEgressGb")]
    DataEgressGB,
    #[serde(rename = "searchQueryGb")]
    SearchQueryGB,
    #[serde(rename = "queryMutationComputeGbHours")]
    QueryMutationComputeGBHours,
    #[serde(rename = "actionComputeConvexGbHours")]
    ActionComputeConvexGBHours,
    #[serde(rename = "actionComputeNodeJsGbHours")]
    ActionComputeNodeJsGBHours,
    #[serde(rename = "actionComputeCpuGbHours")]
    ActionComputeCpuGBHours,
    #[serde(rename = "aiGatewayCostDollars")]
    AiGatewayCostDollars,
}

const BYTES_PER_GB: f64 = (1u64 << 30) as f64;
const SECS_PER_HOUR: f64 = 3600.0;

impl UsageLimitMetric {
    /// Canonical name for this metric in the in-memory usage metric stores.
    /// The seed pipeline's finer-grained metrics are combined into these
    /// buckets at hydration; see [`Self::from_seed_metric`].
    pub fn metric_name(&self) -> &'static str {
        match self {
            Self::FunctionCalls => "function_calls",
            Self::DatabaseIoGB => "database_io_bytes",
            Self::DataEgressGB => "data_egress_bytes",
            Self::SearchQueryGB => "search_query_gb",
            Self::QueryMutationComputeGBHours => "query_mutation_compute_gbs",
            Self::ActionComputeConvexGBHours => "action_compute_convex_gbs",
            Self::ActionComputeNodeJsGBHours => "action_compute_nodejs_gbs",
            Self::ActionComputeCpuGBHours => "action_compute_cpu_gbs",
            Self::AiGatewayCostDollars => "ai_gateway_cost_dollars",
        }
    }

    /// Map one of the usage pipeline's rollup metric names (the
    /// `metric_name` values in `deployment_usage_*_rollup`) to the
    /// enforcement bucket it feeds. Several source metrics can feed one
    /// bucket; hydration sums them.
    ///
    /// Every source metric's unit matches its bucket's raw unit, so seeded
    /// values are used as-is.
    pub fn from_seed_metric(name: &str) -> Option<Self> {
        Some(match name {
            "udf_calls" | "storage_calls" | "udf_storage_calls" => Self::FunctionCalls,
            "reactor_gbs" => Self::QueryMutationComputeGBHours,
            "action_gbs" => Self::ActionComputeConvexGBHours,
            "action_node_gbs" => Self::ActionComputeNodeJsGBHours,
            "action_user_gbs" => Self::ActionComputeCpuGBHours,
            "ai_gateway_cost" => Self::AiGatewayCostDollars,
            "db_ingress" | "db_egress" => Self::DatabaseIoGB,
            "text_query_search_gb" | "vector_query_search_gb_dims" => Self::SearchQueryGB,
            "network_egress" | "storage_bandwidth_egress" | "udf_storage_bandwidth_egress" => {
                Self::DataEgressGB
            },
            _ => return None,
        })
    }

    /// The user-facing unit this metric's limits and usage are expressed in.
    /// Matches the unit `limit_in_raw_units` converts *from* and
    /// `usage_in_display_units` converts *to*.
    pub fn unit(&self) -> MetricUnit {
        match self {
            Self::FunctionCalls => MetricUnit::Calls,
            Self::DatabaseIoGB | Self::DataEgressGB => MetricUnit::Gb,
            Self::SearchQueryGB => MetricUnit::QueryGb,
            Self::AiGatewayCostDollars => MetricUnit::Dollars,
            Self::QueryMutationComputeGBHours
            | Self::ActionComputeConvexGBHours
            | Self::ActionComputeNodeJsGBHours
            | Self::ActionComputeCpuGBHours => MetricUnit::GbHours,
        }
    }

    /// Convert a configured limit from this metric's user-facing unit
    /// (calls, GB, or GB-hours) into the raw unit its store counts in
    /// (calls, bytes, GB, or GB·s).
    pub fn limit_in_raw_units(&self, limit: u64) -> f64 {
        match self {
            Self::FunctionCalls | Self::SearchQueryGB | Self::AiGatewayCostDollars => limit as f64,
            Self::DatabaseIoGB | Self::DataEgressGB => limit as f64 * BYTES_PER_GB,
            Self::QueryMutationComputeGBHours
            | Self::ActionComputeConvexGBHours
            | Self::ActionComputeNodeJsGBHours
            | Self::ActionComputeCpuGBHours => limit as f64 * SECS_PER_HOUR,
        }
    }

    /// Convert a raw usage total (calls, bytes, GB, or GB·s) back into this
    /// metric's user-facing unit, so surfaced usage is directly comparable to
    /// a configured limit. The inverse of `limit_in_raw_units`.
    pub fn usage_in_display_units(&self, raw: f64) -> f64 {
        match self {
            Self::FunctionCalls | Self::SearchQueryGB | Self::AiGatewayCostDollars => raw,
            Self::DatabaseIoGB | Self::DataEgressGB => raw / BYTES_PER_GB,
            Self::QueryMutationComputeGBHours
            | Self::ActionComputeConvexGBHours
            | Self::ActionComputeNodeJsGBHours
            | Self::ActionComputeCpuGBHours => raw / SECS_PER_HOUR,
        }
    }
}

/// The calendar-aligned window a limit is measured over.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    strum::EnumString,
    strum::Display,
    Serialize,
    Deserialize,
    ToSchema,
)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum UsageLimitWindow {
    Day,
    Month,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    strum::EnumString,
    strum::Display,
    Serialize,
    Deserialize,
    ToSchema,
)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum UsageLimitType {
    Warning,
    Disable,
}

// A `name` field was once persisted here. It's been dropped: reads ignore the
// leftover key on older `_usage_limits` documents (and audit-log entries), and
// new writes simply omit it.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedUsageLimitConfig {
    pub metric: String,
    pub window: String,
    pub limit_type: String,
    pub limit: u64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl TryFrom<UsageLimitConfig> for SerializedUsageLimitConfig {
    type Error = anyhow::Error;

    fn try_from(value: UsageLimitConfig) -> Result<Self, Self::Error> {
        value.validate()?;
        Ok(Self {
            metric: value.metric.to_string(),
            window: value.window.to_string(),
            limit_type: value.limit_type.to_string(),
            limit: value.limit,
            enabled: value.enabled,
        })
    }
}

impl TryFrom<SerializedUsageLimitConfig> for UsageLimitConfig {
    type Error = anyhow::Error;

    fn try_from(value: SerializedUsageLimitConfig) -> Result<Self, Self::Error> {
        let config = Self {
            metric: value.metric.parse()?,
            window: value.window.parse()?,
            limit_type: value.limit_type.parse()?,
            limit: value.limit,
            enabled: value.enabled,
        };
        config.validate()?;
        Ok(config)
    }
}

codegen_convex_serialization!(UsageLimitConfig, SerializedUsageLimitConfig);
