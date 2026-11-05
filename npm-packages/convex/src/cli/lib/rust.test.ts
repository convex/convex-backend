import { test, expect, describe, beforeEach, afterEach } from "vitest";
import {
  MANAGED_BEGIN,
  MANAGED_END,
  managedConfigToml,
  displayShimPath,
  usesReqwest,
  scaffoldFiles,
  resolveReqwestShimPath,
  syncManagedPatch,
} from "./rust/scaffold.js";
import {
  parseManifest,
  rustModuleConfigs,
  rustApiType,
  convexModuleName,
  cargoSpawnConfig,
  optimizeWasm,
  resolveRustProfile,
  filterPatchWarning,
  PatchWarningFilter,
  rustSourcesNewerThan,
  parseManifestText,
  pickRustRuntime,
  fetchNativeInfo,
  convexFnVersion,
} from "./rust.js";
import { apiCodegen } from "../codegen_templates/api.js";
import { withRustApi } from "../codegen_templates/rust_api.js";

function leb(n: number): number[] {
  const out: number[] = [];
  do {
    let b = n & 0x7f;
    n >>>= 7;
    if (n) b |= 0x80;
    out.push(b);
  } while (n);
  return out;
}

function customSection(name: string, payload: Uint8Array): number[] {
  const nameBytes = Array.from(new TextEncoder().encode(name));
  const body = [...leb(nameBytes.length), ...nameBytes, ...payload];
  return [0, ...leb(body.length), ...body];
}

function tinyWasm(fragments: object[]): Uint8Array {
  const enc = new TextEncoder();
  const sections = fragments.flatMap((f) =>
    customSection("convex:manifest", enc.encode(JSON.stringify(f) + "\n")),
  );
  return new Uint8Array([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0, ...sections]);
}

const frag = (modulePath: string, name: string, visibility = "public") => ({
  abiVersion: 1,
  modulePath,
  name,
  visibility,
  args: null,
  returns: null,
});

test("parses manifest fragments from custom sections", () => {
  const wasm = tinyWasm([
    frag("bench_rust::cpu", "cpu_001"),
    frag("bench_rust::cpu", "cpu_002", "internal"),
    frag("bench_rust::io", "io_001"),
  ]);
  const entries = parseManifest(wasm);
  expect(entries.map((e) => `${e.modulePath}:${e.name}`)).toEqual([
    "bench_rust::cpu:cpu_001",
    "bench_rust::cpu:cpu_002",
    "bench_rust::io:io_001",
  ]);
});

test("rejects duplicates, mixed abi versions and root functions", () => {
  expect(() =>
    parseManifest(tinyWasm([frag("a::m", "f"), frag("a::m", "f")])),
  ).toThrow(/duplicate/);
  expect(() =>
    parseManifest(
      tinyWasm([frag("a::m", "f"), { ...frag("a::m", "g"), abiVersion: 2 }]),
    ),
  ).toThrow(/abiVersion/);
  expect(() => parseManifest(tinyWasm([frag("a", "f")]))).toThrow(/crate root/);
});

test("convexModuleName strips the crate name", () => {
  expect(convexModuleName("bench_rust::cpu")).toBe("cpu");
  expect(convexModuleName("app::a::b")).toBe("a/b");
});

test("generates one rust ModuleConfig per module with the whole wasm", () => {
  const wasm = tinyWasm([
    frag("bench_rust::cpu", "cpu_001"),
    frag("bench_rust::cpu", "cpu_002"),
    frag("bench_rust::io", "io_001"),
  ]);
  const modules = rustModuleConfigs(wasm, parseManifest(wasm));
  expect(modules.map((m) => m.path)).toEqual(["rust/cpu.js", "rust/io.js"]);
  for (const m of modules) {
    expect(m.environment).toBe("rust");
    expect(Buffer.from(m.source, "base64").equals(Buffer.from(wasm))).toBe(
      true,
    );
  }
});

test("codegen exposes rust functions as action references", () => {
  const wasm = tinyWasm([
    frag("bench_rust::cpu", "cpu_001"),
    frag("bench_rust::cpu", "cpu_002", "internal"),
  ]);
  const type = rustApiType(parseManifest(wasm));
  expect(type).toContain(
    `"cpu_001": FunctionReference<"action", "public", any, any>`,
  );
  expect(type).toContain(
    `"cpu_002": FunctionReference<"action", "internal", any, any>`,
  );
  const { DTS } = withRustApi(apiCodegen([]), type);
  expect(DTS).toContain(`}> & { "rust"`);
});

// ---- DX-01: scaffold ----
import fs from "fs";
import os from "os";
import path from "path";
import { scaffoldRust, RUST_API_STUB } from "./rust/scaffold.js";
import { checkRustToolchain, Exec } from "./rust/toolchain.js";
import { describeWasmSize, wasmSectionSizes } from "./rust/size.js";
import {
  cargoBuildConfig,
  simdCargoConfig,
  wasmSimdEnabled,
  wasmOptFlags,
  SIMD_RUSTFLAG,
  cargoProfileDir,
  ensureRustToolchain,
  resetToolchainCache,
} from "./rust.js";

test("scaffold creates files and is idempotent", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rust-init-"));
  const rustDir = path.join(dir, "convex", "rust");
  const first = scaffoldRust(rustDir, {
    convexFnPath: path.join(dir, "crates", "convex_fn"),
  });
  expect(first.skipped).toEqual([]);
  expect(first.created.sort()).toEqual(
    [
      ".cargo/config.toml",
      ".gitignore",
      "Cargo.toml",
      "rust-toolchain.toml",
      "src/_generated/api.rs",
      "src/example.rs",
      "src/lib.rs",
    ].sort(),
  );
  const lib = fs.readFileSync(path.join(rustDir, "src/lib.rs"), "utf8");
  expect(lib).toContain('#[path = "_generated/api.rs"]\npub mod api;');
  expect(fs.readFileSync(path.join(rustDir, "Cargo.toml"), "utf8")).toContain(
    'serde_json = "1"',
  );
  // the stub is what codegen writes when there are no functions yet
  expect(
    fs.readFileSync(path.join(rustDir, "src/_generated/api.rs"), "utf8"),
  ).toBe(RUST_API_STUB);
  const cargo = fs.readFileSync(path.join(rustDir, "Cargo.toml"), "utf8");
  expect(cargo).toContain('crate-type = ["cdylib"]');
  expect(cargo).toContain('convex_fn = { path = "../../crates/convex_fn" }');
  expect(fs.readFileSync(path.join(rustDir, ".gitignore"), "utf8")).toBe(
    "target/\n",
  );
  expect(
    fs.readFileSync(path.join(rustDir, ".cargo/config.toml"), "utf8"),
  ).not.toContain("target =");

  fs.writeFileSync(path.join(rustDir, "Cargo.toml"), "# mine\n");
  const second = scaffoldRust(rustDir);
  expect(second.created).toEqual([]);
  expect(second.skipped.length).toBe(7);
  expect(fs.readFileSync(path.join(rustDir, "Cargo.toml"), "utf8")).toBe(
    "# mine\n",
  );
});

test("scaffold without CONVEX_FN_PATH leaves a TODO", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rust-init-"));
  scaffoldRust(dir);
  expect(fs.readFileSync(path.join(dir, "Cargo.toml"), "utf8")).toMatch(
    /TODO[\s\S]*convex_fn = "0.1"/,
  );
});

// ---- DX-02: toolchain check ----
const ok = (stdout = ""): any => ({ status: 0, stdout, stderr: "" });
const enoent: any = {
  status: null,
  stdout: "",
  stderr: "",
  error: { code: "ENOENT" },
};

function toolchainDir(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rust-tc-"));
  fs.writeFileSync(
    path.join(dir, "rust-toolchain.toml"),
    '[toolchain]\nchannel = "nightly-x"\n',
  );
  return dir;
}

test("toolchain check: all present", () => {
  const exec: Exec = (cmd, args) =>
    cmd === "rustup" && args[0] === "target"
      ? ok("wasm32-wasip1\nx86_64-apple-darwin\n")
      : ok("v");
  expect(checkRustToolchain(toolchainDir(), exec)).toEqual({ ok: true });
});

test("toolchain check: no rustup/cargo", () => {
  const r = checkRustToolchain(toolchainDir(), () => enoent);
  expect(r).toMatchObject({ ok: false, kind: "no-cargo" });
  expect((r as any).message).toContain("https://rustup.rs");
});

test("toolchain check: missing target gives exact command", () => {
  const exec: Exec = (cmd, args) =>
    cmd === "rustup" && args[0] === "target"
      ? ok("x86_64-apple-darwin\n")
      : ok();
  const r: any = checkRustToolchain(toolchainDir(), exec);
  expect(r.kind).toBe("no-target");
  expect(r.fix).toEqual([
    "rustup",
    "target",
    "add",
    "wasm32-wasip1",
    "--toolchain",
    "nightly-x",
  ]);
  expect(r.message).toContain(
    "rustup target add wasm32-wasip1 --toolchain nightly-x",
  );
});

test("toolchain check: missing toolchain", () => {
  const exec: Exec = (cmd, args) =>
    cmd === "rustup" && args[0] === "target"
      ? {
          status: 1,
          stdout: "",
          stderr: "toolchain 'nightly-x' is not installed",
        }
      : ok();
  const r: any = checkRustToolchain(toolchainDir(), exec);
  expect(r.kind).toBe("no-toolchain");
  expect(r.message).toContain("rustup toolchain install nightly-x");
});

function fakeCtx() {
  const crashes: any[] = [];
  return {
    crashes,
    ctx: {
      crash: async (a: any) => {
        crashes.push(a);
        throw new Error("crash");
      },
    } as any,
  };
}

test("ensureRustToolchain: TTY confirm runs the fix; non-TTY crashes with command", async () => {
  let installed = false;
  const exec: Exec = (cmd, args) => {
    if (cmd === "rustup" && args[0] === "target" && args[1] === "add") {
      installed = true;
      return ok();
    }
    if (cmd === "rustup" && args[0] === "target")
      return ok(installed ? "wasm32-wasip1\n" : "");
    return ok();
  };
  resetToolchainCache();
  const dir = toolchainDir();
  const a = fakeCtx();
  await ensureRustToolchain(a.ctx, dir, {
    exec,
    isTTY: true,
    prompt: async () => true,
  });
  expect(installed).toBe(true);
  expect(a.crashes).toEqual([]);

  resetToolchainCache();
  installed = false;
  const b = fakeCtx();
  await expect(
    ensureRustToolchain(b.ctx, dir, { exec, isTTY: false }),
  ).rejects.toThrow("crash");
  expect(installed).toBe(false);
  expect(b.crashes[0].printedMessage).toContain(
    "rustup target add wasm32-wasip1 --toolchain nightly-x",
  );
});

// ---- DX-03: profiles and size reporting ----
test("build config: dev uses a custom incremental profile, deploy uses release", () => {
  expect(cargoBuildConfig("release")).toEqual({ args: ["--release"], env: {} });
  const dev = cargoBuildConfig("dev");
  expect(dev.args).toEqual(["--profile", "convex-dev"]);
  expect(dev.env.CARGO_PROFILE_CONVEX_DEV_INCREMENTAL).toBe("true");
  expect(cargoProfileDir("dev")).toBe("convex-dev");
  expect(cargoProfileDir("release")).toBe("release");
});

test("wasm size report: sections and threshold warning", () => {
  const wasm = tinyWasm([frag("a::m", "f")]);
  const sizes = wasmSectionSizes(wasm);
  expect(sizes.total).toBe(wasm.length);
  expect(sizes.code).toBe(0);
  expect(sizes.other).toBeGreaterThan(0);
  const r = describeWasmSize(wasm);
  expect(r.message).toMatch(/^Rust wasm: \d+ B/);
  expect(r.warning).toBeUndefined();
  expect(describeWasmSize(wasm, 10).warning).toMatch(
    /above the 10 B threshold/,
  );
  // code (id 10) and data (id 11) sections are attributed
  const withCode = new Uint8Array([...wasm, 10, 3, 0, 0, 0, 11, 2, 0, 0]);
  const s2 = wasmSectionSizes(withCode);
  expect(s2.code).toBe(3);
  expect(s2.data).toBe(2);
});

const CARGO_WARNING = [
  "warning: Patch `reqwest v0.12.99 (/shim/convex_reqwest)` was not used in the crate graph.",
  "Check that the patched package version and available features are compatible",
  "with the dependency requirements. If the patch has a different version from",
  "what is locked in the Cargo.lock file, run `cargo update` to use the new",
  "version. This may also occur with an optional dependency that is not enabled.",
].join("\n");

describe("filterPatchWarning", () => {
  test("variant with `= help: perhaps you meant` and a tab-indented path", () => {
    const input = [
      "warning: patch `reqwest v0.12.99 (/shim)` was not used in the crate graph",
      "  |",
      "  = help: perhaps you meant one of the following:",
      "          \t/other/convex_reqwest",
      "   Compiling foo v0.1.0",
      "",
    ].join("\n");
    expect(filterPatchWarning(input)).toBe("   Compiling foo v0.1.0\n");
  });

  test("newer cargo wording: lowercase `patch` with a help block", () => {
    const input = [
      "   Compiling foo v0.1.0",
      "warning: patch `reqwest v0.12.99 (/shim)` was not used in the crate graph",
      "help: Check that the patched package version and available features are compatible",
      "      with the dependency requirements. If the patch has a different version from",
      "      what is locked in the Cargo.lock file, run `cargo update` to use the new",
      "      version. This may also occur with an optional dependency that is not enabled.",
      "   Compiling bar v0.2.0",
      "",
    ].join("\n");
    expect(filterPatchWarning(input)).toBe(
      "   Compiling foo v0.1.0\n   Compiling bar v0.2.0\n",
    );
  });

  test("drops the warning and its note lines, keeps the rest", () => {
    const input = [
      "   Compiling foo v0.1.0",
      CARGO_WARNING,
      "   Compiling bar v0.2.0",
      "warning: unused variable: `x`",
      "    Finished `release` profile",
      "",
    ].join("\n");
    expect(filterPatchWarning(input)).toBe(
      [
        "   Compiling foo v0.1.0",
        "   Compiling bar v0.2.0",
        "warning: unused variable: `x`",
        "    Finished `release` profile",
        "",
      ].join("\n"),
    );
  });

  test("keeps an unrelated Patch warning and a following diagnostic", () => {
    const input = [
      "warning: Patch `other v1.0.0 (/x)` was not used in the crate graph.",
      "warning: Patch `reqwest v0.12.99 (/x)` was not used in the crate graph.",
      "Check that the patched package version",
      "error: boom",
      "",
    ].join("\n");
    expect(filterPatchWarning(input)).toBe(
      [
        "warning: Patch `other v1.0.0 (/x)` was not used in the crate graph.",
        "error: boom",
        "",
      ].join("\n"),
    );
  });

  test("streaming: lines are emitted as soon as they are complete", () => {
    const f = new PatchWarningFilter();
    expect(f.write("   Compil")).toBe("");
    expect(f.write("ing foo\n   Compiling ba")).toBe("   Compiling foo\n");
    expect(f.write("r\n")).toBe("   Compiling bar\n");
  });

  test("streaming: warning split across arbitrary chunks is dropped", () => {
    const input = `   Compiling a\n${CARGO_WARNING}\n   Compiling b\n`;
    for (const size of [1, 3, 7, 50, input.length]) {
      const f = new PatchWarningFilter();
      let out = "";
      for (let i = 0; i < input.length; i += size) {
        out += f.write(input.slice(i, i + size));
      }
      out += f.end();
      expect(out).toBe("   Compiling a\n   Compiling b\n");
    }
  });

  test("streaming: end flushes a trailing partial line", () => {
    const f = new PatchWarningFilter();
    expect(f.write("hello")).toBe("");
    expect(f.end()).toBe("hello");
    expect(f.end()).toBe("");
  });

  test("streaming: warning at end of output without trailing newline", () => {
    const f = new PatchWarningFilter();
    const out =
      f.write(
        "a\nwarning: Patch `reqwest v0.12.99 (/x)` was not used in the crate graph.",
      ) + f.end();
    expect(out).toBe("a\n");
  });
});

describe("managed patch", () => {
  let dir: string;
  beforeEach(() => {
    dir = fs.mkdtempSync(path.join(os.tmpdir(), "convex-rust-test-"));
    fs.mkdirSync(path.join(dir, ".cargo"));
  });
  afterEach(() => {
    fs.rmSync(dir, { recursive: true, force: true });
  });
  const cfg = () => path.join(dir, ".cargo", "config.toml");

  test("managedConfigToml(shim) has the patch block and no build.target", () => {
    const toml = managedConfigToml("/opt/shim/convex_reqwest", undefined, true);
    expect(toml).not.toContain("[build]");
    expect(toml).not.toContain("target =");
    expect(toml).toContain(
      `${MANAGED_BEGIN}\n[patch.crates-io]\nreqwest = { path = "/opt/shim/convex_reqwest" }\n${MANAGED_END}\n`,
    );
  });

  test("managedConfigToml without a shim has no patch table", () => {
    const toml = managedConfigToml(undefined);
    expect(toml).not.toContain("[patch.crates-io]");
    expect(toml).toContain(MANAGED_BEGIN);
    expect(toml).toContain(MANAGED_END);
  });

  test("syncManagedPatch is idempotent", () => {
    fs.writeFileSync(cfg(), managedConfigToml("/a/shim", undefined, true));
    expect(syncManagedPatch(dir, "/a/shim", true)).toBe(false);
    const before = fs.readFileSync(cfg(), "utf8");
    expect(syncManagedPatch(dir, "/a/shim", true)).toBe(false);
    expect(fs.readFileSync(cfg(), "utf8")).toBe(before);
  });

  test("syncManagedPatch updates a stale path", () => {
    fs.writeFileSync(cfg(), managedConfigToml("/old/shim", undefined, true));
    expect(syncManagedPatch(dir, "/new/shim", true)).toBe(true);
    const after = fs.readFileSync(cfg(), "utf8");
    expect(after).toContain('reqwest = { path = "/new/shim" }');
    expect(after).not.toContain("/old/shim");
    expect(syncManagedPatch(dir, "/new/shim", true)).toBe(false);
  });

  test("syncManagedPatch preserves user content outside the block", () => {
    const user = `# mine\n[env]\nFOO = "bar"\n\n${managedConfigToml("/old/shim", undefined, true)}\n[alias]\nb = "build"\n`;
    fs.writeFileSync(cfg(), user);
    expect(syncManagedPatch(dir, "/new/shim", true)).toBe(true);
    const after = fs.readFileSync(cfg(), "utf8");
    expect(after).toBe(user.replace("/old/shim", "/new/shim"));
    expect(after).toContain('[env]\nFOO = "bar"');
    expect(after).toContain('[alias]\nb = "build"');
  });

  test("syncManagedPatch creates a missing config only next to a Cargo.toml", () => {
    expect(syncManagedPatch(dir, "/a/shim", true)).toBe(false);
    expect(fs.existsSync(cfg())).toBe(false);
    fs.writeFileSync(path.join(dir, "Cargo.toml"), "[package]\n");
    expect(syncManagedPatch(dir, "/a/shim", true)).toBe(true);
    expect(fs.readFileSync(cfg(), "utf8")).toBe(
      managedConfigToml("/a/shim", undefined, true),
    );
    expect(syncManagedPatch(dir, "/a/shim", true)).toBe(false);
  });

  test("syncManagedPatch leaves a config without markers alone", () => {
    fs.rmSync(cfg(), { force: true });
    fs.writeFileSync(cfg(), '[build]\ntarget = "wasm32-wasip1"\n');
    expect(syncManagedPatch(dir, "/a/shim", true)).toBe(false);
    expect(fs.readFileSync(cfg(), "utf8")).toBe(
      '[build]\ntarget = "wasm32-wasip1"\n',
    );
  });

  test("a path convex_fn dependency is patched to the same path", () => {
    fs.writeFileSync(
      path.join(dir, "Cargo.toml"),
      '[dependencies]\nconvex_fn = { path = "../fn" }\n',
    );
    fs.writeFileSync(cfg(), managedConfigToml("/a/shim", undefined, true));
    expect(syncManagedPatch(dir, "/a/shim", true)).toBe(true);
    const after = fs.readFileSync(cfg(), "utf8");
    expect(after).toContain(
      `convex_fn = { path = ${JSON.stringify(path.resolve(dir, "../fn"))} }`,
    );
    expect(syncManagedPatch(dir, "/a/shim", true)).toBe(false);
  });

  test("no patch table unless reqwest is in the dependency graph", () => {
    fs.writeFileSync(
      path.join(dir, "Cargo.toml"),
      '[dependencies]\nserde = "1"\n',
    );
    fs.writeFileSync(cfg(), managedConfigToml("/a/shim"));
    expect(fs.readFileSync(cfg(), "utf8")).not.toContain("[patch");
    // reqwest appears in Cargo.toml: patch added
    fs.writeFileSync(
      path.join(dir, "Cargo.toml"),
      '[dependencies]\nreqwest = "0.12"\n',
    );
    expect(syncManagedPatch(dir, "/a/shim")).toBe(true);
    expect(fs.readFileSync(cfg(), "utf8")).toContain("[patch.crates-io]");
    // reqwest removed again: patch removed
    fs.writeFileSync(path.join(dir, "Cargo.toml"), "[dependencies]\n");
    expect(syncManagedPatch(dir, "/a/shim")).toBe(true);
    expect(fs.readFileSync(cfg(), "utf8")).not.toContain("[patch");
    // an unused patch entry in Cargo.lock is not a dependency
    fs.writeFileSync(
      path.join(dir, "Cargo.lock"),
      '[[patch.unused]]\nname = "reqwest"\nversion = "0.12.99"\n',
    );
    expect(usesReqwest(dir)).toBe(false);
    // a transitive reqwest shows up in Cargo.lock
    fs.writeFileSync(
      path.join(dir, "Cargo.lock"),
      '[[package]]\nname = "reqwest"\nversion = "0.12.1"\n',
    );
    expect(usesReqwest(dir)).toBe(true);
    expect(syncManagedPatch(dir, "/a/shim")).toBe(true);
    expect(fs.readFileSync(cfg(), "utf8")).toContain("[patch.crates-io]");
  });

  test("migrates the legacy managed header with build.target", () => {
    const legacy = `# Managed by \`npx convex\`: edit with care, the block below is regenerated.
[build]
target = "wasm32-wasip1"

${MANAGED_BEGIN}
[patch.crates-io]
reqwest = { path = "/a/shim" }
${MANAGED_END}
`;
    fs.writeFileSync(cfg(), legacy);
    expect(syncManagedPatch(dir, "/a/shim", false)).toBe(true);
    const after = fs.readFileSync(cfg(), "utf8");
    expect(after).not.toContain("[build]");
    expect(after).not.toContain("wasm32-wasip1");
    expect(after).not.toContain("[patch");
    expect(after).toContain("Managed by");
    expect(syncManagedPatch(dir, "/a/shim", false)).toBe(false);
  });

  test("keeps a user's own [build] settings", () => {
    fs.writeFileSync(
      cfg(),
      `[build]\njobs = 2\n\n${managedConfigToml("/a/shim", undefined, true)}`,
    );
    expect(syncManagedPatch(dir, "/a/shim", true)).toBe(false);
    expect(fs.readFileSync(cfg(), "utf8")).toContain("[build]\njobs = 2");
  });

  test("shim inside node_modules is written relative to the rust dir", () => {
    const shim = path.join(
      dir,
      "node_modules",
      "convex",
      "rust",
      "convex_reqwest",
    );
    expect(displayShimPath(dir, shim)).toBe(
      path.join("node_modules", "convex", "rust", "convex_reqwest"),
    );
    const nested = path.join(dir, "rust");
    expect(displayShimPath(nested, shim)).toBe(
      path.join("..", "node_modules", "convex", "rust", "convex_reqwest"),
    );
    // overrides outside node_modules stay absolute
    expect(displayShimPath(dir, "/opt/shim/convex_reqwest")).toBe(
      "/opt/shim/convex_reqwest",
    );
    fs.writeFileSync(cfg(), managedConfigToml(undefined));
    expect(syncManagedPatch(dir, shim, true)).toBe(true);
    const after = fs.readFileSync(cfg(), "utf8");
    expect(after).toContain(
      'reqwest = { path = "node_modules/convex/rust/convex_reqwest" }',
    );
    expect(after).not.toContain(dir);
  });

  test("resolveReqwestShimPath finds the project's node_modules/convex", () => {
    const shim = path.join(
      dir,
      "node_modules",
      "convex",
      "rust",
      "convex_reqwest",
    );
    fs.mkdirSync(shim, { recursive: true });
    const rustDir = path.join(dir, "convex", "rust");
    fs.mkdirSync(rustDir, { recursive: true });
    expect(resolveReqwestShimPath({}, rustDir)).toBe(shim);
    expect(
      resolveReqwestShimPath({ CONVEX_REQWEST_PATH: "/x/y" }, rustDir),
    ).toBe("/x/y");
  });

  test("scaffolded example does not depend on a TS function", () => {
    const files = scaffoldFiles(dir, {});
    expect(files["src/example.rs"]).not.toContain("messages:list");
    expect(files["src/example.rs"]).not.toContain(".run_query(");
    expect(files[".cargo/config.toml"]).not.toContain("[build]");
    expect(files[".cargo/config.toml"]).not.toContain("[patch");
  });
});

describe("resolveReqwestShimPath", () => {
  test("env CONVEX_REQWEST_PATH wins", () => {
    expect(resolveReqwestShimPath({ CONVEX_REQWEST_PATH: "/x/y" })).toBe(
      "/x/y",
    );
  });
});

describe("cargoSpawnConfig", () => {
  test("runs cargo in the crate directory with an absolute manifest path", () => {
    const rel = path.join("convex", "rust", "Cargo.toml");
    const cfg = cargoSpawnConfig(rel, [
      "build",
      "--manifest-path",
      rel,
      "--release",
    ]);
    expect(cfg.cwd).toBe(path.resolve("convex", "rust"));
    expect(cfg.args).toEqual([
      "build",
      "--manifest-path",
      path.resolve(rel),
      "--release",
    ]);
  });
});

describe("optimizeWasm cache", () => {
  let dir: string;
  let wasm: string;
  beforeEach(() => {
    dir = fs.mkdtempSync(path.join(os.tmpdir(), "convex-wasmopt-test-"));
    wasm = path.join(dir, "x.wasm");
    fs.writeFileSync(wasm, "unoptimized-bytes");
  });
  afterEach(() => {
    fs.rmSync(dir, { recursive: true, force: true });
  });
  const shrink = () => {
    let calls = 0;
    const run = (p: string) => {
      calls++;
      fs.writeFileSync(p, "opt");
      return true;
    };
    return { run, calls: () => calls };
  };

  test("rebuilt input and optimized output both skip wasm-opt", () => {
    const s = shrink();
    expect(optimizeWasm(wasm, s.run, () => "v1")).toBe(true);
    expect(s.calls()).toBe(1);
    expect(fs.readFileSync(wasm, "utf8")).toBe("opt");
    // cargo did nothing: we get the optimized file again
    expect(optimizeWasm(wasm, s.run, () => "v1")).toBe(false);
    expect(s.calls()).toBe(1);
    // cargo relinked the same unoptimized bytes: restored from cache
    fs.writeFileSync(wasm, "unoptimized-bytes");
    expect(optimizeWasm(wasm, s.run, () => "v1")).toBe(true);
    expect(s.calls()).toBe(1);
    expect(fs.readFileSync(wasm, "utf8")).toBe("opt");
  });

  test("a different wasm-opt version or new input misses the cache", () => {
    const s = shrink();
    optimizeWasm(wasm, s.run, () => "v1");
    fs.writeFileSync(wasm, "unoptimized-bytes");
    optimizeWasm(wasm, s.run, () => "v2");
    expect(s.calls()).toBe(2);
    fs.writeFileSync(wasm, "changed");
    optimizeWasm(wasm, s.run, () => "v2");
    expect(s.calls()).toBe(3);
  });

  test("an unhelpful wasm-opt run is remembered", () => {
    let calls = 0;
    const run = () => {
      calls++;
      return false;
    };
    optimizeWasm(wasm, run, () => "v1");
    optimizeWasm(wasm, run, () => "v1");
    expect(calls).toBe(1);
  });

  test("without wasm-opt it just delegates", () => {
    let calls = 0;
    const run = () => {
      calls++;
      return false;
    };
    optimizeWasm(wasm, run, () => null);
    optimizeWasm(wasm, run, () => null);
    expect(calls).toBe(2);
  });
});

test("rustApiType: typed args/returns from manifest validators", () => {
  const entry = (args: unknown, returns: unknown) => ({
    abiVersion: 1,
    modulePath: "app::m",
    name: "f",
    visibility: "public" as const,
    args,
    returns,
  });
  const obj = {
    type: "object",
    value: {
      name: { fieldType: { type: "string" }, optional: false },
      n: { fieldType: { type: "number" }, optional: true },
      owner: { fieldType: { type: "id", tableName: "users" }, optional: false },
    },
  };
  const t = rustApiType([
    entry(obj, { type: "array", value: { type: "string" } }),
  ]);
  expect(t).toContain(
    '"f": FunctionReference<"action", "public", { name: string, n?: number, owner: string }, Array<string>>;',
  );
  // no validators or garbage -> any
  expect(rustApiType([entry(null, null)])).toContain('"public", any, any>');
  expect(rustApiType([entry({ type: "nope" }, null)])).toContain(
    '"public", any, any>',
  );
  // explicit null return validator -> null
  expect(rustApiType([entry(null, { type: "null" })])).toContain(
    '"public", any, null>',
  );
});

describe("rustSourcesNewerThan", () => {
  test("detects sources newer than the wasm", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "convex-newer-"));
    try {
      fs.mkdirSync(path.join(dir, "src"));
      fs.writeFileSync(path.join(dir, "Cargo.toml"), "");
      fs.writeFileSync(path.join(dir, "src", "lib.rs"), "");
      const wasm = path.join(dir, "out.wasm");
      fs.writeFileSync(wasm, "");
      const old = new Date(Date.now() - 60_000);
      const fresh = new Date();
      fs.utimesSync(path.join(dir, "Cargo.toml"), old, old);
      fs.utimesSync(path.join(dir, "src", "lib.rs"), old, old);
      fs.utimesSync(wasm, fresh, fresh);
      expect(rustSourcesNewerThan(dir, wasm)).toBe(false);
      fs.utimesSync(path.join(dir, "src", "lib.rs"), fresh, fresh);
      fs.utimesSync(wasm, old, old);
      expect(rustSourcesNewerThan(dir, wasm)).toBe(true);
      expect(rustSourcesNewerThan(dir, path.join(dir, "missing.wasm"))).toBe(
        true,
      );
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe("resolveRustProfile (CONVEX_RUST_PROFILE)", () => {
  test("unset keeps the command's mode", () => {
    expect(resolveRustProfile("dev", {})).toEqual({ mode: "dev", perf: false });
    expect(resolveRustProfile("release", {})).toEqual({
      mode: "release",
      perf: false,
    });
  });
  test("release turns dev into optimised release", () => {
    expect(
      resolveRustProfile("dev", { CONVEX_RUST_PROFILE: "release" }),
    ).toEqual({ mode: "release", perf: true });
  });
  test("explicit dev keeps dev", () => {
    expect(resolveRustProfile("dev", { CONVEX_RUST_PROFILE: "dev" })).toEqual({
      mode: "dev",
      perf: false,
    });
  });
});

test("toolchain check: native target", () => {
  const exec: Exec = (cmd, args) =>
    cmd === "rustup" && args[0] === "target" ? ok("wasm32-wasip1\n") : ok("v");
  const r = checkRustToolchain(toolchainDir(), exec, "aarch64-apple-darwin");
  expect(r).toMatchObject({ ok: false, kind: "no-target" });
  expect((r as any).fix).toContain("aarch64-apple-darwin");
});

describe("rust runtime selection", () => {
  test("flag beats config beats default", () => {
    expect(pickRustRuntime(undefined, undefined)).toBe("wasm");
    expect(pickRustRuntime(undefined, "native")).toBe("native");
    expect(pickRustRuntime("wasm", "native")).toBe("wasm");
    expect(pickRustRuntime(undefined, "bogus")).toBe("wasm");
  });

  test("fetchNativeInfo: 404 means unsupported", async () => {
    const f = (async () => new Response("", { status: 404 })) as any;
    expect(await fetchNativeInfo({ url: "http://x", adminKey: "k" }, f)).toBe(
      null,
    );
  });

  test("fetchNativeInfo: sends admin key and parses", async () => {
    let seen = "";
    const f = (async (u: URL, init: any) => {
      seen = `${u.pathname} ${init.headers.Authorization}`;
      return new Response(JSON.stringify({ enabled: false }), { status: 200 });
    }) as any;
    const r = await fetchNativeInfo({ url: "http://x", adminKey: "k" }, f);
    expect(r).toEqual({ enabled: false });
    expect(seen).toBe("/api/rust_native/info Convex k");
  });

  test("convexFnVersion reads Cargo.lock", () => {
    const lock = 'name = "convex_fn"\nversion = "0.3.1"\n';
    expect(convexFnVersion(lock)).toBe("0.3.1");
    expect(convexFnVersion("")).toBe("unknown");
  });

  test("parseManifestText validates duplicates", () => {
    const l = JSON.stringify({ abiVersion: 1, modulePath: "a::b", name: "f" });
    expect(parseManifestText(l)).toHaveLength(1);
    expect(() => parseManifestText(l + "\n" + l)).toThrow(/duplicate/);
  });
});

describe("wasm SIMD build options", () => {
  const cfg = `target.wasm32-wasip1.rustflags=["-C","${SIMD_RUSTFLAG}"]`;
  test("enabled by default, opt-outs", () => {
    expect(wasmSimdEnabled({}, false)).toBe(true);
    expect(wasmSimdEnabled({ CONVEX_RUST_NO_SIMD: "1" }, false)).toBe(false);
    expect(wasmSimdEnabled({ CONVEX_RUST_SIMD: "0" }, false)).toBe(false);
    expect(wasmSimdEnabled({}, true)).toBe(false);
    expect(wasmSimdEnabled({ CONVEX_RUST_NO_SIMD: "0" }, false)).toBe(true);
  });
  test("no existing rustflags: --config arg", () => {
    expect(simdCargoConfig(true, {})).toEqual({
      args: ["--config", cfg],
      env: {},
    });
  });
  test("appends to RUSTFLAGS", () => {
    expect(simdCargoConfig(true, { RUSTFLAGS: "-C opt-level=3" })).toEqual({
      args: [],
      env: { RUSTFLAGS: `-C opt-level=3 -C ${SIMD_RUSTFLAG}` },
    });
  });
  test("appends to the target-specific variable and encoded flags", () => {
    expect(
      simdCargoConfig(true, {
        CARGO_TARGET_WASM32_WASIP1_RUSTFLAGS: "-C lto",
        RUSTFLAGS: "-C x",
      }).env,
    ).toEqual({
      CARGO_TARGET_WASM32_WASIP1_RUSTFLAGS: `-C lto -C ${SIMD_RUSTFLAG}`,
    });
    expect(
      simdCargoConfig(true, { CARGO_ENCODED_RUSTFLAGS: "-Ca\x1f-Cb" }).env
        .CARGO_ENCODED_RUSTFLAGS,
    ).toBe(`-Ca\x1f-Cb\x1f-C\x1f${SIMD_RUSTFLAG}`);
  });
  test("opt-out adds nothing", () => {
    expect(simdCargoConfig(false, { RUSTFLAGS: "-C x" })).toEqual({
      args: [],
      env: {},
    });
  });
  test("wasm-opt gets --enable-simd only with SIMD", () => {
    expect(wasmOptFlags(true, true)).toContain("--enable-simd");
    expect(wasmOptFlags(false, true)).toContain("--enable-simd");
    expect(wasmOptFlags(true, false)).not.toContain("--enable-simd");
    expect(wasmOptFlags(true, true)).toContain("-O3");
  });
  test("optimizeWasm cache key differs by SIMD", () => {
    // flags feed the cache salt, so a SIMD and non-SIMD build never share an entry
    expect(wasmOptFlags(true, true).join(" ")).not.toBe(
      wasmOptFlags(true, false).join(" "),
    );
  });
});

// ---- F-03: #[http_action] on the backend-hosted runtimes ----
import { rustHttpCodegen } from "../codegen_templates/rust_api.js";

test("http entries are internal in the api type (never callable by clients)", () => {
  const wasm = tinyWasm([
    frag("app::web", "hook", "public"),
    { ...frag("app::web", "act", "public") },
  ]);
  const entries = parseManifest(wasm).map((e) =>
    e.name === "hook" ? { ...e, kind: "http" } : e,
  );
  const type = rustApiType(entries)!;
  expect(type).toContain(`"hook": FunctionReference<"action", "internal"`);
  expect(type).toContain(`"act": FunctionReference<"action", "public"`);
});

test("rustHttp codegen: JS + d.ts, or TS", async () => {
  const js = rustHttpCodegen(false);
  expect(js.TS).toBeUndefined();
  expect(js.JS).toContain("export function rustHttpAction(name)");
  expect(js.JS).toContain('makeFunctionReference("rust/" + name)');
  expect(js.JS).toContain("buffered: true");
  expect(js.DTS).toContain("export declare function rustHttpAction(name: string): PublicHttpAction;");
  const ts = rustHttpCodegen(true);
  expect(ts.JS).toBeUndefined();
  expect(ts.TS).toContain("export function rustHttpAction(name: string)");
  // The generated JS is valid (module syntax) and the helpers round-trip bytes.
  const src = js.JS!.replace(/^import .*$/m, "const httpActionGeneric = (f) => f, makeFunctionReference = (n) => n;").replace("export function", "function");
  const mod = new Function(`${src}; return { rustHttpAction };`)();
  const calls: any[] = [];
  const handler = mod.rustHttpAction("web:echo");
  const ctx = {
    runAction: async (ref: string, args: any) => {
      calls.push([ref, args]);
      return { status: 201, headers: [["x-a", "1"], ["x-a", "2"]], body: args.body };
    },
  };
  const res: Response = await handler(
    ctx,
    new Request("https://x.convex.site/e?q=1", { method: "POST", headers: { "X-T": "v" }, body: "héllo" }),
  );
  expect(calls[0][0]).toBe("rust/web:echo");
  expect(calls[0][1]).toMatchObject({ method: "POST", url: "https://x.convex.site/e?q=1", buffered: true });
  expect(calls[0][1].headers).toContainEqual(["x-t", "v"]);
  expect(res.status).toBe(201);
  expect(res.headers.get("x-a")).toBe("1, 2");
  expect(await res.text()).toBe("héllo");
  const empty = await handler(
    { runAction: async (_r: string, a: any) => { expect(a.body).toBeNull(); return { status: 204, headers: [], body: null }; } },
    new Request("https://x.convex.site/e"),
  );
  expect(empty.status).toBe(204);
});
