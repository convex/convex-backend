/**
 * Friendly explanations for cargo failures when building for wasm32-wasip1.
 * Cargo's own output is always kept; the hints are appended after it.
 *
 * Data-driven: each {@link Rule} looks at the captured stderr and returns zero
 * or more {@link Finding}s. A finding may name a crate, in which case the
 * caller can show who pulls it in (`cargo tree -i <crate>`).
 */

export interface Finding {
  /** Stable rule id. */
  rule: string;
  /** Headline: what went wrong. */
  title: string;
  /** Actionable advice, one bullet per entry. */
  advice: string[];
  /** Crate worth running `cargo tree -i` on. */
  crate?: string;
}

export interface Rule {
  id: string;
  match: (stderr: string) => Finding[];
}

const TARGET = "wasm32-wasip1";

const NODE_FALLBACK =
  'If you cannot avoid it, move that work to a `"use node"` action and call it with `ctx.run_action`.';

/** Crates with C code that cannot be compiled for wasm32-wasip1 as-is. */
const C_DEPS: Record<string, { what: string; advice: string[] }> = {
  "openssl-sys": {
    what: "OpenSSL",
    advice: [
      'Switch the crate that pulls it in to a rustls/pure-Rust feature (for example `default-features = false, features = ["rustls-tls"]`), or drop TLS: HTTP goes through the built-in `reqwest` shim (`ctx.fetch`), which needs no TLS stack.',
      NODE_FALLBACK,
    ],
  },
  ring: {
    what: "ring (C and assembly)",
    advice: [
      "Disable the feature that needs `ring`, or pick a pure-Rust backend (RustCrypto: `sha2`, `hmac`, `aes-gcm`, ...).",
      "For `jsonwebtoken`, enable the `rust_crypto` feature instead of `aws_lc_rs`.",
      "For AWS services use the `convex_aws` crate (HTTP over `ctx.fetch`) instead of the SDK's default TLS client.",
      NODE_FALLBACK,
    ],
  },
  "aws-lc-sys": {
    what: "AWS-LC (C)",
    advice: [
      "For AWS services use the `convex_aws` crate (HTTP over `ctx.fetch`); do not enable the SDK's default HTTPS connector.",
      "For `jsonwebtoken`, enable the `rust_crypto` feature instead of `aws_lc_rs`.",
      "For `rustls`, avoid the `aws_lc_rs` provider (use `default-features = false`), or avoid rustls: outgoing HTTP does not need it here.",
      NODE_FALLBACK,
    ],
  },
  "libsqlite3-sys": {
    what: "SQLite (C)",
    advice: [
      "Convex is already your database: store the data in Convex tables via `ctx.run_query` / `ctx.run_mutation`.",
      'If you need SQLite itself, call a `"use node"` action with `ctx.run_action`.',
    ],
  },
  "zstd-sys": {
    what: "zstd (C)",
    advice: [
      "Use a pure-Rust codec (`ruzstd`, `lz4_flex`) or disable the dependency's zstd feature.",
      NODE_FALLBACK,
    ],
  },
  "lzma-sys": {
    what: "liblzma (C)",
    advice: [
      "Use a pure-Rust codec (`lzma-rs`) or disable the dependency's lzma feature.",
    ],
  },
  "bzip2-sys": {
    what: "libbz2 (C)",
    advice: [
      "Use `bzip2` with the pure-Rust `libbz2-rs-sys` backend, or disable the feature.",
    ],
  },
  "libz-sys": {
    what: "zlib (C)",
    advice: ["Use `flate2` with the `rust_backend` (miniz_oxide) feature."],
  },
};

const GENERIC_C_ADVICE = [
  "Look for a feature flag on the dependency that selects a pure-Rust implementation, or replace the crate.",
  `Native code needing a C toolchain for ${TARGET} is not available in \`use rust\`. ${NODE_FALLBACK}`,
];

/** Raw-socket database/network drivers: no sockets in v1. */
const SOCKET_CRATES = [
  "sqlx",
  "tokio-postgres",
  "postgres",
  "mongodb",
  "redis",
  "mysql_async",
  "mysql",
  "diesel",
  "lettre",
  "rdkafka",
  "lapin",
  "async-nats",
  "tiberius",
  "scylla",
];

const SOCKET_ADVICE = [
  "`use rust` v1 has no raw sockets (no TCP/UDP/DNS), so crates that open their own connections cannot run.",
  "Use an HTTP-based driver or the vendor's HTTP API, which works through `reqwest`/`ctx.fetch` (for example Neon or PlanetScale serverless HTTP drivers, Supabase REST, Upstash Redis REST).",
  'Or do the socket work in a `"use node"` action and call it with `ctx.run_action`.',
];

const THREAD_ADVICE = [
  "`use rust` runs single-threaded: rayon's thread pool and `std::thread::spawn` compile but cannot start threads at runtime.",
  'Replace `par_iter()` with `iter()`, or run the parallel work in a `"use node"` action via `ctx.run_action`.',
];

function compilingCrates(stderr: string): Set<string> {
  const out = new Set<string>();
  for (const m of stderr.matchAll(
    /^\s*(?:Compiling|Checking)\s+([\w.-]+)\s+v/gm,
  )) {
    out.add(m[1]);
  }
  for (const m of stderr.matchAll(/could not compile `([\w.-]+)`/g)) {
    out.add(m[1]);
  }
  return out;
}

function failedBuildScripts(stderr: string): string[] {
  return [
    ...stderr.matchAll(/failed to run custom build command for `([\w.-]+) v/g),
  ].map((m) => m[1]);
}

export const RULES: Rule[] = [
  {
    id: "missing-target",
    match: (s) =>
      /can't find crate for `(std|core)`|target may not be installed|is not installed for the toolchain|does not support target/.test(
        s,
      )
        ? [
            {
              rule: "missing-target",
              title: `The ${TARGET} Rust target is not installed`,
              advice: [`Run: rustup target add ${TARGET}`],
            },
          ]
        : [],
  },
  {
    id: "c-dep",
    match: (s) => {
      const out: Finding[] = [];
      for (const name of new Set(failedBuildScripts(s))) {
        if (name === "psm") continue;
        const known = C_DEPS[name];
        if (!known && !name.endsWith("-sys")) continue;
        out.push({
          rule: "c-dep",
          title: known
            ? `\`${name}\` needs a C library (${known.what}) that is not available for ${TARGET}`
            : `\`${name}\` builds native code that is not available for ${TARGET}`,
          advice: known ? known.advice : GENERIC_C_ADVICE,
          crate: name,
        });
      }
      return out;
    },
  },
  {
    id: "c-toolchain",
    match: (s) => {
      const isC =
        /failed to find tool "[^"]*"|wasm32-wasi[\w-]*-clang|No available targets are compatible with triple "wasm32|error occurred in cc-rs/.test(
          s,
        );
      if (!isC) return [];
      const failed = failedBuildScripts(s);
      // Already reported with specific advice by the `c-dep` rule.
      if (failed.some((c) => C_DEPS[c] || c.endsWith("-sys"))) return [];
      const crate = failed.find((c) => c !== "psm");
      return [
        {
          rule: "c-toolchain",
          title: `A dependency compiles C code and no C compiler for ${TARGET} was found`,
          advice: GENERIC_C_ADVICE,
          ...(crate ? { crate } : {}),
        },
      ];
    },
  },
  {
    id: "sockets",
    match: (s) => {
      const socketError =
        /Socket2 doesn't support the compile target|could not find `(net|unix)` in `(sys|mio)`|unresolved import `(mio|tokio)::net`|mio[^\n]*(not supported|unsupported)[^\n]*(platform|target)|(socket2|mio)[^\n]*not supported on this platform/i.test(
          s,
        );
      if (!socketError) return [];
      const crates = compilingCrates(s);
      const drivers = SOCKET_CRATES.filter((c) => crates.has(c));
      return [
        {
          rule: "sockets",
          title:
            drivers.length > 0
              ? `${drivers.map((d) => `\`${d}\``).join(", ")} need raw sockets, which ${TARGET} does not have`
              : `A dependency needs raw sockets (socket2 / mio / tokio net), which ${TARGET} does not have`,
          advice: SOCKET_ADVICE,
          crate: /socket2/i.test(s) ? "socket2" : "mio",
        },
      ];
    },
  },
  {
    id: "getrandom",
    match: (s) =>
      /getrandom/.test(s) &&
      /the wasm\d*-unknown-unknown targets are not supported by default|target is not supported|unsupported target|wasm_js/i.test(
        s,
      )
        ? [
            {
              rule: "getrandom",
              title: "`getrandom` rejects the target it is being built for",
              advice: [
                `On ${TARGET} getrandom needs no \`js\` / \`wasm_js\` feature. This error usually means the build is not for ${TARGET}: check that \`.cargo/config.toml\` still has \`[build] target = "${TARGET}"\` and that no \`--target\` override or \`getrandom_backend\` cfg in RUSTFLAGS is set.`,
                "If it persists, run `cargo update -p getrandom` (very old releases do not know WASI), and make sure no dependency enables the `custom` or `js` backend.",
              ],
              crate: "getrandom",
            },
          ]
        : [],
  },
  {
    id: "psm",
    match: (s) =>
      /section too large/i.test(s) ||
      failedBuildScripts(s).includes("psm") ||
      /could not compile `psm`/.test(s)
        ? [
            {
              rule: "psm",
              title: "`psm` (pulled in by `stacker`) fails to build for wasm",
              advice: [
                "Commit `Cargo.lock` and pin a working version of the dependent crate (`cargo update -p <crate> --precise <version>`), or drop the dependency that uses `stacker`.",
              ],
              crate: "psm",
            },
          ]
        : [],
  },
  {
    id: "rayon",
    match: (s) =>
      /could not compile `(rayon|rayon-core)`/.test(s)
        ? [
            {
              rule: "rayon",
              title:
                "`rayon` needs OS threads, which are not available in `use rust`",
              advice: THREAD_ADVICE,
              crate: "rayon",
            },
          ]
        : [],
  },
];

/** Findings for a failed `cargo build` (stderr as captured). */
export function diagnoseCargoFailure(stderr: string): Finding[] {
  const seen = new Set<string>();
  const out: Finding[] = [];
  for (const rule of RULES) {
    for (const f of rule.match(stderr)) {
      const key = `${f.rule}:${f.crate ?? ""}`;
      if (seen.has(key)) continue;
      seen.add(key);
      out.push(f);
    }
  }
  return out;
}

/** Arguments of the `cargo tree` call that shows who depends on `crate`. */
export function cargoTreeArgs(crate: string): string[] {
  return ["tree", "-i", crate, "--target", TARGET, "-e", "normal"];
}

/** Human-readable text for findings; `trees` maps crate -> `cargo tree` output. */
export function formatFindings(
  findings: Finding[],
  trees: Record<string, string> = {},
): string {
  const lines: string[] = [];
  for (const f of findings) {
    lines.push("", `Hint: ${f.title}.`);
    for (const a of f.advice) lines.push(`  - ${a}`);
    if (!f.crate) continue;
    const tree = trees[f.crate]?.trimEnd();
    if (tree) {
      lines.push(
        `  Who depends on \`${f.crate}\` (cargo ${cargoTreeArgs(f.crate).join(" ")}):`,
        ...tree
          .split("\n")
          .slice(0, 25)
          .map((l) => `    ${l}`),
      );
    } else {
      lines.push(
        `  To see who pulls it in: cargo ${cargoTreeArgs(f.crate).join(" ")}`,
      );
    }
  }
  return lines.join("\n");
}

/**
 * Hint for a runtime (host) error message from a Rust action, for cases the
 * build cannot catch. Returns null when the text is not recognised.
 */
export function diagnoseRuntimeError(text: string): string | null {
  if (
    /failed to spawn thread|ThreadPoolBuildError|global thread pool has not been initialized|operation not supported on this platform.*thread|thread::spawn/i.test(
      text,
    )
  ) {
    return `Hint: this looks like a thread being started. ${THREAD_ADVICE.join(" ")}`;
  }
  if (/sock_(connect|open|accept)|Unsupported.*(tcp|socket)/i.test(text)) {
    return `Hint: raw sockets are not available in \`use rust\`. ${SOCKET_ADVICE.slice(1).join(" ")}`;
  }
  return null;
}
