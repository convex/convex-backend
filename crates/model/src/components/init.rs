use std::collections::BTreeMap;

use async_recursion::async_recursion;
use common::{
    bootstrap_model::components::definition::{
        ComponentArgument,
        ComponentInstantiation,
    },
    components::{
        CanonicalizedComponentFunctionPath,
        ComponentPath,
        Resource,
    },
    runtime::Runtime,
    schemas::validator::ObjectValidator,
    types::UdfType,
};
use database::Transaction;
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use sync_types::CanonicalizedUdfPath;
use value::{
    ConvexArray,
    ConvexObject,
    ConvexValue,
    FieldName,
};

use super::{
    handles::{
        function_handle_not_found,
        FunctionHandlesModel,
    },
    type_checking::{
        CheckedComponent,
        ResourceTree,
    },
    types::EvaluatedComponentDefinition,
};
use crate::modules::{
    function_validators::ArgsValidator,
    module_versions::{
        AnalyzedFunction,
        Visibility,
    },
};

/// Get the component's init function, if it exists.
/// Returns an Error if the init function is invalid.
pub fn init_function<'a>(
    component_path: &ComponentPath,
    evaluated: &'a EvaluatedComponentDefinition,
) -> anyhow::Result<Option<(&'a CanonicalizedUdfPath, &'a AnalyzedFunction)>> {
    let Some(init_path) = &evaluated.definition.init else {
        return Ok(None);
    };
    let not_found = || {
        ErrorMetadata::bad_request(
            "InvalidInit",
            format!(
                "Component '{component_path}' declares {init_path} as its init, but that function \
                 doesn't exist."
            ),
        )
    };
    let module = evaluated
        .functions
        .get(init_path.module())
        .ok_or_else(not_found)?;
    let function = module
        .functions
        .iter()
        .find(|f| f.name == *init_path.function_name())
        .ok_or_else(not_found)?;
    // An init is declared as an internal mutation, but it runs as `UdfType::Init`
    if function.udf_type != UdfType::Mutation || function.visibility != Some(Visibility::Internal) {
        anyhow::bail!(ErrorMetadata::bad_request(
            "InvalidInit",
            format!(
                "Component '{component_path}' declares {init_path} as its init, but it isn't an \
                 internal mutation. Define it with `internalMutation`."
            ),
        ));
    }
    Ok(Some((init_path, function)))
}

fn invalid_component_argument(msg: String) -> ErrorMetadata {
    ErrorMetadata::bad_request("InvalidComponentArgs", msg)
}

/// The args validator of `component`'s init. Returns an empty object validator
/// if the component has no init.
pub fn init_args_validator(
    component_path: &ComponentPath,
    evaluated: &EvaluatedComponentDefinition,
) -> anyhow::Result<ArgsValidator> {
    let init = if component_path.is_root() {
        None
    } else {
        init_function(component_path, evaluated)?
    };
    match init {
        None => Ok(ArgsValidator::Validated(ObjectValidator(BTreeMap::new()))),
        Some((_, function)) => function.args(),
    }
}

pub async fn resolve_component_args<RT: Runtime>(
    tx: &mut Transaction<RT>,
    parent: &CheckedComponent,
    parent_args_validator: &ArgsValidator,
    parent_args: &ConvexObject,
    instantiation: &ComponentInstantiation,
) -> anyhow::Result<ConvexObject> {
    let mut fields = BTreeMap::new();
    for (arg_name, arg) in &instantiation.args {
        let value = resolve_component_arg(tx, parent, parent_args_validator, parent_args, arg)
            .await
            .map_err(|e| {
                e.wrap_error_message(|msg| {
                    format!(
                        "Invalid argument {arg_name} for component '{}': {msg}",
                        parent.component_path.push(instantiation.name.clone())
                    )
                })
            })?;
        if let Some(value) = value {
            fields.insert(FieldName::try_from(String::from(arg_name.clone()))?, value);
        }
    }
    ConvexObject::try_from(fields)
}

/// Returns `None` for forwarded parent arguments that are undefined.
#[async_recursion]
async fn resolve_component_arg<RT>(
    tx: &mut Transaction<RT>,
    parent: &CheckedComponent,
    parent_args_validator: &ArgsValidator,
    parent_args: &ConvexObject,
    arg: &ComponentArgument,
) -> anyhow::Result<Option<ConvexValue>>
where
    RT: Runtime,
{
    let value = match arg {
        ComponentArgument::Value(value) => value.clone(),
        ComponentArgument::Function(udf_path) => {
            function_handle(
                tx,
                CanonicalizedComponentFunctionPath {
                    component: parent.component_path.clone(),
                    udf_path: udf_path.clone(),
                },
            )
            .await?
        },
        ComponentArgument::ChildComponent {
            component,
            attributes,
        } => {
            let child = parent.child_components.get(component).ok_or_else(|| {
                let parent_path = &parent.component_path;
                let component = &**component;
                invalid_component_argument(if parent_path.is_root() {
                    format!("The app has no child component named '{component}'.")
                } else {
                    format!("Component '{parent_path}' has no child component named '{component}'.")
                })
            })?;
            let child_path = parent.component_path.push(component.clone());
            let export_path = attributes
                .iter()
                .map(|a| &**a)
                .collect::<Vec<_>>()
                .join(".");
            let export = child.resolve_export(attributes)?.ok_or_else(|| {
                invalid_component_argument(format!(
                    "Component '{child_path}' doesn't export '{export_path}'."
                ))
            })?;
            if matches!(&export, ResourceTree::Branch(children) if children.is_empty()) {
                anyhow::bail!(invalid_component_argument(if export_path.is_empty() {
                    format!("Component '{child_path}' doesn't export any functions.")
                } else {
                    format!(
                        "Component '{child_path}' doesn't export any functions under \
                         '{export_path}'."
                    )
                }));
            }
            resource_tree_to_value(tx, &export).await?
        },
        ComponentArgument::ParentArgument(arg_name) => {
            let field = FieldName::try_from(String::from(arg_name.clone()))?;
            if let Some(value) = parent_args.get(&field) {
                value.clone()
            } else {
                let declared = match parent_args_validator {
                    ArgsValidator::Unvalidated => true,
                    ArgsValidator::Validated(validator) => validator.0.contains_key(&**arg_name),
                };
                let parent_path = &parent.component_path;
                anyhow::ensure!(
                    declared,
                    invalid_component_argument(if parent_path.is_root() {
                        format!("The app has no argument named '{arg_name}'.")
                    } else {
                        format!("Component '{parent_path}' has no argument named '{arg_name}'.")
                    })
                );
                return Ok(None);
            }
        },
        ComponentArgument::Object(fields) => {
            let mut resolved = BTreeMap::new();
            for (field, arg) in fields {
                if let Some(value) =
                    resolve_component_arg(tx, parent, parent_args_validator, parent_args, arg)
                        .await?
                {
                    resolved.insert(field.clone(), value);
                }
            }
            ConvexValue::Object(ConvexObject::try_from(resolved)?)
        },
        ComponentArgument::Array(elements) => {
            let mut resolved = Vec::with_capacity(elements.len());
            for arg in elements {
                let Some(value) =
                    resolve_component_arg(tx, parent, parent_args_validator, parent_args, arg)
                        .await?
                else {
                    let ComponentArgument::ParentArgument(name) = arg else {
                        anyhow::bail!("Only a parent argument can resolve to nothing: {arg:?}");
                    };
                    anyhow::bail!(invalid_component_argument(format!(
                        "Parent argument '{name}' is unset and can't be included in an array."
                    )));
                };
                resolved.push(value);
            }
            ConvexValue::Array(ConvexArray::try_from(resolved)?)
        },
    };
    Ok(Some(value))
}

#[async_recursion]
async fn resource_tree_to_value<RT>(
    tx: &mut Transaction<RT>,
    export: &ResourceTree,
) -> anyhow::Result<ConvexValue>
where
    RT: Runtime,
{
    match export {
        ResourceTree::Leaf(Resource::Function(path)) => function_handle(tx, path.clone()).await,
        ResourceTree::Leaf(resource @ (Resource::Value(_) | Resource::ResolvedSystemUdf(_))) => {
            anyhow::bail!("Unexpected resource in a component's exports: {resource:?}")
        },
        ResourceTree::Branch(children) => {
            let mut fields = BTreeMap::new();
            for (name, child) in children {
                fields.insert(
                    String::from(name.clone()).parse::<FieldName>()?,
                    resource_tree_to_value(tx, child).await?,
                );
            }
            Ok(ConvexValue::Object(ConvexObject::try_from(fields)?))
        },
    }
}

async fn function_handle<RT: Runtime>(
    tx: &mut Transaction<RT>,
    path: CanonicalizedComponentFunctionPath,
) -> anyhow::Result<ConvexValue> {
    let handle = FunctionHandlesModel::new(tx)
        .get_with_component_path(path.clone())
        .await
        .map_err(|e| {
            if e.short_msg() == function_handle_not_found().short_msg {
                let udf_path = &path.udf_path;
                let component = &path.component;
                anyhow::anyhow!(invalid_component_argument(if component.is_root() {
                    format!("{udf_path} isn't a function in the app.")
                } else {
                    format!("{udf_path} isn't a function in component '{component}'.")
                }))
            } else {
                e
            }
        })?;
    Ok(ConvexValue::String(String::from(handle).try_into()?))
}
