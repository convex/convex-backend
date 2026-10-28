use anyhow::anyhow;
use deno_core::ModuleSpecifier;
use sync_types::CanonicalizedModulePath;

use crate::isolate::CONVEX_SCHEME;

pub fn module_specifier_from_path(
    path: &CanonicalizedModulePath,
) -> anyhow::Result<ModuleSpecifier> {
    let url = format!("{CONVEX_SCHEME}:/{}", path.as_str());
    Ok(ModuleSpecifier::parse(&url)?)
}

pub fn module_specifier_from_str(path: &str) -> anyhow::Result<ModuleSpecifier> {
    Ok(ModuleSpecifier::parse(path)?)
}

pub fn path_from_module_specifier(
    spec: &ModuleSpecifier,
) -> anyhow::Result<CanonicalizedModulePath> {
    let spec_str = spec.as_str();
    let prefix = format!("{CONVEX_SCHEME}:/");
    spec_str
        .starts_with(&prefix)
        .then(|| {
            spec_str[prefix.len()..]
                .to_string()
                .parse::<CanonicalizedModulePath>()
        })
        .transpose()?
        .ok_or_else(|| anyhow!("module specifier did not start with {}", prefix))
}
