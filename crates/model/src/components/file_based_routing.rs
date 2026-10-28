use std::collections::{
    btree_map::Entry,
    BTreeMap,
};

use common::{
    bootstrap_model::components::definition::ComponentExport,
    components::Reference,
};
use errors::ErrorMetadata;
use sync_types::{
    path::PathComponent,
    CanonicalizedModulePath,
    CanonicalizedUdfPath,
    FunctionName,
    ModulePath,
};

use crate::modules::module_versions::{
    AnalyzedModule,
    Visibility,
};

pub fn file_based_exports<'a>(
    functions: impl IntoIterator<Item = (&'a CanonicalizedModulePath, &'a AnalyzedModule)>,
) -> anyhow::Result<BTreeMap<PathComponent, ComponentExport>> {
    let mut exports = BTreeMap::new();
    for (module_path, module) in functions {
        let stripped = module_path.clone().strip();

        let identifiers = stripped
            .components()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ErrorMetadata::bad_request("InvalidModulePath", e.to_string()))?;
        for function in &module.functions {
            if function.visibility != Some(Visibility::Public) {
                continue;
            }
            let mut path = identifiers.clone();
            path.push(function.name.clone().into());
            let (last, prefix) = path.split_last().unwrap();

            let mut current = &mut exports;
            for identifier in prefix {
                let current_node = current
                    .entry(identifier.clone())
                    .or_insert_with(|| ComponentExport::Branch(BTreeMap::new()));
                current = match current_node {
                    ComponentExport::Branch(branch) => branch,
                    ComponentExport::Leaf(..) => anyhow::bail!(ErrorMetadata::bad_request(
                        "InvalidExport",
                        format!(
                            "Path {module_path:?}:{} conflicts with existing export",
                            function.name
                        )
                    )),
                }
            }
            match current.entry(last.clone()) {
                Entry::Vacant(e) => {
                    let path =
                        CanonicalizedUdfPath::new(module_path.clone(), function.name.clone());
                    let reference = Reference::Function(path);
                    e.insert(ComponentExport::Leaf(reference));
                },
                Entry::Occupied(_) => anyhow::bail!(ErrorMetadata::bad_request(
                    "InvalidExport",
                    format!(
                        "Path {module_path:?}:{} conflicts with existing export",
                        function.name
                    )
                )),
            }
        }
    }
    Ok(exports)
}

pub fn export_to_udf_path(attributes: &[PathComponent]) -> anyhow::Result<CanonicalizedUdfPath> {
    let Some((last, prefix)) = attributes.split_last() else {
        anyhow::bail!("Expected at least one path component");
    };
    let mut module_path = String::new();
    let mut first = true;
    for attribute in prefix {
        if !first {
            module_path.push('/');
        }
        module_path.push_str(&attribute[..]);
        first = false;
    }
    let module_path: ModulePath = module_path.parse()?;
    let name: FunctionName = last.parse()?;
    Ok(CanonicalizedUdfPath::new(module_path.canonicalize(), name))
}
