use std::{
    collections::BTreeMap,
    ops::Deref,
    str::FromStr,
    sync::Arc,
};

use async_lru::async_lru::SizedValue;
use common::{
    http::RoutedHttpPath,
    json::JsonForm as _,
    types::{
        HttpActionRoute,
        RoutableMethod,
        UdfType,
    },
};
use errors::ErrorMetadata;
use packed_value::StringBuffer;
use serde::{
    Deserialize,
    Serialize,
};
use sync_types::{
    CanonicalizedModulePath,
    FunctionName,
};
use value::heap_size::{
    HeapSize,
    WithHeapSize,
};

use super::function_validators::{
    ArgsValidator,
    ReturnsValidator,
};
use crate::cron_jobs::types::{
    CronIdentifier,
    CronSpec,
    SerializedCronSpec,
};

/// User-specified JavaScript source code for a module.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModuleSource {
    source: Arc<str>,
}

impl ModuleSource {
    pub fn new(source: &str) -> Self {
        Self {
            source: source.into(),
        }
    }
}

impl Deref for ModuleSource {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.source
    }
}

impl HeapSize for ModuleSource {
    fn heap_size(&self) -> usize {
        self.source.len()
    }
}

impl From<&str> for ModuleSource {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

/// Bundler-generated source map for a `ModuleSource`.
pub type SourceMap = Arc<str>;

#[derive(Debug, Clone)]
pub struct FullModuleSource {
    pub source: ModuleSource,
    pub source_map: Option<SourceMap>,
}

impl SizedValue for FullModuleSource {
    fn size(&self) -> u64 {
        (self.source.heap_size() + self.source_map.heap_size()) as u64
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AnalyzedModule {
    pub functions: WithHeapSize<Vec<AnalyzedFunction>>,
    pub http_routes: Option<AnalyzedHttpRoutes>,
    pub cron_specs: Option<WithHeapSize<BTreeMap<CronIdentifier, CronSpec>>>,
    /// Index of the module's original source in the source map.
    pub source_index: Option<u32>,
    /// Whether to keep around the JS context after running a function from this
    /// module. If true, state will non-deterministically leak from UDF to UDF
    /// (e.g. on the global object or module attributes).
    ///
    /// This is experimental for now and the reuse isn't guaranteed to happen.
    pub reuse_context: bool,
}

impl HeapSize for AnalyzedModule {
    fn heap_size(&self) -> usize {
        self.functions.heap_size()
            + self.http_routes.heap_size()
            + self.cron_specs.heap_size()
            + self.source_index.heap_size()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedAnalyzedModule {
    functions: Vec<SerializedAnalyzedFunction>,
    http_routes: Option<Vec<SerializedAnalyzedHttpRoute>>,
    cron_specs: Option<Vec<SerializedNamedCronSpec>>,
    source_mapped: Option<SerializedMappedModule>,
    #[serde(default)]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    reuse_context: bool,
}

impl TryFrom<AnalyzedModule> for SerializedAnalyzedModule {
    type Error = anyhow::Error;

    fn try_from(m: AnalyzedModule) -> anyhow::Result<Self> {
        let source_mapped = m
            .source_index
            .as_ref()
            .map(|_source_mapped| SerializedMappedModule::try_from(m.clone()))
            .transpose()?;
        Ok(Self {
            functions: m
                .functions
                .into_iter()
                .map(TryFrom::try_from)
                .try_collect()?,
            http_routes: m
                .http_routes
                .map(|routes| routes.into_iter().map(TryFrom::try_from).try_collect())
                .transpose()?,
            cron_specs: m
                .cron_specs
                .map(|specs| specs.into_iter().map(TryFrom::try_from).try_collect())
                .transpose()?,
            source_mapped,
            reuse_context: m.reuse_context,
        })
    }
}

impl TryFrom<SerializedAnalyzedModule> for AnalyzedModule {
    type Error = anyhow::Error;

    fn try_from(m: SerializedAnalyzedModule) -> anyhow::Result<Self> {
        Ok(Self {
            functions: m
                .functions
                .into_iter()
                .map(TryFrom::try_from)
                .try_collect()?,
            http_routes: m
                .http_routes
                .map(|routes| {
                    let routes = routes.into_iter().map(TryFrom::try_from).try_collect()?;
                    anyhow::Ok(AnalyzedHttpRoutes::new(routes))
                })
                .transpose()?,
            cron_specs: m
                .cron_specs
                .map(|specs| specs.into_iter().map(TryFrom::try_from).try_collect())
                .transpose()?,
            source_index: m
                .source_mapped
                .and_then(|mapped_module| mapped_module.source_index),
            reuse_context: m.reuse_context,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SerializedNamedCronSpec {
    identifier: String,
    spec: SerializedCronSpec,
}

impl TryFrom<(CronIdentifier, CronSpec)> for SerializedNamedCronSpec {
    type Error = anyhow::Error;

    fn try_from((identifier, spec): (CronIdentifier, CronSpec)) -> anyhow::Result<Self> {
        Ok(Self {
            identifier: identifier.to_string(),
            spec: SerializedCronSpec::try_from(spec)?,
        })
    }
}

impl TryFrom<SerializedNamedCronSpec> for (CronIdentifier, CronSpec) {
    type Error = anyhow::Error;

    fn try_from(s: SerializedNamedCronSpec) -> anyhow::Result<Self> {
        Ok((s.identifier.parse()?, CronSpec::try_from(s.spec)?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Visibility {
    Public,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub struct AnalyzedSourcePosition {
    pub path: CanonicalizedModulePath,
    pub start_lineno: u32,
    pub start_col: u32,
    // Consider adding end_* in the future
}

impl HeapSize for AnalyzedSourcePosition {
    fn heap_size(&self) -> usize {
        self.path.as_str().heap_size() + self.start_col.heap_size() + self.start_lineno.heap_size()
    }
}

#[derive(Serialize, Deserialize)]
// NOTE: serde not renamed to camelCase.
struct SerializedAnalyzedSourcePosition {
    path: String,
    start_lineno: u32,
    start_col: u32,
}

impl TryFrom<AnalyzedSourcePosition> for SerializedAnalyzedSourcePosition {
    type Error = anyhow::Error;

    fn try_from(p: AnalyzedSourcePosition) -> anyhow::Result<Self> {
        Ok(Self {
            path: p.path.as_str().to_string(),
            start_lineno: p.start_lineno,
            start_col: p.start_col,
        })
    }
}

impl TryFrom<SerializedAnalyzedSourcePosition> for AnalyzedSourcePosition {
    type Error = anyhow::Error;

    fn try_from(p: SerializedAnalyzedSourcePosition) -> anyhow::Result<Self> {
        Ok(Self {
            path: p.path.parse()?,
            start_lineno: p.start_lineno,
            start_col: p.start_col,
        })
    }
}

pub fn invalid_function_name_error(
    path: &CanonicalizedModulePath,
    e: &anyhow::Error,
) -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "InvalidFunctionName",
        format!("Invalid function name used in `{}`: {}", path.as_str(), e),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzedFunction {
    pub name: FunctionName,
    pub pos: Option<AnalyzedSourcePosition>,
    pub udf_type: UdfType,
    pub visibility: Option<Visibility>,

    // Leave args and returns unparsed to avoid performance overhead in common
    // case of reading ModuleMetadata without needing to validate the function.

    // JSON-serialized ArgsValidator
    // Note that we use `StringBuffer` so that this can be a refcounted slice
    // into the `PackedDocument` stored in the `ModulesTable` memory index
    pub args_str: Option<StringBuffer>,
    // JSON-serialized ReturnsValidator
    pub returns_str: Option<StringBuffer>,
}

impl AnalyzedFunction {
    pub fn new(
        name: FunctionName,
        pos: Option<AnalyzedSourcePosition>,
        udf_type: UdfType,
        visibility: Option<Visibility>,
        args: ArgsValidator,
        returns: ReturnsValidator,
    ) -> anyhow::Result<Self> {
        let args_json = args.json_serialize()?;
        let returns_json = returns.json_serialize()?;
        Ok(Self {
            name,
            pos,
            udf_type,
            visibility,
            args_str: Some(StringBuffer::new(args_json)),
            returns_str: Some(StringBuffer::new(returns_json)),
        })
    }

    pub fn args(&self) -> anyhow::Result<ArgsValidator> {
        match &self.args_str {
            Some(args) => ArgsValidator::json_deserialize(args),
            None => Ok(ArgsValidator::Unvalidated),
        }
    }

    pub fn returns(&self) -> anyhow::Result<ReturnsValidator> {
        match &self.returns_str {
            Some(returns) => ReturnsValidator::json_deserialize(returns),
            None => Ok(ReturnsValidator::Unvalidated),
        }
    }
}

impl HeapSize for AnalyzedFunction {
    fn heap_size(&self) -> usize {
        self.name.heap_size()
            + self.pos.heap_size()
            + self.args_str.heap_size()
            + self.returns_str.heap_size()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedAnalyzedFunction {
    name: String,
    pos: Option<SerializedAnalyzedSourcePosition>,
    udf_type: String,
    visibility: Option<Visibility>,
    args: Option<StringBuffer>,
    returns: Option<StringBuffer>,
}

impl TryFrom<AnalyzedFunction> for SerializedAnalyzedFunction {
    type Error = anyhow::Error;

    fn try_from(f: AnalyzedFunction) -> anyhow::Result<Self> {
        Ok(Self {
            name: f.name.to_string(),
            pos: f.pos.map(TryFrom::try_from).transpose()?,
            udf_type: f.udf_type.to_string(),
            visibility: f.visibility,
            args: f.args_str,
            returns: f.returns_str,
        })
    }
}

impl TryFrom<SerializedAnalyzedFunction> for AnalyzedFunction {
    type Error = anyhow::Error;

    fn try_from(f: SerializedAnalyzedFunction) -> anyhow::Result<Self> {
        Ok(Self {
            name: FunctionName::from_str(&f.name)?,
            pos: f.pos.map(AnalyzedSourcePosition::try_from).transpose()?,
            udf_type: f.udf_type.parse()?,
            visibility: f.visibility,
            args_str: f.args,
            returns_str: f.returns,
        })
    }
}

mod codegen_analyzed_function {
    use value::codegen_convex_serialization;

    use super::{
        AnalyzedFunction,
        SerializedAnalyzedFunction,
    };

    codegen_convex_serialization!(AnalyzedFunction, SerializedAnalyzedFunction);
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SerializedHttpActionRoute {
    path: String,
    method: String,
}

impl TryFrom<HttpActionRoute> for SerializedHttpActionRoute {
    type Error = anyhow::Error;

    fn try_from(r: HttpActionRoute) -> anyhow::Result<Self> {
        Ok(Self {
            path: r.path,
            method: r.method.to_string(),
        })
    }
}

impl TryFrom<SerializedHttpActionRoute> for HttpActionRoute {
    type Error = anyhow::Error;

    fn try_from(r: SerializedHttpActionRoute) -> anyhow::Result<Self> {
        Ok(Self {
            path: r.path.parse()?,
            method: r.method.parse()?,
            matched: true,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzedHttpRoute {
    pub route: HttpActionRoute,
    pub pos: Option<AnalyzedSourcePosition>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SerializedAnalyzedHttpRoute {
    route: SerializedHttpActionRoute,
    pos: Option<SerializedAnalyzedSourcePosition>,
}

impl HeapSize for AnalyzedHttpRoute {
    fn heap_size(&self) -> usize {
        self.route.heap_size() + self.pos.heap_size()
    }
}

impl TryFrom<AnalyzedHttpRoute> for SerializedAnalyzedHttpRoute {
    type Error = anyhow::Error;

    fn try_from(r: AnalyzedHttpRoute) -> anyhow::Result<Self> {
        Ok(Self {
            route: SerializedHttpActionRoute::try_from(r.route)?,
            pos: r.pos.map(TryFrom::try_from).transpose()?,
        })
    }
}

impl TryFrom<SerializedAnalyzedHttpRoute> for AnalyzedHttpRoute {
    type Error = anyhow::Error;

    fn try_from(r: SerializedAnalyzedHttpRoute) -> anyhow::Result<Self> {
        Ok(Self {
            route: HttpActionRoute::try_from(r.route)?,
            pos: r.pos.map(AnalyzedSourcePosition::try_from).transpose()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AnalyzedHttpRoutes {
    routes: WithHeapSize<Vec<AnalyzedHttpRoute>>,
}

impl AnalyzedHttpRoutes {
    pub fn new(routes: Vec<AnalyzedHttpRoute>) -> Self {
        Self {
            routes: routes.into(),
        }
    }

    pub fn route_exact(&self, path: &str, method: RoutableMethod) -> bool {
        self.routes.iter().any(|AnalyzedHttpRoute { route, .. }| {
            if route.path.ends_with('*') {
                return false;
            }
            route.method == method && &route.path[..] == path
        })
    }

    pub fn route_prefix(
        &self,
        path: &RoutedHttpPath,
        method: RoutableMethod,
    ) -> Option<RoutedHttpPath> {
        let mut longest_match: Option<RoutedHttpPath> = None;
        for AnalyzedHttpRoute { route, .. } in &self.routes {
            if route.method != method {
                continue;
            }
            let Some(mut prefix_path) = route.path.strip_suffix('*') else {
                continue;
            };
            if prefix_path.is_empty() {
                prefix_path = "/";
            }
            let Some(match_suffix) = path.strip_prefix(prefix_path) else {
                continue;
            };
            let new_match = RoutedHttpPath(format!("/{match_suffix}"));
            if let Some(ref existing_suffix) = longest_match {
                // If the existing longest match has a shorter suffix, then it
                // matches a longer prefix.
                if existing_suffix.len() < match_suffix.len() {
                    continue;
                }
            }
            longest_match = Some(new_match);
        }
        longest_match
    }
}

impl HeapSize for AnalyzedHttpRoutes {
    fn heap_size(&self) -> usize {
        self.routes.heap_size()
    }
}

impl IntoIterator for AnalyzedHttpRoutes {
    type IntoIter = Box<dyn Iterator<Item = AnalyzedHttpRoute>>;
    type Item = AnalyzedHttpRoute;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.routes.into_iter())
    }
}

impl Deref for AnalyzedHttpRoutes {
    type Target = [AnalyzedHttpRoute];

    fn deref(&self) -> &Self::Target {
        &self.routes
    }
}

// TODO: consider denormalizing SerializedMappedModule into
// SerializedAnalyzedModule and  instead just include source information. This
// requires a decent migration from Dashboard  schema.
//  See https://github.com/convex/convex/pull/14382/files#r1252372646 for further discussion.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SerializedMappedModule {
    source_index: Option<u32>,
    functions: Vec<SerializedAnalyzedFunction>,
    http_routes: Option<Vec<SerializedAnalyzedHttpRoute>>,
    cron_specs: Option<Vec<SerializedNamedCronSpec>>,
}

impl TryFrom<AnalyzedModule> for SerializedMappedModule {
    type Error = anyhow::Error;

    fn try_from(m: AnalyzedModule) -> anyhow::Result<Self> {
        anyhow::ensure!(
            m.source_index.is_some(),
            "source_index must be set to be serializing into SerializedMappedModule"
        );
        Ok(Self {
            source_index: m.source_index,
            functions: m
                .functions
                .into_iter()
                .map(TryFrom::try_from)
                .try_collect()?,
            http_routes: m
                .http_routes
                .map(|routes| routes.into_iter().map(TryFrom::try_from).try_collect())
                .transpose()?,
            cron_specs: m
                .cron_specs
                .map(|specs| specs.into_iter().map(TryFrom::try_from).try_collect())
                .transpose()?,
        })
    }
}
