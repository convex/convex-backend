//! Push-time validation of native ("rust-native") artifacts. Nothing is
//! executed or loaded: the carrier's binary is parsed with `object`.
//! Contract: contracts/NATIVE.md §5.2 (carrier/ref format), .plan/DESIGN.md
//! §5.3 (the 8 checks).

use std::collections::{
    BTreeMap,
    BTreeSet,
};

use common::errors::JsError;
use isolate::environment::rust::native::{
    NATIVE_MAGIC,
    NATIVE_REF_MAGIC,
    target::{
        HostTarget,
        host_target,
    },
};
use model::{
    config::types::ModuleConfig,
    modules::module_versions::{
        AnalyzedFunction,
        AnalyzedModule,
    },
};
use object::{
    Endianness,
    Object,
    ObjectSection,
    ObjectSymbol,
    elf,
    macho,
    read::{
        elf::{
            ElfFile64,
            FileHeader as _,
            ProgramHeader as _,
        },
        macho::{
            MachHeader as _,
            MachOFile64,
        },
    },
};
use serde::Deserialize;
use sha2::{
    Digest,
    Sha256,
};
use sync_types::CanonicalizedModulePath;
use value::heap_size::WithHeapSize;

use super::{
    analyze_manifest_text,
    bad_module,
    rust_module_name,
};

const MANIFEST_SYMBOL_PREFIX: &str = "convex_mf:";
const REQUIRED_EXPORTS: [&str; 3] = ["convex_native_init", "convex_alloc", "convex_free_result"];
const ELF_ALLOWED_NEEDED: [&str; 6] = [
    "libc.so.6",
    "libm.so.6",
    "libgcc_s.so.1",
    "libpthread.so.0",
    "libdl.so.2",
    "librt.so.1",
];
const MACHO_ALLOWED_DYLIBS: [&str; 2] = ["/usr/lib/libSystem.B.dylib", "/usr/lib/libiconv.2.dylib"];
const MACHO_ALLOWED_FRAMEWORKS: [&str; 3] = ["CoreFoundation", "Security", "SystemConfiguration"];
/// 180 MB, the same figure as the CLI (`NATIVE_SIZE_ERROR_BYTES`).
#[cfg(test)]
const DEFAULT_MAX_ARTIFACT_BYTES: u64 = 180 * 1024 * 1024;

pub const KNOB_OFF_MESSAGE: &str = "This backend does not run native Rust actions \
                                    (RUST_NATIVE_MODE is off). Push with --rust-runtime wasm, or \
                                    enable it.";

/// Everything the checks read from the environment, explicit so tests do not
/// touch process env.
#[derive(Clone, Debug)]
pub struct NativeConfig {
    pub mode_on: bool,
    pub max_artifact_bytes: u64,
    pub extra_libs: Vec<String>,
    pub host: HostTarget,
}

impl NativeConfig {
    pub fn from_env() -> Self {
        let max_artifact_bytes = *common::knobs::RUST_NATIVE_MAX_ARTIFACT_BYTES;
        let extra_libs = common::knobs::RUST_NATIVE_EXTRA_LIBS
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Self {
            mode_on: isolate::environment::rust::native::target::native_mode().is_some(),
            max_artifact_bytes,
            extra_libs,
            host: host_target(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceKind {
    Wasm,
    Carrier,
    Ref,
}

/// Branches on the first line of the module source.
pub fn kind(source: &str) -> SourceKind {
    if source.starts_with(NATIVE_MAGIC) {
        SourceKind::Carrier
    } else if source.starts_with(NATIVE_REF_MAGIC) {
        SourceKind::Ref
    } else {
        SourceKind::Wasm
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct CarrierHeader {
    format: u32,
    native_abi: u32,
    abi: u32,
    #[allow(dead_code)]
    triple: String,
    #[allow(dead_code)]
    glibc_max: Option<String>,
    sha256: String,
    size: u64,
    functions: BTreeMap<String, String>,
}

#[derive(Deserialize, Debug)]
struct RefHeader {
    sha256: String,
    carrier: String,
}

/// A validated carrier: the analyzed functions of every Convex module its
/// manifest covers.
pub struct Carrier {
    sha256: String,
    modules: BTreeMap<String, Vec<AnalyzedFunction>>,
}

type AnalyzeResult = anyhow::Result<Result<(CanonicalizedModulePath, AnalyzedModule), JsError>>;

fn module_of(path: CanonicalizedModulePath, functions: Vec<AnalyzedFunction>) -> (CanonicalizedModulePath, AnalyzedModule) {
    (
        path,
        AnalyzedModule {
            functions: WithHeapSize::from(functions),
            ..Default::default()
        },
    )
}

/// Analyzes a carrier or ref module. `modules` are all the rust modules of
/// the push (to resolve refs); `carriers` caches validated carriers by path.
pub fn analyze_native_module(
    cfg: &NativeConfig,
    config: &ModuleConfig,
    modules: &BTreeMap<CanonicalizedModulePath, ModuleConfig>,
    carriers: &mut BTreeMap<CanonicalizedModulePath, Carrier>,
) -> AnalyzeResult {
    let path = config.path.clone().canonicalize();
    let path_str = config.path.as_str();
    let module_name = match rust_module_name(path_str) {
        Ok(n) => n,
        Err(e) => return Ok(Err(e)),
    };
    // Check 1: the knob.
    if !cfg.mode_on {
        return Ok(Err(bad_module(KNOB_OFF_MESSAGE.to_string())));
    }
    let source: &str = &config.source;
    match kind(source) {
        SourceKind::Wasm => unreachable!("not a native module"),
        SourceKind::Carrier => {
            if !carriers.contains_key(&path) {
                match analyze_carrier(cfg, path_str, source) {
                    Ok(c) => carriers.insert(path.clone(), c),
                    Err(e) => return Ok(Err(e)),
                };
            }
            let functions = carriers[&path]
                .modules
                .get(&module_name)
                .cloned()
                .unwrap_or_default();
            Ok(Ok(module_of(path, functions)))
        },
        SourceKind::Ref => {
            let header: RefHeader = match serde_json::from_str(&source[NATIVE_REF_MAGIC.len()..]) {
                Ok(h) => h,
                Err(e) => {
                    return Ok(Err(bad_module(format!(
                        "`{path_str}`: invalid native reference header: {e}"
                    ))))
                },
            };
            let carrier_path = match header.carrier.parse::<sync_types::ModulePath>() {
                Ok(p) => p.canonicalize(),
                Err(e) => {
                    return Ok(Err(bad_module(format!(
                        "`{path_str}`: invalid carrier path `{}`: {e:#}",
                        header.carrier
                    ))))
                },
            };
            let carrier_source = modules
                .get(&carrier_path)
                .map(|c| &*c.source)
                .filter(|s| kind(s) == SourceKind::Carrier);
            let Some(carrier_source) = carrier_source else {
                return Ok(Err(bad_module(format!(
                    "`{path_str}`: refers to the native library in `{}`, which is not part of this \
                     push",
                    header.carrier
                ))));
            };
            if !carriers.contains_key(&carrier_path) {
                match analyze_carrier(cfg, &header.carrier, carrier_source) {
                    Ok(c) => carriers.insert(carrier_path.clone(), c),
                    Err(e) => return Ok(Err(e)),
                };
            }
            let carrier = &carriers[&carrier_path];
            if carrier.sha256 != header.sha256 {
                return Ok(Err(bad_module(format!(
                    "`{path_str}`: refers to native library sha256 {} but `{}` carries {}",
                    header.sha256, header.carrier, carrier.sha256
                ))));
            }
            let functions = carrier.modules.get(&module_name).cloned().unwrap_or_default();
            Ok(Ok(module_of(path, functions)))
        },
    }
}

impl Clone for Carrier {
    fn clone(&self) -> Self {
        Self {
            sha256: self.sha256.clone(),
            modules: self.modules.clone(),
        }
    }
}

fn err<T>(path_str: &str, message: impl std::fmt::Display) -> Result<T, JsError> {
    Err(bad_module(format!("`{path_str}`: {message}")))
}

/// Checks 2-8 for a carrier. `Err` is a user error.
fn analyze_carrier(cfg: &NativeConfig, path_str: &str, source: &str) -> Result<Carrier, JsError> {
    let rest = &source[NATIVE_MAGIC.len()..];
    let (header_line, b64) = rest.split_once('\n').unwrap_or((rest, ""));
    let header: CarrierHeader = match serde_json::from_str(header_line) {
        Ok(h) => h,
        Err(e) => return err(path_str, format!("invalid native carrier header: {e}")),
    };
    if header.format != 1 || header.native_abi != 1 || header.abi != 1 {
        return err(
            path_str,
            format!(
                "unsupported native carrier (format {}, nativeAbi {}, abi {}); this backend \
                 supports format 1, nativeAbi 1, abi 1",
                header.format, header.native_abi, header.abi
            ),
        );
    }
    // Check 2: size and sha256.
    if (b64.len() as u64 / 4) * 3 > cfg.max_artifact_bytes + 3 {
        return err(
            path_str,
            format!(
                "the native Rust library is larger than the limit of {} bytes \
                 (RUST_NATIVE_MAX_ARTIFACT_BYTES)",
                cfg.max_artifact_bytes
            ),
        );
    }
    let bytes = match base64::decode(b64.as_bytes()) {
        Ok(b) => b,
        Err(e) => return err(path_str, format!("source is not valid base64: {e}")),
    };
    if bytes.len() as u64 > cfg.max_artifact_bytes {
        return err(
            path_str,
            format!(
                "the native Rust library is {} bytes, over the limit of {} bytes \
                 (RUST_NATIVE_MAX_ARTIFACT_BYTES)",
                bytes.len(),
                cfg.max_artifact_bytes
            ),
        );
    }
    if bytes.len() as u64 != header.size {
        return err(
            path_str,
            format!(
                "the native Rust library is {} bytes but its header says {}",
                bytes.len(),
                header.size
            ),
        );
    }
    let sha = hex_lower(&Sha256::digest(&bytes));
    if !sha.eq_ignore_ascii_case(&header.sha256) {
        return err(
            path_str,
            format!(
                "sha256 mismatch: the header says {} but the library hashes to {sha}",
                header.sha256
            ),
        );
    }

    // Checks 3-6 and symbol extraction.
    let parsed = parse_native(cfg, path_str, &bytes)?;

    // Check 6: exports.
    for required in REQUIRED_EXPORTS {
        if !parsed.exports.contains(required) {
            return err(path_str, format!("the native library does not export `{required}`"));
        }
    }
    for export in header.functions.values() {
        if !parsed.exports.contains(export) {
            return err(
                path_str,
                format!("the header lists `{export}` but the native library does not export it"),
            );
        }
    }

    // Check 7: manifest.
    if parsed.manifest.is_empty() {
        return err(
            path_str,
            format!("the native library has no `{MANIFEST_SYMBOL_PREFIX}*` manifest symbols"),
        );
    }
    let mut text = Vec::new();
    for (i, bytes) in parsed.manifest.values().enumerate() {
        if i > 0 && text.last() != Some(&b'\n') {
            text.push(b'\n');
        }
        text.extend_from_slice(bytes);
    }
    let text = match std::str::from_utf8(&text) {
        Ok(t) => t,
        Err(e) => return err(path_str, format!("the `{MANIFEST_SYMBOL_PREFIX}*` symbols are not UTF-8: {e}")),
    };
    let analysis = match analyze_manifest_text(path_str, "native library", text, &parsed.exports, None) {
        Ok(Ok(a)) => a,
        Ok(Err(e)) => return Err(e),
        Err(e) => return err(path_str, format!("{e:#}")),
    };
    if analysis.export_map != header.functions {
        let manifest_keys: BTreeSet<_> = analysis.export_map.keys().cloned().collect();
        let header_keys: BTreeSet<_> = header.functions.keys().cloned().collect();
        let detail = if let Some(k) = manifest_keys.difference(&header_keys).next() {
            format!("the manifest has `{k}` but the header does not")
        } else if let Some(k) = header_keys.difference(&manifest_keys).next() {
            format!("the header has `{k}` but the manifest does not")
        } else {
            let k = analysis
                .export_map
                .iter()
                .find(|(k, v)| header.functions[*k] != **v)
                .map(|(k, _)| k.clone())
                .unwrap_or_default();
            format!("`{k}` maps to a different export in the header")
        };
        return err(
            path_str,
            format!("the header's function map does not match the library's manifest: {detail}"),
        );
    }
    Ok(Carrier {
        sha256: sha,
        modules: analysis.functions,
    })
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

struct ParsedNative {
    /// Defined symbol names (leading `_` stripped for Mach-O).
    exports: BTreeSet<String>,
    /// `convex_mf:*` symbol name -> bytes, sorted by name.
    manifest: BTreeMap<String, Vec<u8>>,
}

fn parse_native(cfg: &NativeConfig, path_str: &str, bytes: &[u8]) -> Result<ParsedNative, JsError> {
    let magic = bytes.get(..4).unwrap_or(&[]);
    if magic == b"\x7fELF" {
        if cfg.host.os != "linux" {
            return err(
                path_str,
                format!(
                    "the library is an ELF file but this backend runs on {} ({}); rebuild for \
                     {}",
                    cfg.host.os, cfg.host.triple, cfg.host.triple
                ),
            );
        }
        parse_elf(cfg, path_str, bytes)
    } else if magic == [0xcf, 0xfa, 0xed, 0xfe] {
        if cfg.host.os != "macos" {
            return err(
                path_str,
                format!(
                    "the library is a Mach-O file but this backend runs on {} ({}); rebuild for \
                     {}",
                    cfg.host.os, cfg.host.triple, cfg.host.triple
                ),
            );
        }
        parse_macho(cfg, path_str, bytes)
    } else if magic == [0xca, 0xfe, 0xba, 0xbe] || magic == [0xbe, 0xba, 0xfe, 0xca] {
        err(
            path_str,
            "universal (fat) Mach-O libraries are not supported; build for a single architecture",
        )
    } else {
        err(path_str, "the native library is neither an ELF64 nor a Mach-O 64 file")
    }
}

fn allowed_lib(cfg: &NativeConfig, name: &str, builtin: bool) -> bool {
    builtin || cfg.extra_libs.iter().any(|l| l == name)
}

fn parse_elf(cfg: &NativeConfig, path_str: &str, bytes: &[u8]) -> Result<ParsedNative, JsError> {
    // Check 3: format.
    if bytes.get(4) != Some(&2) || bytes.get(5) != Some(&1) {
        return err(path_str, "only 64-bit little-endian ELF libraries are supported");
    }
    let file = match ElfFile64::<Endianness>::parse(bytes) {
        Ok(f) => f,
        Err(e) => return err(path_str, format!("invalid ELF library: {e}")),
    };
    let endian = file.endian();
    let header = file.elf_header();
    if header.e_type(endian) != elf::ET_DYN {
        return err(
            path_str,
            "the native library is not a shared object (e_type is not ET_DYN); build a `cdylib`",
        );
    }
    let machine = header.e_machine(endian);
    let want = match cfg.host.arch.as_str() {
        "x86_64" => elf::EM_X86_64,
        "aarch64" => elf::EM_AARCH64,
        other => return err(path_str, format!("unsupported backend architecture {other}")),
    };
    if machine != want {
        return err(
            path_str,
            format!(
                "the library was built for a different architecture (e_machine {machine:?}, the \
                 backend is {}); rebuild with --target {}",
                cfg.host.arch, cfg.host.triple
            ),
        );
    }
    if !file
        .elf_program_headers()
        .iter()
        .any(|p| p.p_type(endian) == elf::PT_DYNAMIC)
    {
        return err(path_str, "the native library has no PT_DYNAMIC segment");
    }

    // Check 4: DT_NEEDED.
    let dynamic = match file.elf_dynamic_table() {
        Ok(d) => d,
        Err(e) => return err(path_str, format!("invalid dynamic section: {e}")),
    };
    for d in dynamic.iter() {
        if d.tag != elf::DT_NEEDED {
            continue;
        }
        let name = match dynamic.string(d) {
            Ok(n) => String::from_utf8_lossy(n).into_owned(),
            Err(e) => return err(path_str, format!("invalid DT_NEEDED entry: {e}")),
        };
        let builtin = ELF_ALLOWED_NEEDED.contains(&name.as_str())
            || (name.starts_with("ld-linux") && name.contains(".so"));
        if !allowed_lib(cfg, &name, builtin) {
            return err(
                path_str,
                format!(
                    "the native library depends on the shared library `{name}`, which is not \
                     allowed (allowed: {}; extend with RUST_NATIVE_EXTRA_LIBS)",
                    ELF_ALLOWED_NEEDED.join(", ")
                ),
            );
        }
    }

    // Check 5: glibc.
    if let Some(max) = elf_glibc_max(&file, path_str)? {
        let host_glibc = cfg.host.glibc.as_deref();
        let too_new = match host_glibc {
            Some(h) => version_cmp(&max, h).is_gt(),
            None => true,
        };
        if too_new {
            let have = host_glibc.unwrap_or("no glibc");
            let suffix = host_glibc.map(|h| format!(".{h}")).unwrap_or_default();
            return err(
                path_str,
                format!(
                    "the Rust library needs glibc {max} but the backend has {have}; rebuild with \
                     --target {}{suffix}",
                    cfg.host.triple
                ),
            );
        }
    }
    collect_symbols(&file, false, path_str)
}

fn elf_glibc_max(file: &ElfFile64<'_, Endianness>, path_str: &str) -> Result<Option<String>, JsError> {
    let endian = file.endian();
    let data = file.data();
    let sections = file.elf_section_table();
    let Some((mut needs, link)) = sections
        .gnu_verneed(endian, data)
        .or_else(|e| err(path_str, format!("invalid .gnu.version_r: {e}")))?
    else {
        return Ok(None);
    };
    let strings = sections
        .strings(endian, data, link)
        .or_else(|e| err(path_str, format!("invalid .gnu.version_r strings: {e}")))?;
    let mut best: Option<String> = None;
    while let Some((_, mut aux)) = needs
        .next()
        .or_else(|e| err(path_str, format!("invalid .gnu.version_r: {e}")))?
    {
        while let Some(a) = aux
            .next()
            .or_else(|e| err(path_str, format!("invalid .gnu.version_r: {e}")))?
        {
            let name = a
                .name(endian, strings)
                .or_else(|e| err(path_str, format!("invalid .gnu.version_r name: {e}")))?;
            let Some(ver) = std::str::from_utf8(name)
                .ok()
                .and_then(|n| n.strip_prefix("GLIBC_"))
            else {
                continue;
            };
            if !ver.starts_with(|c: char| c.is_ascii_digit()) {
                continue; // GLIBC_PRIVATE
            }
            if best.as_deref().is_none_or(|b| version_cmp(ver, b).is_gt()) {
                best = Some(ver.to_string());
            }
        }
    }
    Ok(best)
}

fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> Vec<u64> { s.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    let (a, b) = (parse(a), parse(b));
    for i in 0..a.len().max(b.len()) {
        let o = a.get(i).copied().unwrap_or(0).cmp(&b.get(i).copied().unwrap_or(0));
        if o.is_ne() {
            return o;
        }
    }
    std::cmp::Ordering::Equal
}

fn parse_macho(cfg: &NativeConfig, path_str: &str, bytes: &[u8]) -> Result<ParsedNative, JsError> {
    let file = match MachOFile64::<Endianness, _>::parse(bytes) {
        Ok(f) => f,
        Err(e) => return err(path_str, format!("invalid Mach-O library: {e}")),
    };
    let endian = file.endian();
    let header = file.macho_header();
    // Check 3: format.
    if header.filetype(endian) != macho::MH_DYLIB {
        return err(
            path_str,
            "the native library is not a dylib (filetype is not MH_DYLIB); build a `cdylib`",
        );
    }
    let want = match cfg.host.arch.as_str() {
        "x86_64" => macho::CPU_TYPE_X86_64,
        "aarch64" => macho::CPU_TYPE_ARM64,
        other => return err(path_str, format!("unsupported backend architecture {other}")),
    };
    let cpu = header.cputype(endian);
    if cpu != want {
        return err(
            path_str,
            format!(
                "the library was built for a different architecture (cputype {cpu:?}, the \
                 backend is {}); rebuild with --target {}",
                cfg.host.arch, cfg.host.triple
            ),
        );
    }
    // Check 4: LC_LOAD_DYLIB and friends.
    let mut commands = match file.macho_load_commands() {
        Ok(c) => c,
        Err(e) => return err(path_str, format!("invalid Mach-O load commands: {e}")),
    };
    loop {
        let cmd = match commands.next() {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => return err(path_str, format!("invalid Mach-O load command: {e}")),
        };
        let dylib = match cmd.dylib() {
            Ok(Some(d)) => d,
            Ok(None) => continue,
            Err(e) => return err(path_str, format!("invalid Mach-O dylib command: {e}")),
        };
        let name = match cmd.string(endian, dylib.dylib.name) {
            Ok(n) => String::from_utf8_lossy(n).into_owned(),
            Err(e) => return err(path_str, format!("invalid Mach-O dylib name: {e}")),
        };
        let builtin = MACHO_ALLOWED_DYLIBS.contains(&name.as_str())
            || MACHO_ALLOWED_FRAMEWORKS.iter().any(|f| {
                name.strip_prefix(&format!("/System/Library/Frameworks/{f}.framework/"))
                    .is_some()
            });
        if !allowed_lib(cfg, &name, builtin) {
            return err(
                path_str,
                format!(
                    "the native library depends on the shared library `{name}`, which is not \
                     allowed (allowed: {}, the CoreFoundation, Security and SystemConfiguration \
                     frameworks; extend with RUST_NATIVE_EXTRA_LIBS)",
                    MACHO_ALLOWED_DYLIBS.join(", ")
                ),
            );
        }
    }
    collect_symbols(&file, true, path_str)
}

/// Defined symbols and the bytes of the `convex_mf:*` ones. Mach-O symbols
/// have no size: a symbol extends to the next one in its section.
fn collect_symbols<'data, O: Object<'data>>(
    file: &O,
    macho: bool,
    path_str: &str,
) -> Result<ParsedNative, JsError> {
    struct Sym {
        name: String,
        section: object::SectionIndex,
        address: u64,
        size: u64,
    }
    let mut defined = Vec::new();
    let symbols: Box<dyn Iterator<Item = O::Symbol<'_>>> = if macho {
        Box::new(file.symbols())
    } else {
        Box::new(file.dynamic_symbols())
    };
    for sym in symbols {
        if !sym.is_definition() {
            continue;
        }
        let Some(section) = sym.section_index() else {
            continue;
        };
        let Ok(name) = sym.name() else { continue };
        let name = if macho {
            name.strip_prefix('_').unwrap_or(name)
        } else {
            name
        };
        defined.push(Sym {
            name: name.to_string(),
            section,
            address: sym.address(),
            size: sym.size(),
        });
    }
    let mut manifest = BTreeMap::new();
    for sym in defined.iter().filter(|s| s.name.starts_with(MANIFEST_SYMBOL_PREFIX)) {
        let section = file
            .section_by_index(sym.section)
            .or_else(|e| err(path_str, format!("bad section for `{}`: {e}", sym.name)))?;
        let mut size = sym.size;
        if macho {
            let mut end = section.address() + section.size();
            for o in &defined {
                if o.section == sym.section && o.address > sym.address && o.address < end {
                    end = o.address;
                }
            }
            size = end - sym.address;
        }
        let data = section
            .data_range(sym.address, size)
            .ok()
            .flatten()
            .ok_or_else(|| {
                bad_module(format!(
                    "`{path_str}`: manifest symbol `{}` is not backed by file data",
                    sym.name
                ))
            })?;
        let mut data = data;
        if macho {
            while let [rest @ .., 0] = data {
                data = rest; // alignment padding
            }
            // Each entry is one JSON line: whatever follows its newline (the
            // last symbol of a unit runs into unrelated rodata) is not ours.
            if let Some(i) = data.iter().position(|&b| b == b'\n') {
                data = &data[..=i];
            }
        }
        manifest.insert(sym.name.clone(), data.to_vec());
    }
    Ok(ParsedNative {
        exports: defined.into_iter().map(|s| s.name).collect(),
        manifest,
    })
}

#[cfg(test)]
mod tests {
    use common::types::ModuleEnvironment;

    use super::*;
    use crate::application_function_runner::rust_analyze::{
        analyze_rust_modules_with,
    };

    // ---------------------------------------------------------------
    // Synthetic binaries (hand-assembled; never loaded).
    // ---------------------------------------------------------------

    const EM_AARCH64: u16 = 183;
    const EM_X86_64: u16 = 62;
    const CPU_ARM64: u32 = 0x0100_000c;
    const CPU_X86_64: u32 = 0x0100_0007;
    const MH_DYLIB_T: u32 = 6;
    const MH_BUNDLE_T: u32 = 8;

    fn p16(v: &mut Vec<u8>, x: u16) {
        v.extend_from_slice(&x.to_le_bytes());
    }
    fn p32(v: &mut Vec<u8>, x: u32) {
        v.extend_from_slice(&x.to_le_bytes());
    }
    fn p64(v: &mut Vec<u8>, x: u64) {
        v.extend_from_slice(&x.to_le_bytes());
    }
    fn align(v: &mut Vec<u8>, to: usize) {
        while v.len() % to != 0 {
            v.push(0);
        }
    }

    struct ElfSpec {
        machine: u16,
        e_type: u16,
        needed: Vec<&'static str>,
        glibc: Vec<&'static str>,
        symbols: Vec<(String, Vec<u8>)>,
        pt_dynamic: bool,
    }

    impl ElfSpec {
        fn new(machine: u16, symbols: Vec<(String, Vec<u8>)>) -> Self {
            Self {
                machine,
                e_type: 3,
                needed: vec!["libc.so.6", "libgcc_s.so.1"],
                glibc: vec!["2.17", "2.34"],
                symbols,
                pt_dynamic: true,
            }
        }
    }

    /// ELF64 LE with .rodata, .dynstr, .dynsym, .dynamic, [.gnu.version_r],
    /// .shstrtab. vaddr == file offset, one PT_LOAD over the whole file.
    fn build_elf(spec: &ElfSpec) -> Vec<u8> {
        let mut f = vec![0u8; 64 + 56 * 2];
        // .rodata
        align(&mut f, 8);
        let rodata_off = f.len();
        let mut sym_offs = Vec::new();
        for (_, data) in &spec.symbols {
            align(&mut f, 8);
            sym_offs.push(f.len());
            f.extend_from_slice(data);
        }
        let rodata_end = f.len();
        // .dynstr
        let mut dynstr = vec![0u8];
        let mut add = |s: &str| {
            let o = dynstr.len() as u32;
            dynstr.extend_from_slice(s.as_bytes());
            dynstr.push(0);
            o
        };
        let sym_names: Vec<u32> = spec.symbols.iter().map(|(n, _)| add(n)).collect();
        let needed: Vec<u32> = spec.needed.iter().map(|n| add(n)).collect();
        let libc_file = add("libc.so.6");
        let ver_names: Vec<u32> = spec.glibc.iter().map(|v| add(&format!("GLIBC_{v}"))).collect();
        align(&mut f, 8);
        let dynstr_off = f.len();
        f.extend_from_slice(&dynstr);
        // .dynsym
        align(&mut f, 8);
        let dynsym_off = f.len();
        f.extend_from_slice(&[0u8; 24]);
        for (i, (_, data)) in spec.symbols.iter().enumerate() {
            p32(&mut f, sym_names[i]);
            f.push(0x11); // GLOBAL OBJECT
            f.push(0);
            p16(&mut f, 1); // .rodata
            p64(&mut f, sym_offs[i] as u64);
            p64(&mut f, data.len() as u64);
        }
        let dynsym_size = f.len() - dynsym_off;
        // .dynamic
        align(&mut f, 8);
        let dynamic_off = f.len();
        for n in &needed {
            p64(&mut f, 1); // DT_NEEDED
            p64(&mut f, *n as u64);
        }
        p64(&mut f, 5); // DT_STRTAB
        p64(&mut f, dynstr_off as u64);
        p64(&mut f, 0); // DT_NULL
        p64(&mut f, 0);
        let dynamic_size = f.len() - dynamic_off;
        // .gnu.version_r
        align(&mut f, 8);
        let verneed_off = f.len();
        if !ver_names.is_empty() {
            p16(&mut f, 1); // vn_version
            p16(&mut f, ver_names.len() as u16);
            p32(&mut f, libc_file);
            p32(&mut f, 16); // vn_aux
            p32(&mut f, 0); // vn_next
            for (i, n) in ver_names.iter().enumerate() {
                p32(&mut f, 0); // hash
                p16(&mut f, 0);
                p16(&mut f, 2 + i as u16);
                p32(&mut f, *n);
                p32(&mut f, if i + 1 == ver_names.len() { 0 } else { 16 });
            }
        }
        let verneed_size = f.len() - verneed_off;
        // .shstrtab
        let mut shstr = vec![0u8];
        let mut name = |s: &str| {
            let o = shstr.len() as u32;
            shstr.extend_from_slice(s.as_bytes());
            shstr.push(0);
            o
        };
        let n_ro = name(".rodata");
        let n_ds = name(".dynstr");
        let n_sym = name(".dynsym");
        let n_dyn = name(".dynamic");
        let n_ver = name(".gnu.version_r");
        let n_sh = name(".shstrtab");
        let shstr_off = f.len();
        f.extend_from_slice(&shstr);
        align(&mut f, 8);
        let shoff = f.len();
        #[allow(clippy::too_many_arguments)]
        fn sh(f: &mut Vec<u8>, name: u32, ty: u32, flags: u64, off: usize, size: usize, link: u32, info: u32, entsize: u64) {
            p32(f, name);
            p32(f, ty);
            p64(f, flags);
            p64(f, off as u64); // addr == offset
            p64(f, off as u64);
            p64(f, size as u64);
            p32(f, link);
            p32(f, info);
            p64(f, 8);
            p64(f, entsize);
        }
        f.extend_from_slice(&[0u8; 64]);
        sh(&mut f, n_ro, 1, 2, rodata_off, rodata_end - rodata_off, 0, 0, 0);
        sh(&mut f, n_ds, 3, 2, dynstr_off, dynstr.len(), 0, 0, 0);
        sh(&mut f, n_sym, 11, 2, dynsym_off, dynsym_size, 2, 1, 24);
        sh(&mut f, n_dyn, 6, 3, dynamic_off, dynamic_size, 2, 0, 16);
        let mut shnum = 5;
        if !ver_names.is_empty() {
            sh(&mut f, n_ver, 0x6fff_fffe, 2, verneed_off, verneed_size, 2, 1, 0);
            shnum += 1;
        }
        sh(&mut f, n_sh, 3, 0, shstr_off, shstr.len(), 0, 0, 0);
        shnum += 1;
        // headers
        let total = f.len();
        let mut h = Vec::new();
        h.extend_from_slice(b"\x7fELF\x02\x01\x01\0\0\0\0\0\0\0\0\0");
        p16(&mut h, spec.e_type);
        p16(&mut h, spec.machine);
        p32(&mut h, 1);
        p64(&mut h, 0); // entry
        p64(&mut h, 64); // phoff
        p64(&mut h, shoff as u64);
        p32(&mut h, 0);
        p16(&mut h, 64);
        p16(&mut h, 56);
        p16(&mut h, 2);
        p16(&mut h, 64);
        p16(&mut h, shnum as u16);
        p16(&mut h, (shnum - 1) as u16);
        // PT_LOAD
        p32(&mut h, 1);
        p32(&mut h, 5);
        p64(&mut h, 0);
        p64(&mut h, 0);
        p64(&mut h, 0);
        p64(&mut h, total as u64);
        p64(&mut h, total as u64);
        p64(&mut h, 0x1000);
        // PT_DYNAMIC (or PT_NOTE)
        p32(&mut h, if spec.pt_dynamic { 2 } else { 4 });
        p32(&mut h, 6);
        p64(&mut h, dynamic_off as u64);
        p64(&mut h, dynamic_off as u64);
        p64(&mut h, dynamic_off as u64);
        p64(&mut h, dynamic_size as u64);
        p64(&mut h, dynamic_size as u64);
        p64(&mut h, 8);
        f[..h.len()].copy_from_slice(&h);
        f
    }

    struct MachoSpec {
        cputype: u32,
        filetype: u32,
        dylibs: Vec<&'static str>,
        symbols: Vec<(String, Vec<u8>)>,
    }

    /// Thin Mach-O 64 with one section holding the symbols (8-aligned, so
    /// the last symbol includes padding like a real linker output) and
    /// `_`-prefixed names.
    fn build_macho(spec: &MachoSpec) -> Vec<u8> {
        let dylib_cmds: Vec<Vec<u8>> = spec
            .dylibs
            .iter()
            .map(|d| {
                let mut c = Vec::new();
                p32(&mut c, 0xc); // LC_LOAD_DYLIB
                let size = (24 + d.len() + 1 + 7) / 8 * 8;
                p32(&mut c, size as u32);
                p32(&mut c, 24);
                p32(&mut c, 0);
                p32(&mut c, 0);
                p32(&mut c, 0);
                c.extend_from_slice(d.as_bytes());
                c.resize(size, 0);
                c
            })
            .collect();
        let ncmds = 2 + dylib_cmds.len();
        let sizeofcmds = (72 + 80) + 24 + dylib_cmds.iter().map(Vec::len).sum::<usize>();
        let mut data = vec![0u8; 32 + sizeofcmds];
        align(&mut data, 8);
        let sect_off = data.len();
        let mut offs = Vec::new();
        for (_, d) in &spec.symbols {
            align(&mut data, 8);
            offs.push(data.len());
            data.extend_from_slice(d);
        }
        align(&mut data, 8);
        let sect_size = data.len() - sect_off;
        let mut strtab = vec![0u8];
        let mut nlists = Vec::new();
        for (i, (n, _)) in spec.symbols.iter().enumerate() {
            let strx = strtab.len() as u32;
            strtab.push(b'_');
            strtab.extend_from_slice(n.as_bytes());
            strtab.push(0);
            p32(&mut nlists, strx);
            nlists.push(0x0f); // N_SECT | N_EXT
            nlists.push(1);
            p16(&mut nlists, 0);
            p64(&mut nlists, offs[i] as u64); // vmaddr == file offset
        }
        let symoff = data.len();
        data.extend_from_slice(&nlists);
        let stroff = data.len();
        data.extend_from_slice(&strtab);
        let mut h = Vec::new();
        p32(&mut h, 0xfeed_facf);
        p32(&mut h, spec.cputype);
        p32(&mut h, 0);
        p32(&mut h, spec.filetype);
        p32(&mut h, ncmds as u32);
        p32(&mut h, sizeofcmds as u32);
        p32(&mut h, 0);
        p32(&mut h, 0);
        // LC_SEGMENT_64
        p32(&mut h, 0x19);
        p32(&mut h, 72 + 80);
        let mut seg = [0u8; 16];
        seg[..7].copy_from_slice(b"__TEXT\0");
        h.extend_from_slice(&seg);
        p64(&mut h, 0);
        p64(&mut h, data.len() as u64);
        p64(&mut h, 0);
        p64(&mut h, data.len() as u64);
        p32(&mut h, 5);
        p32(&mut h, 5);
        p32(&mut h, 1);
        p32(&mut h, 0);
        let mut sect = [0u8; 16];
        sect[..7].copy_from_slice(b"__const");
        h.extend_from_slice(&sect);
        h.extend_from_slice(&seg);
        p64(&mut h, sect_off as u64);
        p64(&mut h, sect_size as u64);
        p32(&mut h, sect_off as u32);
        p32(&mut h, 3);
        p32(&mut h, 0);
        p32(&mut h, 0);
        p32(&mut h, 0);
        p32(&mut h, 0);
        p32(&mut h, 0);
        p32(&mut h, 0);
        // LC_SYMTAB
        p32(&mut h, 0x2);
        p32(&mut h, 24);
        p32(&mut h, symoff as u32);
        p32(&mut h, spec.symbols.len() as u32);
        p32(&mut h, stroff as u32);
        p32(&mut h, strtab.len() as u32);
        for c in &dylib_cmds {
            h.extend_from_slice(c);
        }
        data[..h.len()].copy_from_slice(&h);
        data
    }

    // ---------------------------------------------------------------
    // Fixtures
    // ---------------------------------------------------------------

    fn entry(module_path: &str, fname: &str, vis: &str) -> String {
        format!(
            r#"{{"abiVersion":1,"modulePath":"{module_path}","name":"{fname}","visibility":"{vis}","args":null,"returns":null}}"#
        )
    }

    /// (symbols, header functions) for `app::hello:hello`, `app::hello:secret`
    /// and `app::other:x`.
    fn symbols() -> (Vec<(String, Vec<u8>)>, BTreeMap<String, String>) {
        let fns = [
            ("app::hello", "hello", "public"),
            ("app::hello", "secret", "internal"),
            ("app::other", "x", "public"),
        ];
        let mut syms = Vec::new();
        let mut map = BTreeMap::new();
        for e in ["convex_native_init", "convex_alloc", "convex_free_result"] {
            syms.push((e.to_string(), vec![0xc3]));
        }
        for (mp, n, vis) in fns {
            syms.push((format!("convex_run:{mp}:{n}"), vec![0xc3]));
            syms.push((
                format!("{MANIFEST_SYMBOL_PREFIX}{mp}:{n}"),
                entry(mp, n, vis).into_bytes(),
            ));
            map.insert(
                format!("{}:{n}", mp.strip_prefix("app::").unwrap()),
                format!("convex_run:{mp}:{n}"),
            );
        }
        (syms, map)
    }

    fn host(os: &str, arch: &str, glibc: Option<&str>) -> HostTarget {
        let (triple, env) = if os == "linux" {
            (format!("{arch}-unknown-linux-gnu"), Some("gnu".to_string()))
        } else {
            (format!("{arch}-apple-darwin"), None)
        };
        HostTarget {
            triple,
            os: os.into(),
            arch: arch.into(),
            env,
            glibc: glibc.map(String::from),
        }
    }

    fn cfg_linux() -> NativeConfig {
        NativeConfig {
            mode_on: true,
            max_artifact_bytes: DEFAULT_MAX_ARTIFACT_BYTES,
            extra_libs: vec![],
            host: host("linux", "aarch64", Some("2.39")),
        }
    }

    fn cfg_macos() -> NativeConfig {
        NativeConfig {
            host: host("macos", "aarch64", None),
            ..cfg_linux()
        }
    }

    fn header_json(bytes: &[u8], map: &BTreeMap<String, String>) -> serde_json::Value {
        serde_json::json!({
            "format": 1, "nativeAbi": 1, "abi": 1,
            "triple": "aarch64-unknown-linux-gnu", "glibcMax": "2.34",
            "sha256": hex_lower(&Sha256::digest(bytes)),
            "size": bytes.len(), "functions": map, "sdk": "0.1.0",
        })
    }

    fn carrier_with(header: &serde_json::Value, bytes: &[u8]) -> String {
        format!("{NATIVE_MAGIC}{header}\n{}", base64::encode(bytes))
    }

    fn carrier(bytes: &[u8], map: &BTreeMap<String, String>) -> String {
        carrier_with(&header_json(bytes, map), bytes)
    }

    fn ref_source(bytes: &[u8], carrier_path: &str) -> String {
        format!(
            "{NATIVE_REF_MAGIC}{}",
            serde_json::json!({"sha256": hex_lower(&Sha256::digest(bytes)), "carrier": carrier_path})
        )
    }

    fn module(path: &str, source: String) -> ModuleConfig {
        ModuleConfig {
            path: path.parse().unwrap(),
            source: source.as_str().into(),
            source_map: None,
            environment: ModuleEnvironment::Rust,
        }
    }

    fn analyze_one(cfg: &NativeConfig, path: &str, source: String) -> Result<AnalyzedModule, String> {
        let config = module(path, source);
        analyze_native_module(cfg, &config, &BTreeMap::new(), &mut BTreeMap::new())
            .unwrap()
            .map(|(_, m)| m)
            .map_err(|e| e.message.to_string())
    }

    fn analyze_set(
        cfg: &NativeConfig,
        modules: Vec<ModuleConfig>,
    ) -> Result<BTreeMap<CanonicalizedModulePath, AnalyzedModule>, String> {
        let map = modules
            .into_iter()
            .map(|m| (m.path.clone().canonicalize(), m))
            .collect();
        analyze_rust_modules_with(&map, cfg)
            .unwrap()
            .map_err(|e| e.message.to_string())
    }

    fn good_elf() -> (Vec<u8>, BTreeMap<String, String>) {
        let (syms, map) = symbols();
        (build_elf(&ElfSpec::new(EM_AARCH64, syms)), map)
    }

    fn fn_names(m: &AnalyzedModule) -> Vec<String> {
        m.functions.iter().map(|f| f.name.to_string()).collect()
    }

    #[track_caller]
    fn assert_err(r: Result<AnalyzedModule, String>, needle: &str) {
        match r {
            Ok(_) => panic!("expected error containing `{needle}`"),
            Err(e) => assert!(e.contains(needle), "`{e}` does not contain `{needle}`"),
        }
    }

    // ---------------------------------------------------------------
    // Tests
    // ---------------------------------------------------------------

    #[test]
    fn valid_elf_carrier() {
        let (bytes, map) = good_elf();
        let m = analyze_one(&cfg_linux(), "rust/hello.js", carrier(&bytes, &map)).unwrap();
        assert_eq!(fn_names(&m), ["hello", "secret"]);
        let m = analyze_one(&cfg_linux(), "rust/other.js", carrier(&bytes, &map)).unwrap();
        assert_eq!(fn_names(&m), ["x"]);
    }

    #[test]
    fn valid_macho_carrier() {
        let (syms, map) = symbols();
        let bytes = build_macho(&MachoSpec {
            cputype: CPU_ARM64,
            filetype: MH_DYLIB_T,
            dylibs: vec![
                "/usr/lib/libSystem.B.dylib",
                "/System/Library/Frameworks/Security.framework/Versions/A/Security",
            ],
            symbols: syms,
        });
        let m = analyze_one(&cfg_macos(), "rust/hello.js", carrier(&bytes, &map)).unwrap();
        assert_eq!(fn_names(&m), ["hello", "secret"]);
    }

    #[test]
    fn macho_manifest_entries_end_at_their_newline() {
        // Real entries end with `\n`; the section continues with unrelated
        // rodata after each (and after the last symbol).
        let (mut syms, map) = symbols();
        for (n, d) in syms.iter_mut() {
            if n.starts_with(MANIFEST_SYMBOL_PREFIX) {
                d.extend_from_slice(b"\ncalled `Result::unwrap()` on an `Err` value\0\0");
            }
        }
        let bytes = build_macho(&MachoSpec {
            cputype: CPU_ARM64,
            filetype: MH_DYLIB_T,
            dylibs: vec![],
            symbols: syms,
        });
        let m = analyze_one(&cfg_macos(), "rust/hello.js", carrier(&bytes, &map)).unwrap();
        assert_eq!(fn_names(&m), ["hello", "secret"]);
    }

    #[test]
    fn knob_off() {
        let (bytes, map) = good_elf();
        let cfg = NativeConfig {
            mode_on: false,
            ..cfg_linux()
        };
        assert_err(
            analyze_one(&cfg, "rust/hello.js", carrier(&bytes, &map)),
            "This backend does not run native Rust actions (RUST_NATIVE_MODE is off). Push with \
             --rust-runtime wasm, or enable it.",
        );
        let r = ref_source(&bytes, "rust/hello.js");
        assert_err(analyze_one(&cfg, "rust/other.js", r), "RUST_NATIVE_MODE is off");
    }

    #[test]
    fn sha_and_size_checks() {
        let (bytes, map) = good_elf();
        let mut h = header_json(&bytes, &map);
        h["sha256"] = "00".repeat(32).into();
        assert_err(
            analyze_one(&cfg_linux(), "rust/hello.js", carrier_with(&h, &bytes)),
            "sha256 mismatch",
        );
        let mut h = header_json(&bytes, &map);
        h["size"] = (bytes.len() + 1).into();
        assert_err(
            analyze_one(&cfg_linux(), "rust/hello.js", carrier_with(&h, &bytes)),
            "its header says",
        );
        let cfg = NativeConfig {
            max_artifact_bytes: 100,
            ..cfg_linux()
        };
        assert_err(
            analyze_one(&cfg, "rust/hello.js", carrier(&bytes, &map)),
            "RUST_NATIVE_MAX_ARTIFACT_BYTES",
        );
    }

    #[test]
    fn elf_format_checks() {
        let (syms, map) = symbols();
        let run = |spec: ElfSpec, cfg: &NativeConfig| {
            let bytes = build_elf(&spec);
            analyze_one(cfg, "rust/hello.js", carrier(&bytes, &map))
        };
        // wrong arch
        assert_err(
            run(ElfSpec::new(EM_X86_64, syms.clone()), &cfg_linux()),
            "different architecture",
        );
        // ET_REL
        let mut spec = ElfSpec::new(EM_AARCH64, syms.clone());
        spec.e_type = 1;
        assert_err(run(spec, &cfg_linux()), "ET_DYN");
        // no PT_DYNAMIC
        let mut spec = ElfSpec::new(EM_AARCH64, syms.clone());
        spec.pt_dynamic = false;
        assert_err(run(spec, &cfg_linux()), "PT_DYNAMIC");
        // ELF on a macOS backend
        assert_err(
            run(ElfSpec::new(EM_AARCH64, syms.clone()), &cfg_macos()),
            "is an ELF file but this backend runs on macos",
        );
        // not an object file at all, and 32-bit ELF
        let junk = b"hello world, definitely not a binary".to_vec();
        assert_err(
            analyze_one(&cfg_linux(), "rust/hello.js", carrier(&junk, &map)),
            "neither an ELF64 nor a Mach-O 64",
        );
        let mut elf32 = build_elf(&ElfSpec::new(EM_AARCH64, syms));
        elf32[4] = 1;
        assert_err(
            analyze_one(&cfg_linux(), "rust/hello.js", carrier(&elf32, &map)),
            "64-bit little-endian",
        );
    }

    #[test]
    fn elf_needed_allowlist() {
        let (syms, map) = symbols();
        let mut spec = ElfSpec::new(EM_AARCH64, syms);
        spec.needed = vec!["libc.so.6", "ld-linux-aarch64.so.1", "libssl.so.3"];
        let bytes = build_elf(&spec);
        let source = carrier(&bytes, &map);
        assert_err(
            analyze_one(&cfg_linux(), "rust/hello.js", source.clone()),
            "depends on the shared library `libssl.so.3`",
        );
        let cfg = NativeConfig {
            extra_libs: vec!["libssl.so.3".into()],
            ..cfg_linux()
        };
        analyze_one(&cfg, "rust/hello.js", source).unwrap();
    }

    #[test]
    fn glibc_too_new() {
        let (syms, map) = symbols();
        let mut spec = ElfSpec::new(EM_AARCH64, syms);
        spec.glibc = vec!["2.17", "2.40", "2.9"];
        let bytes = build_elf(&spec);
        assert_err(
            analyze_one(&cfg_linux(), "rust/hello.js", carrier(&bytes, &map)),
            "the Rust library needs glibc 2.40 but the backend has 2.39; rebuild with --target \
             aarch64-unknown-linux-gnu.2.39",
        );
        // 2.39 itself is fine.
        let mut spec = ElfSpec::new(EM_AARCH64, symbols().0);
        spec.glibc = vec!["2.39"];
        let bytes = build_elf(&spec);
        analyze_one(&cfg_linux(), "rust/hello.js", carrier(&bytes, &map)).unwrap();
    }

    #[test]
    fn missing_required_and_function_exports() {
        let (mut syms, map) = symbols();
        syms.retain(|(n, _)| n != "convex_free_result");
        let bytes = build_elf(&ElfSpec::new(EM_AARCH64, syms));
        assert_err(
            analyze_one(&cfg_linux(), "rust/hello.js", carrier(&bytes, &map)),
            "does not export `convex_free_result`",
        );
        let (mut syms, map) = symbols();
        syms.retain(|(n, _)| n != "convex_run:app::hello:secret");
        let bytes = build_elf(&ElfSpec::new(EM_AARCH64, syms));
        assert_err(
            analyze_one(&cfg_linux(), "rust/hello.js", carrier(&bytes, &map)),
            "convex_run:app::hello:secret",
        );
    }

    #[test]
    fn manifest_checks() {
        // no convex_mf symbols
        let (mut syms, map) = symbols();
        syms.retain(|(n, _)| !n.starts_with(MANIFEST_SYMBOL_PREFIX));
        let bytes = build_elf(&ElfSpec::new(EM_AARCH64, syms));
        assert_err(
            analyze_one(&cfg_linux(), "rust/hello.js", carrier(&bytes, &map)),
            "convex_mf:*",
        );
        // one manifest symbol missing: manifest lacks a header function
        let (mut syms, map) = symbols();
        syms.retain(|(n, _)| n != "convex_mf:app::other:x");
        let bytes = build_elf(&ElfSpec::new(EM_AARCH64, syms));
        assert_err(
            analyze_one(&cfg_linux(), "rust/hello.js", carrier(&bytes, &map)),
            "the header has `other:x` but the manifest does not",
        );
        // header lacks a manifest function
        let (syms, mut map) = symbols();
        map.remove("hello:secret");
        let bytes = build_elf(&ElfSpec::new(EM_AARCH64, syms));
        assert_err(
            analyze_one(&cfg_linux(), "rust/hello.js", carrier(&bytes, &map)),
            "the manifest has `hello:secret` but the header does not",
        );
        // header maps to a different export
        let (syms, mut map) = symbols();
        map.insert("hello:hello".into(), "convex_run:app::hello:secret".into());
        let bytes = build_elf(&ElfSpec::new(EM_AARCH64, syms));
        assert_err(
            analyze_one(&cfg_linux(), "rust/hello.js", carrier(&bytes, &map)),
            "maps to a different export",
        );
        // bad manifest JSON and abi version go through the shared code
        let (mut syms, map) = symbols();
        for (n, d) in &mut syms {
            if n == "convex_mf:app::other:x" {
                *d = b"{not json".to_vec();
            }
        }
        let bytes = build_elf(&ElfSpec::new(EM_AARCH64, syms));
        assert_err(
            analyze_one(&cfg_linux(), "rust/hello.js", carrier(&bytes, &map)),
            "invalid `convex:manifest` entry",
        );
    }

    #[test]
    fn macho_format_checks() {
        let (syms, map) = symbols();
        let spec = |cputype, filetype, dylibs: Vec<&'static str>| MachoSpec {
            cputype,
            filetype,
            dylibs,
            symbols: syms.clone(),
        };
        let run = |s: MachoSpec, cfg: &NativeConfig| {
            analyze_one(cfg, "rust/hello.js", carrier(&build_macho(&s), &map))
        };
        let ok_libs = vec!["/usr/lib/libSystem.B.dylib", "/usr/lib/libiconv.2.dylib"];
        run(spec(CPU_ARM64, MH_DYLIB_T, ok_libs.clone()), &cfg_macos()).unwrap();
        assert_err(
            run(spec(CPU_X86_64, MH_DYLIB_T, ok_libs.clone()), &cfg_macos()),
            "different architecture",
        );
        assert_err(
            run(spec(CPU_ARM64, MH_BUNDLE_T, ok_libs.clone()), &cfg_macos()),
            "MH_DYLIB",
        );
        assert_err(
            run(spec(CPU_ARM64, MH_DYLIB_T, ok_libs.clone()), &cfg_linux()),
            "is a Mach-O file but this backend runs on linux",
        );
        let bad = vec!["/usr/lib/libSystem.B.dylib", "/opt/homebrew/lib/libfoo.dylib"];
        assert_err(
            run(spec(CPU_ARM64, MH_DYLIB_T, bad.clone()), &cfg_macos()),
            "depends on the shared library `/opt/homebrew/lib/libfoo.dylib`",
        );
        let cfg = NativeConfig {
            extra_libs: vec!["/opt/homebrew/lib/libfoo.dylib".into()],
            ..cfg_macos()
        };
        run(spec(CPU_ARM64, MH_DYLIB_T, bad), &cfg).unwrap();
        // fat binary
        let mut fat = build_macho(&spec(CPU_ARM64, MH_DYLIB_T, ok_libs));
        fat[..4].copy_from_slice(&[0xca, 0xfe, 0xba, 0xbe]);
        assert_err(
            analyze_one(&cfg_macos(), "rust/hello.js", carrier(&fat, &map)),
            "fat",
        );
    }

    #[test]
    fn macho_missing_export() {
        let (mut syms, map) = symbols();
        syms.retain(|(n, _)| n != "convex_alloc");
        let bytes = build_macho(&MachoSpec {
            cputype: CPU_ARM64,
            filetype: MH_DYLIB_T,
            dylibs: vec![],
            symbols: syms,
        });
        assert_err(
            analyze_one(&cfg_macos(), "rust/hello.js", carrier(&bytes, &map)),
            "does not export `convex_alloc`",
        );
    }

    #[test]
    fn refs_resolve_against_the_carrier() {
        let (bytes, map) = good_elf();
        let set = analyze_set(
            &cfg_linux(),
            vec![
                module("rust/hello.js", carrier(&bytes, &map)),
                module("rust/other.js", ref_source(&bytes, "rust/hello.js")),
            ],
        )
        .unwrap();
        let by_path = |p: &str| &set[&p.parse::<sync_types::ModulePath>().unwrap().canonicalize()];
        assert_eq!(fn_names(by_path("rust/hello.js")), ["hello", "secret"]);
        assert_eq!(fn_names(by_path("rust/other.js")), ["x"]);
    }

    #[test]
    fn ref_errors() {
        let (bytes, map) = good_elf();
        // no carrier at all
        assert_err(
            analyze_one(&cfg_linux(), "rust/other.js", ref_source(&bytes, "rust/hello.js")),
            "not part of this push",
        );
        // carrier path is not in the push
        let r = analyze_set(
            &cfg_linux(),
            vec![module("rust/other.js", ref_source(&bytes, "rust/hello.js"))],
        );
        assert!(r.unwrap_err().contains("not part of this push"));
        // sha differs from the carrier's
        let r = analyze_set(
            &cfg_linux(),
            vec![
                module("rust/hello.js", carrier(&bytes, &map)),
                module("rust/other.js", ref_source(b"another binary", "rust/hello.js")),
            ],
        );
        assert!(r.unwrap_err().contains("refers to native library sha256"));
        // the referenced module is not a carrier
        let r = analyze_set(
            &cfg_linux(),
            vec![
                module("rust/hello.js", ref_source(&bytes, "rust/other.js")),
                module("rust/other.js", ref_source(&bytes, "rust/hello.js")),
            ],
        );
        assert!(r.unwrap_err().contains("not part of this push"));
        // bad header
        assert_err(
            analyze_one(&cfg_linux(), "rust/other.js", format!("{NATIVE_REF_MAGIC}nope")),
            "invalid native reference header",
        );
    }

    #[test]
    fn bad_carrier_header_and_base64() {
        assert_err(
            analyze_one(&cfg_linux(), "rust/hello.js", format!("{NATIVE_MAGIC}{{}}\nAAAA")),
            "invalid native carrier header",
        );
        let (bytes, map) = good_elf();
        let mut h = header_json(&bytes, &map);
        h["format"] = 2.into();
        assert_err(
            analyze_one(&cfg_linux(), "rust/hello.js", carrier_with(&h, &bytes)),
            "unsupported native carrier",
        );
        let h = header_json(&bytes, &map);
        assert_err(
            analyze_one(&cfg_linux(), "rust/hello.js", format!("{NATIVE_MAGIC}{h}\n!!!")),
            "base64",
        );
    }
}
