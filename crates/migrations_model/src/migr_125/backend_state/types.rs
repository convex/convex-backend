use serde::{
    Deserialize,
    Serialize,
};
use value::codegen_convex_serialization;

/// Frozen copy of BackendState types at migration 125.

#[derive(Clone, Copy, Debug, PartialEq, strum::EnumString, strum::Display)]
#[strum(serialize_all = "snake_case")]
pub enum OldBackendState {
    Disabled,
    Paused,
    Running,
    Suspended,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackendState {
    pub system: SystemStopState,
    pub user: UserStopState,
}

#[derive(Clone, Copy, Debug, PartialEq, strum::EnumString, strum::Display)]
#[strum(serialize_all = "snake_case")]
pub enum SystemStopState {
    None,
    Disabled,
    Suspended,
}

#[derive(Clone, Copy, Debug, PartialEq, strum::EnumString, strum::Display)]
#[strum(serialize_all = "snake_case")]
pub enum UserStopState {
    None,
    Paused,
}

#[derive(Debug, PartialEq, Clone)]
pub enum PersistedBackendState {
    Old(OldBackendState),
    New(BackendState),
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum SerializedBackendState {
    Old { state: String },
    New { system: String, user: String },
}

impl From<PersistedBackendState> for SerializedBackendState {
    fn from(state: PersistedBackendState) -> Self {
        match state {
            PersistedBackendState::Old(state) => Self::Old {
                state: state.to_string(),
            },
            PersistedBackendState::New(state) => Self::New {
                system: state.system.to_string(),
                user: state.user.to_string(),
            },
        }
    }
}

impl TryFrom<SerializedBackendState> for PersistedBackendState {
    type Error = anyhow::Error;

    fn try_from(object: SerializedBackendState) -> anyhow::Result<Self> {
        Ok(match object {
            SerializedBackendState::Old { state } => Self::Old(state.parse()?),
            SerializedBackendState::New { system, user } => Self::New(BackendState {
                system: system.parse()?,
                user: user.parse()?,
            }),
        })
    }
}

codegen_convex_serialization!(PersistedBackendState, SerializedBackendState);
