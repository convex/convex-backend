use common::types::UsageLimitStopState;
pub use common::types::{
    BackendState,
    OldBackendState,
};
use serde::{
    Deserialize,
    Serialize,
};
use value::codegen_convex_serialization;

#[derive(Debug, PartialEq, Clone)]
pub struct PersistedBackendState(pub BackendState);

#[derive(Serialize, Deserialize)]
pub struct SerializedBackendState {
    system: String,
    #[serde(default = "default_usage_limit_stop_state")]
    usage_limit: String,
    user: String,
}

fn default_usage_limit_stop_state() -> String {
    UsageLimitStopState::None.to_string()
}

impl From<PersistedBackendState> for SerializedBackendState {
    fn from(state: PersistedBackendState) -> Self {
        Self {
            system: state.0.system.to_string(),
            usage_limit: state.0.usage_limit.to_string(),
            user: state.0.user.to_string(),
        }
    }
}

impl TryFrom<SerializedBackendState> for PersistedBackendState {
    type Error = anyhow::Error;

    fn try_from(object: SerializedBackendState) -> anyhow::Result<Self> {
        Ok(Self(BackendState {
            system: object.system.parse()?,
            usage_limit: object.usage_limit.parse()?,
            user: object.user.parse()?,
        }))
    }
}

codegen_convex_serialization!(PersistedBackendState, SerializedBackendState);
