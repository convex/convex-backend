import { defineConfig } from "vitest/config";

export default defineConfig({
  ssr: {
    resolve: {
      conditions: [
        "convex-internal-types",
        "module",
        "node",
        "development|production",
      ],
    },
  },
  test: {
    isolate: true,
    watch: false,
  },
});
