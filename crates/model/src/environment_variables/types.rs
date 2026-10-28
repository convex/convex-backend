pub use common::types::{
    EnvVarName,
    EnvVarValue,
    EnvironmentVariable,
};
use serde::{
    Deserialize,
    Serialize,
};
use value::codegen_convex_serialization;

#[derive(Debug, PartialEq, Clone)]
pub struct PersistedEnvironmentVariable(pub EnvironmentVariable);

#[derive(Serialize, Deserialize)]
pub struct SerializedEnvironmentVariable {
    pub name: String,
    pub value: String,
}

impl From<PersistedEnvironmentVariable> for SerializedEnvironmentVariable {
    fn from(
        PersistedEnvironmentVariable(
            EnvironmentVariable { name, value }
        ): PersistedEnvironmentVariable,
    ) -> SerializedEnvironmentVariable {
        SerializedEnvironmentVariable {
            name: name.into(),
            value: value.into(),
        }
    }
}

impl TryFrom<SerializedEnvironmentVariable> for PersistedEnvironmentVariable {
    type Error = anyhow::Error;

    fn try_from(
        obj: SerializedEnvironmentVariable,
    ) -> anyhow::Result<PersistedEnvironmentVariable> {
        Ok(Self(EnvironmentVariable {
            name: obj.name.parse()?,
            value: obj.value.parse()?,
        }))
    }
}

codegen_convex_serialization!(PersistedEnvironmentVariable, SerializedEnvironmentVariable);
