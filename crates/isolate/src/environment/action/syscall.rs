#![allow(non_snake_case)]

use std::str::FromStr;

use common::runtime::Runtime;
use errors::ErrorMetadata;
use serde::Deserialize;
use serde_json::{
    json,
    Value as JsonValue,
};
use value::identifier::Identifier;

use super::ActionSyscallProvider;
use crate::environment::helpers::with_argument_error;

impl<RT: Runtime> ActionSyscallProvider<RT> {
    pub fn syscall_impl(&mut self, name: &str, args: JsonValue) -> anyhow::Result<JsonValue> {
        match name {
            "1.0/componentArgument" => syscall_component_argument(self, args),

            _ => {
                anyhow::bail!(ErrorMetadata::bad_request(
                    "UnknownOperation",
                    format!("Unknown operation {name}")
                ));
            },
        }
    }
}

fn syscall_component_argument<RT: Runtime>(
    provider: &mut ActionSyscallProvider<RT>,
    args: JsonValue,
) -> anyhow::Result<JsonValue> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ComponentArgumentArgs {
        name: String,
    }
    let arg_name = with_argument_error("componentArgument", || {
        let ComponentArgumentArgs { name } = serde_json::from_value(args)?;
        Ok(name)
    })?;
    let component_arguments = provider.phase.component_arguments()?;
    let value = match Identifier::from_str(&arg_name) {
        Ok(identifier) => component_arguments.get(&identifier).cloned(),
        Err(_) => None,
    };
    let result = match value {
        Some(value) => json!({ "value": value.to_internal_json() }),
        None => json!({}),
    };
    Ok(result)
}
