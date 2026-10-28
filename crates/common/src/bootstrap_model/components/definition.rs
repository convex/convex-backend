use std::{
    collections::BTreeMap,
    ops::Deref,
    str::FromStr,
};

use json_trait::JsonForm as _;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::Value as JsonValue;
use sync_types::path::PathComponent;
use value::{
    codegen_convex_serialization,
    heap_size::HeapSize,
    identifier::Identifier,
    ConvexValue,
    TableMapping,
    TableNamespace,
};

use crate::{
    components::{
        ComponentDefinitionPath,
        ComponentName,
        Reference,
    },
    schemas::validator::Validator,
    types::EnvVarName,
    virtual_system_mapping::VirtualSystemMapping,
};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ComponentDefinitionMetadata {
    pub path: ComponentDefinitionPath,
    pub definition_type: ComponentDefinitionType,

    pub child_components: Vec<ComponentInstantiation>,

    pub http_mounts: BTreeMap<HttpMountPath, Reference>,

    /// For App definitions only: the HTTP path prefix under which the app's
    /// own `http.ts` routes are served. Child component mounts are specified
    /// as absolute paths (via `http_mounts`) and are unaffected by this field.
    pub http_prefix: Option<HttpMountPath>,

    pub exports: BTreeMap<PathComponent, ComponentExport>,

    pub env_vars: BTreeMap<Identifier, EnvVarValidator>,
}

impl ComponentDefinitionMetadata {
    pub fn default_root() -> Self {
        Self {
            path: ComponentDefinitionPath::root(),
            definition_type: ComponentDefinitionType::App,
            child_components: Vec::new(),
            http_mounts: BTreeMap::new(),
            http_prefix: None,
            exports: BTreeMap::new(),
            env_vars: BTreeMap::new(),
        }
    }

    pub fn is_app(&self) -> bool {
        self.definition_type == ComponentDefinitionType::App
    }

    pub fn required_env_var_names(&self) -> Vec<String> {
        self.env_vars
            .iter()
            .filter(|(_, v)| !v.optional)
            .map(|(name, _)| name.to_string())
            .collect()
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct HttpMountPath(String);

impl Deref for HttpMountPath {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<HttpMountPath> for String {
    fn from(value: HttpMountPath) -> Self {
        value.0
    }
}

impl HeapSize for HttpMountPath {
    fn heap_size(&self) -> usize {
        self.0.heap_size()
    }
}

impl FromStr for HttpMountPath {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        anyhow::ensure!(s.starts_with('/'));
        anyhow::ensure!(s.ends_with('/'));
        anyhow::ensure!(!s.contains('*'));
        let path: http::uri::PathAndQuery = s.parse()?;
        anyhow::ensure!(path.query().is_none());
        Ok(Self(s.to_string()))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ComponentDefinitionType {
    App,
    ChildComponent {
        name: ComponentName,
        args: BTreeMap<Identifier, ComponentArgumentValidator>,
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ComponentInstantiation {
    pub name: ComponentName,
    pub path: ComponentDefinitionPath,
    pub args: Option<BTreeMap<Identifier, ComponentArgument>>,
    pub env: BTreeMap<Identifier, EnvBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentExport {
    Branch(BTreeMap<PathComponent, ComponentExport>),
    Leaf(Reference),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ComponentArgumentValidator {
    Value(Validator),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EnvVarValidator {
    pub validator: Validator,
    pub optional: bool,
}

impl EnvVarValidator {
    /// Checks that a value directly provided for a component env var matches
    /// the validator that the component defined.
    ///
    /// The internal validator might constrain string further (e.g. to be one
    /// of a set of literal values) so it's not sufficient to just know the
    /// type is a string.
    pub fn check_provided_value(&self, value: &str) -> anyhow::Result<()> {
        // Empty mappings are safe: env var validators are constrained to be
        // string-like, and check_value only consults table mappings for
        // Validator::Id which can't appear here.
        // TODO(CX-6540): Remove hack where we pass in empty mappings.
        let table_mapping = TableMapping::new().namespace(TableNamespace::by_component_TODO());
        let virtual_system_mapping = VirtualSystemMapping::default();
        let convex_value = ConvexValue::String(value.try_into()?);
        self.validator
            .check_value(&convex_value, &table_mapping, &virtual_system_mapping)
            .map_err(|e| anyhow::anyhow!("{e}"))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ComponentArgument {
    Value(ConvexValue),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum EnvBinding {
    Value(
        String,
    ),
    EnvVar(EnvVarName),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedComponentDefinitionMetadata {
    path: String,
    definition_type: SerializedComponentDefinitionType,
    child_components: Vec<SerializedComponentInstantiation>,
    http_mounts: Option<BTreeMap<String, String>>,
    http_prefix: Option<String>,
    exports: SerializedComponentExport,
    #[serde(default)]
    env_vars: Option<Vec<(String, SerializedEnvVarValidator)>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SerializedComponentDefinitionType {
    App {},
    ChildComponent {
        name: String,
        args: Vec<(String, SerializedComponentArgumentValidator)>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SerializedComponentArgumentValidator {
    Value { value: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SerializedEnvVarValidator {
    Value {
        value: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        optional: Option<bool>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SerializedComponentArgument {
    Value { value: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SerializedEnvBinding {
    Value { value: String },
    EnvVar { name: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
struct SerializedComponentInstantiation {
    name: String,
    path: String,
    args: Option<Vec<(String, SerializedComponentArgument)>>,
    #[serde(default)]
    env: Vec<(String, SerializedEnvBinding)>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum SerializedComponentExport {
    Branch {
        branch: Vec<(String, SerializedComponentExport)>,
    },
    Leaf {
        leaf: String,
    },
}

impl TryFrom<ComponentDefinitionMetadata> for SerializedComponentDefinitionMetadata {
    type Error = anyhow::Error;

    fn try_from(m: ComponentDefinitionMetadata) -> anyhow::Result<Self> {
        let env_vars = if m.env_vars.is_empty() {
            None
        } else {
            Some(
                m.env_vars
                    .into_iter()
                    .map(|(name, v)| anyhow::Ok((String::from(name), v.try_into()?)))
                    .try_collect()?,
            )
        };
        Ok(Self {
            path: String::from(m.path),
            definition_type: m.definition_type.try_into()?,
            child_components: m
                .child_components
                .into_iter()
                .map(TryFrom::try_from)
                .try_collect()?,
            http_mounts: Some(
                m.http_mounts
                    .into_iter()
                    .map(|(k, v)| (String::from(k), String::from(v)))
                    .collect(),
            ),
            http_prefix: m.http_prefix.map(String::from),
            exports: ComponentExport::Branch(m.exports).try_into()?,
            env_vars,
        })
    }
}

impl TryFrom<SerializedComponentDefinitionMetadata> for ComponentDefinitionMetadata {
    type Error = anyhow::Error;

    fn try_from(m: SerializedComponentDefinitionMetadata) -> anyhow::Result<Self> {
        let ComponentExport::Branch(exports) = m.exports.try_into()? else {
            anyhow::bail!("Expected branch of exports at the top level");
        };
        let env_vars: BTreeMap<Identifier, EnvVarValidator> = m
            .env_vars
            .unwrap_or_default()
            .into_iter()
            .map(|(name, v)| anyhow::Ok((name.parse()?, v.try_into()?)))
            .try_collect()?;
        Ok(Self {
            path: m.path.parse()?,
            definition_type: m.definition_type.try_into()?,
            child_components: m
                .child_components
                .into_iter()
                .map(TryFrom::try_from)
                .try_collect()?,
            http_mounts: m
                .http_mounts
                .unwrap_or_default()
                .into_iter()
                .map(|(k, v)| anyhow::Ok((k.parse()?, v.parse()?)))
                .try_collect()?,
            http_prefix: m.http_prefix.map(|s| s.parse()).transpose()?,
            exports,
            env_vars,
        })
    }
}

impl TryFrom<ComponentDefinitionType> for SerializedComponentDefinitionType {
    type Error = anyhow::Error;

    fn try_from(t: ComponentDefinitionType) -> anyhow::Result<Self> {
        Ok(match t {
            ComponentDefinitionType::App => Self::App {},
            ComponentDefinitionType::ChildComponent { name, args } => Self::ChildComponent {
                name: name.to_string(),
                args: args
                    .into_iter()
                    .map(|(k, v)| anyhow::Ok((String::from(k), v.try_into()?)))
                    .try_collect()?,
            },
        })
    }
}

impl TryFrom<SerializedComponentDefinitionType> for ComponentDefinitionType {
    type Error = anyhow::Error;

    fn try_from(t: SerializedComponentDefinitionType) -> anyhow::Result<Self> {
        Ok(match t {
            SerializedComponentDefinitionType::App {} => Self::App,
            SerializedComponentDefinitionType::ChildComponent { name, args } => {
                Self::ChildComponent {
                    name: name.parse()?,
                    args: args
                        .into_iter()
                        .map(|(k, v)| anyhow::Ok((k.parse()?, v.try_into()?)))
                        .try_collect()?,
                }
            },
        })
    }
}

impl TryFrom<ComponentInstantiation> for SerializedComponentInstantiation {
    type Error = anyhow::Error;

    fn try_from(i: ComponentInstantiation) -> anyhow::Result<Self> {
        Ok(Self {
            name: String::from(i.name),
            path: String::from(i.path),
            args: i
                .args
                .map(|args| {
                    args.into_iter()
                        .map(|(k, v)| anyhow::Ok((String::from(k), v.try_into()?)))
                        .try_collect()
                })
                .transpose()?,
            env: i
                .env
                .into_iter()
                .map(|(k, v)| anyhow::Ok((String::from(k), v.try_into()?)))
                .try_collect()?,
        })
    }
}

impl TryFrom<SerializedComponentInstantiation> for ComponentInstantiation {
    type Error = anyhow::Error;

    fn try_from(i: SerializedComponentInstantiation) -> anyhow::Result<Self> {
        Ok(Self {
            name: i.name.parse()?,
            path: i.path.parse()?,
            args: i
                .args
                .map(|args| {
                    args.into_iter()
                        .map(|(k, v)| anyhow::Ok((k.parse()?, v.try_into()?)))
                        .try_collect()
                })
                .transpose()?,
            env: i
                .env
                .into_iter()
                .map(|(k, v)| anyhow::Ok((k.parse()?, v.try_into()?)))
                .try_collect()?,
        })
    }
}

impl TryFrom<ComponentArgumentValidator> for SerializedComponentArgumentValidator {
    type Error = anyhow::Error;

    fn try_from(r: ComponentArgumentValidator) -> anyhow::Result<Self> {
        Ok(match r {
            ComponentArgumentValidator::Value(v) => SerializedComponentArgumentValidator::Value {
                value: v.json_serialize()?,
            },
        })
    }
}

impl TryFrom<SerializedComponentArgumentValidator> for ComponentArgumentValidator {
    type Error = anyhow::Error;

    fn try_from(r: SerializedComponentArgumentValidator) -> anyhow::Result<Self> {
        Ok(match r {
            SerializedComponentArgumentValidator::Value { value: v } => {
                ComponentArgumentValidator::Value(Validator::json_deserialize(&v)?)
            },
        })
    }
}

impl TryFrom<EnvVarValidator> for SerializedEnvVarValidator {
    type Error = anyhow::Error;

    fn try_from(v: EnvVarValidator) -> anyhow::Result<Self> {
        Ok(Self::Value {
            value: v.validator.json_serialize()?,
            optional: if v.optional { Some(true) } else { None },
        })
    }
}

impl TryFrom<SerializedEnvVarValidator> for EnvVarValidator {
    type Error = anyhow::Error;

    fn try_from(v: SerializedEnvVarValidator) -> anyhow::Result<Self> {
        let SerializedEnvVarValidator::Value { value, optional } = v;
        Ok(Self {
            validator: Validator::json_deserialize(&value)?,
            optional: optional.unwrap_or(false),
        })
    }
}

impl TryFrom<ComponentArgument> for SerializedComponentArgument {
    type Error = anyhow::Error;

    fn try_from(r: ComponentArgument) -> anyhow::Result<Self> {
        Ok(match r {
            ComponentArgument::Value(v) => SerializedComponentArgument::Value {
                value: v.json_serialize()?,
            },
        })
    }
}

impl TryFrom<SerializedComponentArgument> for ComponentArgument {
    type Error = anyhow::Error;

    fn try_from(r: SerializedComponentArgument) -> anyhow::Result<Self> {
        Ok(match r {
            SerializedComponentArgument::Value { value: v } => ComponentArgument::Value(
                ConvexValue::try_from(serde_json::from_str::<JsonValue>(&v)?)?,
            ),
        })
    }
}

impl TryFrom<EnvBinding> for SerializedEnvBinding {
    type Error = anyhow::Error;

    fn try_from(b: EnvBinding) -> anyhow::Result<Self> {
        Ok(match b {
            EnvBinding::Value(s) => SerializedEnvBinding::Value { value: s },
            EnvBinding::EnvVar(name) => SerializedEnvBinding::EnvVar {
                name: String::from(name),
            },
        })
    }
}

impl TryFrom<SerializedEnvBinding> for EnvBinding {
    type Error = anyhow::Error;

    fn try_from(b: SerializedEnvBinding) -> anyhow::Result<Self> {
        Ok(match b {
            SerializedEnvBinding::Value { value } => EnvBinding::Value(value),
            SerializedEnvBinding::EnvVar { name } => EnvBinding::EnvVar(name.parse()?),
        })
    }
}

impl TryFrom<ComponentExport> for SerializedComponentExport {
    type Error = anyhow::Error;

    fn try_from(e: ComponentExport) -> anyhow::Result<Self> {
        Ok(match e {
            ComponentExport::Branch(b) => SerializedComponentExport::Branch {
                branch: b
                    .into_iter()
                    .map(|(k, v)| anyhow::Ok((k.to_string(), v.try_into()?)))
                    .try_collect()?,
            },
            ComponentExport::Leaf(f) => SerializedComponentExport::Leaf {
                leaf: String::from(f),
            },
        })
    }
}

impl TryFrom<SerializedComponentExport> for ComponentExport {
    type Error = anyhow::Error;

    fn try_from(e: SerializedComponentExport) -> anyhow::Result<Self> {
        Ok(match e {
            SerializedComponentExport::Branch { branch: b } => ComponentExport::Branch(
                b.into_iter()
                    .map(|(k, v)| anyhow::Ok((k.parse()?, v.try_into()?)))
                    .try_collect()?,
            ),
            SerializedComponentExport::Leaf { leaf: f } => ComponentExport::Leaf(f.parse()?),
        })
    }
}

codegen_convex_serialization!(
    ComponentDefinitionMetadata,
    SerializedComponentDefinitionMetadata
);
