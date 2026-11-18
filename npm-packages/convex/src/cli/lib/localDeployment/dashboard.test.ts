import http from "node:http";
import net from "node:net";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { Context } from "../../../bundler/context.js";
import { handleDashboard } from "./dashboard.js";
import { saveProjectDashboardConfig } from "./filePaths.js";
import { choosePorts } from "./utils.js";

vi.mock("./download.js", () => ({
  ensureBackendBinaryDownloaded: async () => ({ version: "test-version" }),
  ensureDashboardDownloaded: async () => {},
}));
vi.mock("./filePaths.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./filePaths.js")>()),
  loadUuidForAnonymousUser: () => null,
  saveProjectDashboardConfig: vi.fn(),
}));
vi.mock("./utils.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./utils.js")>()),
  choosePorts: vi.fn(),
}));

const deployment = { name: "local-test", cloudPort: 3210, adminKey: "key" };
const servers: net.Server[] = [];
const cleanups: Array<() => Promise<void>> = [];

beforeEach(() => {
  vi.mocked(choosePorts).mockReset();
  vi.mocked(saveProjectDashboardConfig).mockReset();
});

afterEach(async () => {
  for (const server of servers.splice(0)) {
    server.close();
  }
  for (const cleanup of cleanups.splice(0)) {
    await cleanup();
  }
});

async function listenOnFreePort() {
  const server = net.createServer();
  servers.push(server);
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return { server, port: (server.address() as net.AddressInfo).port };
}

const ctx = {
  crash: async (args: { printedMessage: string | null }) => {
    throw new Error(`ctx.crash: ${args.printedMessage}`);
  },
  registerCleanup: (fn: () => Promise<void>) => {
    cleanups.push(fn);
    return "handle";
  },
  removeCleanup: () => cleanups.pop(),
} as unknown as Context;

function settleWithin<T>(promise: Promise<T>, ms = 2000) {
  return Promise.race([
    promise,
    new Promise<never>((_, reject) =>
      setTimeout(() => reject(new Error(`not settled after ${ms} ms`)), ms),
    ),
  ]);
}

test("handleDashboard retries on another port when the chosen one is taken", async () => {
  const { port: taken } = await listenOnFreePort();
  const free = await listenOnFreePort();
  free.server.close();
  vi.mocked(choosePorts)
    .mockResolvedValueOnce([taken])
    .mockResolvedValueOnce([free.port]);

  const result = await settleWithin(
    handleDashboard(ctx, deployment, { backendVersion: undefined }),
  );

  expect(result.dashboardPort).toBe(free.port);
  expect(vi.mocked(choosePorts).mock.calls[1][1].startPort).toBe(taken + 1);
  expect(saveProjectDashboardConfig).toHaveBeenCalledOnce();
  expect(saveProjectDashboardConfig).toHaveBeenCalledWith(ctx, "local-test", {
    port: free.port,
  });
  const body = await new Promise<string>((resolve, reject) => {
    http
      .get(
        {
          host: "127.0.0.1",
          port: free.port,
          path: "/api/current_deployment",
          agent: false,
        },
        (response) => {
          response.setEncoding("utf8");
          let data = "";
          response.on("data", (chunk) => (data += chunk));
          response.on("end", () => resolve(data));
        },
      )
      .on("error", reject);
  });
  expect(JSON.parse(body).name).toBe("local-test");
});

test("handleDashboard crashes once it runs out of ports to try", async () => {
  const { port: taken } = await listenOnFreePort();
  vi.mocked(choosePorts).mockResolvedValue([taken]);

  await expect(
    settleWithin(
      handleDashboard(ctx, deployment, { backendVersion: undefined }),
    ),
  ).rejects.toThrow("ctx.crash");
  expect(saveProjectDashboardConfig).not.toHaveBeenCalled();
});
