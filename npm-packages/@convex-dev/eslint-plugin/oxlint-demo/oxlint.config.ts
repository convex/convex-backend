import { defineConfig } from "oxlint";
import convexPlugin from "@convex-dev/eslint-plugin";

export default defineConfig({
  jsPlugins: ["@convex-dev/eslint-plugin"],
  ignorePatterns: ["convex/_generated"],
  overrides: [
    {
      files: ["**/convex/**/*.ts"],
      rules: convexPlugin.configs.recommended[0].rules,
    },
  ],
});
