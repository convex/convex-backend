import path from "path";
// eslint-disable-next-line no-restricted-imports
import nodeFs from "fs";
import { spawnSync } from "child_process";
import { RUST_TARGET } from "./scaffold.js";

export interface ExecResult {
  status: number | null;
  stdout: string;
  stderr: string;
  error?: { code?: string } | Error | undefined;
}
export type Exec = (
  cmd: string,
  args: string[],
  opts: { cwd: string; inherit?: boolean },
) => ExecResult;

export const defaultExec: Exec = (cmd, args, opts) => {
  const r = spawnSync(cmd, args, {
    cwd: opts.cwd,
    encoding: "utf8",
    stdio: opts.inherit ? "inherit" : ["ignore", "pipe", "pipe"],
  });
  return {
    status: r.status,
    stdout: r.stdout ?? "",
    stderr: r.stderr ?? "",
    error: r.error,
  };
};

export type ToolchainStatus =
  | { ok: true }
  | {
      ok: false;
      kind: "no-cargo" | "no-rustup" | "no-toolchain" | "no-target";
      message: string;
      /** Command that fixes it (for no-target / no-toolchain). */
      fix?: string[];
    };

/** `channel` from `<rustDir>/rust-toolchain.toml`, if any. */
export function readToolchainChannel(rustDir: string): string | null {
  for (const f of ["rust-toolchain.toml", "rust-toolchain"]) {
    try {
      const text = nodeFs.readFileSync(path.join(rustDir, f), "utf8");
      const m = text.match(/^\s*channel\s*=\s*"([^"]+)"/m);
      if (m) return m[1];
      const bare = text.trim();
      if (f === "rust-toolchain" && bare && !bare.includes("\n")) return bare;
    } catch {
      // not present
    }
  }
  return null;
}

const RUSTUP_URL = "https://rustup.rs";

export function checkRustToolchain(
  rustDir: string,
  exec: Exec = defaultExec,
): ToolchainStatus {
  const notFound = (r: ExecResult) => (r.error as any)?.code === "ENOENT";
  const cargo = exec("cargo", ["--version"], { cwd: rustDir });
  const rustup = exec("rustup", ["--version"], { cwd: rustDir });
  if (notFound(rustup)) {
    if (notFound(cargo)) {
      return {
        ok: false,
        kind: "no-cargo",
        message: `Neither \`cargo\` nor \`rustup\` was found. Install Rust with rustup: ${RUSTUP_URL}\nThen re-run this command (the ${RUST_TARGET} target will be set up for you).`,
      };
    }
    return {
      ok: false,
      kind: "no-rustup",
      message: `\`rustup\` was not found, so the ${RUST_TARGET} target can't be verified. Install rustup: ${RUSTUP_URL}\n(If you manage Rust another way, make sure the ${RUST_TARGET} target is installed.)`,
    };
  }
  if (notFound(cargo)) {
    return {
      ok: false,
      kind: "no-cargo",
      message: `\`cargo\` was not found. Install Rust with rustup: ${RUSTUP_URL}`,
    };
  }
  const channel = readToolchainChannel(rustDir);
  const toolchainArgs = channel ? ["--toolchain", channel] : [];
  const list = exec(
    "rustup",
    ["target", "list", "--installed", ...toolchainArgs],
    { cwd: rustDir },
  );
  if (list.status !== 0) {
    const fix = [
      "rustup",
      "toolchain",
      "install",
      channel ?? "stable",
      "--profile",
      "minimal",
      "--target",
      RUST_TARGET,
    ];
    if (/not installed|no such toolchain|is not installed/i.test(list.stderr)) {
      return {
        ok: false,
        kind: "no-toolchain",
        message: `The Rust toolchain \`${channel ?? "stable"}\` is not installed. Run:\n  ${fix.join(" ")}`,
        fix,
      };
    }
    // Unknown rustup failure: don't block the build, cargo will report it.
    return { ok: true };
  }
  const installed = list.stdout.split("\n").map((l) => l.trim());
  if (installed.includes(RUST_TARGET)) return { ok: true };
  const fix = ["rustup", "target", "add", RUST_TARGET, ...toolchainArgs];
  return {
    ok: false,
    kind: "no-target",
    message: `The ${RUST_TARGET} Rust target is not installed${channel ? ` for toolchain ${channel}` : ""}. Run:\n  ${fix.join(" ")}`,
    fix,
  };
}
