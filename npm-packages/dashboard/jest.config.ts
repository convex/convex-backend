import nextJest from "next/jest.js";

const createJestConfig = nextJest({
  dir: "./",
});

const customJestConfig = {
  moduleDirectories: ["node_modules", "src"],
  testEnvironment: "jest-environment-jsdom",
  setupFilesAfterEnv: ["<rootDir>/setupTests.ts"],
  moduleNameMapper: {
    "^dashboard-common/(.*)$": "<rootDir>/../dashboard-common/src/$1",
    "^@common/(.*)$": "<rootDir>/../dashboard-common/src/$1",
    "^@ui/(.*)$": "<rootDir>/../@convex-dev/design-system/src/$1",
    "lodash-es": "<rootDir>/../dashboard/node_modules/lodash",
  },
  roots: [
    "<rootDir>",
    "<rootDir>/../dashboard-common",
    "<rootDir>/../@convex-dev/design-system",
  ],
};

const config = createJestConfig(customJestConfig);

// `next/jest` leaves everything under `node_modules` untranspiled, but
// `@pierre/trees` (behind the virtualized functions sidebar) ships only ES
// modules.
const ES_MODULE_PACKAGES = ["@pierre", "preact", "preact-render-to-string"];

export default async () => {
  const resolved = await config();
  return {
    ...resolved,
    resolver: "<rootDir>/jest.resolver.cjs",
    transformIgnorePatterns: [
      `/node_modules/(?!\\.pnpm)(?!(${ES_MODULE_PACKAGES.join("|")})/)`,
      `/node_modules/\\.pnpm/(?!(${ES_MODULE_PACKAGES.map((p) => p.replace("/", "\\+")).join("|")})[+@])`,
      // CSS modules, which `next/jest` maps to proxies instead.
      ...(resolved.transformIgnorePatterns ?? []).filter(
        (pattern) => !pattern.includes("node_modules"),
      ),
    ],
  };
};
