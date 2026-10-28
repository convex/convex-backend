import child_process from "child_process";
import path from "path";
import { promisify } from "util";
import * as Sentry from "@sentry/node";
import { z } from "zod";
import { nodeFs } from "../../../bundler/fs.js";

/** The file the shell runs for `name`, like `which`. */
export function onPath(name: string, pathDirs: string): string[] {
  const dir = pathDirs
    .split(path.delimiter)
    .find((d) => nodeFs.exists(path.join(d, name)));
  return dir === undefined ? [] : [path.join(dir, name)];
}

/** The folders directly inside `dir`, sorted by name, or none when `dir` doesn't exist. */
export function subfolders(dir: string): string[] {
  let entries;
  try {
    entries = nodeFs.listDir(dir);
  } catch {
    return [];
  }
  return entries
    .filter((entry) => entry.isDirectory())
    .map((entry) => path.join(dir, entry.name))
    .sort();
}

/** The first line a CLI prints for `--version`. */
export async function versionOf(cli: string): Promise<string> {
  const { stdout } = await promisify(child_process.execFile)(cli, [
    "--version",
  ]);
  return stdout.split("\n")[0];
}

/** Runs a command and resolves to whether it succeeded and everything it printed. */
export function run(
  command: string,
  args: string[],
): Promise<{ ok: boolean; output: string }> {
  return new Promise((resolve) => {
    let output = "";

    const proc = child_process.spawn(command, args, {
      stdio: ["ignore", "pipe", "pipe"],
    });

    proc.stdout.on("data", (chunk) => (output += chunk));
    proc.stderr.on("data", (chunk) => (output += chunk));
    proc.on("error", () => resolve({ ok: false, output }));
    proc.on("close", (code) => resolve({ ok: code === 0, output }));
  });
}

/** Runs a command and resolves to its stdout, or null when it can't be started. */
export function capture(
  command: string,
  args: string[],
): Promise<string | null> {
  return new Promise((resolve) => {
    let stdout = "";

    const proc = child_process.spawn(command, args, {
      stdio: ["ignore", "pipe", "inherit"],
    });

    proc.stdout.on("data", (chunk) => (stdout += chunk));
    proc.on("error", () => resolve(null));
    proc.on("close", () => resolve(stdout));
  });
}

/** Fetches a JSON file from GitHub, or resolves to null when it can't, reporting why to Sentry. */
export async function fetchGitHubJson<T>(path: string): Promise<T | null> {
  try {
    const response = await fetch(`https://raw.githubusercontent.com/${path}`);
    if (!response.ok) {
      Sentry.captureException(
        new Error(`Failed to fetch ${path}: status = ${response.status}`),
      );
      return null;
    }
    return (await response.json()) as T;
  } catch (error) {
    Sentry.captureException(error);
    return null;
  }
}

/** Parses a JSON list from a harness CLI, or returns null and reports to Sentry when it isn't JSON or doesn't match. */
export function parseList<T>(
  schema: z.ZodType<T, z.ZodTypeDef, unknown>,
  list: string,
  command: string,
): T | null {
  let result;
  try {
    result = schema.safeParse(JSON.parse(list));
  } catch {
    // A failing CLI can print nothing, which isn't JSON.
    result = undefined;
  }
  if (!result?.success) {
    Sentry.captureMessage(`Unexpected output from \`${command}\``, "error");
    return null;
  }
  return result.data;
}

/** Pads each column to its widest cell and indents every row. */
export function formatRows(rows: string[][], indent = "  "): string[] {
  if (rows.length === 0) return [];
  const widths = rows[0].map((_, i) =>
    Math.max(...rows.map((row) => row[i].length)),
  );
  return rows.map((row) =>
    `${indent}${row.map((cell, i) => cell.padEnd(widths[i])).join("  ")}`.trimEnd(),
  );
}
