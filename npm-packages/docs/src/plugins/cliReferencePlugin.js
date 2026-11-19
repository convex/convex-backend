// Generates the CLI command reference pages into docs/cli/reference/ from the
// `convex` package. Docusaurus initializes every plugin before the docs plugin
// reads its files, so this generates them in the initializer, and the dev
// server picks up CLI changes on restart.
const { execFileSync } = require("child_process");
const path = require("path");

const CONVEX_PACKAGE_DIR = path.resolve(__dirname, "../../../convex");
const OUT_DIR = path.resolve(__dirname, "../../docs/cli/reference");

module.exports = function cliReferencePlugin() {
  execFileSync(
    path.join(CONVEX_PACKAGE_DIR, "node_modules/.bin/tsx"),
    ["scripts/generate-cli-docs.ts", "--out", OUT_DIR],
    { cwd: CONVEX_PACKAGE_DIR, stdio: "inherit" },
  );
  return { name: "cli-reference" };
};
