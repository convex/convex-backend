import { defineConfig } from "vitest/config";
import { storybookTest } from "@storybook/addon-vitest/vitest-plugin";
import { playwright } from "@vitest/browser-playwright";
import path from "node:path";
import { fileURLToPath } from "node:url";

const dirname = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  resolve: {
    alias: [
      {
        find: /^@ui\/(.*)/,
        replacement:
          path.resolve(dirname, "../@convex-dev/design-system/src") + "/$1",
      },
      {
        find: /^@common\/(.*)/,
        replacement: path.resolve(dirname, "../dashboard-common/src") + "/$1",
      },
      {
        find: /^api\/(.*)/,
        replacement: path.resolve(dirname, "../dashboard/src/api") + "/$1",
      },
      {
        find: /^hooks\/(.*)/,
        replacement: path.resolve(dirname, "../dashboard/src/hooks") + "/$1",
      },
      {
        find: /^react($|\/.*)/,
        replacement: path.resolve(dirname, "node_modules/react") + "$1",
      },
      {
        find: /^react-dom($|\/.*)/,
        replacement: path.resolve(dirname, "node_modules/react-dom") + "$1",
      },
    ],
  },
  test: {
    projects: [
      {
        extends: true,
        plugins: [
          storybookTest({
            configDir: path.join(dirname, ".storybook"),
            storybookScript: "npm run storybook -- --no-open",
          }),
        ],
        test: {
          name: "storybook",
          browser: {
            enabled: true,
            provider: playwright(),
            headless: true,
            instances: [{ browser: "chromium" }],
          },
          setupFiles: ["./.storybook/vitest.setup.ts"],
        },
      },
    ],
  },
});
