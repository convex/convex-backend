//! Analysis of `"use rust"` (WASM) modules. Nothing is executed: the
//! `convex:manifest` custom section (contracts/MANIFEST.md) is read with
//! `wasmparser` and cross-checked against the module's
//! `convex_run:<modulePath>:<name>` exports.

use std::collections::{
    BTreeMap,
    BTreeSet,
};

use anyhow::Context;
use common::{
    errors::JsError,
    types::{
        ModuleEnvironment,
        UdfType,
    },
};
use model::{
    config::types::ModuleConfig,
    modules::{
        function_validators::{
            ArgsValidator,
            ArgsValidatorJson,
            ReturnsValidator,
            ReturnsValidatorJson,
        },
        module_versions::{
            AnalyzedFunction,
            AnalyzedModule,
            Visibility,
        },
    },
};
use serde::Deserialize;
use sync_types::{
    CanonicalizedModulePath,
    FunctionName,
};
use value::heap_size::WithHeapSize;
use wasmparser::{
    ExternalKind,
    Parser,
    Payload,
    TypeRef,
    Validator,
};

const MANIFEST_SECTION: &str = "convex:manifest";
const SUPPORTED_ABI_VERSION: u32 = 1;
/// Imports a guest may declare: module `convex` (ABI.md "Imports") and WASI
/// preview 1 (any function; the host linker rejects unknown ones).
const CONVEX_IMPORT_MODULE: &str = "convex";
const CONVEX_IMPORTS: [&str; 4] = ["task_submit", "task_poll", "sync_syscall", "log"];
const WASI_IMPORT_MODULE: &str = "wasi_snapshot_preview1";
/// Convex module paths of `"use rust"` modules live under this directory:
/// `rust/<module>.js` holds the functions `rust/<module>:<name>`.
pub const RUST_MODULE_DIR: &str = "rust";

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ManifestEntry {
    abi_version: u32,
    module_path: String,
    name: String,
    visibility: String,
    #[serde(default)]
    args: Option<serde_json::Value>,
    #[serde(default)]
    returns: Option<serde_json::Value>,
    /// `"http"` for `#[http_action]` entries; absent for actions.
    #[serde(default)]
    kind: Option<String>,
}

/// Convex module name for a Rust `module_path!()`: the segments after the crate
/// name joined by `/` (`bench_rust::cpu` -> `cpu`).
fn convex_module_name(rust_module_path: &str) -> Option<String> {
    let mut segments = rust_module_path.split("::");
    segments.next()?;
    let rest: Vec<&str> = segments.collect();
    if rest.is_empty() || rest.iter().any(|s| s.is_empty()) {
        return None;
    }
    Some(rest.join("/"))
}

struct ParsedWasm {
    manifest: Vec<u8>,
    exports: BTreeSet<String>,
}

/// Validates the module like the runtime would and checks it fits the ABI:
/// only the `convex` host imports and WASI, one linear memory, and the
/// `memory` / `convex_alloc` exports. Errors are user errors.
fn parse_wasm(bytes: &[u8]) -> anyhow::Result<ParsedWasm> {
    Validator::new()
        .validate_all(bytes)
        .context("module failed WebAssembly validation")?;
    let mut manifest = Vec::new();
    let mut found = false;
    let mut exports = BTreeSet::new();
    let mut memory_export = false;
    let mut memories = 0usize;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload? {
            Payload::CustomSection(reader) if reader.name() == MANIFEST_SECTION => {
                // Fragments are concatenated by the linker; separate by `\n`.
                if found {
                    manifest.push(b'\n');
                }
                found = true;
                manifest.extend_from_slice(reader.data());
            },
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    let import = import?;
                    let ok = matches!(import.ty, TypeRef::Func(_))
                        && match import.module {
                            CONVEX_IMPORT_MODULE => CONVEX_IMPORTS.contains(&import.name),
                            WASI_IMPORT_MODULE => true,
                            _ => false,
                        };
                    anyhow::ensure!(
                        ok,
                        "unsupported import `{}::{}`; a \"use rust\" module may only import \
                         the `{CONVEX_IMPORT_MODULE}` host functions and \
                         `{WASI_IMPORT_MODULE}` functions",
                        import.module,
                        import.name
                    );
                }
            },
            Payload::MemorySection(reader) => memories += reader.count() as usize,
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export?;
                    match export.kind {
                        ExternalKind::Func => {
                            exports.insert(export.name.to_string());
                        },
                        ExternalKind::Memory if export.name == "memory" => memory_export = true,
                        _ => {},
                    }
                }
            },
            _ => {},
        }
    }
    anyhow::ensure!(found, "missing `{MANIFEST_SECTION}` custom section");
    anyhow::ensure!(
        memories == 1,
        "module must define exactly one linear memory, found {memories}"
    );
    anyhow::ensure!(memory_export, "module does not export its `memory`");
    anyhow::ensure!(
        exports.contains("convex_alloc"),
        "module does not export the `convex_alloc` function"
    );
    Ok(ParsedWasm { manifest, exports })
}

fn bad_module(message: String) -> JsError {
    JsError::from_message(message)
}

/// Builds the analyzed module for one `"use rust"` module config
/// (`rust/<module>.js`, source = base64 wasm). Returns `Err(JsError)` for user
/// errors (bad wasm, missing manifest/exports).
pub fn analyze_rust_module(
    config: &ModuleConfig,
) -> anyhow::Result<Result<(CanonicalizedModulePath, AnalyzedModule), JsError>> {
    let path = config.path.clone().canonicalize();
    let path_str = config.path.as_str();
    let module_name = match path_str
        .strip_prefix(&format!("{RUST_MODULE_DIR}/"))
        .and_then(|p| p.strip_suffix(".js"))
    {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => {
            return Ok(Err(bad_module(format!(
                "\"use rust\" module paths must look like `{RUST_MODULE_DIR}/<module>.js`, got \
                 `{path_str}`"
            ))))
        },
    };
    let bytes = match base64::decode(config.source.as_bytes()) {
        Ok(b) => b,
        Err(e) => {
            return Ok(Err(bad_module(format!(
                "`{path_str}`: source is not valid base64 wasm: {e}"
            ))))
        },
    };
    let parsed = match parse_wasm(&bytes) {
        Ok(p) => p,
        Err(e) => {
            return Ok(Err(bad_module(format!(
                "`{path_str}`: invalid wasm: {e:#}"
            ))))
        },
    };
    let manifest_text = match std::str::from_utf8(&parsed.manifest) {
        Ok(t) => t,
        Err(e) => {
            return Ok(Err(bad_module(format!(
                "`{path_str}`: `{MANIFEST_SECTION}` is not UTF-8: {e}"
            ))))
        },
    };

    let mut entries = Vec::new();
    for line in manifest_text.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<ManifestEntry>(line) {
            Ok(e) => entries.push(e),
            Err(e) => {
                return Ok(Err(bad_module(format!(
                    "`{path_str}`: invalid `{MANIFEST_SECTION}` entry: {e}"
                ))))
            },
        }
    }
    if let Some(e) = entries
        .iter()
        .find(|e| e.abi_version != SUPPORTED_ABI_VERSION)
    {
        return Ok(Err(bad_module(format!(
            "`{path_str}`: unsupported abiVersion {} for `{}:{}` (host supports \
             {SUPPORTED_ABI_VERSION})",
            e.abi_version, e.module_path, e.name
        ))));
    }

    let mut seen = BTreeSet::new();
    let mut functions = Vec::new();
    for entry in &entries {
        let key = format!("{}:{}", entry.module_path, entry.name);
        // `#[http_action]` entries are registered as *internal* actions: V8 `httpAction`
        // routes (`rustHttpAction` in the generated `_generated/rustHttp.js`) call them with
        // `ctx.runAction`, passing the request in the args (head, base64 body, `buffered`) and
        // getting `{status, headers, body}` back. Never public, so a client cannot forge a
        // request head. Other kinds are unknown to this host and skipped.
        let is_http = entry.kind.as_deref() == Some("http");
        if entry.kind.as_deref().is_some_and(|k| k != "action" && k != "http") {
            tracing::warn!(
                "`{path_str}`: skipping `{key}` (kind `{}`): unknown function kind",
                entry.kind.as_deref().unwrap_or_default()
            );
            continue;
        }
        if !seen.insert(key.clone()) {
            return Ok(Err(bad_module(format!(
                "`{path_str}`: duplicate manifest entry `{key}`"
            ))));
        }
        let Some(entry_module) = convex_module_name(&entry.module_path) else {
            return Ok(Err(bad_module(format!(
                "`{path_str}`: functions at the crate root are not allowed (`{key}`); put them in \
                 a submodule"
            ))));
        };
        // Every manifest entry needs its export, whichever Convex module it
        // belongs to.
        let export = format!("convex_run:{key}");
        if !parsed.exports.contains(&export) {
            return Ok(Err(bad_module(format!(
                "`{path_str}`: manifest lists `{key}` but the wasm has no `{export}` export"
            ))));
        }
        if entry_module != module_name {
            continue;
        }
        let visibility = match entry.visibility.as_str() {
            _ if is_http => Visibility::Internal,
            "public" => Visibility::Public,
            "internal" => Visibility::Internal,
            other => {
                return Ok(Err(bad_module(format!(
                    "`{path_str}`: `{key}` has invalid visibility `{other}`"
                ))))
            },
        };
        let name: FunctionName = match entry.name.parse() {
            Ok(n) => n,
            Err(e) => {
                return Ok(Err(bad_module(format!(
                    "`{path_str}`: invalid function name `{}`: {e:#}",
                    entry.name
                ))))
            },
        };
        let args = match &entry.args {
            None | Some(serde_json::Value::Null) => ArgsValidator::Unvalidated,
            Some(v) => {
                let json: ArgsValidatorJson = match serde_json::from_value(v.clone()) {
                    Ok(j) => j,
                    Err(e) => {
                        return Ok(Err(bad_module(format!(
                            "`{path_str}`: invalid args validator for `{key}`: {e}"
                        ))))
                    },
                };
                match ArgsValidator::try_from(json) {
                    Ok(a) => a,
                    Err(e) => {
                        return Ok(Err(bad_module(format!(
                            "`{path_str}`: bad args validator for `{key}`: {e:#}"
                        ))))
                    },
                }
            },
        };
        let returns = match &entry.returns {
            None | Some(serde_json::Value::Null) => ReturnsValidator::Unvalidated,
            Some(v) => {
                let json: ReturnsValidatorJson = match serde_json::from_value(v.clone()) {
                    Ok(j) => j,
                    Err(e) => {
                        return Ok(Err(bad_module(format!(
                            "`{path_str}`: invalid returns validator for `{key}`: {e}"
                        ))))
                    },
                };
                match ReturnsValidator::try_from(json) {
                    Ok(r) => r,
                    Err(e) => {
                        return Ok(Err(bad_module(format!(
                            "`{path_str}`: bad returns validator for `{key}`: {e:#}"
                        ))))
                    },
                }
            },
        };
        functions.push(AnalyzedFunction::new(
            name,
            None,
            UdfType::Action,
            Some(visibility),
            args,
            returns,
        )?);
    }
    functions.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Ok((
        path,
        AnalyzedModule {
            functions: WithHeapSize::from(functions),
            ..Default::default()
        },
    )))
}

type ModulesByPath = BTreeMap<CanonicalizedModulePath, ModuleConfig>;

/// Splits modules into `(node, isolate, rust)` by environment, keyed by
/// canonicalized path.
pub fn split_by_environment(
    new_modules: Vec<ModuleConfig>,
) -> (ModulesByPath, ModulesByPath, ModulesByPath) {
    let mut node_modules = BTreeMap::new();
    let mut isolate_modules = BTreeMap::new();
    let mut rust_modules = BTreeMap::new();
    for module in new_modules {
        let target = match module.environment {
            ModuleEnvironment::Node => &mut node_modules,
            ModuleEnvironment::Rust => &mut rust_modules,
            ModuleEnvironment::Isolate | ModuleEnvironment::Invalid => &mut isolate_modules,
        };
        target.insert(module.path.clone().canonicalize(), module);
    }
    (node_modules, isolate_modules, rust_modules)
}

/// Analyzes the `"use rust"` modules and adds them to `result`.
pub fn analyze_into(
    modules: &ModulesByPath,
    result: &mut BTreeMap<CanonicalizedModulePath, AnalyzedModule>,
) -> anyhow::Result<Result<(), JsError>> {
    match analyze_rust_modules(modules)? {
        Ok(analyzed) => {
            result.extend(analyzed);
            Ok(Ok(()))
        },
        Err(e) => Ok(Err(e)),
    }
}

/// Analyzes every `"use rust"` module. Never executes guest code.
pub fn analyze_rust_modules(
    modules: &BTreeMap<CanonicalizedModulePath, ModuleConfig>,
) -> anyhow::Result<Result<BTreeMap<CanonicalizedModulePath, AnalyzedModule>, JsError>> {
    let mut result = BTreeMap::new();
    for config in modules.values() {
        match analyze_rust_module(config)? {
            Ok((path, module)) => {
                result.insert(path, module);
            },
            Err(e) => return Ok(Err(e)),
        }
    }
    Ok(Ok(result))
}

#[cfg(test)]
mod tests {
    use common::types::ModuleEnvironment;
    use model::{
        config::types::ModuleConfig,
        modules::module_versions::Visibility,
    };

    use super::*;

    fn leb(mut n: usize, out: &mut Vec<u8>) {
        loop {
            let byte = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
    }

    fn section(id: u8, body: &[u8], out: &mut Vec<u8>) {
        out.push(id);
        leb(body.len(), out);
        out.extend_from_slice(body);
    }

    fn name(s: &str, out: &mut Vec<u8>) {
        leb(s.len(), out);
        out.extend_from_slice(s.as_bytes());
    }

    /// Hand-assembled, valid wasm: one `convex:manifest` custom section per
    /// fragment, `imports` (all `() -> ()` functions), one empty function per
    /// entry of `exports` (the name `memory` instead exports the memory),
    /// and `memories` linear memories. Never instantiated.
    fn wasm_with(
        manifests: &[&str],
        imports: &[(&str, &str)],
        exports: &[&str],
        memories: usize,
    ) -> Vec<u8> {
        let mut out = b"\0asm\x01\0\0\0".to_vec();
        for m in manifests {
            let mut body = Vec::new();
            name("convex:manifest", &mut body);
            body.extend_from_slice(m.as_bytes());
            section(0, &body, &mut out);
        }
        // type section: one `() -> ()`
        section(1, &[1, 0x60, 0, 0], &mut out);
        if !imports.is_empty() {
            let mut body = Vec::new();
            leb(imports.len(), &mut body);
            for (m, n) in imports {
                name(m, &mut body);
                name(n, &mut body);
                body.extend_from_slice(&[0, 0]); // func, type 0
            }
            section(2, &body, &mut out);
        }
        let funcs: Vec<&&str> = exports.iter().filter(|e| **e != "memory").collect();
        let mut body = Vec::new();
        leb(funcs.len(), &mut body);
        body.extend(std::iter::repeat(0).take(funcs.len()));
        section(3, &body, &mut out);
        if memories > 0 {
            let mut body = Vec::new();
            leb(memories, &mut body);
            for _ in 0..memories {
                body.extend_from_slice(&[0, 1]); // no max, 1 page
            }
            section(5, &body, &mut out);
        }
        let mut body = Vec::new();
        leb(exports.len(), &mut body);
        let mut next_func = imports.len();
        for e in exports {
            name(e, &mut body);
            if *e == "memory" {
                body.extend_from_slice(&[2, 0]);
            } else {
                body.push(0);
                leb(next_func, &mut body);
                next_func += 1;
            }
        }
        section(7, &body, &mut out);
        let mut body = Vec::new();
        leb(funcs.len(), &mut body);
        for _ in &funcs {
            body.extend_from_slice(&[2, 0, 0x0b]); // size 2: no locals, end
        }
        section(10, &body, &mut out);
        out
    }

    /// A well-formed ABI module exporting `exports` plus `memory` and
    /// `convex_alloc`.
    fn wasm(manifests: &[&str], exports: &[&str]) -> Vec<u8> {
        let mut all: Vec<&str> = exports.to_vec();
        all.extend(["memory", "convex_alloc"]);
        wasm_with(manifests, &[("convex", "log")], &all, 1)
    }

    fn entry(abi: u32, module_path: &str, fname: &str, vis: &str) -> String {
        format!(
            r#"{{"abiVersion":{abi},"modulePath":"{module_path}","name":"{fname}","visibility":"{vis}","args":null,"returns":null}}"#
        )
    }

    fn config(path: &str, bytes: &[u8]) -> ModuleConfig {
        ModuleConfig {
            path: path.parse().unwrap(),
            source: base64::encode(bytes).as_str().into(),
            source_map: None,
            environment: ModuleEnvironment::Rust,
        }
    }

    fn user_error(path: &str, bytes: &[u8]) -> String {
        let r = analyze_rust_module(&config(path, bytes)).unwrap();
        r.expect_err("expected a user error").message.to_string()
    }

    #[test]
    fn valid_manifest() {
        let m1 = entry(1, "app::hello", "hello", "public");
        let m2 = entry(1, "app::hello", "secret", "internal");
        let m3 = entry(1, "app::other", "x", "public");
        let bytes = wasm(
            &[&m1, &m2, &m3],
            &[
                "convex_run:app::hello:hello",
                "convex_run:app::hello:secret",
                "convex_run:app::other:x",
            ],
        );
        let (path, module) = analyze_rust_module(&config("rust/hello.js", &bytes))
            .unwrap()
            .unwrap();
        assert_eq!(path.as_str(), "rust/hello.js");
        let fns: Vec<_> = module
            .functions
            .iter()
            .map(|f| (f.name.to_string(), f.visibility.clone(), f.udf_type))
            .collect();
        assert_eq!(
            fns,
            vec![
                ("hello".to_string(), Some(Visibility::Public), UdfType::Action),
                (
                    "secret".to_string(),
                    Some(Visibility::Internal),
                    UdfType::Action
                ),
            ]
        );
        // Nested module path.
        let m = entry(1, "app::a::b", "f", "public");
        let bytes = wasm(&[&m], &["convex_run:app::a::b:f"]);
        let (_, module) = analyze_rust_module(&config("rust/a/b.js", &bytes))
            .unwrap()
            .unwrap();
        assert_eq!(module.functions.len(), 1);
    }

    #[test]
    fn http_entries_register_as_internal_actions() {
        let action = entry(1, "app::web", "act", "public");
        let http = entry(1, "app::web", "hook", "public").replace(r#""args":null"#, r#""kind":"http","args":null"#);
        let other = entry(1, "app::web", "odd", "public").replace(r#""args":null"#, r#""kind":"future","args":null"#);
        let bytes = wasm(
            &[&action, &http, &other],
            &["convex_run:app::web:act", "convex_run:app::web:hook", "convex_run:app::web:odd"],
        );
        let (_, module) = analyze_rust_module(&config("rust/web.js", &bytes)).unwrap().unwrap();
        let fns: Vec<_> = module
            .functions
            .iter()
            .map(|f| (f.name.to_string(), f.visibility.clone(), f.udf_type))
            .collect();
        // The http entry is internal whatever the macro said; an unknown kind is skipped.
        assert_eq!(
            fns,
            vec![
                ("act".to_string(), Some(Visibility::Public), UdfType::Action),
                ("hook".to_string(), Some(Visibility::Internal), UdfType::Action),
            ]
        );
    }

    #[test]
    fn bad_base64() {
        let mut c = config("rust/hello.js", b"");
        c.source = "!!!not base64!!!".into();
        let e = analyze_rust_module(&c).unwrap().unwrap_err();
        assert!(e.message.to_string().contains("base64"), "{}", e.message);
    }

    #[test]
    fn bad_wasm() {
        let e = user_error("rust/hello.js", b"definitely not wasm");
        assert!(e.contains("invalid wasm"), "{e}");
    }

    #[test]
    fn missing_manifest() {
        let e = user_error("rust/hello.js", &wasm(&[], &["convex_run:app::hello:hello"]));
        assert!(e.contains("missing `convex:manifest`"), "{e}");
    }

    #[test]
    fn unsupported_abi_version() {
        let m = entry(2, "app::hello", "hello", "public");
        let e = user_error(
            "rust/hello.js",
            &wasm(&[&m], &["convex_run:app::hello:hello"]),
        );
        assert!(e.contains("unsupported abiVersion 2"), "{e}");
    }

    #[test]
    fn duplicate_entry() {
        let m = entry(1, "app::hello", "hello", "public");
        let e = user_error(
            "rust/hello.js",
            &wasm(&[&m, &m], &["convex_run:app::hello:hello"]),
        );
        assert!(e.contains("duplicate manifest entry"), "{e}");
    }

    #[test]
    fn crate_root_function() {
        let m = entry(1, "app", "hello", "public");
        let e = user_error("rust/hello.js", &wasm(&[&m], &["convex_run:app:hello"]));
        assert!(e.contains("crate root"), "{e}");
    }

    #[test]
    fn missing_export() {
        let m = entry(1, "app::hello", "hello", "public");
        let e = user_error("rust/hello.js", &wasm(&[&m], &[]));
        assert!(e.contains("no `convex_run:app::hello:hello` export"), "{e}");
    }

    fn hello_manifest() -> String {
        entry(1, "app::hello", "hello", "public")
    }

    #[test]
    fn invalid_wasm_body_is_rejected() {
        // Exports a function index that does not exist: fails validation.
        let mut bytes = b"\0asm\x01\0\0\0".to_vec();
        let mut body = Vec::new();
        name("convex:manifest", &mut body);
        body.extend_from_slice(hello_manifest().as_bytes());
        section(0, &body, &mut bytes);
        section(7, &[1, 1, b'f', 0, 5], &mut bytes);
        let e = user_error("rust/hello.js", &bytes);
        assert!(e.contains("validation"), "{e}");
    }

    #[test]
    fn unsupported_imports() {
        let m = hello_manifest();
        let exports = ["convex_run:app::hello:hello", "memory", "convex_alloc"];
        for imports in [
            vec![("env", "evil")],
            vec![("convex", "not_a_host_fn")],
            vec![("wasi_snapshot_preview1", "fd_write"), ("sock", "open")],
        ] {
            let e = user_error("rust/hello.js", &wasm_with(&[&m], &imports, &exports, 1));
            assert!(e.contains("unsupported import"), "{e}");
        }
        // The documented imports are fine.
        let imports = [
            ("convex", "task_submit"),
            ("convex", "task_poll"),
            ("convex", "sync_syscall"),
            ("convex", "log"),
            ("wasi_snapshot_preview1", "fd_write"),
        ];
        analyze_rust_module(&config(
            "rust/hello.js",
            &wasm_with(&[&m], &imports, &exports, 1),
        ))
        .unwrap()
        .unwrap();
    }

    #[test]
    fn memory_and_alloc_exports_required() {
        let m = hello_manifest();
        let f = "convex_run:app::hello:hello";
        let e = user_error("rust/hello.js", &wasm_with(&[&m], &[], &[f, "convex_alloc"], 1));
        assert!(e.contains("export its `memory`"), "{e}");
        let e = user_error("rust/hello.js", &wasm_with(&[&m], &[], &[f, "memory"], 1));
        assert!(e.contains("`convex_alloc`"), "{e}");
        let e = user_error(
            "rust/hello.js",
            &wasm_with(&[&m], &[], &[f, "memory", "convex_alloc"], 2),
        );
        assert!(e.contains("exactly one linear memory"), "{e}");
    }

    #[test]
    fn bad_validator_json_is_a_user_error() {
        for field in ["args", "returns"] {
            let m = format!(
                r#"{{"abiVersion":1,"modulePath":"app::hello","name":"hello","visibility":"public","{field}":{{"type":"nonsense"}}}}"#
            );
            let e = user_error(
                "rust/hello.js",
                &wasm(&[&m], &["convex_run:app::hello:hello"]),
            );
            assert!(e.contains(&format!("{field} validator")), "{e}");
        }
    }

    /// Manifest with validators as `#[derive(ConvexArgs)]` emits them.
    fn validated_entry(args: &str, returns: &str) -> String {
        format!(
            r#"{{"abiVersion":1,"modulePath":"app::hello","name":"hello","visibility":"public","args":{args},"returns":{returns}}}"#
        )
    }

    #[test]
    fn manifest_validators_become_function_validators() {
        let args = r#"{"type":"object","value":{"body":{"fieldType":{"type":"string"},"optional":false},"author":{"fieldType":{"type":"id","tableName":"users"},"optional":false},"n":{"fieldType":{"type":"bigint"},"optional":true},"tags":{"fieldType":{"type":"array","value":{"type":"string"}},"optional":false},"kind":{"fieldType":{"type":"union","value":[{"type":"literal","value":"a"},{"type":"null"}]},"optional":false},"meta":{"fieldType":{"type":"record","keys":{"type":"string"},"values":{"fieldType":{"type":"any"},"optional":false}},"optional":false}}}"#;
        let returns = r#"{"type":"array","value":{"type":"object","value":{"x":{"fieldType":{"type":"number"},"optional":false}}}}"#;
        let m = validated_entry(args, returns);
        let bytes = wasm(&[&m], &["convex_run:app::hello:hello"]);
        let (_, module) = analyze_rust_module(&config("rust/hello.js", &bytes))
            .unwrap()
            .unwrap();
        let f = &module.functions[0];
        // Same types the V8 analysis produces.
        let ArgsValidator::Validated(_) = f.args().unwrap() else {
            panic!("args should be validated");
        };
        let ReturnsValidator::Validated(_) = f.returns().unwrap() else {
            panic!("returns should be validated");
        };
        // The stored JSON (what the dashboard and `function-spec` read) is
        // the validator the macro described.
        let stored: serde_json::Value =
            serde_json::from_str(f.args_str.as_ref().unwrap()).unwrap();
        assert_eq!(stored, serde_json::from_str::<serde_json::Value>(args).unwrap());
        let stored: serde_json::Value =
            serde_json::from_str(f.returns_str.as_ref().unwrap()).unwrap();
        assert_eq!(
            stored,
            serde_json::from_str::<serde_json::Value>(returns).unwrap()
        );
    }

    #[test]
    fn null_validators_stay_unvalidated() {
        let m = validated_entry("null", "null");
        let bytes = wasm(&[&m], &["convex_run:app::hello:hello"]);
        let (_, module) = analyze_rust_module(&config("rust/hello.js", &bytes))
            .unwrap()
            .unwrap();
        let f = &module.functions[0];
        assert_eq!(f.args().unwrap(), ArgsValidator::Unvalidated);
        assert_eq!(f.returns().unwrap(), ReturnsValidator::Unvalidated);
    }

    #[test]
    fn non_object_args_validator_is_a_user_error() {
        let m = validated_entry(r#"{"type":"string"}"#, "null");
        let e = user_error(
            "rust/hello.js",
            &wasm(&[&m], &["convex_run:app::hello:hello"]),
        );
        assert!(e.contains("args validator"), "{e}");
    }

    #[test]
    fn bad_module_path() {
        let m = entry(1, "app::hello", "hello", "public");
        let e = user_error("hello.js", &wasm(&[&m], &["convex_run:app::hello:hello"]));
        assert!(e.contains("rust/<module>.js"), "{e}");
    }
}
