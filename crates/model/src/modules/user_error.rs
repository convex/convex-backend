use thiserror::Error;

#[derive(Debug, Error)]
#[error("{msg}")]
pub struct ModuleNotFoundError {
    msg: String,
}

impl ModuleNotFoundError {
    pub fn new(module_path: &str) -> Self {
        let msg = format!(
            "Couldn't find JavaScript module '{module_path}'. Did you forget to run `npx convex \
             dev`?",
        );
        Self { msg }
    }
}

#[derive(Debug, Error)]
#[error("{msg}")]
pub struct FunctionNotFoundError {
    msg: String,
}

impl FunctionNotFoundError {
    pub fn new(function_name: &str, module_path: &str) -> Self {
        let msg = format!(r#"Couldn't find {function_name:?} in module {module_path:?}."#);
        Self { msg }
    }
}

#[derive(Debug, Error)]
#[error("{msg}")]
pub struct SystemModuleNotFoundError {
    msg: String,
}

impl SystemModuleNotFoundError {
    pub fn new(module_path: &str) -> Self {
        let msg = format!("Couldn't find system module '{module_path:?}'.");
        Self { msg }
    }
}
