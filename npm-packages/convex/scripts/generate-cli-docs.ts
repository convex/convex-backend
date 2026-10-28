#!/usr/bin/env tsx
/**
 * Generate Markdown reference docs for the `convex` CLI and write
 * them to `npm-packages/docs/docs/cli/reference/`.
 *
 * Run from `npm-packages/convex/`:
 *
 *   npx tsx scripts/generate-cli-docs.ts
 *
 * Pass `--dry-run` to instead check that the existing files match what
 * would be generated. Prints a diff and exits non-zero if anything
 * would change.
 */
import { codeFrameColumns } from "@babel/code-frame";
import chalk from "chalk";
import { readdir, readFile, mkdir, rm, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { buildProgram } from "../src/cli/program.js";
import { generateDocs } from "../src/cli/lib/generateDocs.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUTPUT_DIR = resolve(__dirname, "../../docs/docs/cli/reference");

const CATEGORY_FILE = "_category_.json";
const CATEGORY_CONTENTS =
  JSON.stringify({ label: "Command Reference", position: 99 }, null, 2) + "\n";

async function main() {
  const dryRun = process.argv.includes("--dry-run");

  const program = buildProgram();
  const generated: Record<string, string> = {
    [CATEGORY_FILE]: CATEGORY_CONTENTS,
    ...generateDocs(program),
  };

  const existing = await readExisting(OUTPUT_DIR);
  const changes = diffTrees(existing, generated);

  if (changes.length === 0) {
    // eslint-disable-next-line no-console
    console.log(
      chalk.green.bold(
        `✅ CLI reference docs are up to date (${Object.keys(generated).length} files).`,
      ),
    );
    return;
  }

  for (const change of changes) {
    printChange(change);
  }

  if (dryRun) {
    // eslint-disable-next-line no-console
    console.error(
      chalk.red.bold(`\n❌ ${changes.length} file(s) would be modified.`) +
        chalk.yellow(
          ` Run ${chalk.cyan("`just regenerate-cli-docs`")} to update.`,
        ),
    );
    process.exit(1);
  }

  await rm(OUTPUT_DIR, { recursive: true, force: true });
  await mkdir(OUTPUT_DIR, { recursive: true });
  for (const [filePath, contents] of Object.entries(generated)) {
    const fullPath = join(OUTPUT_DIR, filePath);
    await mkdir(dirname(fullPath), { recursive: true });
    await writeFile(fullPath, contents);
  }

  // eslint-disable-next-line no-console
  console.log(
    chalk.green.bold(
      `\n✨ Wrote ${Object.keys(generated).length} command reference page(s)`,
    ) + chalk.dim(` to ${OUTPUT_DIR}`),
  );
}

async function readExisting(dir: string): Promise<Record<string, string>> {
  const out: Record<string, string> = {};
  let entries;
  try {
    entries = await readdir(dir, { withFileTypes: true, recursive: true });
  } catch (err: any) {
    if (err.code === "ENOENT") return out;
    throw err;
  }
  for (const entry of entries) {
    if (!entry.isFile()) continue;
    const full = join((entry as any).parentPath ?? dir, entry.name);
    const rel = full.slice(dir.length + 1);
    out[rel] = await readFile(full, "utf8");
  }
  return out;
}

type Change =
  | { kind: "added"; path: string; newContents: string }
  | { kind: "removed"; path: string; oldContents: string }
  | {
      kind: "modified";
      path: string;
      oldContents: string;
      newContents: string;
    };

function diffTrees(
  existing: Record<string, string>,
  generated: Record<string, string>,
): Change[] {
  const changes: Change[] = [];
  const paths = new Set([...Object.keys(existing), ...Object.keys(generated)]);
  for (const path of [...paths].sort()) {
    const oldContents = existing[path];
    const newContents = generated[path];
    if (oldContents === undefined) {
      changes.push({ kind: "added", path, newContents });
    } else if (newContents === undefined) {
      changes.push({ kind: "removed", path, oldContents });
    } else if (oldContents !== newContents) {
      changes.push({ kind: "modified", path, oldContents, newContents });
    }
  }
  return changes;
}

function printChange(change: Change) {
  // eslint-disable-next-line no-console
  const log = console.error;
  if (change.kind === "added") {
    log(
      chalk.green(`\n  ✓ ${chalk.white.bgGreen("  Created  ")} ${change.path}`),
    );
    log(frame(change.newContents, fullRange(change.newContents)));
    return;
  }
  if (change.kind === "removed") {
    log(chalk.red(`\n  ✓ ${chalk.white.bgRed("  Deleted  ")} ${change.path}`));
    log(frame(change.oldContents, fullRange(change.oldContents)));
    return;
  }
  log(chalk.blue(`\n  ✓ ${chalk.white.bgBlue("  Updated  ")} ${change.path}`));
  const range = changedLineRange(change.oldContents, change.newContents);
  log(chalk.red("  Before:"));
  log(frame(change.oldContents, range.old));
  log(chalk.green("  After:"));
  log(frame(change.newContents, range.new));
}

function frame(
  contents: string,
  range: { start: number; end: number },
): string {
  if (range.start === 0) return chalk.dim("    (empty)");
  return codeFrameColumns(
    contents,
    {
      start: { line: range.start },
      end: { line: range.end },
    },
    { linesAbove: 1, linesBelow: 1, forceColor: true, highlightCode: true },
  );
}

function fullRange(contents: string): { start: number; end: number } {
  const lines = contents.split("\n");
  if (contents === "") return { start: 0, end: 0 };
  return { start: 1, end: lines.length };
}

function changedLineRange(
  oldStr: string,
  newStr: string,
): {
  old: { start: number; end: number };
  new: { start: number; end: number };
} {
  const oldLines = oldStr.split("\n");
  const newLines = newStr.split("\n");
  let prefix = 0;
  while (
    prefix < oldLines.length &&
    prefix < newLines.length &&
    oldLines[prefix] === newLines[prefix]
  ) {
    prefix++;
  }
  let suffix = 0;
  while (
    suffix < oldLines.length - prefix &&
    suffix < newLines.length - prefix &&
    oldLines[oldLines.length - 1 - suffix] ===
      newLines[newLines.length - 1 - suffix]
  ) {
    suffix++;
  }
  const oldEnd = oldLines.length - suffix;
  const newEnd = newLines.length - suffix;
  return {
    old: {
      start: prefix + 1 > oldEnd ? Math.max(oldEnd, 1) : prefix + 1,
      end: Math.max(oldEnd, prefix + 1),
    },
    new: {
      start: prefix + 1 > newEnd ? Math.max(newEnd, 1) : prefix + 1,
      end: Math.max(newEnd, prefix + 1),
    },
  };
}

await main();
