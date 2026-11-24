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
use sync_types::{
    path::PathComponent,
    CanonicalizedUdfPath,
};
use value::{
    codegen_convex_serialization,
    heap_size::HeapSize,
    identifier::Identifier,
    ConvexValue,
    FieldName,
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

    /// The path to the component's init function
    pub init: Option<CanonicalizedUdfPath>,
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
            init: None,
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
    ChildComponent { name: ComponentName },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ComponentInstantiation {
    pub name: ComponentName,
    pub path: ComponentDefinitionPath,
    pub env: BTreeMap<Identifier, EnvBinding>,
    pub args: BTreeMap<Identifier, ComponentArgument>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentExport {
    Branch(BTreeMap<PathComponent, ComponentExport>),
    Leaf(Reference),
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

/// An argument a parent component passes to its child component's init
/// function. References resolve in the parent component.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ComponentArgument {
    Value(ConvexValue),
    /// A reference to a function in the parent component
    Function(CanonicalizedUdfPath),
    /// A reference to a child of the parent component. Equivalent to
    /// `Reference::ChildComponent`.
    ChildComponent {
        component: ComponentName,
        attributes: Vec<PathComponent>,
    },
    /// One of the parent's own arguments, forwarded to the child
    ParentArgument(Identifier),
    /// An object whose fields may hold references or parent arguments
    Object(BTreeMap<FieldName, ComponentArgument>),
    /// An array whose elements may hold references or parent arguments
    Array(Vec<ComponentArgument>),
}

impl TryFrom<Reference> for ComponentArgument {
    type Error = anyhow::Error;

    fn try_from(reference: Reference) -> anyhow::Result<Self> {
        match reference {
            Reference::Function(path) => Ok(Self::Function(path)),
            Reference::ChildComponent {
                component,
                attributes,
            } => Ok(Self::ChildComponent {
                component,
                attributes,
            }),
            Reference::CurrentSystemUdfInComponent { .. } => {
                anyhow::bail!("Invalid component argument reference: {reference:?}")
            },
        }
    }
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
    #[serde(default)]
    init: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SerializedComponentDefinitionType {
    App {},
    ChildComponent {
        name: String,
        #[serde(default)]
        #[deprecated]
        args: Vec<(String, JsonValue)>,
    },
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
    Value {
        value: String,
    },
    Reference {
        reference: String,
    },
    ParentArgument {
        name: String,
    },
    Object {
        fields: Vec<(String, SerializedComponentArgument)>,
    },
    Array {
        elements: Vec<SerializedComponentArgument>,
    },
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
    /// CLIs up to 1.38 validate this field as `z.nullable(...)` in the push
    /// analysis response and reject it when the key is absent.
    #[serde(default)]
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
            init: m.init.map(String::from),
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
            init: m.init.map(|s| s.parse()).transpose()?,
        })
    }
}

impl TryFrom<ComponentDefinitionType> for SerializedComponentDefinitionType {
    type Error = anyhow::Error;

    fn try_from(t: ComponentDefinitionType) -> anyhow::Result<Self> {
        Ok(match t {
            ComponentDefinitionType::App => Self::App {},
            ComponentDefinitionType::ChildComponent { name } => Self::ChildComponent {
                name: name.to_string(),
                #[allow(deprecated)]
                args: vec![],
            },
        })
    }
}

impl TryFrom<SerializedComponentDefinitionType> for ComponentDefinitionType {
    type Error = anyhow::Error;

    fn try_from(t: SerializedComponentDefinitionType) -> anyhow::Result<Self> {
        Ok(match t {
            SerializedComponentDefinitionType::App {} => Self::App,
            SerializedComponentDefinitionType::ChildComponent { name, .. } => {
                Self::ChildComponent {
                    name: name.parse()?,
                }
            },
        })
    }
}

impl ComponentInstantiation {
    /// The bytes `args` occupies in the stored definition document.
    pub fn serialized_args_size(&self) -> anyhow::Result<usize> {
        let args: Vec<(String, SerializedComponentArgument)> = self
            .args
            .iter()
            .map(|(k, v)| anyhow::Ok((String::from(k.clone()), v.clone().try_into()?)))
            .try_collect()?;
        Ok(serde_json::to_vec(&args)?.len())
    }
}

impl TryFrom<ComponentInstantiation> for SerializedComponentInstantiation {
    type Error = anyhow::Error;

    fn try_from(i: ComponentInstantiation) -> anyhow::Result<Self> {
        Ok(Self {
            name: String::from(i.name),
            path: String::from(i.path),
            env: i
                .env
                .into_iter()
                .map(|(k, v)| anyhow::Ok((String::from(k), v.try_into()?)))
                .try_collect()?,
            args: Some(
                i.args
                    .into_iter()
                    .map(|(k, v)| anyhow::Ok((String::from(k), v.try_into()?)))
                    .try_collect()?,
            ),
        })
    }
}

impl TryFrom<SerializedComponentInstantiation> for ComponentInstantiation {
    type Error = anyhow::Error;

    fn try_from(i: SerializedComponentInstantiation) -> anyhow::Result<Self> {
        Ok(Self {
            name: i.name.parse()?,
            path: i.path.parse()?,
            env: i
                .env
                .into_iter()
                .map(|(k, v)| anyhow::Ok((k.parse()?, v.try_into()?)))
                .try_collect()?,
            args: i
                .args
                .unwrap_or_default()
                .into_iter()
                .map(|(k, v)| anyhow::Ok((k.parse()?, v.try_into()?)))
                .try_collect()?,
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

    fn try_from(a: ComponentArgument) -> anyhow::Result<Self> {
        Ok(match a {
            ComponentArgument::Value(v) => SerializedComponentArgument::Value {
                value: v.json_serialize()?,
            },
            ComponentArgument::Function(path) => SerializedComponentArgument::Reference {
                reference: String::from(Reference::Function(path)),
            },
            ComponentArgument::ChildComponent {
                component,
                attributes,
            } => SerializedComponentArgument::Reference {
                reference: String::from(Reference::ChildComponent {
                    component,
                    attributes,
                }),
            },
            ComponentArgument::ParentArgument(name) => {
                SerializedComponentArgument::ParentArgument {
                    name: String::from(name),
                }
            },
            ComponentArgument::Object(fields) => SerializedComponentArgument::Object {
                fields: fields
                    .into_iter()
                    .map(|(k, v)| anyhow::Ok((k.to_string(), v.try_into()?)))
                    .try_collect()?,
            },
            ComponentArgument::Array(elements) => SerializedComponentArgument::Array {
                elements: elements.into_iter().map(TryInto::try_into).try_collect()?,
            },
        })
    }
}

impl TryFrom<SerializedComponentArgument> for ComponentArgument {
    type Error = anyhow::Error;

    fn try_from(a: SerializedComponentArgument) -> anyhow::Result<Self> {
        Ok(match a {
            SerializedComponentArgument::Value { value } => ComponentArgument::Value(
                ConvexValue::try_from(serde_json::from_str::<JsonValue>(&value)?)?,
            ),
            SerializedComponentArgument::Reference { reference } => {
                reference.parse::<Reference>()?.try_into()?
            },
            SerializedComponentArgument::ParentArgument { name } => {
                ComponentArgument::ParentArgument(name.parse()?)
            },
            SerializedComponentArgument::Object { fields } => ComponentArgument::Object(
                fields
                    .into_iter()
                    .map(|(k, v)| anyhow::Ok((k.parse()?, v.try_into()?)))
                    .try_collect()?,
            ),
            SerializedComponentArgument::Array { elements } => {
                ComponentArgument::Array(elements.into_iter().map(TryInto::try_into).try_collect()?)
            },
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
