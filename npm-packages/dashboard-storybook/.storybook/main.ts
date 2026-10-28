import { StorybookConfig } from "@storybook/nextjs-vite";
import { createRequire } from "module";
import path from "path";
import { UserConfig, mergeConfig } from "vite";

const require = createRequire(import.meta.url);

const config: StorybookConfig = {
  // Every package listed here must also be a dependency in package.json:
  // Vercel decides whether to redeploy Storybook by walking the workspace
  // dependency graph, and skips changes it can't see from this package.
  stories: [
    "../../dashboard/src/**/*.stories.@(js|jsx|ts|tsx)",
    "../../dashboard-common/src/**/*.stories.@(js|jsx|ts|tsx)",
    "../../dashboard-self-hosted/src/**/*.stories.@(js|jsx|ts|tsx)",
    "../../@convex-dev/design-system/src/**/*.stories.@(js|jsx|ts|tsx)",
  ],
  addons: [
    "@storybook/addon-links",
    "@storybook/addon-themes",
    "@storybook/addon-docs",
    "@storybook/addon-a11y",
    "@storybook/addon-vitest",
  ],
  staticDirs: [
    // Use the Storybook logo as a favicon
    {
      from: path.resolve(
        require.resolve("storybook/package.json"),
        "../assets/browser/favicon.svg",
      ),
      to: "/favicon.svg",
    },
    path.resolve(import.meta.dirname, "../../dashboard/public"),
    {
      from: path.resolve(
        import.meta.dirname,
        "../../@convex-dev/design-system/node_modules/@fontsource-variable/inter/files",
      ),
      to: "/assets/files",
    },
  ],
  framework: {
    name: "@storybook/nextjs-vite",
    options: {
      nextConfigPath: path.resolve(
        import.meta.dirname,
        "../../dashboard/next.config.js",
      ),
    },
  },
  viteFinal: async (config) => {
    return mergeConfig(config, {
      css: {
        postcss: path.resolve(
          import.meta.dirname,
          "../../dashboard/postcss.config.js",
        ),
      },
      resolve: {
        alias: {
          // Match dashboard's tsconfig baseUrl: "src" for api/* imports
          api: path.resolve(import.meta.dirname, "../../dashboard/src/api"),
          hooks: path.resolve(import.meta.dirname, "../../dashboard/src/hooks"),
          // Match dashboard/dashboard-common tsconfig path alias "@common/*"
          "@common": path.resolve(
            import.meta.dirname,
            "../../dashboard-common/src",
          ),
          // Match design-system's tsconfig path alias "@ui/*": ["*"]
          "@ui": path.resolve(
            import.meta.dirname,
            "../../@convex-dev/design-system/src",
          ),
          // Storybook's Vite build can't bundle `saffron`'s `.wasm` dependency
          // (see vite:wasm-fallback errors). For Storybook only, swap in a
          // minimal JS mock so stories can render.
          saffron: path.resolve(import.meta.dirname, "./mocks/saffron.ts"),
        },
      },
      server: {
        fs: {
          allow: [
            ...(config.server?.fs?.allow || []),
            path.resolve(import.meta.dirname, "../../"),
          ],
        },
      },
      optimizeDeps: {
        esbuildOptions: {
          tsconfig: "../dashboard/tsconfig.json",
        },
      },
    } satisfies UserConfig);
  },
};

export default config;
