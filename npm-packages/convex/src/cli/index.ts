import { chalkStderr } from "chalk";
import * as Sentry from "@sentry/node";
import { initSentry } from "./lib/utils/sentry.js";
import { format } from "util";
import dns from "node:dns";
import net from "node:net";
import { setGlobalDispatcher, EnvHttpProxyAgent } from "undici";
import { logVerbose } from "../bundler/log.js";
import { buildProgram } from "./program.js";

const HARD_MINIMUM_NODE_MAJOR_VERSION = 20;
const HARD_MINIMUM_NODE_MINOR_VERSION = 0;

// console.error before it started being red by default in Node.js v20
function logToStderr(...args: unknown[]) {
  process.stderr.write(`${format(...args)}\n`);
}

async function main() {
  const nodeVersion = process.versions.node;
  const majorVersion = parseInt(nodeVersion.split(".")[0], 10);
  const minorVersion = parseInt(nodeVersion.split(".")[1], 10);

  const proxy = process.env.HTTPS_PROXY || process.env.HTTP_PROXY;
  if (proxy) {
    setGlobalDispatcher(new EnvHttpProxyAgent());
    logVerbose(`[proxy-bootstrap] Using proxy: ${proxy}`);
  }

  // Use ipv4 first for 127.0.0.1 in tests
  dns.setDefaultResultOrder("ipv4first");

  // Increase the timeout from default 250ms for high latency situations,
  // see https://github.com/nodejs/node/issues/54359.
  if (majorVersion >= 20) {
    // While we use Node.js v18 types
    (net as any).setDefaultAutoSelectFamilyAttemptTimeout?.(1000);
  }

  initSentry();

  if (
    majorVersion < HARD_MINIMUM_NODE_MAJOR_VERSION ||
    (majorVersion === HARD_MINIMUM_NODE_MAJOR_VERSION &&
      minorVersion < HARD_MINIMUM_NODE_MINOR_VERSION)
  ) {
    logToStderr(
      chalkStderr.red(
        `Your Node version ${nodeVersion} is too old. Convex requires at least Node v${HARD_MINIMUM_NODE_MAJOR_VERSION}.${HARD_MINIMUM_NODE_MINOR_VERSION}`,
      ),
    );
    logToStderr(
      chalkStderr.gray(
        `You can use ${chalkStderr.bold(
          "nvm",
        )} (https://github.com/nvm-sh/nvm#installing-and-updating) to manage different versions of Node.`,
      ),
    );
    logToStderr(
      chalkStderr.gray(
        "After installing `nvm`, install the latest version of Node with " +
          chalkStderr.bold("`nvm install node`."),
      ),
    );
    logToStderr(
      chalkStderr.gray(
        "Then, activate the installed version in your terminal with " +
          chalkStderr.bold("`nvm use`."),
      ),
    );
    process.exit(1);
  }

  const program = buildProgram();

  // Run the command and be sure to flush Sentry before exiting.
  try {
    await program.parseAsync(process.argv);
  } catch (e) {
    Sentry.captureException(e);
    process.exitCode = 1;
    // This is too early to use `logError`, so just log directly.
    // eslint-disable-next-line no-console
    console.error(chalkStderr.red("Unexpected Error: " + e));
  } finally {
    await Sentry.close();
  }
  // When stdout is a pipe, Node buffers writes and `process.exit()` does not
  // wait for them to flush — drain first so piped output isn't truncated.
  await Promise.all([
    new Promise<void>((resolve) => process.stdout.write("", () => resolve())),
    new Promise<void>((resolve) => process.stderr.write("", () => resolve())),
  ]);
  process.exit();
}
void main();
