// ----------------------------------------------------------------------------
// Anonymous (No account)

import path from "path";
import { Context } from "../../../bundler/context.js";
import {
  logFinishedStep,
  logMessage,
  logVerbose,
  logWarning,
} from "../../../bundler/log.js";
import { promptSearch, promptYesNo } from "../utils/prompts.js";
import { bigBrainPause, bigBrainStart } from "./bigBrain.js";
import { LocalDeploymentError, printLocalDeploymentOnError } from "./errors.js";
import {
  LocalDeploymentKind,
  deploymentStateDir,
  ensureUuidForAnonymousUser,
  legacyDeploymentStateDir,
  loadDeploymentConfig,
  loadDeploymentConfigFromDir,
  loadProjectLocalConfig,
  saveDeploymentConfig,
} from "./filePaths.js";
import { rootDeploymentStateDir } from "./filePaths.js";
import { LocalDeploymentConfig } from "./filePaths.js";
import { ensureBackendStopped, localDeploymentUrl } from "./run.js";
import { handlePotentialUpgradeAndStart } from "./upgrade.js";
import { chooseLocalBackendPorts } from "./utils.js";
import { recursivelyDelete, recursivelyCopy } from "../fsUtils.js";
import { ensureBackendBinaryDownloaded } from "./download.js";
import { DeploymentDetails, isAnonymousDeployment } from "../deployment.js";
import { createProject } from "../api.js";
import { removeAnonymousPrefix } from "../deployment.js";
import { nodeFs } from "../../../bundler/fs.js";
import { doInitConvexFolder } from "../codegen.js";
import { readProjectConfig } from "../config.js";
import { functionsDir } from "../utils/utils.js";
import { attemptSetupAiFiles } from "../aiFiles/index.js";
import {
  generateLocalDevSecretsWithLatestBinary,
  LEGACY_LOCAL_BACKEND_INSTANCE_SECRET,
} from "./secrets.js";

export async function handleAnonymousDeployment(
  ctx: Context,
  options: {
    ports: {
      cloud: number | undefined;
      site: number | undefined;
    };
    backendVersion?: string | undefined;
    dashboardVersion?: string | undefined;
    forceUpgrade: boolean;
    deploymentName: string | null;
    chosenConfiguration: "new" | "existing" | "ask" | null;
  },
): Promise<DeploymentDetails> {
  const deployment = await chooseDeployment(ctx, {
    deploymentName: options.deploymentName,
    chosenConfiguration: options.chosenConfiguration,
  });
  if (
    deployment.kind === "first" &&
    process.env.CONVEX_AGENT_MODE !== "anonymous" &&
    process.stdin.isTTY
  ) {
    logMessage(
      "This command, `npx convex dev`, will run your Convex backend locally and update it with the function you write in the `convex/` directory.",
    );
    logMessage(
      "Use `npx convex dashboard` to view and interact with your project from a web UI.",
    );
    logMessage(
      "Use `npx convex docs` to read the docs and `npx convex help` to see other commands.",
    );
    ensureUuidForAnonymousUser(ctx);
    if (process.stdin.isTTY) {
      const result = await promptYesNo(ctx, {
        message: "Continue?",
        default: true,
      });
      if (!result) {
        return ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage: "Exiting",
        });
      }
    }
  }
  ctx.registerCleanup(async (_exitCode, err) => {
    if (err instanceof LocalDeploymentError) {
      printLocalDeploymentOnError();
    }
  });
  const { binaryPath, version } = await ensureBackendBinaryDownloaded(
    ctx,
    options.backendVersion === undefined
      ? {
          kind: "latest",
        }
      : { kind: "version", version: options.backendVersion },
  );

  const existingCredentials =
    deployment.kind === "existing"
      ? {
          adminKey: deployment.config.adminKey,
          instanceSecret:
            deployment.config.instanceSecret ??
            LEGACY_LOCAL_BACKEND_INSTANCE_SECRET,
        }
      : null;
  if (deployment.kind === "existing") {
    // If it's still running for some reason, exit and tell the user to kill it.
    // It's fine if a different backend is running on these ports though since we'll
    // pick new ones.
    await ensureBackendStopped(ctx, {
      ports: {
        cloud: deployment.config.ports.cloud,
      },
      maxTimeSecs: 5,
      deploymentName: deployment.deploymentName,
      allowOtherDeployments: true,
    });
  }

  const { cloudPort, sitePort } = await chooseLocalBackendPorts(ctx, {
    requestedPorts: options.ports,
    suggestedPorts:
      deployment.kind === "existing" ? deployment.config.ports : undefined,
  });

  const { cleanupHandle, adminKey } = await handlePotentialUpgradeAndStart(
    ctx,
    {
      deploymentName: deployment.deploymentName,
      deploymentKind: "anonymous",
      oldVersion:
        deployment.kind === "existing"
          ? deployment.config.backendVersion
          : null,
      newBinaryPath: binaryPath,
      newVersion: version,
      ports: { cloud: cloudPort, site: sitePort },
      existingCredentials,
      forceUpgrade: options.forceUpgrade,
      // Anonymous deployments aren't registered against a cloud project.
      cloudProjectId: undefined,
    },
  );

  const cleanupFunc = ctx.removeCleanup(cleanupHandle);
  ctx.registerCleanup(async (exitCode, err) => {
    if (cleanupFunc !== null) {
      await cleanupFunc(exitCode, err);
    }
  });

  if (deployment.kind === "new") {
    await doInitConvexFolder(ctx);
    const { configPath, projectConfig } = await readProjectConfig(ctx);
    const convexDir = path.resolve(functionsDir(configPath, projectConfig));
    const projectDir = path.resolve(path.dirname(configPath));
    await attemptSetupAiFiles({
      ctx,
      aiFilesConfig: projectConfig.aiFiles,
      convexDir,
      projectDir,
    });
  }
  return {
    adminKey,
    deploymentName: deployment.deploymentName,
    deploymentUrl: localDeploymentUrl(cloudPort),
    reference: null,
    isDefault: false,
  };
}

export async function loadAnonymousDeployment(
  ctx: Context,
  deploymentName: string,
): Promise<LocalDeploymentConfig> {
  const config = loadDeploymentConfig(ctx, "anonymous", deploymentName);
  if (config === null) {
    return ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `Could not find deployment with name ${deploymentName}!`,
    });
  }
  return config;
}

/**
 * List legacy anonymous deployments from the home directory.
 * These are deployments stored in ~/.convex/anonymous-convex-backend-state/
 */
export function listLegacyAnonymousDeployments(ctx: Context): Array<{
  deploymentName: string;
  config: LocalDeploymentConfig;
}> {
  const deployments: Array<{
    deploymentName: string;
    config: LocalDeploymentConfig;
  }> = [];

  const dir = rootDeploymentStateDir("anonymous");
  if (ctx.fs.exists(dir)) {
    const deploymentNames = ctx.fs
      .listDir(dir)
      .map((d) => d.name)
      .filter((d) => isAnonymousDeployment(d));
    for (const deploymentName of deploymentNames) {
      const legacyDir = legacyDeploymentStateDir("anonymous", deploymentName);
      const config = loadDeploymentConfigFromDir(ctx, legacyDir);
      if (config !== null) {
        deployments.push({ deploymentName, config });
      }
    }
  }

  return deployments;
}

export async function listExistingAnonymousDeployments(ctx: Context): Promise<
  Array<{
    deploymentName: string;
    config: LocalDeploymentConfig;
  }>
> {
  const deployments: Array<{
    deploymentName: string;
    config: LocalDeploymentConfig;
  }> = [];

  // Check project-local storage first
  const projectLocal = loadProjectLocalConfig(ctx);
  if (
    projectLocal !== null &&
    isAnonymousDeployment(projectLocal.deploymentName)
  ) {
    deployments.push(projectLocal);
  }

  // Check legacy home directory, avoiding duplicates
  for (const legacy of listLegacyAnonymousDeployments(ctx)) {
    if (!deployments.some((d) => d.deploymentName === legacy.deploymentName)) {
      deployments.push(legacy);
    }
  }

  return deployments;
}

async function chooseDeployment(
  ctx: Context,
  options: {
    deploymentName: string | null;
    chosenConfiguration: "new" | "existing" | "ask" | null;
  },
): Promise<
  | {
      kind: "existing";
      deploymentName: string;
      config: LocalDeploymentConfig;
    }
  | {
      kind: "new";
      deploymentName: string;
    }
  | {
      kind: "first";
      deploymentName: string;
    }
> {
  // Check for existing project-local deployment first - use it if it exists
  const projectLocal = loadProjectLocalConfig(ctx);
  if (projectLocal !== null) {
    if (isAnonymousDeployment(projectLocal.deploymentName)) {
      // Already an anonymous deployment - use it as-is
      return {
        kind: "existing",
        deploymentName: projectLocal.deploymentName,
        config: projectLocal.config,
      };
    }
    // Project-local has data from a different deployment type (e.g., "local-*")
    // Create a new anonymous deployment that will reuse this data and update the config
    logVerbose(
      `Project-local has ${projectLocal.deploymentName}, switching to anonymous`,
    );
    return { deploymentName: generateDeploymentName(), kind: "new" };
  }

  // Check if a specific deployment name was requested (legacy support)
  if (options.deploymentName !== null && options.chosenConfiguration === null) {
    const deployments = await listExistingAnonymousDeployments(ctx);
    const existing = deployments.find(
      (d) => d.deploymentName === options.deploymentName,
    );
    if (existing === undefined) {
      logWarning(`Could not find project with name ${options.deploymentName}!`);
    } else {
      return {
        kind: "existing",
        deploymentName: existing.deploymentName,
        config: existing.config,
      };
    }
  }

  // Handle agent mode - use fixed name since there's one deployment per project
  if (process.env.CONVEX_AGENT_MODE === "anonymous") {
    const deploymentName = "anonymous-agent";
    logVerbose(`Deployment name: ${deploymentName}`);
    return {
      kind: "new",
      deploymentName,
    };
  }

  // No project-local data - check for legacy deployments in home directory
  const legacyDeployments = listLegacyAnonymousDeployments(ctx);

  // No legacy deployments - auto-create a new project without prompting
  if (legacyDeployments.length === 0) {
    logMessage("Setting up a new project...");
    return { deploymentName: generateDeploymentName(), kind: "first" };
  }

  // User explicitly wants a new deployment - create without prompting for name
  if (options.chosenConfiguration === "new") {
    return { deploymentName: generateDeploymentName(), kind: "new" };
  }

  // Non-interactive terminal - auto-create a new deployment
  if (!process.stdin.isTTY) {
    return { deploymentName: generateDeploymentName(), kind: "new" };
  }

  // Legacy deployments exist - prompt user to choose
  const newOrExisting = await promptSearch(ctx, {
    message: "Which project would you like to use?",
    choices: [
      ...(options.chosenConfiguration === "existing"
        ? []
        : [
            {
              name: "Create a new one",
              value: "new",
            },
          ]),
      ...legacyDeployments.map((d) => ({
        name: d.deploymentName,
        value: d.deploymentName,
      })),
    ],
  });

  if (newOrExisting !== "new") {
    const existingDeployment = legacyDeployments.find(
      (d) => d.deploymentName === newOrExisting,
    );
    if (existingDeployment === undefined) {
      return ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `Could not find project with name ${newOrExisting}!`,
      });
    }
    return {
      kind: "existing",
      deploymentName: existingDeployment.deploymentName,
      config: existingDeployment.config,
    };
  }

  // User chose to create a new one - no name prompt needed
  return { deploymentName: generateDeploymentName(), kind: "new" };
}

/**
 * Returns a name for a new anonymous deployment.
 */
function generateDeploymentName() {
  const baseName = path.basename(process.cwd());
  const deploymentName = `anonymous-${baseName}`;
  logVerbose(`Deployment name: ${deploymentName}`);
  return deploymentName;
}

/**
 * This takes an "anonymous" deployment and makes it a "local" deployment
 * that is associated with a project in the given team.
 */
export async function handleLinkToProject(
  ctx: Context,
  args: {
    deploymentName: string;
    teamSlug: string;
    teamId: number;
    projectSlug: string | null;
  },
): Promise<{
  deploymentName: string;
  deploymentUrl: string;
  projectSlug: string;
}> {
  logVerbose(
    `Linking ${args.deploymentName} to a project in team ${args.teamSlug}`,
  );
  const config = loadDeploymentConfig(ctx, "anonymous", args.deploymentName);
  if (config === null) {
    return ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage:
        "Failed to load deployment config - try running `npx convex dev --configure`",
    });
  }
  await ensureBackendStopped(ctx, {
    ports: {
      cloud: config.ports.cloud,
    },
    deploymentName: args.deploymentName,
    allowOtherDeployments: true,
    maxTimeSecs: 5,
  });
  const projectName = removeAnonymousPrefix(args.deploymentName);
  let projectSlug: string;
  if (args.projectSlug !== null) {
    projectSlug = args.projectSlug;
  } else {
    const { projectSlug: newProjectSlug } = await createProject(ctx, {
      teamId: args.teamId,
      projectName,
      deploymentToProvision: null,
    });
    projectSlug = newProjectSlug;
  }
  logVerbose(`Creating local deployment in project ${projectSlug}`);
  // Register it in big brain
  const { deploymentName: localDeploymentName, projectId } =
    await bigBrainStart(ctx, {
      port: config.ports.cloud,
      projectSlug,
      teamSlug: args.teamSlug,
      instanceName: null,
    });
  const { instanceSecret, adminKey } =
    await generateLocalDevSecretsWithLatestBinary(ctx, {
      deploymentName: localDeploymentName,
    });
  const localConfig = loadDeploymentConfig(ctx, "local", localDeploymentName);
  if (localConfig !== null) {
    return ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `Project ${projectSlug} already has a local deployment, so we cannot link this anonymous local deployment to it.`,
    });
  }
  logVerbose(`Moving ${args.deploymentName} to ${localDeploymentName}`);
  await moveDeployment(
    ctx,
    {
      deploymentKind: "anonymous",
      deploymentName: args.deploymentName,
    },
    {
      deploymentKind: "local",
      deploymentName: localDeploymentName,
    },
  );
  logVerbose(`Saving deployment config for ${localDeploymentName}`);
  saveDeploymentConfig(ctx, "local", localDeploymentName, {
    instanceSecret,
    adminKey,
    backendVersion: config.backendVersion,
    ports: config.ports,
    cloudProjectId: projectId,
  });
  await bigBrainPause(ctx, {
    projectSlug,
    teamSlug: args.teamSlug,
  });
  logFinishedStep(`Linked ${args.deploymentName} to project ${projectSlug}`);
  return {
    projectSlug,
    deploymentName: localDeploymentName,
    deploymentUrl: localDeploymentUrl(config.ports.cloud),
  };
}

export async function moveDeployment(
  ctx: Context,
  oldDeployment: {
    deploymentKind: LocalDeploymentKind;
    deploymentName: string;
  },
  newDeployment: {
    deploymentKind: LocalDeploymentKind;
    deploymentName: string;
  },
) {
  const oldPath = deploymentStateDir(
    ctx,
    oldDeployment.deploymentKind,
    oldDeployment.deploymentName,
  );
  const newPath = deploymentStateDir(
    ctx,
    newDeployment.deploymentKind,
    newDeployment.deploymentName,
  );

  // If both paths are the same (project-local storage), no file movement needed.
  // The config will be updated separately by saveDeploymentConfig.
  if (oldPath === newPath) {
    logVerbose(
      `Source and destination are the same (${oldPath}), skipping file copy`,
    );
    return;
  }

  await recursivelyCopy(ctx, nodeFs, oldPath, newPath);
  recursivelyDelete(ctx, oldPath);
}
