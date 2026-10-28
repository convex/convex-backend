use std::{
    fmt::{
        self,
        Display,
    },
    str::FromStr,
    sync::LazyLock,
};

use anyhow::Context;
use cmd_util::env::env_config;
use errors::ErrorMetadata;
pub use metrics::SERVER_VERSION_STR;
pub use semver::Version;
use serde::{
    Deserialize,
    Serialize,
};
use value::export::ValueFormat;

// Threshold for each of our clients
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VersionThreshold {
    pub upgrade_required: Version,
    pub unsupported: Version,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DeprecationThreshold {
    pub npm: VersionThreshold,
    pub python: VersionThreshold,
    pub rust: VersionThreshold,
}

pub static DEPRECATION_THRESHOLD: LazyLock<DeprecationThreshold> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../deprecation.json"))
        .expect("Couldn't parse deprecation.json")
});

// Enabled in 1.7.0 but we use 1.6.1000 to allow for pre-releases to have this
// feature enabled
pub static MIN_NPM_VERSION_FOR_PREFIX_SEARCH: LazyLock<Version> = LazyLock::new(|| {
    env_config(
        "MIN_NPM_VERSION_FOR_PREFIX_SEARCH",
        Version::new(1, 6, 1000),
    )
});

// Enabled in 1.27.5
pub static MIN_NPM_VERSION_FOR_TRANSITION_CHUNKS: LazyLock<Version> = LazyLock::new(|| {
    env_config(
        // Until clients can handle these better use a large number we'll never hit.
        "MIN_NPM_VERSION_FOR_TRANSITION_CHUNKS",
        Version::parse("1.28.0").expect("Invalid version"),
    )
});

#[derive(Debug, Serialize, PartialEq, Eq)]
pub enum ClientVersionState {
    Unsupported(String),
    // This version is deprecated and will be unsupported soon
    UpgradeRequired(String),
    Supported,
}

impl ClientVersionState {
    pub fn variant_name(&self) -> &str {
        match self {
            ClientVersionState::Unsupported(_) => "Unsupported",
            // NOTE: The string "UpgradeCritical" causes old CLI versions
            // <=1.4.1 to throw a bad error. So we send a different string
            // that makes all CLI versions just print the warning.
            ClientVersionState::UpgradeRequired(_) => "UpgradeRequired",
            ClientVersionState::Supported => "Supported",
        }
    }
}

#[derive(PartialEq, Eq, Clone, Ord, PartialOrd)]
pub struct ClientVersion {
    client: ClientType,
    version: ClientVersionIdent,
}

impl From<ClientVersion> for pb::common::ClientVersion {
    fn from(ClientVersion { client, version }: ClientVersion) -> Self {
        Self {
            client: Some(client.to_string()),
            version: Some(version.into()),
        }
    }
}

impl TryFrom<pb::common::ClientVersion> for ClientVersion {
    type Error = anyhow::Error;

    fn try_from(
        pb::common::ClientVersion { client, version }: pb::common::ClientVersion,
    ) -> anyhow::Result<Self> {
        let client = client.context("Missing `client` field")?.parse()?;
        let version = version.context("Missing `version` field")?.try_into()?;
        Ok(ClientVersion { client, version })
    }
}

#[derive(PartialEq, Eq, Clone, Ord, PartialOrd, Debug)]
pub enum ClientVersionIdent {
    Semver(Version),
    Unrecognized(String),
}

impl Display for ClientVersionIdent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Semver(v) => write!(f, "{v}"),
            Self::Unrecognized(ident) => write!(f, "{ident}"),
        }
    }
}

impl ClientVersionIdent {
    fn below_threshold(&self, threshold: &Version) -> bool {
        match self {
            Self::Semver(version) => version <= threshold,
            Self::Unrecognized(_) => true,
        }
    }

    fn above_threshold(&self, threshold: &Version) -> bool {
        match self {
            Self::Semver(version) => version >= threshold,
            Self::Unrecognized(_) => true,
        }
    }
}

impl From<ClientVersionIdent> for pb::common::ClientVersionIdent {
    fn from(value: ClientVersionIdent) -> Self {
        let version = match value {
            ClientVersionIdent::Semver(version) => {
                pb::common::client_version_ident::Version::Semver(version.to_string())
            },
            ClientVersionIdent::Unrecognized(str) => {
                pb::common::client_version_ident::Version::Unrecognized(str)
            },
        };
        pb::common::ClientVersionIdent {
            version: Some(version),
        }
    }
}

impl TryFrom<pb::common::ClientVersionIdent> for ClientVersionIdent {
    type Error = anyhow::Error;

    fn try_from(msg: pb::common::ClientVersionIdent) -> anyhow::Result<Self> {
        let version = match msg.version {
            Some(pb::common::client_version_ident::Version::Semver(version)) => {
                ClientVersionIdent::Semver(version.parse()?)
            },
            Some(pb::common::client_version_ident::Version::Unrecognized(str)) => {
                ClientVersionIdent::Unrecognized(str)
            },
            None => anyhow::bail!("Missing `version` field"),
        };
        Ok(version)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Ord, PartialOrd)]
pub enum ClientType {
    Python,
    CLI,
    // `npm create convex` CLI tool https://github.com/convex/templates/tree/main/create-convex
    CreateConvex,
    NPM,
    // Actions running in node call into queries/mutations/etc. with this client.
    Actions,
    Rust,
    StreamingImport,
    AirbyteExport,
    FivetranImport,
    FivetranExport,
    // For HTTP requests from the dashboard. Requests from the dashboard via a
    // Convex client will have an `NPM` version
    Dashboard,
    Swift,
    Kotlin,
    // We convert to lower case when we parse, so lets just generate lowercase strings.
    Unrecognized(String),
}

impl FromStr for ClientType {
    type Err = !;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let client_type = match &*s.to_ascii_lowercase() {
            "python-convex" => Self::Python,
            "python" => Self::Python,
            "npm-cli" => Self::CLI,
            "npm" => Self::NPM,
            "create-convex" => Self::CreateConvex,
            "actions" => Self::Actions,
            "rust" => Self::Rust,
            "streaming-import" => Self::StreamingImport,
            "airbyte-export" => Self::AirbyteExport,
            "fivetran-import" => Self::FivetranImport,
            "fivetran-export" => Self::FivetranExport,
            "dashboard" => Self::Dashboard,
            "swift" => Self::Swift,
            "kotlin" => Self::Kotlin,
            unrecognized => Self::Unrecognized(unrecognized.to_string()),
        };
        Ok(client_type)
    }
}

impl Display for ClientType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Python => write!(f, "python"),
            Self::CLI => write!(f, "npm-cli"),
            Self::CreateConvex => write!(f, "create-convex"),
            Self::NPM => write!(f, "npm"),
            Self::Actions => write!(f, "actions"),
            Self::Rust => write!(f, "rust"),
            Self::StreamingImport => write!(f, "streaming-import"),
            Self::AirbyteExport => write!(f, "airbyte-export"),
            Self::FivetranImport => write!(f, "fivetran-import"),
            Self::FivetranExport => write!(f, "fivetran-export"),
            Self::Dashboard => write!(f, "dashboard"),
            Self::Swift => write!(f, "swift"),
            Self::Kotlin => write!(f, "kotlin"),
            Self::Unrecognized(other_client) => write!(f, "{other_client}"),
        }
    }
}

impl ClientType {
    fn upgrade_required_threshold(&self) -> Option<Version> {
        let DeprecationThreshold { npm, python, rust } = &*DEPRECATION_THRESHOLD;
        match self {
            Self::NPM | Self::CLI | Self::Actions => Some(npm.upgrade_required.clone()),
            Self::Python => Some(python.upgrade_required.clone()),
            Self::Rust => Some(rust.upgrade_required.clone()),
            Self::CreateConvex
            | Self::StreamingImport
            | Self::AirbyteExport
            | Self::FivetranImport
            | Self::FivetranExport
            | Self::Dashboard
            | Self::Swift
            | Self::Kotlin
            | Self::Unrecognized(_) => None,
        }
    }

    fn unsupported_threshold(&self) -> Option<Version> {
        let DeprecationThreshold { npm, python, rust } = &*DEPRECATION_THRESHOLD;
        match self {
            Self::NPM | Self::CLI | Self::Actions => Some(npm.unsupported.clone()),
            Self::Python => Some(python.unsupported.clone()),
            Self::Rust => Some(rust.unsupported.clone()),
            Self::CreateConvex
            | Self::StreamingImport
            | Self::AirbyteExport
            | Self::FivetranImport
            | Self::FivetranExport
            | Self::Dashboard
            | Self::Swift
            | Self::Kotlin
            | Self::Unrecognized(_) => None,
        }
    }

    fn upgrade_instructions(&self) -> &str {
        match self {
            Self::NPM | Self::CLI | Self::Actions => {
                "Update your Convex npm version with `npx convex update` or `npm update`."
            },
            Self::Python => {
                "Update your Convex python version with `pip install --upgrade convex`."
            },
            Self::Rust => {
                "Update your Convex rust version with `cargo update -p convex` or by updating \
                 `Cargo.toml`."
            },
            Self::CreateConvex
            | Self::StreamingImport
            | Self::AirbyteExport
            | Self::FivetranImport
            | Self::FivetranExport
            | Self::Dashboard
            | Self::Swift
            | Self::Kotlin
            | Self::Unrecognized(_) => "",
        }
    }
}

impl FromStr for ClientVersion {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        // Use the longest parseable semver spec from the right.
        let parts: Vec<&str> = s.split('-').collect();

        if parts.len() < 2 {
            anyhow::bail!(ErrorMetadata::bad_request(
                "InvalidVersion",
                format!(
                    "Failed to parse client version string: '{s}'. Expected format is \
                     {{client_name}}-{{semver}}, e.g. my-esolang-client-0.0.1"
                ),
            ));
        }

        let split = (1..parts.len()).find_map(|n| {
            Version::parse(parts[n..parts.len()].join("-").as_str())
                .map(|v| (n, v))
                .ok()
        });
        let (client_str, version) = match split {
            Some((n, version)) => {
                let client_str = parts[0..n].join("-");
                (client_str, ClientVersionIdent::Semver(version))
            },
            None => {
                let client_str = parts[0].to_string();
                let version = ClientVersionIdent::Unrecognized(parts[1..].join("-"));
                (client_str, version)
            },
        };
        let Ok(client) = client_str.parse::<ClientType>();
        Ok(Self { client, version })
    }
}

impl ClientVersion {
    pub fn client(&self) -> &ClientType {
        &self.client
    }

    pub fn version(&self) -> &ClientVersionIdent {
        &self.version
    }

    pub fn current_state(&self) -> ClientVersionState {
        let client = &self.client;
        let version = &self.version;
        let upgrade_instructions = self.client.upgrade_instructions();

        if let Some(unsupported_threshold) = self.client.unsupported_threshold()
            && self.version.below_threshold(&unsupported_threshold)
        {
            return ClientVersionState::Unsupported(format!(
                "The Convex {client} package at version {version} is no longer supported. \
                 {upgrade_instructions}"
            ));
        }
        if let Some(upgrade_required_threshold) = self.client.upgrade_required_threshold()
            && self.version.below_threshold(&upgrade_required_threshold)
        {
            return ClientVersionState::UpgradeRequired(format!(
                "The Convex {client} package at {version} is deprecated and will no longer be \
                 supported soon. When this version is no longer supported, requests to Convex \
                 will fail, so it is best to upgrade and redeploy your application as soon as \
                 possible. {upgrade_instructions}",
            ));
        }

        ClientVersionState::Supported
    }

    // FIXME: remove this From impl once all clients using version params are
    // deprecated.
    pub fn from_path_param(v: Version, path: &str) -> ClientVersion {
        Self {
            client: if path.ends_with("/sync") || path.ends_with("/udf") {
                ClientType::NPM
            } else {
                ClientType::CLI
            },
            version: ClientVersionIdent::Semver(v),
        }
    }

    pub fn unknown() -> ClientVersion {
        Self {
            client: ClientType::Unrecognized("unknown".into()),
            version: ClientVersionIdent::Unrecognized("unknownversion".into()),
        }
    }

    /// Returns true if the client version new enough to require a format param.
    /// Python and JS client got explicit about this at some point, but old
    /// clients still implicitly need the encoded format.
    pub fn default_format(&self) -> ValueFormat {
        let clean_format = match self.client() {
            ClientType::CLI | ClientType::NPM | ClientType::Actions => {
                self.version().above_threshold(&Version::new(1, 4, 1))
            },
            ClientType::Python => self.version().above_threshold(&Version::new(0, 5, 0)),
            ClientType::Rust
            | ClientType::CreateConvex
            | ClientType::StreamingImport
            | ClientType::AirbyteExport
            | ClientType::FivetranImport
            | ClientType::FivetranExport
            | ClientType::Dashboard
            | ClientType::Swift
            | ClientType::Kotlin
            | ClientType::Unrecognized(_) => true,
        };

        // Old clients use the encoded format by default
        match clean_format {
            true => ValueFormat::ConvexCleanJSON,
            false => ValueFormat::ConvexEncodedJSON,
        }
    }
}

impl fmt::Display for ClientVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.client, self.version)
    }
}

impl fmt::Debug for ClientVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} ({:?})", self, self.current_state())
    }
}
