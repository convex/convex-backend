import { defineConfig, configDefaults } from "vitest/config";

export default defineConfig({
  test: {
    isolate: true,
    watch: false,
    environment: "edge-runtime",
    server: { deps: { inline: ["convex-test"] } },
    exclude: [...configDefaults.exclude, "**/dist/**"],
  },
});
