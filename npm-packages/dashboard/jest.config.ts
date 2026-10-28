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

export default config;
