import { chalkStderr } from "chalk";
import os from "os";
import path from "path";
import { rootDirectory } from "./utils.js";
import { Context } from "../../../bundler/context.js";
import { logError, logVerbose, logWarning } from "../../../bundler/log.js";
import { z } from "zod";

export function globalConfigPath(): string {
  return path.join(rootDirectory(), "config.json");
}

// GlobalConfig is stored in a file that very old versions of Convex also need to access.
// Everything besides accessToken must be optional forever.
// GlobalConfig is deleted on logout. It is primarily used for the accessToken.
export type GlobalConfig = {
  accessToken: string;
};

const schema = z.object({
  accessToken: z.string().min(1),
});

export function readGlobalConfig(ctx: Context): GlobalConfig | null {
  const configPath = globalConfigPath();
  let configFile;
  try {
    configFile = ctx.fs.readUtf8File(configPath);
  } catch {
    return null;
  }
  restrictConfigPermissions(ctx, configPath);
  try {
    const storedConfig = JSON.parse(configFile);
    const config: GlobalConfig = schema.parse(storedConfig);
    return config;
  } catch (err) {
    // Print an error and act as if the file does not exist.
    logError(
      chalkStderr.red(
        `Failed to parse global config in ${configPath} with error ${
          err as any
        }.`,
      ),
    );
    return null;
  }
}

/**
 * The config holds the CLI access token. Older CLI versions wrote it with
 * default permissions (usually 0644), so remove group and other access
 * whenever it's read.
 */
export function restrictConfigPermissions(ctx: Context, configPath: string) {
  if (process.platform === "win32") {
    return;
  }
  try {
    const mode = ctx.fs.stat(configPath).mode & 0o777;
    if ((mode & 0o077) !== 0) {
      // Only remove group and other bits, so this never grants the owner a
      // permission they didn't already have.
      const restricted = mode & 0o700;
      ctx.fs.chmod(configPath, restricted);
      logVerbose(
        `Restricted permissions on ${formatPathForPrinting(configPath)} from ${mode.toString(8)} to ${restricted.toString(8)}`,
      );
    }
  } catch (err) {
    // Usually the file belongs to another user (e.g. root after `sudo`).
    // Refusing to read it wouldn't make the token any less exposed, so warn.
    logWarning(
      chalkStderr.yellow(
        `Couldn't restrict permissions on ${formatPathForPrinting(configPath)}, which holds your Convex access token and is readable by other users: ${err as any}. Run \`chmod 600 ${formatPathForPrinting(configPath)}\`.`,
      ),
    );
  }
}

/** Write the global config, preserving existing properties we don't understand. */
export async function modifyGlobalConfig(ctx: Context, config: GlobalConfig) {
  const configPath = globalConfigPath();
  let configFile;
  try {
    configFile = ctx.fs.readUtf8File(configPath);
    // totally fine for it not to exist
    // eslint-disable-next-line no-empty
  } catch {}
  // storedConfig may contain properties this version of the CLI doesn't understand.
  let storedConfig = {};
  if (configFile) {
    try {
      storedConfig = JSON.parse(configFile);
      schema.parse(storedConfig);
    } catch (err) {
      logError(
        chalkStderr.red(
          `Failed to parse global config in ${configPath} with error ${
            err as any
          }.`,
        ),
      );
      storedConfig = {};
    }
  }
  const newConfig: GlobalConfig = { ...storedConfig, ...config };
  await overrwriteGlobalConfig(ctx, newConfig);
}

/**
 * Crash if the CLI is running as root via `sudo`. `sudo` usually keeps the
 * invoking user's home directory, so writing the config here would leave a
 * root-owned 0600 file that the user's own CLI can't read. Plain root (for
 * example in a Docker container) doesn't set SUDO_UID and is allowed.
 */
export async function ensureNotRunningUnderSudo(ctx: Context) {
  const sudoUid = process.env.SUDO_UID;
  if (process.getuid?.() !== 0 || !sudoUid || sudoUid === "0") {
    return;
  }
  return await ctx.crash({
    exitCode: 1,
    errorType: "fatal",
    printedMessage: chalkStderr.red(
      `Don't run this command with sudo. It would write ${formatPathForPrinting(globalConfigPath())} as root, and your own user couldn't read the credentials saved there. Run it again without sudo.`,
    ),
  });
}

/** Write global config, overwriting any existing settings. */
async function overrwriteGlobalConfig(ctx: Context, config: GlobalConfig) {
  await ensureNotRunningUnderSudo(ctx);
  const dirName = rootDirectory();
  ctx.fs.mkdir(dirName, { allowExisting: true });
  const path = globalConfigPath();
  try {
    // The config holds the CLI access token, so only the owner may read it.
    ctx.fs.writeUtf8File(path, JSON.stringify(config, null, 2), 0o600);
  } catch (err) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      errForSentry: err,
      printedMessage: chalkStderr.red(
        `Failed to write auth config to ${path} with error: ${err as any}`,
      ),
    });
  }
  logVerbose(`Saved credentials to ${formatPathForPrinting(path)}`);
}

export function formatPathForPrinting(path: string) {
  const homedir = os.homedir();
  if (process.platform === "darwin" && path.startsWith(homedir)) {
    return path.replace(homedir, "~");
  }
  return path;
}
