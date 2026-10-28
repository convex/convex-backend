// Run a command against a fresh local Convex backend, handling setup and teardown.
// Uses the locally-built convex-local-backend binary from this monorepo.

import http from "node:http";
import { spawn } from "node:child_process";
import { existsSync, readFileSync, rmSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "../../..");

const backendCloudPort = 3210;
const backendSitePort = 3211;
const parsedUrl = new URL(`http://127.0.0.1:${backendCloudPort}`);

// Path to the locally-built binary
const binaryPath = path.join(repoRoot, "target/debug/convex-local-backend");

// Dev instance name + secret used for the matching dev admin key.
const devInstanceName = readFileSync(
  path.join(repoRoot, "crates/keybroker/dev/instance_name.txt"),
  "utf8",
).trim();
const devInstanceSecret = readFileSync(
  path.join(repoRoot, "crates/keybroker/dev/secret.txt"),
  "utf8",
).trim();

function logToStderr(...args) {
  process.stderr.write(args.join(" ") + "\n");
}

async function isBackendRunning(backendUrl) {
  return new Promise((resolve) => {
    http
      .request(
        {
          hostname: backendUrl.hostname,
          port: backendUrl.port,
          path: "/version",
          method: "GET",
        },
        (res) => {
          resolve(res.statusCode === 200);
        },
      )
      .on("error", () => {
        resolve(false);
      })
      .end();
  });
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

let backendProcess = null;

const waitForLocalBackendRunning = async (backendUrl) => {
  let isRunning = await isBackendRunning(backendUrl);
  let i = 0;
  while (!isRunning) {
    if (i % 10 === 0) {
      logToStderr("Waiting for backend to be running...");
    }
    await sleep(500);
    isRunning = await isBackendRunning(backendUrl);
    const isDead = backendProcess.exitCode !== null;
    if (isDead) {
      throw new Error("Backend exited unexpectedly");
    }
    i += 1;
  }
};

function cleanup() {
  if (backendProcess !== null) {
    logToStderr("Cleaning up running backend");
    backendProcess.kill("SIGTERM");
    rmSync("convex_local_storage", { recursive: true, force: true });
    rmSync("convex_local_backend.sqlite3", { force: true });
  }
}

async function runWithLocalBackend(command, backendUrl) {
  const isRunning = await isBackendRunning(backendUrl);
  if (isRunning) {
    logToStderr(
      "Local backend is already running on port",
      backendCloudPort,
      ". Stop it and restart this command.",
    );
    process.exit(1);
  }

  if (!existsSync(binaryPath)) {
    logToStderr(
      `convex-local-backend binary not found at ${binaryPath}\n` +
        `Build it first with: cargo build --bin convex-local-backend`,
    );
    process.exit(1);
  }

  rmSync("convex_local_storage", { recursive: true, force: true });
  rmSync("convex_local_backend.sqlite3", { force: true });

  logToStderr(`Starting local backend from ${binaryPath}`);
  backendProcess = spawn(
    binaryPath,
    [
      "--port",
      String(backendCloudPort),
      "--site-proxy-port",
      String(backendSitePort),
      "--disable-beacon",
      "--instance-name",
      devInstanceName,
      "--instance-secret",
      devInstanceSecret,
    ],
    { env: { CONVEX_TRACE_FILE: "1" } },
  );
  backendProcess.stdout.pipe(process.stderr);
  backendProcess.stderr.pipe(process.stderr);

  await waitForLocalBackendRunning(backendUrl);
  logToStderr("Backend running!");
  logToStderr("Running command:", command);

  const code = await new Promise((resolve) => {
    const c = spawn(command, {
      shell: true,
      stdio: "pipe",
      cwd: __dirname,
      env: { ...process.env, FORCE_COLOR: "1" },
    });
    c.stdout.on("data", (data) => {
      process.stdout.write(data);
    });
    c.stderr.on("data", (data) => {
      process.stderr.write(data);
    });
    c.on("exit", (exitCode) => {
      logToStderr(`Command exited with code ${exitCode}`);
      resolve(exitCode);
    });
  });
  return code;
}

(async function main() {
  let code;
  try {
    code = await runWithLocalBackend(process.argv[2], parsedUrl);
  } finally {
    cleanup();
  }
  if (code !== undefined) {
    process.exit(code);
  }
})();
