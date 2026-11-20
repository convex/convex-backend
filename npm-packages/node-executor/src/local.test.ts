import { afterEach, expect, test } from "vitest";
import { build } from "esbuild";
import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, rm, stat } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";

const POLL_INTERVAL_MS = 50;
const POLL_TIMEOUT_MS = 5_000;

async function waitFor(
  predicate: () => Promise<boolean>,
  timeoutMessage: () => string,
) {
  const deadline = Date.now() + POLL_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (await predicate()) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS));
  }
  throw new Error(timeoutMessage());
}

async function pathExists(filePath: string) {
  try {
    await stat(filePath);
    return true;
  } catch {
    return false;
  }
}

function processExists(pid: number) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

function processStatus(pid: number) {
  const result = spawnSync(
    "ps",
    ["-o", "pid=,ppid=,state=,command=", "-p", String(pid)],
    { encoding: "utf8" },
  );
  if (result.status === 0) {
    return result.stdout.trim();
  }
  return `not listed by ps (exit ${result.status}): ${result.stderr.trim()}`;
}

let childPid: number | undefined;

afterEach(() => {
  if (childPid !== undefined && processExists(childPid)) {
    process.kill(childPid, "SIGKILL");
  }
  childPid = undefined;
});

test.skipIf(process.platform === "win32")(
  "closes its listener and exits when its supervisor is killed",
  async () => {
    const tempdir = await mkdtemp(path.join(os.tmpdir(), "node-executor-test-"));
    const bundlePath = path.join(tempdir, "local.cjs");
    const socketPath = path.join(tempdir, "executor.sock");
    let supervisor: ChildProcess | undefined;
    let connection: net.Socket | undefined;
    let childOutput = "";
    try {
      await build({
        bundle: true,
        entryPoints: [path.join(process.cwd(), "src", "local.ts")],
        outfile: bundlePath,
        platform: "node",
        target: "esnext",
      });
      const supervisorScript = `
      const { spawn } = require("node:child_process");
      const child = spawn(process.execPath, [
        process.argv[1],
        "--ipc-path", process.argv[2],
        "--tempdir", process.argv[3],
        "--parent-pid", String(process.pid),
      ], { stdio: ["ignore", "pipe", "pipe"] });
      child.stdout.pipe(process.stderr);
      child.stderr.pipe(process.stderr);
      child.once("exit", (code, signal) => {
        process.stderr.write(
          "node executor exited with code " + code + " and signal " + signal + "\\n",
        );
      });
      process.stdout.write(String(child.pid));
      setInterval(() => {}, 1_000);
    `;
      supervisor = spawn(
        process.execPath,
        ["-e", supervisorScript, bundlePath, socketPath, tempdir],
        { stdio: ["ignore", "pipe", "pipe"] },
      );
      supervisor.stderr!.on("data", (data) => {
        childOutput += data.toString();
      });
      const output = await new Promise<string>((resolve, reject) => {
        supervisor!.stdout!.once("data", (data) => resolve(data.toString()));
        supervisor!.once("error", reject);
      });
      childPid = Number.parseInt(output, 10);
      expect(childPid).toBeGreaterThan(0);
      await waitFor(
        async () => pathExists(socketPath),
        () =>
          `Timed out waiting for socket ${socketPath} to be created; child process status: ${processStatus(childPid!)}, child output: ${childOutput || "(none)"}`,
      );
      connection = net.createConnection(socketPath);
      await once(connection, "connect");

      supervisor.kill("SIGKILL");
      await new Promise<void>((resolve) => supervisor!.once("exit", () => resolve()));

      await waitFor(
        async () => !(await pathExists(socketPath)),
        () =>
          `Timed out waiting for socket ${socketPath} to be removed; child process status: ${processStatus(childPid!)}, child output: ${childOutput || "(none)"}`,
      );
      await waitFor(
        async () => !processExists(childPid!),
        () =>
          `Timed out waiting for child process ${childPid} to exit; socket was removed, child process status: ${processStatus(childPid!)}, child output: ${childOutput || "(none)"}`,
      );
    } finally {
      connection?.destroy();
      supervisor?.kill("SIGKILL");
      await rm(tempdir, { force: true, recursive: true });
    }
  },
);
