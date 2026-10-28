// @ts-check

import tseslint from "typescript-eslint";
import reactHooksPlugin from "eslint-plugin-react-hooks";
import reactPlugin from "eslint-plugin-react";
import vitest from "@vitest/eslint-plugin";
import { fixupPluginRules } from "@eslint/compat";
import globals from "globals";
import path from "node:path";
import { fileURLToPath } from "node:url";
import js from "@eslint/js";
import { FlatCompat } from "@eslint/eslintrc";
import eslintConfigPrettier from "eslint-config-prettier";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const compat = new FlatCompat({
  baseDirectory: __dirname,
  recommendedConfig: js.configs.recommended,
  allConfig: js.configs.all,
});

export default [
  {
    ignores: [
      "**/node_modules",
      "**/dist",
      "tmpDist*",
      "**/tmpPackage*",
      "**/custom-vitest-environment.ts",
      "src/cli/generatedApi.ts",
      "src/cli/lib/generatedFunctionLogsApi.ts",
      // TODO use a separate config for files that doesn't use TypeScript
      "**/*.js",
      "vitest.config.js",
      "scripts",
      ".prettierrc.js",
      "eslint.config.mjs",
      "jest.config.mjs",
      // we could ignore everything in vendor
      "src/vendor/progress",
    ],
  },
  ...compat.plugins("require-extensions"),
  ...compat.extends("plugin:require-extensions/recommended"),
  js.configs.recommended,
  {
    files: ["**/*.js", "**/*.jsx", "**/*ts", "**/*.tsx"],
    plugins: {
      "@typescript-eslint": tseslint.plugin,
      react: reactPlugin,
      "react-hooks": fixupPluginRules(reactHooksPlugin),
      vitest,
    },

    languageOptions: {
      globals: {
        ...globals.amd,
        ...globals.browser,
        ...globals.node,
      },

      parser: tseslint.parser,
      ecmaVersion: 2022,
      sourceType: "module",

      parserOptions: {
        project: ["./tsconfig.json"],
        tsconfigRootDir: __dirname,
      },
    },

    rules: {
      ...reactHooksPlugin.configs.recommended.rules,
      "no-debugger": "error",
      // any is terrible but we use it a lot (even in our public code).
      "@typescript-eslint/no-explicit-any": "off",
      // asserting that values aren't null is risky but useful.
      "@typescript-eslint/no-non-null-assertion": "off",
      // Warn against interpolating objects
      "@typescript-eslint/restrict-template-expressions": "error",

      "no-redeclare": "off", // breaks for overloads
      "@typescript-eslint/no-redeclare": "error",

      "no-undef": "off",

      // allow (_arg: number) => {}
      "no-unused-vars": "off",
      "@typescript-eslint/no-unused-vars": [
        "error",
        {
          argsIgnorePattern: "^_",
          varsIgnorePattern: "^_",
        },
      ],

      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "warn",

      "no-restricted-syntax": [
        "error",
        {
          // From https://github.com/typescript-eslint/typescript-eslint/issues/1391#issuecomment-1124154589
          // Prefer `private` ts keyword to `#private` private methods
          selector:
            ":matches(PropertyDefinition, MethodDefinition) > PrivateIdentifier.key",
          message: "Use `private` instead",
        },
      ],

      // Makes it harder to accidentally fire off a promise without waiting for it.
      "@typescript-eslint/no-floating-promises": "error",

      // Since `const x = <number>foo;` syntax is ambiguous with JSX syntax some tools don't support it.
      // In particular we need this for depcheck https://github.com/depcheck/depcheck/issues/585
      "@typescript-eslint/consistent-type-assertions": [
        "error",
        {
          assertionStyle: "as",
        },
      ],

      eqeqeq: ["error", "always"],

      // vitest (manually enabled until we can upgrade eslint)
      "vitest/no-focused-tests": [
        "error",
        {
          fixable: false,
        },
      ],

      // Use `logMessage` and friends (CLI specific) or `logger.log` instead.
      "no-console": "error",
    },
  },
  {
    name: "CLI-specific",
    files: [
      "src/cli/**/*.ts",
      "src/bundler/**/*.ts",
      "src/bundle-analyzer-internal/**/*.ts",
    ],
    ignores: ["**/*.test.ts"],
    rules: {
      "no-restricted-imports": [
        "warn",
        {
          patterns: [
            {
              group: ["fs", "node:fs"],
              message:
                "Use a `Filesystem` implementation like `nodeFs` instead of Node's 'fs' package directly.",
            },
            {
              group: ["fs/promises", "node:fs/promises"],
              message:
                "Use a `Filesystem` implementation like `nodeFs` instead of Node's 'fs/promises' package directly. Additionally, use synchronous filesystem IO within our CLI.",
            },
          ],
          paths: [
            {
              name: "chalk",
              importNames: ["default"],
              message:
                "Import { chalkStderr } from 'chalk' instead of the default chalk import. Use chalkStderr for stderr output (most cases). Only use default chalk with an eslint-disable comment if formatting text for stdout (e.g., logOutput() or console.log).",
            },
          ],
        },
      ],
      "no-restricted-syntax": [
        "error",
        {
          selector:
            ":matches(PropertyDefinition, MethodDefinition) > PrivateIdentifier.key",
          message: "Use `private` instead",
        },
        {
          selector: "ThrowStatement",
          message:
            "Don't use `throw` if this is a developer-facing error message and this code could be called by `npx convex dev`. Instead use `ctx.crash`.",
        },
        // TODO: fix to allow process.exit(0) but not process.exit(1)
        // {
        //   message: "Use flushAndExit from convex/src/cli/utils.ts instead of process.exit so that Sentry gets flushed.",
        //   selector: "CallExpression[callee.object.name='process'][callee.property.name='exit'][callee.value=1]"
        // }
      ],
      "no-throw-literal": ["error"],
    },
  },
  eslintConfigPrettier,
];
