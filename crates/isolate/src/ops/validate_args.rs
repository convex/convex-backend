use common::json::JsonForm as _;
use errors::ErrorMetadata;
use model::{
    modules::function_validators::ArgsValidator,
    virtual_system_mapping,
};
use serde_json::{
    json,
    Value as JsonValue,
};
use udf::helpers::UdfArgsJson;
use value::{
    serialized_args_ext::SerializedArgsExt,
    PendingValue,
};

use super::OpProvider;

#[convex_macro::op]
pub fn op_validate_args<P: OpProvider>(
    provider: &mut P,
    validator: String,
    args: String,
) -> anyhow::Result<JsonValue> {
    let args_validator = match ArgsValidator::json_deserialize(&validator) {
        Ok(v) => v,
        Err(json_error) => {
            let message = format!("Unable to parse JSON returned from `exportArgs`: {json_error}");
            return Err(anyhow::anyhow!(message));
        },
    };

    let args: UdfArgsJson = serde_json::from_str(&args)?;
    // Arguments may contain unresolved commit timestamps.
    let args_vec = args
        .into_serialized_args()?
        .into_args()?
        .into_iter()
        .map(PendingValue::from_uncommitted_json)
        .collect::<anyhow::Result<Vec<_>>>()
        // Arguments come from the caller, so a conversion failure (e.g. an array
        // over the length limit) is a user error.
        .map_err(|err| {
            anyhow::anyhow!(ErrorMetadata::bad_request(
                "InvalidArguments",
                format!("Invalid arguments: {err}"),
            ))
        })?;

    let table_mapping = provider.get_all_table_mappings()?;
    match args_validator.check_pending_args(args_vec, &table_mapping, virtual_system_mapping())? {
        Some(js_error) => Ok(json!({
            "valid": false,
            "message": format!("{}", js_error)
        })),
        None => Ok(json!({
            "valid": true,
        })),
    }
}
