import { Context } from "../../../bundler/context.js";
import { logVerbose } from "../../../bundler/log.js";
import { ensureBackendBinaryDownloaded } from "./download.js";
import { LocalDeploymentError } from "./errors.js";
import { execFile } from "child_process";
import { promisify } from "util";
import crypto from "crypto";

export const LEGACY_LOCAL_BACKEND_INSTANCE_SECRET =
  "4361726e697461732c206c69746572616c6c79206d65616e696e6720226c6974";

/**
 * Generates an instance secret and an admin key for a new (or just upgraded) local deployment.
 */
export async function generateLocalDevSecrets(
  ctx: Context,
  {
    deploymentName,
    latestBinaryPath,
  }: {
    deploymentName: string;
    /**
     * A path to the local backend binary.
     *
     * This must always be the latest version of the local binary that has been published,
     * in order to make sure the `keygen admin-key` command is available. If you need to call
     * this without having access to the latest binary, use `generateLocalDevSecretsWithLatestBinary`
     * which will download the latest version when necessary.
     */
    latestBinaryPath: string;
  },
): Promise<{
  instanceSecret: string;
  adminKey: string;
}> {
  logVerbose("Generating local dev secrets");

  const instanceSecret = generateInstanceSecret();

  let stdout: string;
  try {
    ({ stdout } = await promisify(execFile)(latestBinaryPath, [
      "keygen",
      "admin-key",
      "--instance-name",
      deploymentName,
      "--instance-secret",
      instanceSecret,
    ]));
  } catch (e) {
    const err = e as { stderr?: string; message: string };
    const detail = err.stderr ? err.stderr : err.message;
    const message = `Failed to generate admin key:\n${detail}`;
    return ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: message,
      errForSentry: new LocalDeploymentError(message),
    });
  }

  const adminKey = stdout.trim();

  return {
    instanceSecret,
    adminKey,
  };
}

function generateInstanceSecret(): string {
  return crypto.randomBytes(32).toString("hex");
}

/**
 * Similar to generateLocalDevSecrets, but can be called when we’re not confident
 * we have a binary that supports `keygen admin-key`
 */
export async function generateLocalDevSecretsWithLatestBinary(
  ctx: Context,
  {
    deploymentName,
  }: {
    deploymentName: string;
  },
) {
  const { binaryPath: latestBinaryPath } = await ensureBackendBinaryDownloaded(
    ctx,
    {
      kind: "latest",
    },
  );
  return generateLocalDevSecrets(ctx, { deploymentName, latestBinaryPath });
}
