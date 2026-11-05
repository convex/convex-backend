/* eslint-disable no-restricted-syntax -- pure helpers; WN-10 callers convert thrown errors with ctx.crash (as rust.ts parseManifest does) */
/**
 * Native ("use rust" cdylib) CLI helpers. Pure: no process spawning, no I/O
 * besides what callers inject. See contracts/NATIVE.md section 5.2.
 */
import path from "path";
import { createHash } from "crypto";
import type { Bundle } from "../../../bundler/index.js";
import {
  cargoBuildConfig,
  cargoProfileDir,
  convexModuleName,
  parseManifestText,
  type RustBuildMode,
  type RustManifestEntry,
} from "../rust.js";

// ---------------------------------------------------------------------------
// Targets
// ---------------------------------------------------------------------------

export type NativeOs = "linux" | "macos";
export type NativeArch = "aarch64" | "x86_64";

export interface NativeTarget {
  triple: string;
  os: NativeOs;
  arch: NativeArch;
  /** `gnu` for linux, `null` for macOS. */
  env: "gnu" | null;
}

/** Buildable native targets. musl is not supported (dlopen needs glibc). */
export const NATIVE_TARGETS: readonly NativeTarget[] = [
  { triple: "aarch64-apple-darwin", os: "macos", arch: "aarch64", env: null },
  { triple: "x86_64-apple-darwin", os: "macos", arch: "x86_64", env: null },
  {
    triple: "aarch64-unknown-linux-gnu",
    os: "linux",
    arch: "aarch64",
    env: "gnu",
  },
  {
    triple: "x86_64-unknown-linux-gnu",
    os: "linux",
    arch: "x86_64",
    env: "gnu",
  },
];

export function findNativeTarget(triple: string): NativeTarget | null {
  return NATIVE_TARGETS.find((t) => t.triple === triple) ?? null;
}

/** The `target` object of `GET /api/rust_native/info` (NATIVE.md 5.1). */
export interface BackendTargetInfo {
  triple: string;
  os?: string;
  arch?: string;
  env?: string | null;
  glibc?: string | null;
}

/** Host target from `process.platform` / `process.arch` values. */
export function hostNativeTarget(
  platform: string,
  arch: string,
): NativeTarget | null {
  const os = platform === "darwin" ? "macos" : platform;
  const a = arch === "arm64" ? "aarch64" : arch === "x64" ? "x86_64" : arch;
  return NATIVE_TARGETS.find((t) => t.os === os && t.arch === a) ?? null;
}

export type NativeBuildPlan =
  | {
      kind: "host";
      /** Triple passed to `--target` and used for the artifact dir. */
      triple: string;
    }
  | {
      kind: "zigbuild";
      triple: string;
      glibc: string;
      /** `<triple>.<glibc>`, the value passed to `--target`. */
      cargoTarget: string;
    }
  | { kind: "unsupported"; reason: string };

/**
 * Pick the build strategy: plain cargo when host == backend target,
 * `cargo zigbuild --target <triple>.<glibc>` for linux gnu backends,
 * otherwise unsupported.
 */
export function selectNativeBuild(
  host: NativeTarget | null,
  backend: BackendTargetInfo,
): NativeBuildPlan {
  const bt = findNativeTarget(backend.triple);
  if (bt === null) {
    return {
      kind: "unsupported",
      reason: `The backend target ${backend.triple} is not supported for native Rust functions.`,
    };
  }
  if (host !== null && host.triple === bt.triple) {
    return { kind: "host", triple: bt.triple };
  }
  if (bt.os === "linux" && bt.env === "gnu") {
    if (!backend.glibc || !/^\d+\.\d+$/.test(backend.glibc)) {
      return {
        kind: "unsupported",
        reason: `The backend did not report a usable glibc version (${String(backend.glibc)}).`,
      };
    }
    return {
      kind: "zigbuild",
      triple: bt.triple,
      glibc: backend.glibc,
      cargoTarget: `${bt.triple}.${backend.glibc}`,
    };
  }
  return {
    kind: "unsupported",
    reason: `Cannot cross-compile native Rust functions from ${host?.triple ?? "this host"} to ${bt.triple}.`,
  };
}

export const ZIGBUILD_MISSING_HINT =
  "Cross-compiling native Rust functions needs cargo-zigbuild and zig. Install them with " +
  "`cargo install cargo-zigbuild` and `brew install zig` (or `pip install ziglang`), or deploy with `--rust-runtime wasm`.";

// ---------------------------------------------------------------------------
// Cargo command
// ---------------------------------------------------------------------------

export interface NativeCargoOptions {
  manifestPath: string;
  plan: Exclude<NativeBuildPlan, { kind: "unsupported" }>;
  mode: RustBuildMode;
  /** Existing RUSTFLAGS from the environment; kept in front of ours. */
  existingRustflags?: string | undefined;
}

export interface NativeCargoCommand {
  command: "cargo";
  args: string[];
  env: Record<string, string>;
}

/** `CARGO_TARGET_<TRIPLE>_RUSTFLAGS` variable name for a triple. */
export function targetRustflagsVar(triple: string): string {
  return `CARGO_TARGET_${triple.toUpperCase().replace(/[-.]/g, "_")}_RUSTFLAGS`;
}

export function nativeCargoCommand(o: NativeCargoOptions): NativeCargoCommand {
  const { args: profileArgs, env: profileEnv } = cargoBuildConfig(o.mode);
  const profile = cargoProfileDir(o.mode);
  const target =
    o.plan.kind === "zigbuild" ? o.plan.cargoTarget : o.plan.triple;
  const args = [
    o.plan.kind === "zigbuild" ? "zigbuild" : "build",
    "--lib",
    "--manifest-path",
    o.manifestPath,
    "--target",
    target,
    ...profileArgs,
    "--config",
    `profile.${profile}.panic="unwind"`,
    "--config",
    `profile.${profile}.strip="debuginfo"`,
  ];
  // Hardware AES/POLYVAL for RustCrypto crates pinned to aes 0.8 / polyval 0.6
  // (newer releases detect it themselves; the cfgs are then unused).
  const armv8 = o.plan.triple.startsWith("aarch64-")
    ? "--cfg aes_armv8 --cfg polyval_armv8"
    : undefined;
  const flags = [o.existingRustflags?.trim(), "--cfg convex_native", armv8]
    .filter((s) => s)
    .join(" ");
  return {
    command: "cargo",
    args,
    env: { ...profileEnv, [targetRustflagsVar(o.plan.triple)]: flags },
  };
}

/**
 * Location of the built library:
 * `<targetDir>/<triple>/<profile>/lib<crate>.{so,dylib}`.
 */
export function nativeArtifactPath(
  targetDir: string,
  triple: string,
  mode: RustBuildMode,
  crateName: string,
): string {
  const t = findNativeTarget(triple);
  if (t === null) throw new Error(`Unsupported native target ${triple}.`);
  const ext = t.os === "macos" ? "dylib" : "so";
  return path.join(
    targetDir,
    triple,
    cargoProfileDir(mode),
    `lib${crateName.replace(/-/g, "_")}.${ext}`,
  );
}

// ---------------------------------------------------------------------------
// ELF64 / Mach-O 64 reader
// ---------------------------------------------------------------------------

export const MANIFEST_SYMBOL_PREFIX = "convex_mf:";

function cstr(b: Uint8Array, off: number, limit = b.length): string {
  let end = off;
  while (end < limit && b[end] !== 0) end++;
  return new TextDecoder().decode(b.subarray(off, end));
}

function view(b: Uint8Array): DataView {
  return new DataView(b.buffer, b.byteOffset, b.byteLength);
}

function u64(v: DataView, off: number): number {
  return Number(v.getBigUint64(off, true));
}

function check(b: Uint8Array, off: number, len: number, what: string) {
  if (off < 0 || len < 0 || off + len > b.length) {
    throw new Error(`Corrupt native library: ${what} is out of bounds.`);
  }
}

interface ElfSection {
  type: number;
  offset: number;
  size: number;
  link: number;
}

interface ElfInfo {
  v: DataView;
  sections: ElfSection[];
  loads: { offset: number; vaddr: number; filesz: number }[];
}

function parseElf(b: Uint8Array): ElfInfo {
  if (b.length < 64) throw new Error("Corrupt native library: ELF too short.");
  if (b[4] !== 2) throw new Error("Only 64-bit ELF libraries are supported.");
  if (b[5] !== 1) throw new Error("Only little-endian ELF is supported.");
  const v = view(b);
  const phoff = u64(v, 32);
  const shoff = u64(v, 40);
  const phentsize = v.getUint16(54, true);
  const phnum = v.getUint16(56, true);
  const shentsize = v.getUint16(58, true);
  const shnum = v.getUint16(60, true);
  const loads: ElfInfo["loads"] = [];
  check(b, phoff, phentsize * phnum, "program headers");
  for (let i = 0; i < phnum; i++) {
    const p = phoff + i * phentsize;
    if (v.getUint32(p, true) === 1) {
      loads.push({
        offset: u64(v, p + 8),
        vaddr: u64(v, p + 16),
        filesz: u64(v, p + 32),
      });
    }
  }
  const sections: ElfSection[] = [];
  check(b, shoff, shentsize * shnum, "section headers");
  for (let i = 0; i < shnum; i++) {
    const s = shoff + i * shentsize;
    sections.push({
      type: v.getUint32(s + 4, true),
      offset: u64(v, s + 24),
      size: u64(v, s + 32),
      link: v.getUint32(s + 40, true),
    });
  }
  return { v, sections, loads };
}

function readElfManifestSymbols(b: Uint8Array): Map<string, Uint8Array> {
  const { v, sections, loads } = parseElf(b);
  const out = new Map<string, Uint8Array>();
  const dynsym = sections.find((s) => s.type === 11); // SHT_DYNSYM
  if (!dynsym) return out;
  const strtab = sections[dynsym.link];
  if (!strtab) throw new Error("Corrupt native library: bad dynsym link.");
  check(b, dynsym.offset, dynsym.size, ".dynsym");
  check(b, strtab.offset, strtab.size, ".dynstr");
  for (let o = dynsym.offset; o + 24 <= dynsym.offset + dynsym.size; o += 24) {
    const nameOff = v.getUint32(o, true);
    const shndx = v.getUint16(o + 6, true);
    if (shndx === 0) continue; // undefined
    const name = cstr(b, strtab.offset + nameOff, strtab.offset + strtab.size);
    if (!name.startsWith(MANIFEST_SYMBOL_PREFIX)) continue;
    const value = u64(v, o + 8);
    const size = u64(v, o + 16);
    const seg = loads.find(
      (l) => value >= l.vaddr && value + size <= l.vaddr + l.filesz,
    );
    if (!seg) {
      throw new Error(`Corrupt native library: symbol ${name} is not mapped.`);
    }
    const fileOff = seg.offset + (value - seg.vaddr);
    check(b, fileOff, size, `symbol ${name}`);
    out.set(
      name.slice(MANIFEST_SYMBOL_PREFIX.length),
      b.slice(fileOff, fileOff + size),
    );
  }
  return out;
}

/** Highest `GLIBC_x.y[.z]` version an ELF64 library requires, e.g. "2.34". */
export function elfGlibcMax(b: Uint8Array): string | null {
  const { v, sections } = parseElf(b);
  let best: number[] | null = null;
  for (const s of sections) {
    if (s.type !== 0x6ffffffe) continue; // SHT_GNU_verneed
    const strtab = sections[s.link];
    if (!strtab) continue;
    check(b, s.offset, s.size, ".gnu.version_r");
    let n = s.offset;
    for (;;) {
      const cnt = v.getUint16(n + 2, true);
      const aux = v.getUint32(n + 8, true);
      const next = v.getUint32(n + 12, true);
      let a = n + aux;
      for (let i = 0; i < cnt; i++) {
        const name = cstr(b, strtab.offset + v.getUint32(a + 8, true));
        const m = /^GLIBC_(\d+(?:\.\d+)+)$/.exec(name);
        if (m) {
          const ver = m[1].split(".").map(Number);
          if (best === null || compareVersions(ver, best) > 0) best = ver;
        }
        a += v.getUint32(a + 12, true);
      }
      if (next === 0) break;
      n += next;
    }
  }
  return best === null ? null : best.join(".");
}

function compareVersions(a: number[], b: number[]): number {
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const d = (a[i] ?? 0) - (b[i] ?? 0);
    if (d !== 0) return d;
  }
  return 0;
}

function readMachOManifestSymbols(b: Uint8Array): Map<string, Uint8Array> {
  const v = view(b);
  const out = new Map<string, Uint8Array>();
  if (b.length < 32)
    throw new Error("Corrupt native library: Mach-O too short.");
  const ncmds = v.getUint32(16, true);
  let p = 32;
  const sects: { addr: number; size: number; offset: number }[] = [];
  let symtab: {
    symoff: number;
    nsyms: number;
    stroff: number;
    strsize: number;
  } | null = null;
  for (let i = 0; i < ncmds; i++) {
    check(b, p, 8, "load command");
    const cmd = v.getUint32(p, true);
    const cmdsize = v.getUint32(p + 4, true);
    if (cmd === 0x19) {
      // LC_SEGMENT_64
      const nsects = v.getUint32(p + 64, true);
      for (let s = 0; s < nsects; s++) {
        const so = p + 72 + s * 80;
        check(b, so, 80, "section");
        sects.push({
          addr: u64(v, so + 32),
          size: u64(v, so + 40),
          offset: v.getUint32(so + 48, true),
        });
      }
    } else if (cmd === 0x2) {
      symtab = {
        symoff: v.getUint32(p + 8, true),
        nsyms: v.getUint32(p + 12, true),
        stroff: v.getUint32(p + 16, true),
        strsize: v.getUint32(p + 20, true),
      };
    }
    p += cmdsize;
  }
  if (!symtab) return out;
  check(b, symtab.symoff, symtab.nsyms * 16, "symbol table");
  check(b, symtab.stroff, symtab.strsize, "string table");
  // nlist_64 has no size: a symbol extends to the next symbol in its section.
  const defined: { name: string; sect: number; value: number }[] = [];
  for (let i = 0; i < symtab.nsyms; i++) {
    const o = symtab.symoff + i * 16;
    const type = b[o + 4];
    if ((type & 0xe0) !== 0 || (type & 0x0e) !== 0x0e) continue; // stab / not N_SECT
    const nameOff = v.getUint32(o, true);
    let name = cstr(b, symtab.stroff + nameOff, symtab.stroff + symtab.strsize);
    if (name.startsWith("_")) name = name.slice(1);
    defined.push({ name, sect: b[o + 5], value: u64(v, o + 8) });
  }
  for (const sym of defined) {
    if (!sym.name.startsWith(MANIFEST_SYMBOL_PREFIX)) continue;
    const sec = sects[sym.sect - 1];
    if (!sec)
      throw new Error(`Corrupt native library: bad section for ${sym.name}.`);
    let end = sec.addr + sec.size;
    for (const o of defined) {
      if (o.sect === sym.sect && o.value > sym.value && o.value < end) {
        end = o.value;
      }
    }
    const start = sec.offset + (sym.value - sec.addr);
    const len = end - sym.value;
    check(b, start, len, `symbol ${sym.name}`);
    let bytes = b.slice(start, start + len);
    // The span can run past the entry into unrelated data (the last symbol of a codegen unit),
    // so stop at the entry's terminating newline; fall back to trimming alignment padding.
    const nl = bytes.indexOf(0x0a);
    let n = nl >= 0 ? nl : bytes.length;
    while (n > 0 && bytes[n - 1] === 0) n--;
    bytes = bytes.subarray(0, n);
    out.set(sym.name.slice(MANIFEST_SYMBOL_PREFIX.length), bytes);
  }
  return out;
}

/**
 * Bytes of every exported `convex_mf:<modulePath>:<name>` symbol, keyed by
 * `<modulePath>:<name>`. Accepts ELF64 LE and thin Mach-O 64 LE.
 */
export function readManifestSymbols(lib: Uint8Array): Map<string, Uint8Array> {
  if (lib.length >= 4) {
    if (
      lib[0] === 0x7f &&
      lib[1] === 0x45 &&
      lib[2] === 0x4c &&
      lib[3] === 0x46
    ) {
      return readElfManifestSymbols(lib);
    }
    const magic = view(lib).getUint32(0, true);
    if (magic === 0xfeedfacf) return readMachOManifestSymbols(lib);
    if (magic === 0xbebafeca || magic === 0xcafebabe) {
      throw new Error(
        "Universal (fat) Mach-O libraries are not supported; build for a single architecture.",
      );
    }
  }
  throw new Error("Native library is neither an ELF64 nor a Mach-O 64 file.");
}

/** Manifest entries of a native library (ELF64 or Mach-O 64). */
export function parseNativeManifest(lib: Uint8Array): RustManifestEntry[] {
  const dec = new TextDecoder();
  const text = [...readManifestSymbols(lib).entries()]
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
    .map(([, bytes]) => dec.decode(bytes))
    .join("\n");
  return parseManifestText(text);
}

// ---------------------------------------------------------------------------
// Size thresholds
// ---------------------------------------------------------------------------

export const NATIVE_SIZE_WARN_BYTES = 100 * 1024 * 1024;
export const NATIVE_SIZE_ERROR_BYTES = 180 * 1024 * 1024;

export type NativeSizeCheck =
  | { level: "ok" }
  | { level: "warn" | "error"; message: string };

/** Check the pushed size (carrier source length, i.e. base64) in bytes. */
export function checkNativeSize(bytes: number): NativeSizeCheck {
  const mb = (bytes / (1024 * 1024)).toFixed(1);
  if (bytes > NATIVE_SIZE_ERROR_BYTES) {
    return {
      level: "error",
      message: `The native Rust library is ${mb} MB once encoded, over the 180 MB limit (pushes are capped at 200 MB). Reduce dependencies or enable LTO.`,
    };
  }
  if (bytes > NATIVE_SIZE_WARN_BYTES) {
    return {
      level: "warn",
      message: `The native Rust library is ${mb} MB once encoded; pushes are capped at 200 MB.`,
    };
  }
  return { level: "ok" };
}

// ---------------------------------------------------------------------------
// Carrier and ref modules
// ---------------------------------------------------------------------------

export interface NativeArtifactInfo {
  triple: string;
  glibcMax: string | null;
  sdk: string;
}

export function sha256Hex(bytes: Uint8Array): string {
  return createHash("sha256").update(bytes).digest("hex");
}

/** `{"<module>:<name>": "convex_run:<modulePath>:<name>"}`, sorted by key. */
export function nativeFunctionMap(
  entries: RustManifestEntry[],
): Record<string, string> {
  const pairs = entries.map(
    (e) =>
      [
        `${convexModuleName(e.modulePath)}:${e.name}`,
        `convex_run:${e.modulePath}:${e.name}`,
      ] as const,
  );
  pairs.sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
  return Object.fromEntries(pairs);
}

export function carrierSource(
  binary: Uint8Array,
  entries: RustManifestEntry[],
  info: NativeArtifactInfo,
): string {
  const header = {
    format: 1,
    nativeAbi: 1,
    abi: 1,
    triple: info.triple,
    glibcMax: info.glibcMax,
    sha256: sha256Hex(binary),
    size: binary.length,
    functions: nativeFunctionMap(entries),
    sdk: info.sdk,
  };
  return (
    "//convex-rust-native v1\n" +
    JSON.stringify(header) +
    "\n" +
    Buffer.from(binary).toString("base64")
  );
}

export function refSource(sha256: string, carrierPath: string): string {
  return (
    "//convex-rust-native-ref v1\n" +
    JSON.stringify({ sha256, carrier: carrierPath })
  );
}

/**
 * Replacement for `rustModuleConfigs`: the first module (sorted) carries the
 * binary, every other module is a small reference to it.
 */
export function nativeModuleConfigs(
  binary: Uint8Array,
  entries: RustManifestEntry[],
  info: NativeArtifactInfo,
): Bundle[] {
  const names = Array.from(
    new Set(entries.map((e) => convexModuleName(e.modulePath))),
  ).sort();
  if (names.length === 0) return [];
  const carrierPath = `rust/${names[0]}.js`;
  const sha = sha256Hex(binary);
  return names.map((name, i) => ({
    path: `rust/${name}.js`,
    source:
      i === 0
        ? carrierSource(binary, entries, info)
        : refSource(sha, carrierPath),
    environment: "rust" as const,
  }));
}
