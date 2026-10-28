import { Plugin } from "esbuild";

// Stub `import "server-only"` to an empty module so user code that uses
// Next.js's server-only can be bundled and analyzed correctly:
// https://nextjs.org/docs/app/getting-started/server-and-client-components#preventing-environment-poisoning
export const serverOnlyPlugin: Plugin = {
  name: "convex-server-only",
  setup(build) {
    build.onResolve({ filter: /^server-only$/ }, (args) => ({
      path: args.path,
      namespace: "server-only-stub",
    }));
    build.onLoad({ filter: /.*/, namespace: "server-only-stub" }, () => ({
      contents: "",
      loader: "js",
    }));
  },
};
