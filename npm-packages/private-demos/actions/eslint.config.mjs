import eslint from "@eslint/js";
import tseslint from "typescript-eslint";
import convexPlugin from "@convex-dev/eslint-plugin";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

export default tseslint.config(
  {
    ignores: ["dist", "convex/_generated/**"],
  },
  eslint.configs.recommended,
  ...convexPlugin.configs.recommended,
  ...tseslint.configs.recommended,

  {
    rules: {
      "@typescript-eslint/no-require-imports": "off",
      "@typescript-eslint/no-explicit-any": "off",
    },
  },
  {
    files: ["**/convex/**/*.ts"],
    languageOptions: {
      parser: tseslint.parser,
      parserOptions: {
        project: [path.join(__dirname, "convex", "tsconfig.json")],
        tsconfigRootDir: __dirname,
      },
    },
    rules: {
      "@typescript-eslint/no-floating-promises": "error",
      "@convex-dev/require-args-validator": [
        "error",
        {
          ignoreUnusedArguments: true,
        },
      ],
    },
  },
);
