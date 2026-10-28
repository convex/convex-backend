use std::{
    ffi::OsStr,
    fmt::Write as _,
    fs,
    io::Result,
    path::{
        Path,
        PathBuf,
    },
};

use prost::Message;
use tonic_prost_build::FileDescriptorSet;

fn find_packages(proto_dir: &Path) -> Result<Vec<String>> {
    let mut packages = vec![];
    for dent in std::fs::read_dir(proto_dir)? {
        let dent = dent?;
        let path = dent.path();
        if path.extension() == Some(OsStr::new("proto")) {
            let package_name = path.file_stem().unwrap().to_str().unwrap().to_owned();
            packages.push(package_name);
        }
    }
    Ok(packages)
}

pub fn pb_build(features: Vec<&'static str>, extra_includes: Vec<&'static str>) -> Result<()> {
    println!("cargo:rerun-if-changed=protos");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let mut packages = find_packages(Path::new("protos/"))?;
    let paths: Vec<_> = packages
        .iter()
        .map(|package| format!("protos/{package}.proto"))
        .collect();

    let mut external_paths = vec![];
    for include in &extra_includes {
        let include_path = Path::new(include);
        let crate_name = include_path
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        let mut packages = find_packages(include_path)?
            .into_iter()
            .map(|package| (format!(".{package}"), format!("::{crate_name}::{package}")))
            .collect();
        external_paths.append(&mut packages)
    }

    let mut includes = vec!["protos/".to_string()];
    includes.extend(extra_includes.into_iter().map(str::to_string));

    let descriptor_set_path = out_dir.join("descriptors.bin");
    let mut builder = tonic_prost_build::configure().file_descriptor_set_path(&descriptor_set_path);
    for (proto_path, rust_path) in external_paths {
        builder = builder.extern_path(proto_path, rust_path);
    }
    builder.compile_protos(&paths, &includes)?;

    // We sort the package names just so we're generating the lib.rs
    // deterministically to avoid NOOP commits.
    packages.sort();

    let mut mods = vec![];
    for m in std::fs::read_dir("src")? {
        let m = m?;
        let path = m.path();
        if path.extension() == Some(OsStr::new("rs")) && path.file_stem() != Some(OsStr::new("lib"))
        {
            let mod_name = path.file_stem().unwrap().to_str().unwrap().to_owned();
            mods.push(mod_name);
        }
    }
    mods.sort();

    // Read back the file descriptor set to codegen `ReflectionService` impls.
    let file_descriptor_set = FileDescriptorSet::decode(&std::fs::read(&descriptor_set_path)?[..])?;
    let mut extras = String::new();
    for file in file_descriptor_set.file {
        let Some(package_name) = file.package else {
            continue;
        };
        if !packages.contains(&package_name) {
            continue;
        }
        for service in file.service {
            let Some(service_name) = service.name else {
                continue;
            };
            let lower_name = naive_snake_case(&service_name);
            let server_mod = format!("crate::{package_name}::{lower_name}_server");
            write!(
                &mut extras,
                r#"impl<T> pb_extras::ReflectionService for {server_mod}::{service_name}Server<T> {{
    const METHODS: &[&str] = &[
"#
            )
            .unwrap();
            for method in service.method {
                let Some(method_name) = method.name else {
                    continue;
                };
                writeln!(&mut extras, "        {method_name:?},").unwrap();
            }
            write!(
                &mut extras,
                r#"    ];
}}
"#
            )
            .unwrap();
        }
    }

    std::fs::write(out_dir.join("_extras.rs"), extras)?;

    // Now let's build the lib.rs file.
    let mut lib_file_contents = String::new();
    lib_file_contents.push_str("// @generated - do not modify. Modify build.rs instead.\n");
    for feature in features {
        lib_file_contents.push_str(&format!("#![feature({feature})]\n"));
    }
    lib_file_contents.push_str("#![allow(clippy::match_single_binding)]\n");
    for m in mods {
        lib_file_contents.push_str(&format!("pub mod {m};\n"));
    }
    for package_name in packages {
        lib_file_contents.push_str(&format!(
            "pub mod {package_name} {{\n    include!(concat!(env!(\"OUT_DIR\"), \
             \"/{package_name}.rs\"));\n}}\n",
        ));
    }

    lib_file_contents.push_str(
        r#"
include!(concat!(env!("OUT_DIR"), "/_extras.rs"));
use std::sync::LazyLock;

use prost_reflect::DescriptorPool;

const FILE_DESCRIPTOR_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/descriptors.bin"));
pub static DESCRIPTOR_POOL: LazyLock<DescriptorPool> =
    LazyLock::new(|| DescriptorPool::decode(FILE_DESCRIPTOR_BYTES).unwrap());
"#,
    );

    let out_file = Path::new("src/lib.rs");
    if fs::read_to_string(out_file)? != lib_file_contents {
        fs::write(out_file, lib_file_contents)?;
    }

    Ok(())
}

// copied from `tonic-build`
fn naive_snake_case(name: &str) -> String {
    let mut s = String::new();
    let mut it = name.chars().peekable();

    while let Some(x) = it.next() {
        s.push(x.to_ascii_lowercase());
        if let Some(y) = it.peek()
            && y.is_uppercase()
        {
            s.push('_');
        }
    }

    s
}
