#!/usr/bin/env tsx
/**
 * Generate Markdown reference docs for the `convex` CLI into a directory. The
 * docs site runs this at build and dev time (see
 * `npm-packages/docs/src/plugins/cliReferencePlugin.js`).
 *
 * Run from `npm-packages/convex/`:
 *
 *   npx tsx scripts/generate-cli-docs.ts --out <dir> [--source <dir>]
 *
 * `--source` is the `convex` package whose CLI to document, e.g. an extracted
 * release. It defaults to this package. Either way, this package's generator
 * renders the pages.
 */
import { mkdir, rm, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import { generateDocs } from "../src/cli/lib/generateDocs.js";

const __dirname = dirname(fileURLToPath(import.meta.url));

const CATEGORY_FILE = "_category_.json";
const CATEGORY_CONTENTS =
  JSON.stringify({ label: "Command Reference", position: 99 }, null, 2) + "\n";

async function main() {
  const { values } = parseArgs({
    options: {
      out: { type: "string" },
      source: { type: "string", default: resolve(__dirname, "..") },
    },
  });
  if (values.out === undefined) {
    throw new Error("Pass --out <dir>");
  }
  const outDir = resolve(values.out);

  const { buildProgram } = await import(
    pathToFileURL(join(resolve(values.source), "src/cli/program.ts")).href
  );
  const generated: Record<string, string> = {
    [CATEGORY_FILE]: CATEGORY_CONTENTS,
    ...generateDocs(buildProgram()),
  };

  await rm(outDir, { recursive: true, force: true });
  await mkdir(outDir, { recursive: true });
  for (const [filePath, contents] of Object.entries(generated)) {
    const fullPath = join(outDir, filePath);
    await mkdir(dirname(fullPath), { recursive: true });
    await writeFile(fullPath, contents);
  }
}

await main();
