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

/// Value that `npx convex env list` prints in place of a secret's real
/// value. Setting a secret to this value keeps the stored one, so the output
/// of `env list` can be fed back to `env set`. Kept in sync with
/// `SECRET_ENV_VAR_PLACEHOLDER` in `npm-packages/system-udfs`.
pub const SECRET_ENV_VAR_PLACEHOLDER: &str = "[Secret omitted]";

/// Who can read an environment variable's value. Functions read every
/// variable through `process.env` regardless of its visibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EnvVarVisibility {
    /// The dashboard, the CLI and the platform API can read the value.
    Config,
    /// Write-only: the dashboard, the CLI and the platform API can set the
    /// value but never read it back.
    Secret,
}

#[derive(Debug, PartialEq, Clone)]
pub struct PersistedEnvironmentVariable {
    pub variable: EnvironmentVariable,
    /// `None` while the visibility is undecided: the variable predates
    /// secrets, or was set without a visibility. It is readable like a
    /// config variable, and stays distinguishable from a variable that
    /// someone explicitly made a config variable, so that a later write or
    /// migration can still decide.
    pub visibility: Option<EnvVarVisibility>,
}

impl PersistedEnvironmentVariable {
    /// A variable whose visibility is undecided.
    pub fn new(variable: EnvironmentVariable) -> Self {
        Self {
            variable,
            visibility: None,
        }
    }

    pub fn is_secret(&self) -> bool {
        match self.visibility {
            Some(EnvVarVisibility::Secret) => true,
            Some(EnvVarVisibility::Config) | None => false,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct SerializedEnvironmentVariable {
    pub name: String,
    pub value: String,
    /// `true` for secrets, `false` for variables explicitly made a config
    /// variable, absent while the visibility is undecided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<bool>,
}

impl From<PersistedEnvironmentVariable> for SerializedEnvironmentVariable {
    fn from(
        PersistedEnvironmentVariable {
            variable: EnvironmentVariable { name, value },
            visibility,
        }: PersistedEnvironmentVariable,
    ) -> SerializedEnvironmentVariable {
        SerializedEnvironmentVariable {
            name: name.into(),
            value: value.into(),
            secret: visibility.map(|visibility| match visibility {
                EnvVarVisibility::Config => false,
                EnvVarVisibility::Secret => true,
            }),
        }
    }
}

impl TryFrom<SerializedEnvironmentVariable> for PersistedEnvironmentVariable {
    type Error = anyhow::Error;

    fn try_from(
        obj: SerializedEnvironmentVariable,
    ) -> anyhow::Result<PersistedEnvironmentVariable> {
        Ok(Self {
            variable: EnvironmentVariable {
                name: obj.name.parse()?,
                value: obj.value.parse()?,
            },
            visibility: obj.secret.map(|secret| match secret {
                true => EnvVarVisibility::Secret,
                false => EnvVarVisibility::Config,
            }),
        })
    }
}

codegen_convex_serialization!(PersistedEnvironmentVariable, SerializedEnvironmentVariable);
