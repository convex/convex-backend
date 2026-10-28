#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */
const fs = require("fs");
const path = require("path");
const process = require("process");

// when browser/index-node.ts imports simple_client, don't bundle it
const importPathPlugin = {
  name: "import-path",
  setup(build) {
    build.onResolve({ filter: /^\.\/simple_client/ }, (args) => {
      return { path: args.path, external: true };
    });
  },
};

// esbuild is a bundler, but we're not bundling: we're compiling per-file
const allSourceFiles = [...walkSync("src")].filter((name) => {
  if (name.startsWith("api")) {
    console.log("api:", name);
  }
  if (name.includes("test")) return false;
  // .d.ts files are manually copied over
  if (name.endsWith(".d.ts")) return false;
  return (
    name.endsWith(".ts") ||
    name.endsWith(".tsx") ||
    name.endsWith(".js") ||
    name.endsWith(".jsx")
  );
});

const [tempDir] = process.argv
  .filter((arg) => arg.startsWith("tempDir="))
  .map((arg) => arg.slice(8));

if (process.argv.includes("esm")) {
  const opts = {
    entryPoints: allSourceFiles.filter(
      (f) => !f.includes("simple_client-node"),
    ),
    bundle: false,
    sourcemap: true,
    outdir: tempDir + "/esm",
    target: "es2020",
  };
  require("esbuild")
    .build(opts)
    .catch(() => process.exit(1));

  // bundle a WebSocket implementation into Node.js build
  require("esbuild")
    .build({
      ...opts,
      entryPoints: ["src/browser/simple_client-node.ts"],
      outdir: undefined,
      outfile: tempDir + "/esm/browser/simple_client-node.js",
      platform: "node",
      format: "esm",
      bundle: true,
      external: ["./src/browser/simple_client.ts", "stream"],
      plugins: [importPathPlugin],
      banner: {
        // https://github.com/evanw/esbuild/issues/1921
        js: "import {createRequire} from 'module';import {resolve as nodePathResolve} from 'path';const require=createRequire(nodePathResolve('.'));",
      },
    })
    .catch(() => process.exit(1));
}

if (process.argv.includes("cjs")) {
  const opts = {
    entryPoints: allSourceFiles.filter(
      (f) => !f.includes("simple_client-node"),
    ),
    format: "cjs",
    bundle: false,
    sourcemap: true,
    outdir: tempDir + "/cjs",
    target: "es2020",
  };
  require("esbuild")
    .build(opts)
    .catch(() => process.exit(1));

  // bundle a WebSocket implementation into Node.js build
  require("esbuild")
    .build({
      ...opts,
      bundle: true,
      outdir: undefined,
      entryPoints: ["src/browser/simple_client-node.ts"],
      outfile: tempDir + "/cjs/browser/simple_client-node.js",
      platform: "node",
      external: ["./src/browser/simple_client.ts"],
      plugins: [importPathPlugin],
    })
    .catch(() => process.exit(1));
}

if (process.argv.includes("browser-script-tag")) {
  require("esbuild")
    .build({
      entryPoints: ["browser-bundle.js"],
      bundle: true,
      platform: "browser",
      sourcemap: true,
      outfile: tempDir + "/browser.bundle.js",
      globalName: "convex",
      logLevel: "warning",
    })
    .catch(() => process.exit(1));
}

if (process.argv.includes("react-script-tag")) {
  const esbuild = require("esbuild");
  const { externalGlobalPlugin } = require("esbuild-plugin-external-global");
  esbuild
    .build({
      entryPoints: ["src/react/index.ts"],
      bundle: true,
      platform: "browser",
      external: ["react", "react-dom"],
      sourcemap: true,
      outfile: tempDir + "/react.bundle.js",
      globalName: "convex",
      logLevel: "warning",
      plugins: [
        externalGlobalPlugin({
          react: "window.React",
          "react-dom": "window.ReactDOM",
        }),
      ],
    })
    .catch(() => process.exit(1));
}

if (process.argv.includes("standalone-cli")) {
  // Bundle in all dependencies except binaries like esbuild and fsevents.
  require("esbuild")
    .build({
      entryPoints: ["src/cli/index.ts"],
      bundle: true,
      platform: "node",
      sourcemap: true,
      target: "node20",
      external: [
        // contains a binary
        "esbuild",
        // contains a binary
        "fsevents",
        // prettier 3 is more difficult to bundle into a CJS bundle.
        // TODO figure out how to do this (making import.meta work?)
        "prettier",
        // Bundling ws externally seems to fix some issues with Bun compatibility
        "ws",
      ],
      outfile: tempDir + "/cli.bundle.cjs",
      logLevel: "warning",
    })
    .catch(() => process.exit(1));
}

function* walkSync(dir) {
  const files = fs.readdirSync(dir, { withFileTypes: true });
  for (const file of files) {
    if (file.isDirectory()) {
      yield* walkSync(path.join(dir, file.name));
    } else {
      yield path.join(dir, file.name);
    }
  }
}
