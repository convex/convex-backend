#!/usr/bin/env node
import { fileURLToPath } from "url";
import { dirname } from "path";
import skott from "skott";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const __root = dirname(__dirname);

async function entrypointHasCycles(entrypoint) {
  // Note that skott can do a lot of other things too!
  const { useGraph } = await skott({
    entrypoint: `./dist/esm/${entrypoint}/index.js`,
    incremental: false,
    cwd: __root,
    includeBaseDir: true,
    verbose: false,
  });
  const { findCircularDependencies } = useGraph();

  const circular = findCircularDependencies();
  if (circular.length) {
    console.log("Found import cycles by traversing", entrypoint);
    console.log(circular);
    return false;
  }
  return true;
}

let allOk = true;
// These haven't been fixed yet so we don't fail if they have cycles.
for (const entrypoint of [
  "bundler",
  "nextjs",
  "react",
  "react-auth0",
  "react-clerk",
  "values",
  // don't care about cycles in CLI
]) {
  const ok = await entrypointHasCycles(entrypoint);
  allOk &&= ok;
}

for (const entrypoint of ["server", "browser"]) {
  if (!(await entrypointHasCycles(entrypoint))) {
    process.exit(1);
  }
  console.log(`No import cycles found in ${entrypoint}.`);
}
process.exit(0);
