// Generates the CLI command reference pages into docs/cli/reference/ from the
// `convex` package that the reference pages document (`options.source`, see
// scripts/reference-source.js). Docusaurus initializes every plugin before the
// docs plugin reads its files, so this generates them in the initializer, and
// the dev server picks up CLI changes on restart.
const { execFileSync } = require("child_process");
const fs = require("fs");
const path = require("path");
const { addPageBanner } = require("../../scripts/page-banner.js");

const CONVEX_PACKAGE_DIR = path.resolve(__dirname, "../../../convex");
const OUT_DIR = path.resolve(__dirname, "../../docs/cli/reference");

module.exports = function cliReferencePlugin(context, { source }) {
  execFileSync(
    path.join(CONVEX_PACKAGE_DIR, "node_modules/.bin/tsx"),
    [
      "scripts/generate-cli-docs.ts",
      "--out",
      OUT_DIR,
      "--source",
      source.packageDir,
    ],
    { cwd: CONVEX_PACKAGE_DIR, stdio: "inherit" },
  );

  if (source.pageBanner) {
    for (const file of fs.readdirSync(OUT_DIR, { recursive: true })) {
      if (!file.endsWith(".mdx")) {
        continue;
      }
      const filePath = path.join(OUT_DIR, file);
      fs.writeFileSync(
        filePath,
        addPageBanner(fs.readFileSync(filePath, "utf8"), source.pageBanner),
      );
    }
  }

  return { name: "cli-reference" };
};
