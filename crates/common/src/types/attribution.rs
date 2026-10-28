use serde::{
    Deserialize,
    Serialize,
};
use sync_types::CanonicalizedUdfPath;

use crate::{
    components::ComponentPath,
    types::{
        HttpActionRoute,
        UdfType,
    },
};

/// The wire format of the claims sent to the gateway.
///
/// All three are absent when we cannot identify the caller. A caller in the
/// root component has a `function_name` but no `component_path`.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttributionClaims {
    pub component_path: Option<String>,
    pub function_name: Option<String>,
    pub function_type: Option<String>,
}

impl AttributionClaims {
    pub fn validate(&self) -> Result<(), &'static str> {
        match (
            &self.component_path,
            &self.function_name,
            &self.function_type,
        ) {
            (None, None, None) | (_, Some(_), Some(_)) => Ok(()),
            _ => Err("functionName and functionType must be provided together"),
        }
    }

    /// An unknown caller.
    pub fn unknown() -> Self {
        Self {
            component_path: None,
            function_name: None,
            function_type: None,
        }
    }
}

/// What ran, as the executor knows it. An HTTP action is named by its route
/// because every route shares one router module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttributedCaller {
    Action {
        component_path: ComponentPath,
        udf_path: CanonicalizedUdfPath,
    },
    /// Only a matched route gets here; an unmatched request is answered
    /// before user code runs.
    HttpAction {
        component_path: ComponentPath,
        route: HttpActionRoute,
    },
}

impl AttributedCaller {
    pub fn component_path(&self) -> &ComponentPath {
        match self {
            AttributedCaller::Action { component_path, .. }
            | AttributedCaller::HttpAction { component_path, .. } => component_path,
        }
    }

    /// A function path, or `METHOD /path` for an HTTP action.
    pub fn name(&self) -> String {
        match self {
            AttributedCaller::Action { udf_path, .. } => udf_path.to_string(),
            AttributedCaller::HttpAction { route, .. } => route.to_string(),
        }
    }

    pub fn udf_type(&self) -> UdfType {
        match self {
            AttributedCaller::Action { .. } => UdfType::Action,
            AttributedCaller::HttpAction { .. } => UdfType::HttpAction,
        }
    }
}

impl From<AttributedCaller> for AttributionClaims {
    fn from(caller: AttributedCaller) -> Self {
        Self {
            component_path: caller.component_path().clone().serialize(),
            function_name: Some(caller.name()),
            function_type: Some(caller.udf_type().to_lowercase_string().to_owned()),
        }
    }
}
