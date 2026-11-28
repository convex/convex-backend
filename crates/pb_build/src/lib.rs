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
    let packages = find_packages(Path::new("protos/"))?;
    let protos = packages
        .iter()
        .map(|package| PathBuf::from(format!("protos/{package}.proto")))
        .collect();

    let mut extern_paths = vec![];
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
        extern_paths.append(&mut packages)
    }

    let mut includes = vec![PathBuf::from("protos/")];
    includes.extend(extra_includes.into_iter().map(PathBuf::from));

    let mut modules = vec![];
    for m in std::fs::read_dir("src")? {
        let m = m?;
        let path = m.path();
        if path.extension() == Some(OsStr::new("rs")) && path.file_stem() != Some(OsStr::new("lib"))
        {
            let mod_name = path.file_stem().unwrap().to_str().unwrap().to_owned();
            modules.push(mod_name);
        }
    }

    let lib_file_contents = Codegen {
        protoc: None,
        out_dir,
        protos,
        includes,
        extern_paths,
        features: features.into_iter().map(str::to_owned).collect(),
        modules,
    }
    .generate()?;

    let out_file = Path::new("src/lib.rs");
    if fs::read_to_string(out_file)? != lib_file_contents {
        fs::write(out_file, lib_file_contents)?;
    }

    Ok(())
}

/// One pb crate's codegen. `pb_build` fills it in from the crate's directory;
/// Buck2's driver (src/bin/pb_codegen.rs) from its arguments, so both builds
/// produce the same files.
pub struct Codegen {
    /// protoc to run; `None` finds it as prost-build does (`PROTOC` or PATH).
    pub protoc: Option<PathBuf>,
    pub out_dir: PathBuf,
    /// `.proto` files, compiled in sorted order: protoc writes descriptors.bin
    /// in input order, and Cargo discovers the files in directory order while
    /// Buck2 passes them sorted. Each file stem is both its proto package and
    /// its Rust module.
    pub protos: Vec<PathBuf>,
    pub includes: Vec<PathBuf>,
    /// (proto path, Rust path) for packages other crates generate.
    pub extern_paths: Vec<(String, String)>,
    /// `#![feature(...)]` lines of the generated lib.rs.
    pub features: Vec<String>,
    /// Hand-written modules of the crate, which get `pub mod` lines.
    pub modules: Vec<String>,
}

impl Codegen {
    /// Writes each package's `.rs`, `descriptors.bin` and `_extras.rs` to
    /// `out_dir`, and returns the crate's lib.rs.
    pub fn generate(&self) -> Result<String> {
        let descriptor_set_path = self.out_dir.join("descriptors.bin");
        let mut config = tonic_prost_build::Config::new();
        if let Some(protoc) = &self.protoc {
            config.protoc_executable(protoc);
        }
        let mut builder = tonic_prost_build::configure()
            .out_dir(&self.out_dir)
            .file_descriptor_set_path(&descriptor_set_path);
        for (proto_path, rust_path) in &self.extern_paths {
            builder = builder.extern_path(proto_path, rust_path);
        }
        let mut protos = self.protos.clone();
        protos.sort();
        builder.compile_with_config(config, &protos, &self.includes)?;

        // We sort the package names just so we're generating the lib.rs
        // deterministically to avoid NOOP commits.
        let mut packages: Vec<String> = self
            .protos
            .iter()
            .map(|path| path.file_stem().unwrap().to_str().unwrap().to_owned())
            .collect();
        packages.sort();
        let mut modules = self.modules.clone();
        modules.sort();

        // Read back the file descriptor set to codegen `ReflectionService` impls.
        let file_descriptor_set =
            FileDescriptorSet::decode(&std::fs::read(&descriptor_set_path)?[..])?;
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

        std::fs::write(self.out_dir.join("_extras.rs"), extras)?;

        // Now let's build the lib.rs file.
        let mut lib_file_contents = String::new();
        lib_file_contents.push_str("// @generated - do not modify. Modify build.rs instead.\n");
        for feature in &self.features {
            lib_file_contents.push_str(&format!("#![feature({feature})]\n"));
        }
        lib_file_contents.push_str("#![allow(clippy::match_single_binding)]\n");
        for m in modules {
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
        Ok(lib_file_contents)
    }
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
