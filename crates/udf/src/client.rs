use std::collections::BTreeMap;

use common::{
    bootstrap_model::components::definition::ComponentDefinitionMetadata,
    components::ComponentDefinitionPath,
    errors::JsError,
};
use pb::common::{
    function_result::Result as FunctionResultTypeProto,
    FunctionResult as FunctionResultProto,
};
use value::JsonPackedValue;

pub type EvaluateAppDefinitionsResult =
    BTreeMap<ComponentDefinitionPath, ComponentDefinitionMetadata>;

#[derive(Clone, Debug)]
pub struct FunctionResult {
    pub result: Result<JsonPackedValue, JsError>,
}

impl TryFrom<FunctionResultProto> for FunctionResult {
    type Error = anyhow::Error;

    fn try_from(result: FunctionResultProto) -> anyhow::Result<Self> {
        let result = match result.result {
            Some(FunctionResultTypeProto::JsonPackedValue(value)) => {
                Ok(JsonPackedValue::from_network(value)?)
            },
            Some(FunctionResultTypeProto::JsError(js_error)) => Err(js_error.try_into()?),
            None => anyhow::bail!("Missing result"),
        };
        Ok(FunctionResult { result })
    }
}

impl TryFrom<FunctionResult> for FunctionResultProto {
    type Error = anyhow::Error;

    fn try_from(result: FunctionResult) -> anyhow::Result<Self> {
        let result = match result.result {
            Ok(value) => FunctionResultTypeProto::JsonPackedValue(value.as_str().to_owned()),
            Err(js_error) => FunctionResultTypeProto::JsError(js_error.try_into()?),
        };
        Ok(FunctionResultProto {
            result: Some(result),
        })
    }
}
