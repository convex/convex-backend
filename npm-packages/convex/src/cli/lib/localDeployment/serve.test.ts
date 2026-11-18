import http from "node:http";
import net from "node:net";
import { afterEach, expect, test } from "vitest";
import type { Context } from "../../../bundler/context.js";
import { startServer } from "./serve.js";

const heldServers: net.Server[] = [];

afterEach(() => {
  for (const server of heldServers.splice(0)) {
    server.close();
  }
});

/** Listens on a free port with another server and returns that port. */
async function holdPort() {
  const server = net.createServer();
  heldServers.push(server);
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return (server.address() as net.AddressInfo).port;
}

function makeContext() {
  const cleanups = new Map<string, () => Promise<void>>();
  const ctx = {
    registerCleanup: (fn: () => Promise<void>) => {
      const handle = `cleanup-${cleanups.size}`;
      cleanups.set(handle, fn);
      return handle;
    },
    removeCleanup: (handle: string) => {
      const fn = cleanups.get(handle);
      cleanups.delete(handle);
      return fn;
    },
  } as unknown as Context;
  return { ctx, cleanups };
}

function settleWithin<T>(promise: Promise<T>, ms = 2000) {
  return Promise.race([
    promise,
    new Promise<never>((_, reject) =>
      setTimeout(() => reject(new Error(`not settled after ${ms} ms`)), ms),
    ),
  ]);
}

test("startServer rejects when the port is already taken", async () => {
  const port = await holdPort();
  const { ctx, cleanups } = makeContext();

  await expect(
    settleWithin(startServer(ctx, port, async () => {}, {})),
  ).rejects.toMatchObject({ code: "EADDRINUSE" });
  expect(cleanups.size).toBe(0);
});

test("startServer resolves on a free port and serves requests", async () => {
  const port = await holdPort();
  heldServers.pop()!.close();
  const { ctx, cleanups } = makeContext();

  const { cleanupHandle } = await settleWithin(
    startServer(
      ctx,
      port,
      async (_request, response) => {
        response.end("ok");
      },
      {},
    ),
  );
  const body = await new Promise<string>((resolve, reject) => {
    http
      .get({ host: "127.0.0.1", port, agent: false }, (response) => {
        response.setEncoding("utf8");
        let data = "";
        response.on("data", (chunk) => (data += chunk));
        response.on("end", () => resolve(data));
      })
      .on("error", reject);
  });
  expect(body).toBe("ok");
  await cleanups.get(cleanupHandle)!();
});
