import { Context } from "../../../bundler/context.js";
import {
  logFinishedStep,
  logVerbose,
  logWarning,
} from "../../../bundler/log.js";
import {
  LocalDeploymentConfig,
  LocalDeploymentKind,
  saveDeploymentConfig,
} from "./filePaths.js";
import { runLocalBackend } from "./run.js";
import { releaseDate } from "./utils.js";
import { promptYesNo } from "../utils/prompts.js";
import { ensureBackendBinaryDownloaded } from "./download.js";
import {
  generateLocalDevSecretsWithLatestBinary,
  LEGACY_LOCAL_BACKEND_INSTANCE_SECRET,
} from "./secrets.js";

export async function handlePotentialUpgradeAndStart(
  ctx: Context,
  args: {
    deploymentKind: LocalDeploymentKind;
    deploymentName: string;
    oldVersion: string | null;
    newBinaryPath: string;
    newVersion: string;
    ports: {
      cloud: number;
      site: number;
    };
    existingCredentials: { adminKey: string; instanceSecret: string } | null;
    forceUpgrade: boolean;
    cloudProjectId: number | undefined;
  },
): Promise<{ cleanupHandle: string; adminKey: string }> {
  const { adminKey, instanceSecret } =
    args.existingCredentials === null ||
    args.existingCredentials.instanceSecret ===
      LEGACY_LOCAL_BACKEND_INSTANCE_SECRET
      ? // Using `generateLocalDevSecretsFromLatestBinary` instead of `generateLocalDevSecrets`
        // here, because `newBinaryPath` can be a binary that doesn’t support
        // the `keygen admin-key` subcommand (when the --local-backend-version flag is provided to the CLI)
        //
        // In most cases (the user is not using the flag), we have already downloaded the latest binary
        // shortly before in handleLocalDeployment/handleAnonymousDeployment, so this doesn’t cause an
        // extra download (even if the user chooses later not to upgrade their deployment)
        await generateLocalDevSecretsWithLatestBinary(ctx, {
          deploymentName: args.deploymentName,
        })
      : args.existingCredentials;
  const newConfig: LocalDeploymentConfig = {
    ports: args.ports,
    backendVersion: args.newVersion,
    adminKey,
    instanceSecret,
    cloudProjectId: args.cloudProjectId,
  };
  if (args.oldVersion === null || args.oldVersion === args.newVersion) {
    // No upgrade needed. Save the current config and start running the backend.
    saveDeploymentConfig(
      ctx,
      args.deploymentKind,
      args.deploymentName,
      newConfig,
    );
    const { cleanupHandle } = await runLocalBackend(ctx, {
      binaryPath: args.newBinaryPath,
      backendVersion: args.newVersion,
      deploymentKind: args.deploymentKind,
      deploymentName: args.deploymentName,
      ports: args.ports,
      instanceSecret,
      isLatestVersion: true,
    });
    return { cleanupHandle, adminKey };
  }
  logVerbose(
    `Considering upgrade from ${args.oldVersion} to ${args.newVersion}`,
  );
  const confirmed =
    args.forceUpgrade ||
    !process.stdin.isTTY ||
    (await promptYesNo(ctx, {
      message: `This deployment is using an older version of the Convex backend. Upgrade now?`,
      default: true,
    }));
  if (!confirmed) {
    const { binaryPath: oldBinaryPath } = await ensureBackendBinaryDownloaded(
      ctx,
      {
        kind: "version",
        version: args.oldVersion,
      },
    );
    // Skipping upgrade, save the config with the old version and run.
    saveDeploymentConfig(ctx, args.deploymentKind, args.deploymentName, {
      ...newConfig,
      backendVersion: args.oldVersion,
    });
    const { cleanupHandle } = await runLocalBackend(ctx, {
      binaryPath: oldBinaryPath,
      backendVersion: args.oldVersion,
      ports: args.ports,
      deploymentKind: args.deploymentKind,
      deploymentName: args.deploymentName,
      instanceSecret,
      isLatestVersion: false,
    });
    return { cleanupHandle, adminKey };
  }
  const { cleanupHandle } = await handleUpgrade(ctx, {
    deploymentKind: args.deploymentKind,
    deploymentName: args.deploymentName,
    oldVersion: args.oldVersion,
    newBinaryPath: args.newBinaryPath,
    newVersion: args.newVersion,
    ports: args.ports,
    adminKey,
    instanceSecret,
    cloudProjectId: args.cloudProjectId,
  });
  return { cleanupHandle, adminKey };
}

/**
 * Upgrade in place: hand the existing data directory to the new binary, which
 * migrates its own on-disk state on startup (the same path self-hosted
 * deployments take).
 */
async function handleUpgrade(
  ctx: Context,
  args: {
    deploymentName: string;
    deploymentKind: LocalDeploymentKind;
    oldVersion: string;
    newBinaryPath: string;
    newVersion: string;
    ports: {
      cloud: number;
      site: number;
    };
    instanceSecret: string;
    adminKey: string;
    cloudProjectId: number | undefined;
  },
): Promise<{ cleanupHandle: string }> {
  const isDowngrade = _isDowngrade(args.oldVersion, args.newVersion);
  if (isDowngrade) {
    logWarning(
      `Moving a local deployment back to an older backend version (${args.oldVersion} → ${args.newVersion}) isn't supported: ` +
        `the older backend may not be able to read data written by the newer one. ` +
        `If it fails to start, delete the deployment's state directory to start fresh on this version.`,
    );
  }

  logVerbose(`Running backend on new version ${args.newVersion}`);
  const { cleanupHandle } = await runLocalBackend(ctx, {
    binaryPath: args.newBinaryPath,
    backendVersion: args.newVersion,
    ports: args.ports,
    deploymentKind: args.deploymentKind,
    deploymentName: args.deploymentName,
    instanceSecret: args.instanceSecret,
    isLatestVersion: true,
  });

  logFinishedStep(
    `Successfully ${isDowngrade ? "moved" : "upgraded"} to backend version ${args.newVersion}`,
  );
  saveDeploymentConfig(ctx, args.deploymentKind, args.deploymentName, {
    ports: args.ports,
    backendVersion: args.newVersion,
    adminKey: args.adminKey,
    instanceSecret: args.instanceSecret,
    cloudProjectId: args.cloudProjectId,
  });

  return { cleanupHandle };
}

/** Whether `newVersion` predates `oldVersion`, as far as we can tell. */
export function _isDowngrade(oldVersion: string, newVersion: string): boolean {
  const oldDate = releaseDate(oldVersion);
  const newDate = releaseDate(newVersion);
  return oldDate !== null && newDate !== null && newDate < oldDate;
}
