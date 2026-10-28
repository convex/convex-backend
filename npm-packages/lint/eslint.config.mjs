import { defineConfig, globalIgnores } from "eslint/config";

import tsParser from "@typescript-eslint/parser";
import typescriptEslint from "@typescript-eslint/eslint-plugin";
import reactHooks from "eslint-plugin-react-hooks";
import react from "eslint-plugin-react";
import jsxA11y from "eslint-plugin-jsx-a11y";
import nextPlugin from "@next/eslint-plugin-next";
import jest from "eslint-plugin-jest";
import importPlugin from "eslint-plugin-import";
import eslintPlugin from "eslint-plugin-eslint-plugin";
import storybook from "eslint-plugin-storybook";
import betterTailwindcss from "eslint-plugin-better-tailwindcss";
import boundaries from "eslint-plugin-boundaries";
import convexPlugin from "@convex-dev/eslint-plugin";

import { fixupPluginRules } from "@eslint/compat";

import globals from "globals";
import js from "@eslint/js";

import { FlatCompat } from "@eslint/eslintrc";

import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const compat = new FlatCompat({
  baseDirectory: __dirname,
  recommendedConfig: js.configs.recommended,
  allConfig: js.configs.all,
});

const npmPackagesDir = path.join(__dirname, "..");

// The dashboard and design-system packages, which share a common set of
// lint rules (see the dedicated config blocks below).
const dashboardPackages = [
  "dashboard",
  "dashboard-self-hosted",
  "dashboard-common",
  "dashboard-storybook",
  "@convex-dev/design-system",
];
const dashboardFiles = dashboardPackages.map(
  (pkg) => `${pkg}/**/*.{js,jsx,ts,tsx}`,
);
// Storybook story and config files within those packages.
const storybookFiles = dashboardPackages.flatMap((pkg) => [
  `${pkg}/**/*.stories.{js,jsx,ts,tsx}`,
  `${pkg}/.storybook/**/*.{js,jsx,ts,tsx}`,
]);
// Config and script files within those packages. These live outside the
// TypeScript `project` (tsconfig only covers `src`) so they can't be
// type-checked, and they conventionally use default exports.
const dashboardConfigFiles = dashboardPackages.flatMap((pkg) => [
  `${pkg}/.storybook/**/*.{js,jsx,ts,tsx}`,
  `${pkg}/scripts/**/*.{js,jsx,ts,tsx}`,
  `${pkg}/*.config.{js,cjs,mjs,ts,cts,mts}`,
]);
const dashboardRestrictedImportOptions = {
  paths: ["lodash"],
  patterns: [
    {
      group: ["react-day-picker"],
      importNames: ["Button"],
      message: "You probably mean to import from @ui/Button.",
    },
  ],
};

/** @type {import("eslint/config").Config[]} */
const config = [
  {
    languageOptions: {
      parser: tsParser,

      globals: {
        ...globals.amd,
        ...globals.browser,
        ...globals.jest,
        ...globals.node,
      },
    },

    plugins: {
      "@typescript-eslint": typescriptEslint,
      "react-hooks": fixupPluginRules(reactHooks),
      react,
      jest,
    },

    extends: compat.extends(
      "eslint:recommended",
      "plugin:@typescript-eslint/recommended",
      "prettier",
      "plugin:jest/recommended",
    ),

    rules: {
      // any is terrible but we use it a lot (even in our public code).
      "@typescript-eslint/no-explicit-any": "off",

      // asserting that values aren't null is risky but useful.
      "@typescript-eslint/no-non-null-assertion": "off",

      // Add React hooks rules so we don't misuse them.
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "warn",

      eqeqeq: ["error", "always"],

      // allow (_arg: number) => {}
      "@typescript-eslint/no-unused-vars": [
        "error",
        {
          argsIgnorePattern: "^_",
          varsIgnorePattern: "^_",
        },
      ],

      "jest/expect-expect": "off",
      "jest/no-conditional-expect": "off",
    },
  },

  // Disable `console` except in packages where we need it
  {
    rules: {
      "no-console": "error",
    },
    basePath: path.join(__dirname, "..", ".."),
    ignores: [
      "**/dont-publish-alpha-as-latest.mjs",
      "**/version-check.mjs",
      "npm-packages-private/auth-emails/**",
      "npm-packages-private/data/**",
      "npm-packages-private/grafana-mcp/**",
      "npm-packages-private/postalservice/**",
      "npm-packages/@convex-dev/codemod/**",
      "npm-packages/tests/component-tests/**",
      "npm-packages/convex-analytics/**",
      "npm-packages/convex-chat-speculative/**",
      "npm-packages/demos/**",
      "npm-packages/docs/**",
      "npm-packages/tests/js-integration-tests/**",
      "npm-packages/local-store/**",
      "npm-packages/node-executor/**",
      "npm-packages/private-demos/**",
      "npm-packages/tests/publishing-tests/**",
      "npm-packages/tests/retention-tester/**",
      "npm-packages/scenario-runner/**",
      "npm-packages/shared-cursors/**",
      "npm-packages/tests/simulation/**",
      "npm-packages/text-importer/**",
      "npm-packages/udf-runtime/**",
      "npm-packages/tests/udf-tests/**",
      "npm-packages/version/**",
      "npm-packages/wasm-runtime-bundler/scripts/**",
      "npm-packages/demo_browser_tests/**",
      "npm-packages/components/ratelimiter/node10stubs.mjs",

      // FIXME: make this stricter
      "npm-packages/dashboard/**",
      "npm-packages/dashboard-self-hosted/**",
      "npm-packages/dashboard-common/**",
      "npm-packages/dashboard-storybook/**",
    ],
  },

  // Allow CJS imports in `.js` and `.cjs` files
  {
    files: ["**/*.js", "**/*.cjs"],
    rules: {
      "@typescript-eslint/no-require-imports": "off",
    },
  },

  // Set-up typescript-eslint rules that need a project configuration
  ...[
    "@convex-dev/eslint-plugin",
    "@convex-dev/ai-sdk-provider",
    "@convex-dev/bundle-analyzer",
    "tests/js-integration-tests",
    "udf-runtime",
    "scenario-runner/convex",
    "system-udfs",
    "tests/udf-tests/convex",
    "components/ratelimiter",
    "dashboard",
    "dashboard-self-hosted",
    "dashboard-common",
    "dashboard-storybook",
    "@convex-dev/design-system",
    // FIXME: Ideally we’d add many more packages here
  ].map((pkg) => ({
    files: [path.join(pkg, "**/*.ts"), path.join(pkg, "**/*.tsx")],
    // Config and script files aren't part of the TypeScript project, so they
    // can't be parsed with `parserOptions.project`.
    ignores: dashboardConfigFiles,
    rules: {
      "@typescript-eslint/no-floating-promises": "error",
    },
    languageOptions: {
      parserOptions: {
        project: path.join(__dirname, "..", pkg, "tsconfig.json"),
      },
    },
  })),

  // eslint-plugin-import rules.
  //
  // For now these only apply to the dashboard and design-system packages.
  {
    files: dashboardFiles,
    plugins: {
      import: importPlugin,
    },
    rules: {
      // Cross-package relative imports (e.g.
      // `../../../@convex-dev/design-system/...`) should go through the package
      // name instead. A few intentional exceptions are silenced with inline
      // `eslint-disable` comments.
      "import/no-relative-packages": "error",
    },
  },
  {
    files: dashboardFiles,
    ignores: [
      // Next.js pages, Storybook stories, and config/script files must use
      // default exports.
      "**/pages/**",
      "**/*.stories.{js,jsx,ts,tsx}",
      ...dashboardConfigFiles,
    ],
    rules: {
      // We prefer named exports over default exports because a default export
      // with a different name from the import site can be confusing.
      "import/no-default-export": "error",
    },
  },

  // eslint-plugin-boundaries: enforce the dashboard package's internal
  // architecture (which folders may import from which).
  {
    files: ["dashboard/**/*.{js,jsx,ts,tsx}"],
    plugins: { boundaries },
    settings: {
      "import/resolver": {
        typescript: {
          alwaysTryTypes: true,
          project: path.join(__dirname, "..", "dashboard", "tsconfig.json"),
        },
      },
      // FIXME: uncomment to enforce `pages` boundaries (the `mode: "full"`
      // pattern only matches with this set). It surfaces pre-existing
      // violations that need triaging first.
      // "boundaries/root-path": path.join(__dirname, "..", "dashboard", "src"),
      // FIXME: `<folder>/*` only matches nested files, so folders made of flat
      // files (`hooks`, `elements`, `lib`, `api`, `layouts`, and flat
      // `providers`) resolve to "unknown" and aren't enforced. Changing these
      // to `<folder>` would type them, but surfaces pre-existing violations to
      // triage first. Today only `components` (and nested `docs`) are enforced.
      "boundaries/elements": [
        {
          type: "docs",
          pattern: "docs/*",
        },
        {
          type: "hooks",
          pattern: "hooks/*",
        },
        {
          type: "elements",
          pattern: "elements/*",
        },
        {
          type: "lib",
          pattern: "lib/*",
        },
        {
          type: "components",
          pattern: "components/*",
          capture: ["feature"],
        },
        {
          type: "providers",
          pattern: "providers/*",
        },
        {
          type: "api",
          pattern: "api/*",
        },
        {
          type: "pages",
          pattern: "pages/**",
          mode: "full",
        },
        {
          type: "layouts",
          pattern: "layouts/*",
        },
      ],
    },
    rules: {
      "boundaries/element-types": [
        2,
        {
          default: "disallow",
          rules: [
            {
              from: "hooks",
              allow: ["hooks"],
            },
            {
              from: "providers",
              allow: ["providers"],
            },
            {
              from: "api",
              allow: ["api"],
            },
            {
              from: "elements",
              allow: ["elements"],
            },
            {
              from: "lib",
              allow: ["lib"],
            },
            {
              from: "pages",
              allow: ["components", "hooks", "lib", "elements", "layouts"],
            },
            {
              from: "layouts",
              allow: ["elements"],
            },
            {
              from: "components",
              allow: [
                ["components", { family: "${from.family}" }],
                "hooks",
                "lib",
                "elements",
              ],
            },
            {
              from: "docs",
              allow: ["pages", "lib", "components"],
            },
          ],
        },
      ],
    },
  },

  // React, accessibility, and Next.js rules for the dashboard + design-system.
  {
    files: dashboardFiles,
    ...react.configs.flat.recommended,
    // Detect the installed React version (silences eslint-plugin-react's
    // "React version not specified" warning).
    settings: { react: { version: "detect" } },
  },
  { files: dashboardFiles, ...jsxA11y.flatConfigs.recommended },
  {
    files: dashboardFiles,
    rules: {
      "react/react-in-jsx-scope": "off",
      "react/prop-types": "off",

      "react/no-unescaped-entities": "off",
      "jsx-a11y/no-autofocus": "off",
      "jsx-a11y/anchor-is-valid": "off",

      "jsx-a11y/label-has-associated-control": [
        "error",
        {
          assert: "either",
          controlComponents: ["Checkbox"],
        },
      ],
      "react/forbid-elements": [
        1,
        {
          forbid: [
            {
              element: "button",
              message:
                "use @ui/Button instead. If you really need a custom button, disable this rule and leave a comment explaining why.",
            },
            {
              element: "details",
              message: "use Disclosure from headlessui instead.",
            },
            {
              element: "summary",
              message: "use Disclosure from headlessui instead.",
            },
          ],
        },
      ],
      // https://stackoverflow.com/a/73967427/1526986
      "react/jsx-no-useless-fragment": ["error", { allowExpressions: true }],
    },
  },

  {
    files: dashboardFiles,
    rules: {
      // FIXME We can probably make this stricter
      "@typescript-eslint/no-empty-object-type": "off",
      "@typescript-eslint/no-unused-expressions": "off",
      "@typescript-eslint/no-non-null-asserted-optional-chain": "off",

      // We want to allow named `function`s used as arguments to HoCs, see
      // https://react.dev/reference/react/memo#reference as an example.
      "prefer-arrow-callback": ["error", { allowNamedFunctions: true }],

      "no-restricted-imports": [2, dashboardRestrictedImportOptions],

      // http://eslint.org/docs/rules/no-restricted-syntax
      "no-restricted-syntax": [
        "error",
        "ForInStatement",
        // "ForOfStatement",  // for-of is fine
        "LabeledStatement",
        "WithStatement",
        {
          message: "useEffectDebugger calls should not be merged in to main.",
          selector: "CallExpression[callee.name='useEffectDebugger']",
        },
        {
          message:
            "Please call `captureMessage` with an explicit severity level (e.g., 'error', 'warning', 'info').",
          selector:
            "CallExpression[callee.name='captureMessage'][arguments.length=1]",
        },
        {
          message:
            "Please call `Sentry.captureMessage` with an explicit severity level (e.g., 'error', 'warning', 'info').",
          selector:
            "CallExpression[callee.type='MemberExpression'][callee.property.name='captureMessage'][arguments.length=1]",
        },
        {
          message:
            "You probably want to use the themed error colors instead  (e.g. text-content-error). If you really want red, disable this lint rule for this line",
          selector: "Literal[value=/^.*-red-.*$/i]",
        },
        {
          message:
            "You probably want to use a header tag. If you really want this text size, disable this lint rule for this line",
          selector: "Literal[value=/^.*text-([1-4]?xl|lg).*$/i]",
        },
        {
          message:
            "You don't need to specify light and dark colors separately anymore. Use the themed colors instead (e.g. text-content-primary).",
          selector: "Literal[value=/^.*-light-.*$/i]",
        },
        {
          message:
            "You don't need to specify light and dark colors separately anymore. Use the themed colors instead (e.g. text-content-primary).",
          selector: "Literal[value=/^.*-dark-.*$/i]",
        },
        {
          message: "Don't use content text colors for backgrounds.",
          selector: "Literal[value=/^bg-content-.*$/i]",
        },
        {
          message: "Don't use background colors for text",
          selector: "Literal[value=/^text-background-.*$/i]",
        },
        {
          message:
            ".bottom-4 is blocked on convex.dev by easylist_cookie; use .bottom-four instead",
          selector: "Literal[value=/bottom-4(?:\\D|$)/i]",
        },
        {
          message:
            "Use the Link component from @ui/Link instead of manually adding the text-content-link class.",
          selector: "Literal[value=/text-content-link/]",
        },
      ],
    },
  },

  // The dashboard package additionally forbids importing the shared
  // NoPermissionMessage element directly (it has its own wrapper).
  {
    files: ["dashboard/**/*.{js,jsx,ts,tsx}"],
    rules: {
      "no-restricted-imports": [
        2,
        {
          ...dashboardRestrictedImportOptions,
          paths: [
            ...dashboardRestrictedImportOptions.paths,
            {
              name: "@common/elements/NoPermissionMessage",
              message:
                "Use 'elements/NoPermissionMessage' from the dashboard package instead.",
            },
          ],
        },
      ],
    },
  },

  {
    files: dashboardFiles,
    plugins: { "@next/next": nextPlugin },
    rules: {
      ...nextPlugin.configs.recommended.rules,
      ...nextPlugin.configs["core-web-vitals"].rules,
    },
    settings: {
      next: {
        rootDir: [
          path.join(npmPackagesDir, "dashboard"),
          path.join(npmPackagesDir, "dashboard-self-hosted"),
        ],
      },
    },
  },

  // better-tailwindcss rules for the dashboard and design-system packages.
  {
    files: dashboardFiles,
    plugins: betterTailwindcss.configs["recommended-error"].plugins,
    settings: {
      "better-tailwindcss": {
        // Allows eslint-plugin-better-tailwindcss to correctly detect the Tailwind version we use
        cwd: path.join(__dirname, "..", "@convex-dev", "design-system"),
        entryPoint: "src/styles/shared.css",
      },
    },
    rules: {
      ...betterTailwindcss.configs["recommended-error"].rules,
      // This would cause most of the existing code to be reformatted, so I don’t think it’s worth it
      "better-tailwindcss/enforce-consistent-line-wrapping": "off",
      "better-tailwindcss/no-unknown-classes": [
        "error",
        {
          ignore: [
            // For some reason the ESLint plugin doesn’t recognize classes
            // defined in CSS files, so let’s ignore them manually for now.
            "animate-fadeInToVar",
            "animate-gift.+",
            "bg-stripes",
            "bottom-four",
            "commandPaletteEditor",
            "DataRow",
            "disabled",
            "focused",
            "gift-paper",
            "gift-spark",
            "hover-decoration",

            // Classes not used for styling but only for referencing from JS code
            "js-.+",

            // Monaco classes
            "codicon-.+",
            "mtk.+",
          ],
        },
      ],
    },
  },

  {
    files: ["**/convex/**/*.{js,ts}", "components/**/*.ts"],
    ignores: [
      // Some tests rely on using the old Convex function syntax, so we disable
      // the linter on test files
      "tests/js-integration-tests/**",
      "tests/udf-tests/**",

      // TODO(nicolas) Lint Postalservice too
      "postalservice/**",
    ],
    plugins: {
      "@convex-dev": convexPlugin,
    },
    rules: convexPlugin.configs.recommended[0].rules,
  },

  // @convex-dev/eslint-plugin: lint with eslint-plugin-eslint-plugin
  {
    files: ["@convex-dev/eslint-plugin/src/**/*.ts"],
    ...eslintPlugin.configs.recommended,
  },

  // Storybook recommended rules, scoped to the story and Storybook config
  // files in the dashboard and design-system packages.
  ...storybook.configs["flat/recommended"].map((storybookConfig) => ({
    ...storybookConfig,
    // Each sub-config scopes itself to the right files (story files vs.
    // `.storybook/main`); keep that distinction but restrict it to the
    // dashboard and design-system packages. The setup config has no `files`,
    // so scope it to all Storybook files in those packages.
    files: storybookConfig.files
      ? dashboardPackages.flatMap((pkg) =>
          storybookConfig.files.map((pattern) => `${pkg}/${pattern}`),
        )
      : storybookFiles,
  })),
  {
    files: storybookFiles,
    rules: {
      // `no-uninstalled-addons` checks that the Storybook addons are installed
      // by reading a package.json. By default it looks in the working
      // directory, which is the monorepo root (no package.json), so point it at
      // the Storybook package instead.
      "storybook/no-uninstalled-addons": [
        "error",
        {
          packageJsonLocation: path.join(
            __dirname,
            "..",
            "dashboard-storybook",
            "package.json",
          ),
        },
      ],
      // Storybook decorators are anonymous render functions, which this rule
      // (from eslint-plugin-react's recommended config) flags as missing a
      // display name.
      "react/display-name": "off",
    },
  },

  // system-udfs
  {
    files: ["system-udfs/**/*.ts"],
    rules: {
      // TODO(nicolas): use the new `ctx.db` APIs in system-udfs
      "@convex-dev/explicit-table-ids": "off",

      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["**/_generated/server"],

              importNames: [
                "query",
                "mutation",
                "action",
                "internalQuery",
                "internalMutation",
                "internalAction",
              ],

              message:
                "Use the query wrappers from convex/_system/server.ts instead for system UDF argument validation.",
            },
            {
              group: ["convex/server"],

              importNames: [
                "queryGeneric",
                "mutationGeneric",
                "actionGeneric",
                "internalQueryGeneric",
                "internalMutationGeneric",
                "internalActionGeneric",
              ],

              message:
                "Use the query wrappers from convex/_system/server.ts instead for system UDF argument validation.",
            },
          ],
        },
      ],
    },
  },

  globalIgnores([
    "**/.next/**",
    "**/.nuxt/**",
    "**/.output/**",
    "**/storybook-static/**",
    "dashboard-self-hosted/out", // Next.js static export output
    "**/node_modules",
    "**/dist",
    "**/.next",
    "**/.nuxt",
    "**/build",
    "common/temp", // leftover Rush workspace in pre-migration checkouts
    "tests/udf-tests/convex/wpt/generated", // Files imported directly form Web Platform Tests (generated by `just update-wpt`)
    "convex", // has a similar config, separate so vscode can find it
    "@convex-dev/react-query", // separate because OSS
    "docs/.docusaurus", // auto-generated by Docusaurus
    "demos/nextjs-pages-router", // has its own config using different dependencies
    "demos/nextjs-app-router", // has its own config using different dependencies
    "private-demos/react-native", // has its own config using different dependencies
    "private-demos/actions", // has its own ESLint
    "private-demos/npm-showcase", // has its own ESLint and `"@typescript-eslint"`
    "private-demos/quickstarts/nodejs/script.js", // uses require
    "private-demos/quickstarts/nextjs-app-dir", // should match Next.js quickstart
    "private-demos/quickstarts/nextjs-app-dir-14", // should match Next.js quickstart
    "private-demos/nextjs-app-router-snippets", // should match Next.js quickstart
    "private-demos/nextjs-15-app", // should match Next.js quickstart
    "private-demos/nextjs-15-app-clerk", // should match Next.js quickstart
    "private-demos/quickstarts/sveltekit", // sveltekit linting is annoying to set up
    "private-demos/quickstarts/remix", // won't have these deps installed
    "private-demos/quickstarts/vue", // won't have these deps installed
    "private-demos/snippets", // has its own config
    "private-demos/tanstack-start", // has its own config
    "private-demos/tanstack-start-clerk", // has its own config
    "private-demos/tutorial/src/App.tsx", // tutorial warning will go away once user does tutorial
    "demos/html/script.js", // uses js-doc
    "demos/html/browser.bundle.js", // just until we can link to a CDN for this
    "create-convex/template-*",
    "@convex-dev/codemod/test/explicit-ids/**/*.before.{ts,js}", // codemod fixtures with intentionally flaggable patterns
    "@convex-dev/eslint-plugin/src/test/fixtures/**", // rule fixtures with intentionally flaggable patterns
    "@convex-dev/eslint-plugin/oxlint-demo/**", // linted by oxlint; intentionally flaggable patterns suppressed with oxlint-disable comments
    "**/_generated/**", // auto-generated files
    "**/next-env.d.ts", // auto-generated by Next.js
  ]),
];

/**
 * @param {import("eslint/config").Config} configObject
 * @returns {import("eslint/config").Config}
 */
const pinToNpmPackages = (configObject) => {
  const files = configObject.files ?? [];
  if (
    "basePath" in configObject ||
    files.length === 0 ||
    files.some(
      (pattern) => typeof pattern === "string" && pattern.startsWith("**/"),
    )
  ) {
    return configObject;
  }
  return { ...configObject, basePath: npmPackagesDir };
};

export default defineConfig(config.map(pinToNpmPackages));
