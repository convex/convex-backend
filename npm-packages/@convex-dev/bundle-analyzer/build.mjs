import * as esbuild from "esbuild";

await esbuild.build({
  entryPoints: ["src/cli.ts"],
  outfile: "dist/cli.js",
  bundle: true,
  platform: "node",
  target: "node20",
  format: "esm",
  // `convex/bundle-analyzer-internal` only resolves under this condition, to
  // the convex package's source.
  conditions: ["convex-internal-types"],
  external: ["esbuild"],
  banner: {
    // esbuild's ESM output turns `require` calls in bundled CommonJS
    // dependencies (e.g. commander's `require("node:events")`) into a shim that
    // throws unless a real `require` is in scope.
    js: [
      "#!/usr/bin/env node",
      "import { createRequire as __bundleAnalyzerCreateRequire } from 'module';",
      "const require = __bundleAnalyzerCreateRequire(import.meta.url);",
    ].join("\n"),
  },
  logLevel: "warning",
});
