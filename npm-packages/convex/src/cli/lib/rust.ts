import path from "path";
import { spawn, spawnSync } from "child_process";
import { createHash } from "crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  statSync,
  rmSync,
  writeFileSync,
} from "fs";
import { chalkStderr } from "chalk";
import { Context } from "../../bundler/context.js";
import { Bundle } from "../../bundler/index.js";
import { logMessage, logVerbose, logWarning } from "../../bundler/log.js";
import { promptYesNo } from "./utils/prompts.js";
import { RUST_TARGET, syncManagedPatch } from "./rust/scaffold.js";
import { checkRustToolchain, defaultExec, Exec } from "./rust/toolchain.js";
import { prepareRustApiForBuild } from "../codegen_templates/rust_api.js";
import { describeWasmSize } from "./rust/size.js";
import { validatorToType } from "../codegen_templates/validator_helpers.js";
import { ConvexValidator } from "./deployApi/validator.js";
import {
  cargoTreeArgs,
  diagnoseCargoFailure,
  formatFindings,
} from "./rust/diagnostics.js";

/** Name of the wasm custom section holding the manifest (contracts/ABI.md). */
export const MANIFEST_SECTION = "convex:manifest";
export { RUST_TARGET };

/** `dev` = fast incremental profile; `release` = full release (+ wasm-opt). */
export type RustBuildMode = "dev" | "release";
let buildMode: RustBuildMode = "release";
/** Benchmark flavour: `wasm-opt -O3` without `-g` (set by CONVEX_RUST_PROFILE=release). */
let perfBuild = false;

/**
 * Applies the `CONVEX_RUST_PROFILE=release|dev` override to the mode a command
 * asks for. Only `release` has an effect (it makes `dev` build the full
 * release profile and run the performance `wasm-opt -O3`); unset or `dev`
 * keeps the command's own mode.
 */
export function resolveRustProfile(
  mode: RustBuildMode,
  env: Record<string, string | undefined> = process.env,
): { mode: RustBuildMode; perf: boolean } {
  const v = env.CONVEX_RUST_PROFILE;
  if (v === "release") return { mode: "release", perf: true };
  if (v !== undefined && v !== "" && v !== "dev") {
    logWarning(`Ignoring CONVEX_RUST_PROFILE=${v} (expected release or dev).`);
  }
  return { mode, perf: false };
}

export function setRustBuildMode(mode: RustBuildMode) {
  const r = resolveRustProfile(mode);
  buildMode = r.mode;
  perfBuild = r.perf;
}
export function getRustBuildMode(): RustBuildMode {
  return buildMode;
}

/** Cargo profile (and target subdirectory) for a build mode. */
export function cargoProfileDir(mode: RustBuildMode): string {
  return mode === "dev" ? "convex-dev" : "release";
}

/** Cargo args and env for a build mode. The dev profile is defined via env so
 * the user's Cargo.toml needs no `[profile]` section. */
export function cargoBuildConfig(mode: RustBuildMode): {
  args: string[];
  env: Record<string, string>;
} {
  if (mode === "release") return { args: ["--release"], env: {} };
  return {
    args: ["--profile", "convex-dev"],
    env: {
      CARGO_PROFILE_CONVEX_DEV_INHERITS: "release",
      CARGO_PROFILE_CONVEX_DEV_OPT_LEVEL: "1",
      CARGO_PROFILE_CONVEX_DEV_INCREMENTAL: "true",
      CARGO_PROFILE_CONVEX_DEV_LTO: "false",
      CARGO_PROFILE_CONVEX_DEV_CODEGEN_UNITS: "16",
      CARGO_PROFILE_CONVEX_DEV_DEBUG: "false",
    },
  };
}

export interface RustManifestEntry {
  abiVersion: number;
  /** Rust `module_path!()`, e.g. `bench_rust::cpu`. */
  modulePath: string;
  name: string;
  visibility: "public" | "internal";
  args: unknown | null;
  returns: unknown | null;
  /** `"http"` for `#[http_action]` functions; absent for actions. */
  kind?: string | undefined;
}

/** Does `<functionsDir>/rust/Cargo.toml` exist? */
export function hasRustFunctions(ctx: Context, functionsDir: string): boolean {
  return ctx.fs.exists(path.join(functionsDir, "rust", "Cargo.toml"));
}

/**
 * Convex module name for a Rust `module_path!()`: the segments after the
 * crate name joined by `/` (`bench_rust::cpu` -> `cpu`, `app::a::b` -> `a/b`).
 */
export function convexModuleName(modulePath: string): string {
  const segments = modulePath.split("::");
  if (segments.length < 2) {
    throw new Error(
      `Rust function in the crate root (module path "${modulePath}") is not supported; move it into a module.`,
    );
  }
  return segments.slice(1).join("/");
}

/** Parse the `convex:manifest` custom section(s) out of wasm bytes. */
export function parseManifest(wasm: Uint8Array): RustManifestEntry[] {
  let module: WebAssembly.Module;
  try {
    module = new WebAssembly.Module(wasm as BufferSource);
  } catch (e: any) {
    throw new Error(`Invalid WebAssembly module: ${e?.message ?? e}`);
  }
  const decoder = new TextDecoder("utf-8");
  const entries: RustManifestEntry[] = [];
  for (const section of WebAssembly.Module.customSections(
    module,
    MANIFEST_SECTION,
  )) {
    const text = decoder.decode(section);
    for (const line of text.split("\n")) {
      const trimmed = line.trim();
      if (trimmed === "") continue;
      entries.push(JSON.parse(trimmed) as RustManifestEntry);
    }
  }
  const seen = new Set<string>();
  for (const e of entries) {
    if (e.abiVersion !== entries[0].abiVersion) {
      throw new Error(
        `Rust manifest has mixed abiVersion values (${entries[0].abiVersion} and ${e.abiVersion}).`,
      );
    }
    const key = `${e.modulePath}:${e.name}`;
    if (seen.has(key)) {
      throw new Error(`Rust manifest has duplicate function ${key}.`);
    }
    seen.add(key);
    convexModuleName(e.modulePath); // validates
  }
  return entries;
}

/** Group manifest entries by Convex module name (e.g. `cpu`, `a/b`). */
export function groupByModule(
  entries: RustManifestEntry[],
): Map<string, RustManifestEntry[]> {
  const grouped = new Map<string, RustManifestEntry[]>();
  for (const e of entries) {
    const name = convexModuleName(e.modulePath);
    const list = grouped.get(name) ?? [];
    list.push(e);
    grouped.set(name, list);
  }
  return grouped;
}

/**
 * One module per distinct modulePath, all carrying the whole wasm (the backend
 * dedups by hash). Path convention matches node modules: `rust/<module>.js`,
 * so function path `rust/cpu:cpu_001` resolves to module `rust/cpu`.
 */
export function rustModuleConfigs(
  wasm: Uint8Array,
  entries: RustManifestEntry[],
): Bundle[] {
  const source = Buffer.from(wasm).toString("base64");
  return Array.from(groupByModule(entries).keys())
    .sort()
    .map((name) => ({
      path: `rust/${name}.js`,
      source,
      environment: "rust" as const,
    }));
}

/**
 * Streaming filter for cargo's stderr: drops the "Patch `reqwest ...` was not
 * used in the crate graph" warning and its follow-up note lines (the patch is
 * only installed when reqwest is in the graph, so this is a safety net).
 * Feed chunks to `write`, call `end` once; both return text to forward.
 */
export class PatchWarningFilter {
  private pending = "";
  private skipping = false;

  write(chunk: string): string {
    this.pending += chunk;
    const nl = this.pending.lastIndexOf("\n");
    if (nl < 0) return "";
    const complete = this.pending.slice(0, nl);
    this.pending = this.pending.slice(nl + 1);
    return complete
      .split("\n")
      .map((l) => this.line(l))
      .filter((l): l is string => l !== null)
      .map((l) => l + "\n")
      .join("");
  }

  end(): string {
    const rest = this.pending;
    this.pending = "";
    if (rest === "") return "";
    return this.line(rest) ?? "";
  }

  /** Returns the line to keep, or null to drop it. */
  private line(l: string): string | null {
    if (
      /^warning: [Pp]atch `(reqwest|convex_fn)\b.*was not used in the crate graph/.test(
        l,
      )
    ) {
      this.skipping = true;
      return null;
    }
    if (this.skipping) {
      // The note is unindented prose; cargo status lines are indented and
      // diagnostics start with `warning`/`error`.
      if (l !== "" && /^\S/.test(l) && !/^(warning|error)\b/.test(l)) {
        return null;
      }
      // Newer cargo: `help: ...` followed by deeply indented continuation lines
      // (cargo status lines are indented at most 5 columns).
      if (/^ {6}\S/.test(l)) return null;
      // Variant when another patch candidate exists: `  |`, `  = help: ...`
      // and a tab-indented path.
      if (
        /^\s*\|\s*$/.test(l) ||
        /^\s*= (help|note):/.test(l) ||
        /^\s*\t/.test(l)
      )
        return null;
      this.skipping = false;
    }
    return l;
  }
}

/** One-shot form of {@link PatchWarningFilter}. */
export function filterPatchWarning(stderr: string): string {
  const f = new PatchWarningFilter();
  return f.write(stderr) + f.end();
}

interface CargoResult {
  status: number | null;
  error?: NodeJS.ErrnoException | undefined;
  stdout: string;
  /** Cargo's stderr (after the patch-warning filter), capped for diagnostics. */
  stderr: string;
}

/** Cap on stderr kept for diagnostics (the head holds the first errors). */
const MAX_CAPTURED_STDERR = 8 * 1024 * 1024;

/**
 * Runs cargo, streaming stderr live (minus the unused-patch warning). stdout
 * is inherited, or captured when `inherit` is false.
 */
/**
 * Args and cwd for a cargo invocation. Cargo (and rustup) read
 * `.cargo/config.toml` and `rust-toolchain.toml` relative to the cwd, not the
 * manifest, so the cwd is the crate's directory.
 */
export function cargoSpawnConfig(
  manifestPath: string,
  args: string[],
): { args: string[]; cwd: string } {
  const abs = path.resolve(manifestPath);
  return {
    args: args.map((a) => (a === manifestPath ? abs : a)),
    cwd: path.dirname(abs),
  };
}

function runCargo(
  manifestPath: string,
  args: string[],
  inherit: boolean,
  env: Record<string, string> = {},
): Promise<CargoResult> {
  return new Promise((resolve) => {
    const cfg = cargoSpawnConfig(manifestPath, args);
    const child = spawn("cargo", cfg.args, {
      cwd: cfg.cwd,
      env: { ...process.env, ...env },
      stdio: ["ignore", inherit ? "inherit" : "pipe", "pipe"],
    });
    const filter = new PatchWarningFilter();
    let stdout = "";
    let stderr = "";
    const forward = (out: string) => {
      if (!out) return;
      process.stderr.write(out);
      if (stderr.length < MAX_CAPTURED_STDERR) stderr += out;
    };
    let error: NodeJS.ErrnoException | undefined;
    child.stdout?.setEncoding("utf8");
    child.stdout?.on("data", (d: string) => (stdout += d));
    child.stderr?.setEncoding("utf8");
    child.stderr?.on("data", (d: string) => {
      forward(filter.write(d));
    });
    child.on("error", (e) => {
      error = e;
    });
    child.on("close", (status) => {
      forward(filter.end());
      resolve({ status, error, stdout, stderr });
    });
  });
}

async function rustCrash(ctx: Context, message: string): Promise<never> {
  return await ctx.crash({
    exitCode: 1,
    errorType: "invalid filesystem data",
    printedMessage: chalkStderr.red(message),
  });
}

/**
 * Actionable hints for a failed build (see rust/diagnostics.ts), including
 * `cargo tree -i` output for the offending crate. Empty string when nothing
 * is recognised. Never throws.
 */
export async function buildFailureHints(
  manifestPath: string,
  stderr: string,
  runTree: (manifestPath: string, crate: string) => Promise<string> = async (
    m,
    crate,
  ) => {
    const r = await runCargo(
      m,
      [...cargoTreeArgs(crate), "--manifest-path", m],
      false,
    );
    return r.status === 0 ? r.stdout : "";
  },
): Promise<string> {
  try {
    const findings = diagnoseCargoFailure(stderr);
    const trees: Record<string, string> = {};
    for (const crate of new Set(findings.map((f) => f.crate))) {
      if (!crate || Object.keys(trees).length >= 3) continue;
      try {
        trees[crate] = await runTree(manifestPath, crate);
      } catch {
        // best effort
      }
    }
    const text = formatFindings(findings, trees);
    return text === "" ? "" : "\n" + text;
  } catch (e: any) {
    logVerbose(`Could not diagnose the build failure: ${e?.message ?? e}`);
    return "";
  }
}

/** Locate the release wasm for the crate via `cargo metadata`. */
export async function findRustWasm(
  ctx: Context,
  manifestPath: string,
): Promise<string> {
  const r = await runCargo(
    manifestPath,
    [
      "metadata",
      "--format-version",
      "1",
      "--no-deps",
      "--manifest-path",
      manifestPath,
    ],
    false,
  );
  if (r.error || r.status !== 0) {
    return await rustCrash(
      ctx,
      `Rust build failed: \`cargo metadata\` failed for ${manifestPath}.`,
    );
  }
  const meta = JSON.parse(r.stdout);
  const manifestAbs = path.resolve(manifestPath);
  const pkg =
    meta.packages.find(
      (p: any) => path.resolve(p.manifest_path) === manifestAbs,
    ) ?? meta.packages[0];
  const lib =
    pkg.targets.find((t: any) => t.kind.includes("cdylib")) ??
    pkg.targets.find((t: any) => t.kind.includes("lib")) ??
    pkg.targets[0];
  const libName = String(lib?.name ?? pkg.name).replace(/-/g, "_");
  return path.join(
    meta.target_directory,
    RUST_TARGET,
    cargoProfileDir(buildMode),
    `${libName}.wasm`,
  );
}

let toolchainOk = new Set<string>();

/** Verifies rustup/cargo/target; offers to install the target in a TTY. */
export async function ensureRustToolchain(
  ctx: Context,
  rustDir: string,
  deps: { exec?: Exec; isTTY?: boolean; prompt?: typeof promptYesNo } = {},
): Promise<void> {
  if (toolchainOk.has(rustDir)) return;
  const exec = deps.exec ?? defaultExec;
  const isTTY = deps.isTTY ?? !!process.stdin.isTTY;
  let status = checkRustToolchain(rustDir, exec);
  if (status.ok) {
    toolchainOk.add(rustDir);
    return;
  }
  if (status.fix && isTTY) {
    logMessage(chalkStderr.yellow(status.message));
    const yes = await (deps.prompt ?? promptYesNo)(ctx, {
      message: `Run \`${status.fix.join(" ")}\` now?`,
      default: true,
    });
    if (yes) {
      const [cmd, ...args] = status.fix;
      const r = exec(cmd, args, { cwd: rustDir, inherit: true });
      if (r.status === 0) {
        status = checkRustToolchain(rustDir, exec);
        if (status.ok) {
          toolchainOk.add(rustDir);
          return;
        }
      }
    }
  }
  await rustCrash(ctx, `Rust toolchain check failed.\n${status.message}`);
}

/** Test hook. */
export function resetToolchainCache() {
  toolchainOk = new Set();
}

// Panic locations (file:line:col) come from the SDK's panic hook, which reads
// core::panic::Location (strings in the data section), not the name section;
// so the performance flavour drops -g. The size flavour keeps names for traps.
const WASM_OPT_FEATURES = [
  // features rustc emits for wasm32-wasip1; not --all-features, which
  // can emit things runtimes don't accept
  "--enable-bulk-memory",
  "--enable-sign-ext",
  "--enable-mutable-globals",
  "--enable-nontrapping-float-to-int",
  "--enable-multivalue",
];
const WASM_OPT_FLAGS = ["-Oz", "-g", ...WASM_OPT_FEATURES];
const WASM_OPT_PERF_FLAGS = ["-O3", ...WASM_OPT_FEATURES];

function wasmOptVersion(): string | null {
  const r = spawnSync("wasm-opt", ["--version"], { encoding: "utf8" });
  return r.status === 0 ? r.stdout.trim() : null;
}

/**
 * `wasm-opt -Oz` with a content-addressed cache next to the wasm (under the
 * cargo target dir), keyed by sha256(input) + flags + wasm-opt version. An
 * unchanged redeploy hits the cache (wasm-opt takes ~minute on big crates).
 * Marks already-optimized outputs too, because cargo does not rebuild (and so
 * hands us the optimized file again) when nothing changed.
 * Returns true when the file was replaced.
 */
export function optimizeWasm(
  wasmPath: string,
  run?: (wasmPath: string) => boolean,
  version: () => string | null = wasmOptVersion,
  perf: boolean = false,
): boolean {
  const flags = perf ? WASM_OPT_PERF_FLAGS : WASM_OPT_FLAGS;
  run ??= (p) => runWasmOpt(p, flags);
  const v = version();
  if (v === null) return run(wasmPath); // wasm-opt missing: logs and skips
  const salt = createHash("sha256")
    .update(v + "\0" + flags.join(" "))
    .digest("hex")
    .slice(0, 16);
  const dir = path.join(path.dirname(wasmPath), "convex-wasm-opt-cache");
  const sha = (b: Uint8Array) =>
    createHash("sha256").update(salt).update(b).digest("hex");
  const key = sha(readFileSync(wasmPath));
  const optimized = path.join(dir, `${key}.wasm`);
  const done = path.join(dir, `${key}.done`);
  if (existsSync(done)) {
    logVerbose("wasm-opt: input already optimized (cache hit)");
    return false;
  }
  if (existsSync(optimized)) {
    const tmp = `${wasmPath}.cached`;
    copyFileSync(optimized, tmp);
    renameSync(tmp, wasmPath);
    logVerbose("wasm-opt: cache hit");
    return true;
  }
  const replaced = run(wasmPath);
  try {
    mkdirSync(dir, { recursive: true });
    if (replaced) {
      const out = new Uint8Array(readFileSync(wasmPath));
      copyFileSync(wasmPath, optimized);
      writeFileSync(path.join(dir, `${sha(out)}.done`), "");
    } else {
      writeFileSync(done, ""); // nothing to gain; do not retry
    }
  } catch (e: any) {
    logVerbose(`could not write wasm-opt cache: ${e?.message ?? e}`);
  }
  return replaced;
}

/** Runs `wasm-opt -Oz` if available; returns true when the file was replaced. */
function runWasmOpt(wasmPath: string, flags: string[]): boolean {
  const out = `${wasmPath}.opt`;
  const r = spawnSync("wasm-opt", [...flags, wasmPath, "-o", out], {
    stdio: ["ignore", "ignore", "pipe"],
    encoding: "utf8",
  });
  if ((r.error as any)?.code === "ENOENT") {
    logVerbose("wasm-opt not found on PATH; skipping wasm optimization");
    return false;
  }
  if (r.status !== 0) {
    logVerbose(`wasm-opt failed (${r.status}); using unoptimized wasm`);
    rmSync(out, { force: true });
    return false;
  }
  try {
    const before = new Uint8Array(readFileSync(wasmPath));
    const after = new Uint8Array(readFileSync(out));
    // The size flavour must shrink; the performance flavour (-O3) may not.
    if (
      (flags !== WASM_OPT_PERF_FLAGS && after.length >= before.length) ||
      parseManifest(after).length !== parseManifest(before).length
    ) {
      logVerbose("wasm-opt output not smaller or lost manifest; ignoring");
      rmSync(out, { force: true });
      return false;
    }
    renameSync(out, wasmPath);
    logMessage(
      chalkStderr.gray(`wasm-opt: ${before.length} -> ${after.length} bytes`),
    );
    return true;
  } catch (e: any) {
    logVerbose(`wasm-opt output unusable: ${e?.message ?? e}`);
    rmSync(out, { force: true });
    return false;
  }
}

/** Files under `<rustDir>` that affect the build (ignoring `target/`). */
function recordRustInputs(ctx: Context, rustDir: string) {
  const visit = (dir: string) => {
    let entries;
    try {
      entries = ctx.fs.listDir(dir);
    } catch {
      return;
    }
    for (const e of entries) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) visit(p);
      else {
        try {
          ctx.fs.stat(p);
        } catch {
          // raced with deletion
        }
      }
    }
  };
  for (const f of [
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    path.join(".cargo", "config.toml"),
  ]) {
    ctx.fs.exists(path.join(rustDir, f));
    try {
      ctx.fs.stat(path.join(rustDir, f));
    } catch {
      // optional
    }
  }
  visit(path.join(rustDir, "src"));
  // build.rs and similar top-level sources
  try {
    for (const e of ctx.fs.listDir(rustDir)) {
      if (e.isFile() && e.name.endsWith(".rs")) {
        ctx.fs.stat(path.join(rustDir, e.name));
      }
    }
  } catch {
    // ignore
  }
}

/**
 * Sidecar `<wasm>.buildinfo.json` (profile, wasm-opt version, sizes) that the
 * bench scripts copy into bench/convex/.build-info.json. Best effort.
 */
function writeBuildInfo(wasmPath: string, sizeBefore: number, perf: boolean) {
  try {
    const file = `${wasmPath}.buildinfo.json`;
    const bytes = statSync(wasmPath).size;
    let bytesBeforeOpt = sizeBefore;
    try {
      // cargo may hand us last time's optimized output again; keep the
      // original pre-opt size in that case.
      const prev = JSON.parse(readFileSync(file, "utf8"));
      if (prev.bytes === sizeBefore && typeof prev.bytesBeforeOpt === "number")
        bytesBeforeOpt = prev.bytesBeforeOpt;
    } catch {
      // no previous sidecar
    }
    writeFileSync(
      file,
      JSON.stringify({
        profile: "release",
        wasmOpt: wasmOptVersion(),
        wasmOptFlags: (perf ? WASM_OPT_PERF_FLAGS : WASM_OPT_FLAGS).join(" "),
        bytesBeforeOpt,
        bytes,
      }),
    );
  } catch (e: any) {
    logVerbose(`could not write build info: ${e?.message ?? e}`);
  }
}

/** Run `cargo build` for the Rust crate; returns the .wasm path. */
export async function buildRustCrate(
  ctx: Context,
  functionsDir: string,
): Promise<string> {
  const rustDir = path.join(functionsDir, "rust");
  const manifestPath = path.join(rustDir, "Cargo.toml");
  await ensureRustToolchain(ctx, rustDir);
  if (!existsSync(path.join(rustDir, "Cargo.lock"))) {
    // Resolve (not build) first, so a transitive reqwest is seen by the sync.
    await runCargo(
      manifestPath,
      ["metadata", "--format-version", "1", "--manifest-path", manifestPath],
      false,
    );
  }
  syncManagedPatch(rustDir);
  await prepareRustApiForBuild(ctx, functionsDir);
  const cfg = cargoBuildConfig(buildMode);
  logMessage(
    chalkStderr.gray(
      `Building Rust functions (${manifestPath}, ${buildMode === "dev" ? "dev profile" : "release"})...`,
    ),
  );
  const r = await runCargo(
    manifestPath,
    [
      "build",
      ...cfg.args,
      "--target",
      RUST_TARGET,
      "--manifest-path",
      manifestPath,
    ],
    true,
    cfg.env,
  );
  if ((r.error as any)?.code === "ENOENT") {
    return await rustCrash(
      ctx,
      `Rust build failed: \`cargo\` was not found. Install Rust from https://rustup.rs, then run:\n  rustup target add ${RUST_TARGET}`,
    );
  }
  if (r.status !== 0) {
    const hints = await buildFailureHints(manifestPath, r.stderr);
    return await rustCrash(
      ctx,
      `Rust build failed (cargo exited with ${r.status}).\nIf the error mentions a missing target, run:\n  rustup target add ${RUST_TARGET}${hints}`,
    );
  }
  const wasmPath = await findRustWasm(ctx, manifestPath);
  if (buildMode === "release") {
    const before = statSync(wasmPath).size;
    optimizeWasm(wasmPath, undefined, undefined, perfBuild);
    writeBuildInfo(wasmPath, before, perfBuild);
  }
  try {
    const { message, warning } = describeWasmSize(
      new Uint8Array(readFileSync(wasmPath)),
    );
    logMessage(chalkStderr.gray(message));
    if (warning) logWarning(chalkStderr.yellow(warning));
  } catch (e: any) {
    logVerbose(`Could not report wasm size: ${e?.message ?? e}`);
  }
  return wasmPath;
}

/** Build the crate and produce its ModuleConfigs (empty if no Rust crate). */
export async function bundleRust(
  ctx: Context,
  functionsDir: string,
): Promise<Bundle[]> {
  if (!hasRustFunctions(ctx, functionsDir)) return [];
  const wasmPath = await buildRustCrate(ctx, functionsDir);
  const wasm = new Uint8Array(readFileSync(wasmPath));
  let entries: RustManifestEntry[];
  try {
    entries = parseManifest(wasm);
  } catch (e: any) {
    return await rustCrash(ctx, `Rust build failed: ${e?.message ?? e}`);
  }
  const modules = rustModuleConfigs(wasm, entries);
  // Register build inputs with the dev watcher (after the build, since cargo
  // rewrites Cargo.lock). `target/` is deliberately never observed.
  recordRustInputs(ctx, path.join(functionsDir, "rust"));
  logVerbose(`Rust modules: ${modules.map((m) => m.path).join(", ")}`);
  return modules;
}

/**
 * Is any build input (`Cargo.toml`, `Cargo.lock`, `src/**`) newer than the
 * wasm? Guards codegen against reading a stale wasm (for example one left by
 * an earlier version of the code, or by another crate with the same name in a
 * shared `CARGO_TARGET_DIR`).
 */
export function rustSourcesNewerThan(
  rustDir: string,
  wasmPath: string,
): boolean {
  let wasmTime: number;
  try {
    wasmTime = statSync(wasmPath).mtimeMs;
  } catch {
    return true;
  }
  const newer = (p: string): boolean => {
    let st;
    try {
      st = statSync(p);
    } catch {
      return false;
    }
    if (!st.isDirectory()) return st.mtimeMs > wasmTime;
    return readdirSync(p).some((name) => newer(path.join(p, name)));
  };
  return ["Cargo.toml", "Cargo.lock", "src"].some((f) =>
    newer(path.join(rustDir, f)),
  );
}

/**
 * Manifest for codegen. Uses an existing build if present, builds otherwise.
 * Returns [] when there is no crate or the manifest cannot be read.
 */
export async function loadRustManifestForCodegen(
  ctx: Context,
  functionsDir: string,
): Promise<RustManifestEntry[]> {
  if (!hasRustFunctions(ctx, functionsDir)) return [];
  const manifestPath = path.join(functionsDir, "rust", "Cargo.toml");
  let wasmPath: string | null = null;
  try {
    const meta = await runCargo(
      manifestPath,
      [
        "metadata",
        "--format-version",
        "1",
        "--no-deps",
        "--manifest-path",
        manifestPath,
      ],
      false,
    );
    if (meta.status === 0) {
      wasmPath = await findRustWasm(ctx, manifestPath);
      if (
        !ctx.fs.exists(wasmPath) ||
        rustSourcesNewerThan(path.join(functionsDir, "rust"), wasmPath)
      )
        wasmPath = await buildRustCrate(ctx, functionsDir);
    }
    if (wasmPath === null) return [];
    return parseManifest(new Uint8Array(readFileSync(wasmPath)));
  } catch (e: any) {
    logVerbose(`Skipping Rust codegen: ${e?.message ?? e}`);
    return [];
  }
}

/**
 * TypeScript type for the `api.rust.<module>.<fn>` references, to be
 * intersected with `ApiFromModules<...>`. Args and return types come from the
 * manifest validators (ids as `string`, as in the Node `api.d.ts` without a
 * data model import); `any` when a function declares no validator.
 */
export function manifestValidatorType(v: unknown): string {
  if (v === null || v === undefined) return "any";
  try {
    return validatorToType(v as ConvexValidator, false);
  } catch {
    return "any";
  }
}

export function rustApiType(entries: RustManifestEntry[]): string | null {
  if (entries.length === 0) return null;
  type Tree = { fns: RustManifestEntry[]; kids: Map<string, Tree> };
  const root: Tree = { fns: [], kids: new Map() };
  for (const [mod, fns] of groupByModule(entries)) {
    let node = root;
    for (const part of mod.split("/")) {
      if (!node.kids.has(part))
        node.kids.set(part, { fns: [], kids: new Map() });
      node = node.kids.get(part)!;
    }
    node.fns.push(...fns);
  }
  const render = (t: Tree): string => {
    const fnParts = [...t.fns]
      .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0))
      .map(
        (f) =>
          `${JSON.stringify(f.name)}: FunctionReference<"action", "${f.kind === "http" ? "internal" : f.visibility}", ${manifestValidatorType(f.args)}, ${manifestValidatorType(f.returns)}>;`,
      );
    const kidParts = Array.from(t.kids.entries())
      .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
      .map(([k, v]) => `${JSON.stringify(k)}: ${render(v)};`);
    return `{ ${[...fnParts, ...kidParts].join(" ")} }`;
  };
  return `{ "rust": ${render(root)}; }`;
}

/**
 * `<dir>/rust` is a Cargo crate ("use rust"), built separately to WASM, so
 * `entryPoints` must not treat it as a source directory.
 */
export function isRustCrateDir(
  ctx: Context,
  dir: string,
  dirPath: string,
): boolean {
  if (!ctx.fs.exists(path.join(dir, "rust", "Cargo.toml"))) return false;
  if (path.relative(dir, dirPath) !== "rust") return false;
  logVerbose(chalkStderr.yellow(`Skipping Rust crate ${dirPath}`));
  return true;
}

/** "use rust" actions are only supported in the root app; fail fast. */
export async function rejectRustInComponent(
  ctx: Context,
  componentDir: string,
): Promise<void> {
  if (ctx.fs.exists(path.join(componentDir, "rust", "Cargo.toml"))) {
    await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      printedMessage: `Rust actions are not supported in components (${componentDir}).`,
    });
  }
}
