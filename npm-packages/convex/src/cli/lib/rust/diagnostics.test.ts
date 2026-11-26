import { test, expect, describe } from "vitest";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";
import {
  diagnoseCargoFailure,
  diagnoseRuntimeError,
  formatFindings,
} from "./diagnostics.js";
import { buildFailureHints } from "../rust.js";

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures");
const fixture = (n: string) => fs.readFileSync(path.join(dir, n), "utf8");

describe("diagnoseCargoFailure (real logs)", () => {
  test("openssl-sys", () => {
    const f = diagnoseCargoFailure(fixture("openssl-sys.log"));
    expect(f.map((x) => x.crate)).toEqual(["openssl-sys"]);
    expect(f[0].advice.join(" ")).toMatch(/rustls/);
  });

  test("ring (cc-rs clang failure) names ring and jsonwebtoken fix", () => {
    const f = diagnoseCargoFailure(fixture("ring.log"));
    expect(f.map((x) => x.crate)).toEqual(["ring"]);
    expect(f[0].advice.join(" ")).toMatch(/rust_crypto/);
    expect(f[0].advice.join(" ")).toMatch(/convex_aws/);
  });

  test("aws-lc-sys", () => {
    const f = diagnoseCargoFailure(fixture("aws-lc-sys.log"));
    expect(f.map((x) => x.crate)).toEqual(["aws-lc-sys"]);
    expect(f[0].advice.join(" ")).toMatch(/convex_aws/);
  });

  test("socket2 compile_error", () => {
    const f = diagnoseCargoFailure(fixture("socket2.log"));
    expect(f).toHaveLength(1);
    expect(f[0].rule).toBe("sockets");
    expect(f[0].crate).toBe("socket2");
    expect(f[0].advice.join(" ")).toMatch(/HTTP/);
    expect(f[0].advice.join(" ")).toMatch(/use node/);
  });
});

describe("diagnoseCargoFailure (synthetic)", () => {
  test("names raw-socket driver crates that were being built", () => {
    const log = [
      "   Compiling tokio-postgres v0.7.13",
      "   Compiling socket2 v0.5.8",
      "error: Socket2 doesn't support the compile target",
      "error: could not compile `socket2` (lib) due to 1 previous error",
    ].join("\n");
    const f = diagnoseCargoFailure(log);
    expect(f[0].title).toContain("`tokio-postgres`");
  });

  test("mio net errors", () => {
    const f = diagnoseCargoFailure(
      "error[E0432]: unresolved import `mio::net`\n",
    );
    expect(f[0].rule).toBe("sockets");
  });

  test("generic -sys crate and missing wasm clang", () => {
    const log =
      'error: failed to run custom build command for `libsqlite3-sys v0.30.1`\n  error occurred in cc-rs: failed to find tool "wasm32-wasi-clang": No such file or directory\n';
    const f = diagnoseCargoFailure(log);
    expect(f.map((x) => x.rule)).toEqual(["c-dep"]);
    expect(f[0].advice.join(" ")).toMatch(/run_query/);
  });

  test("cc-rs failure in a non-sys crate", () => {
    const log =
      'error: failed to run custom build command for `blake3 v1.5.0`\n  error occurred in cc-rs: failed to find tool "wasm32-wasi-clang"\n';
    const f = diagnoseCargoFailure(log);
    expect(f[0].rule).toBe("c-toolchain");
    expect(f[0].crate).toBe("blake3");
  });

  test("getrandom without a supported backend", () => {
    const log =
      'error: the wasm32-unknown-unknown targets are not supported by default, you may need to enable the "js" feature. For more information see: https://docs.rs/getrandom/#webassembly-support\n --> getrandom-0.2.15/src/lib.rs\n';
    const f = diagnoseCargoFailure(log);
    expect(f[0].rule).toBe("getrandom");
    expect(f[0].advice.join(" ")).toMatch(/\[build\] target/);
  });

  test("psm section too large", () => {
    const f = diagnoseCargoFailure(
      "error: linking with `rust-lld` failed\n  = note: section too large psm\n",
    );
    expect(f[0].rule).toBe("psm");
    expect(f[0].advice.join(" ")).toMatch(/Cargo\.lock/);
  });

  test("rayon", () => {
    const f = diagnoseCargoFailure(
      "error: could not compile `rayon-core` (lib) due to 2 previous errors\n",
    );
    expect(f[0].rule).toBe("rayon");
  });

  test("missing target", () => {
    const f = diagnoseCargoFailure(
      "error[E0463]: can't find crate for `std`\n  = note: the `wasm32-wasip1` target may not be installed\n",
    );
    expect(f[0].advice[0]).toBe("Run: rustup target add wasm32-wasip1");
  });

  test("unrelated failures produce nothing; successful logs too", () => {
    expect(
      diagnoseCargoFailure("error[E0308]: mismatched types\n --> src/lib.rs"),
    ).toEqual([]);
    expect(
      diagnoseCargoFailure(fixture("openssl-sys.log").slice(0, 0)),
    ).toEqual([]);
  });
});

describe("formatting", () => {
  test("includes cargo tree output when available, a command otherwise", () => {
    const f = diagnoseCargoFailure(fixture("openssl-sys.log"));
    const withTree = formatFindings(f, {
      "openssl-sys": "openssl-sys v0.9.117\n└── native-tls v0.2\n",
    });
    expect(withTree).toContain("Hint:");
    expect(withTree).toContain("└── native-tls");
    expect(formatFindings(f)).toContain(
      "cargo tree -i openssl-sys --target wasm32-wasip1 -e normal",
    );
  });

  test("buildFailureHints runs cargo tree for each crate (capped) and never throws", async () => {
    const calls: string[] = [];
    const text = await buildFailureHints(
      "/x/Cargo.toml",
      fixture("ring.log"),
      async (_m, crate) => {
        calls.push(crate);
        return `${crate} v1\n└── octocrab\n`;
      },
    );
    expect(calls).toEqual(["ring"]);
    expect(text).toContain("└── octocrab");
    const failing = await buildFailureHints(
      "/x/Cargo.toml",
      fixture("ring.log"),
      async () => {
        throw new Error("boom");
      },
    );
    expect(failing).toContain("cargo tree -i ring");
    expect(await buildFailureHints("/x", "error: nothing known")).toBe("");
  });
});

describe("diagnoseRuntimeError", () => {
  test("threads", () => {
    expect(
      diagnoseRuntimeError(
        "ThreadPoolBuildError { kind: IOError(Error { kind: Unsupported }) }",
      ),
    ).toMatch(/thread/);
    expect(
      diagnoseRuntimeError("failed to spawn thread: Os { code: 58 }"),
    ).toMatch(/par_iter/);
  });
  test("unrecognised text", () => {
    expect(diagnoseRuntimeError("index out of bounds")).toBeNull();
  });
});
