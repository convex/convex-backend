import { Context } from "../../../bundler/context.js";
import {
  logFinishedStep,
  logVerbose,
  logWarning,
  showSpinner,
  stopSpinner,
} from "../../../bundler/log.js";
import { logAndHandleFetchError, ThrowingFetchError } from "../utils/utils.js";
import {
  bigBrainPause,
  bigBrainRecordActivity,
  bigBrainStart,
} from "./bigBrain.js";
import {
  LocalDeploymentConfig,
  loadDeploymentConfig,
  loadDeploymentConfigFromDir,
  loadProjectLocalConfig,
  legacyDeploymentStateDir,
  rootDeploymentStateDir,
} from "./filePaths.js";
import {
  ensureBackendStopped,
  localDeploymentUrl,
  withRunningBackend,
} from "./run.js";
import { handlePotentialUpgradeAndStart } from "./upgrade.js";
import { LocalDeploymentError, printLocalDeploymentOnError } from "./errors.js";
import {
  chooseLocalBackendPorts,
  printLocalDeploymentWelcomeMessage,
} from "./utils.js";
import { ensureBackendBinaryDownloaded } from "./download.js";
import { defaultEnvBackend } from "../defaultEnv.js";
import { deploymentEnvBackend, EnvVar } from "../env.js";
import { getProjectDetails } from "../deploymentSelection.js";
import { DeploymentDetails } from "../deployment.js";
import { LEGACY_LOCAL_BACKEND_INSTANCE_SECRET } from "./secrets.js";

export async function handleLocalDeployment(
  ctx: Context,
  options: {
    teamSlug: string;
    projectSlug: string;
    ports: {
      cloud: number | undefined;
      site: number | undefined;
    };
    backendVersion?: string | undefined;
    forceUpgrade: boolean;
  },
): Promise<DeploymentDetails> {
  const existingDeploymentForProject = await getExistingDeployment(ctx, {
    projectSlug: options.projectSlug,
    teamSlug: options.teamSlug,
  });
  const isFirstTime = existingDeploymentForProject === null;
  if (isFirstTime) {
    printLocalDeploymentWelcomeMessage();
  }
  ctx.registerCleanup(async (_exitCode, err) => {
    if (err instanceof LocalDeploymentError) {
      printLocalDeploymentOnError();
    }
  });
  if (existingDeploymentForProject !== null) {
    logVerbose(`Found existing deployment for project ${options.projectSlug}`);
    // If it's still running for some reason, exit and tell the user to kill it.
    // It's fine if a different backend is running on these ports though since we'll
    // pick new ones.
    await ensureBackendStopped(ctx, {
      ports: {
        cloud: existingDeploymentForProject.config.ports.cloud,
      },
      maxTimeSecs: 5,
      deploymentName: existingDeploymentForProject.deploymentName,
      allowOtherDeployments: true,
    });
  }

  const { binaryPath, version } = await ensureBackendBinaryDownloaded(
    ctx,
    options.backendVersion === undefined
      ? {
          kind: "latest",
          allowedVersion: existingDeploymentForProject?.config.backendVersion,
        }
      : { kind: "version", version: options.backendVersion },
  );
  const { cloudPort, sitePort } = await chooseLocalBackendPorts(ctx, {
    requestedPorts: options.ports,
    suggestedPorts: existingDeploymentForProject?.config.ports,
  });
  const { deploymentName, projectId } = await bigBrainStart(ctx, {
    port: cloudPort,
    projectSlug: options.projectSlug,
    teamSlug: options.teamSlug,
    instanceName: existingDeploymentForProject?.deploymentName ?? null,
  });

  const { cleanupHandle, adminKey } = await handlePotentialUpgradeAndStart(
    ctx,
    {
      deploymentKind: "local",
      deploymentName,
      oldVersion: existingDeploymentForProject?.config.backendVersion ?? null,
      newBinaryPath: binaryPath,
      newVersion: version,
      ports: { cloud: cloudPort, site: sitePort },
      existingCredentials: existingDeploymentForProject?.config
        ? {
            adminKey: existingDeploymentForProject?.config.adminKey,
            instanceSecret:
              existingDeploymentForProject?.config.instanceSecret ??
              LEGACY_LOCAL_BACKEND_INSTANCE_SECRET,
          }
        : null,
      forceUpgrade: options.forceUpgrade,
      cloudProjectId: projectId,
    },
  );

  if (isFirstTime) {
    await importDefaultEnvVars(ctx, {
      teamSlug: options.teamSlug,
      projectSlug: options.projectSlug,
      deploymentName,
      deploymentUrl: localDeploymentUrl(cloudPort),
      adminKey,
    });
  }

  // Periodically report activity to BigBrain every 60 seconds.
  // Uses self-scheduling setTimeout to avoid overlapping requests.
  let activityTimeout: ReturnType<typeof setTimeout> | null = null;
  let activityPingStopped = false;
  async function activityPing() {
    if (activityPingStopped) {
      return;
    }
    try {
      await bigBrainRecordActivity(ctx, {
        instanceName: deploymentName,
        adminKey,
      });
    } catch {
      // Best-effort: don't crash on failed pings
    }

    if (activityPingStopped) {
      return;
    }
    activityTimeout = setTimeout(async () => {
      void activityPing();
    }, 60_000);
  }
  void activityPing();

  const cleanupFunc = ctx.removeCleanup(cleanupHandle);
  ctx.registerCleanup(async (exitCode, err) => {
    activityPingStopped = true;
    if (activityTimeout !== null) {
      clearTimeout(activityTimeout);
    }
    if (cleanupFunc !== null) {
      await cleanupFunc(exitCode, err);
    }
    await bigBrainPause(ctx, {
      projectSlug: options.projectSlug,
      teamSlug: options.teamSlug,
    });
  });

  return {
    adminKey,
    deploymentName,
    deploymentUrl: localDeploymentUrl(cloudPort),
    reference: null,
    isDefault: false,
  };
}

export async function loadLocalDeploymentCredentials(
  ctx: Context,
  deploymentName: string,
): Promise<{
  deploymentName: string;
  deploymentUrl: string;
  adminKey: string;
}> {
  const config = loadDeploymentConfig(ctx, "local", deploymentName);
  if (config === null) {
    return ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage:
        "Failed to load deployment config - try running `npx convex dev --configure`",
    });
  }
  return {
    deploymentName,
    deploymentUrl: localDeploymentUrl(config.ports.cloud),
    adminKey: config.adminKey,
  };
}

async function getExistingDeployment(
  ctx: Context,
  options: {
    projectSlug: string;
    teamSlug: string;
  },
): Promise<{ deploymentName: string; config: LocalDeploymentConfig } | null> {
  const { projectSlug, teamSlug } = options;

  // Check project-local storage first - this is the new default location
  const projectLocal = loadProjectLocalConfig(ctx);
  if (projectLocal !== null) {
    // Verify this deployment is for the expected project (matches the naming pattern)
    const expectedPrefix = `local-${teamSlug.replace(/-/g, "_")}-${projectSlug.replace(/-/g, "_")}`;
    if (projectLocal.deploymentName.startsWith(expectedPrefix)) {
      return projectLocal;
    }
    logVerbose(
      `Project-local deployment ${projectLocal.deploymentName} doesn't match expected prefix ${expectedPrefix}`,
    );
  }

  // Fall back to checking legacy home directory
  const prefix = `local-${teamSlug.replace(/-/g, "_")}-${projectSlug.replace(/-/g, "_")}`;
  const legacyDeployments = await getLegacyLocalDeployments(ctx);
  const existingDeploymentForProject = legacyDeployments.find((d) =>
    d.deploymentName.startsWith(prefix),
  );
  if (existingDeploymentForProject === undefined) {
    return null;
  }
  return {
    deploymentName: existingDeploymentForProject.deploymentName,
    config: existingDeploymentForProject.config,
  };
}

/**
 * Get local deployments from the legacy home directory location.
 * This is used for backward compatibility and for listing deployments in offline mode.
 */
async function getLegacyLocalDeployments(ctx: Context): Promise<
  Array<{
    deploymentName: string;
    config: LocalDeploymentConfig;
  }>
> {
  const dir = rootDeploymentStateDir("local");
  if (!ctx.fs.exists(dir)) {
    return [];
  }
  const deploymentNames = ctx.fs
    .listDir(dir)
    .map((d) => d.name)
    .filter((d) => d.startsWith("local-"));
  return deploymentNames.flatMap((deploymentName) => {
    const legacyDir = legacyDeploymentStateDir("local", deploymentName);
    const config = loadDeploymentConfigFromDir(ctx, legacyDir);
    if (config !== null) {
      return [{ deploymentName, config }];
    }
    return [];
  });
}

/** Copies the default dev env vars from big brain the first time the local dev backend is started */
export async function importDefaultEnvVars(
  ctx: Context,
  {
    teamSlug,
    projectSlug,
    deploymentName,
    deploymentUrl,
    adminKey,
  }: {
    teamSlug: string;
    projectSlug: string;
    deploymentName: string;
    deploymentUrl: string;
    adminKey: string;
  },
) {
  showSpinner("Importing default env vars...");

  const project = await getProjectDetails(ctx, {
    kind: "teamAndProjectSlugs",
    teamSlug,
    projectSlug,
  });
  let defaults: EnvVar[];
  try {
    defaults = await defaultEnvBackend(ctx, project.id, "dev").list();
  } catch (err) {
    if (err instanceof ThrowingFetchError && err.response.status === 403) {
      stopSpinner();
      logWarning(
        `Skipping default env var import: ${err.serverErrorData?.message ?? err.message}`,
      );
      return;
    }
    return await logAndHandleFetchError(ctx, err);
  }
  if (defaults.length === 0) {
    logFinishedStep("No default env vars to import.");
    return;
  }

  const deployment = {
    deploymentUrl,
    deploymentFields: {
      deploymentName,
      deploymentType: "local" as const,
      projectSlug,
      teamSlug,
      reference: null,
      isDefault: false,
    },
  };

  await withRunningBackend({
    ctx,
    deployment,
    action: async () => {
      await deploymentEnvBackend(ctx, { deploymentUrl, adminKey }).update(
        defaults.map((v) => ({ name: v.name, value: v.value })),
      );
      logFinishedStep(
        `Imported ${defaults.length} environment ${defaults.length === 1 ? "variable" : "variables"} from default environment variables: ${defaults.map((v) => v.name).join(", ")}`,
      );
    },
  });
}
