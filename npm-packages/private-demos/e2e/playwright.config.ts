import { defineConfig, devices } from "@playwright/test";

const projectMap: Record<string, string> = {
  "tanstack-start-clerk": "../tanstack-start-clerk",
  "tanstack-start-workos": "../tanstack-start-workos",
  "tanstack-start": "../tanstack-start",
  "tanstack-start-quickstart": "../quickstarts/tanstack-start",
};

const project = process.env.PROJECT || "tanstack-start-clerk";
const projectDir = projectMap[project];

if (!projectDir) {
  throw new Error(
    `Unknown project: ${project}. Valid options: ${Object.keys(projectMap).join(", ")}`,
  );
}

const port = 5176;

export default defineConfig({
  testDir: `./tests/${project}`,
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: 1,
  reporter: "html",
  timeout: 60_000,
  use: {
    baseURL: `http://localhost:${port}`,
    trace: "on-first-retry",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: {
    command: `cd ${projectDir} && npx vite dev --port ${port}`,
    port,
    reuseExistingServer: false,
    timeout: 120_000,
    env: {
      VITE_CONVEX_URL: process.env.CONVEX_URL || "http://127.0.0.1:3210",
    },
  },
});
