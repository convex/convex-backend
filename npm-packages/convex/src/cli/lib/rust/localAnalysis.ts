import path from "path";
import os from "os";
import { spawn } from "child_process";
// eslint-disable-next-line no-restricted-imports
import nodeFs from "fs";
import { Context } from "../../../bundler/context.js";
import { bundle, entryPointsByEnvironment } from "../../../bundler/index.js";
import { logVerbose } from "../../../bundler/log.js";
import { AnalyzedModule } from "../deployApi/modules.js";

/**
 * Local (pre-push) analysis of the project's TypeScript Convex functions.
 *
 * The Rust build needs `src/_generated/api.rs` to already know the TS
 * functions that Rust calls, but the server only analyzes functions during a
 * push, which comes after the Rust build. So we bundle the isolate-runtime
 * modules (as `convex dev` does anyway), import them in a short-lived Node
 * subprocess, and read the registered functions' visibility and validators
 * (`exportArgs()` / `exportReturns()`), the same data the server extracts.
 */

/** Runs in the subprocess: prints one JSON line `{ [module]: fn[] }`. */
export const ANALYZE_SCRIPT = `
import { pathToFileURL } from "node:url";
const [dir, ...mods] = process.argv.slice(1);
const out = {};
for (const m of mods) {
  let ns;
  try {
    ns = await import(pathToFileURL(dir + "/" + m).href);
  } catch (e) {
    process.stderr.write("convex: could not load " + m + ": " + (e && e.message || e) + "\\n");
    continue;
  }
  const fns = [];
  for (const [name, f] of Object.entries(ns)) {
    if (typeof f !== "function") continue;
    const udfType = f.isQuery ? "Query" : f.isMutation ? "Mutation" : f.isAction ? "Action" : null;
    if (udfType === null) continue;
    const kind = f.isPublic ? "public" : f.isInternal ? "internal" : null;
    if (kind === null) continue;
    let args = null, returns = null;
    try { args = f.exportArgs(); returns = f.exportReturns(); } catch {}
    fns.push({ name, udfType, visibility: { kind }, args, returns });
  }
  out[m] = fns;
}
process.stdout.write("\\n@@convex-analysis@@" + JSON.stringify(out) + "\\n");
`;

/** Parse the subprocess output into analyzed modules. */
export function parseAnalysisOutput(
  stdout: string,
): Record<string, AnalyzedModule> | null {
  const marker = "@@convex-analysis@@";
  const i = stdout.lastIndexOf(marker);
  if (i < 0) return null;
  const line = stdout.slice(i + marker.length).split("\n")[0];
  try {
    const raw = JSON.parse(line) as Record<string, any[]>;
    const result: Record<string, AnalyzedModule> = {};
    for (const [mod, functions] of Object.entries(raw)) {
      result[mod] = {
        functions,
        httpRoutes: null,
        cronSpecs: null,
        sourceMapped: null,
      } as AnalyzedModule;
    }
    return result;
  } catch {
    return null;
  }
}

function runNode(
  script: string,
  args: string[],
  timeoutMs: number,
): Promise<{ stdout: string; ok: boolean }> {
  return new Promise((resolve) => {
    const child = spawn(
      process.execPath,
      ["--input-type=module", "-e", script, "--", ...args],
      { stdio: ["ignore", "pipe", "pipe"], env: process.env },
    );
    let stdout = "";
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (d: string) => (stdout += d));
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (d: string) => logVerbose(d.trimEnd()));
    const timer = setTimeout(() => child.kill("SIGKILL"), timeoutMs);
    child.on("error", () => {
      clearTimeout(timer);
      resolve({ stdout, ok: false });
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve({ stdout, ok: code === 0 });
    });
  });
}

/**
 * Analyze the TS functions under `functionsDir` locally. Returns null when
 * anything goes wrong (callers then fall back to the post-push analysis).
 */
export async function analyzeTsFunctionsLocally(
  ctx: Context,
  functionsDir: string,
): Promise<Record<string, AnalyzedModule> | null> {
  let tmp: string | null = null;
  try {
    const { isolate } = await entryPointsByEnvironment(ctx, functionsDir);
    if (isolate.length === 0) return {};
    const result = await bundle({
      ctx,
      dir: functionsDir,
      entryPoints: isolate,
      generateSourceMaps: false,
      platform: "browser",
    });
    tmp = nodeFs.mkdtempSync(path.join(os.tmpdir(), "convex-analyze-"));
    nodeFs.writeFileSync(path.join(tmp, "package.json"), '{"type":"module"}\n');
    for (const m of result.modules) {
      const file = path.join(tmp, m.path);
      nodeFs.mkdirSync(path.dirname(file), { recursive: true });
      nodeFs.writeFileSync(file, m.source);
    }
    const entryNames = isolate.map(
      (p) =>
        path
          .relative(functionsDir, p)
          .split(path.sep)
          .join("/")
          .replace(/\.[^./]+$/, "") + ".js",
    );
    const { stdout, ok } = await runNode(
      ANALYZE_SCRIPT,
      [tmp, ...entryNames],
      30_000,
    );
    if (!ok) return null;
    return parseAnalysisOutput(stdout);
  } catch (e: any) {
    logVerbose(`Local analysis of TS functions failed: ${e?.message ?? e}`);
    return null;
  } finally {
    if (tmp) nodeFs.rmSync(tmp, { recursive: true, force: true });
  }
}
