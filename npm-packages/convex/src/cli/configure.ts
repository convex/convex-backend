import { chalkStderr } from "chalk";
import { Context } from "../bundler/context.js";
import { handleNoAccessToConfiguredProject } from "./account.js";
import {
  logFailure,
  logFinishedStep,
  logMessage,
  logWarning,
  showSpinner,
} from "../bundler/log.js";
import {
  DeploymentType,
  DeploymentName,
  fetchDeploymentCredentialsProvisioningDevOrProdMaybeThrows,
  createProject,
  loadSelectedDeploymentCredentials,
  checkAccessToSelectedProject,
  DeploymentSelectionWithinProject,
} from "./lib/api.js";
import { readProjectConfig, ensureConvexFunctionsDir } from "./lib/config.js";
import {
  DeploymentDetails,
  eraseDeploymentEnvVar,
  writeDeploymentEnvVar,
} from "./lib/deployment.js";
import { finalizeConfiguration } from "./lib/init.js";
import {
  CONVEX_DEPLOYMENT_ENV_VAR_NAME,
  functionsDir,
  hasProjects,
  logAndHandleFetchError,
  selectDevDeploymentType,
  selectRegionOrUseDefault,
  validateOrSelectProject,
  validateOrSelectTeam,
} from "./lib/utils/utils.js";
import { writeUrlsToEnvFile } from "./lib/envvars.js";
import path from "path";
import { projectDashboardUrl } from "./lib/dashboard.js";
import { doInitConvexFolder } from "./lib/codegen.js";
import { handleLocalDeployment } from "./lib/localDeployment/localDeployment.js";
import {
  promptOptions,
  promptString,
  promptYesNo,
} from "./lib/utils/prompts.js";
import { attemptSetupAiFiles } from "./lib/aiFiles/index.js";
import {
  DeploymentSelection,
  deploymentNameFromSelection,
  shouldAllowAnonymousDevelopment,
} from "./lib/deploymentSelection.js";
import { ensureLoggedIn } from "./lib/login.js";
import { handleAnonymousDeployment } from "./lib/localDeployment/anonymous.js";
import { fetchDeploymentCanonicalUrls } from "./lib/deploy2.js";
type DeploymentCredentials = {
  url: string;
  adminKey: string;
};

type ChosenConfiguration =
  // `--configure new`
  | "new"
  // `--configure existing`
  | "existing"
  // `--configure`
  | "ask"
  // `--configure` was not specified
  | null;

type ConfigureCmdOptions = {
  prod: boolean;
  localOptions: {
    ports: {
      cloud: number | undefined;
      site: number | undefined;
    };
    backendVersion?: string | undefined;
    dashboardVersion?: string | undefined;
    forceUpgrade: boolean;
  };
  team?: string | undefined;
  project?: string | undefined;
  devDeployment?: "cloud" | "local" | undefined;
  url?: string | undefined;
  adminKey?: string | undefined;
  envFile?: string | undefined;
  overrideAuthUrl?: string | undefined;
  overrideAuthClient?: string | undefined;
  overrideAuthUsername?: string | undefined;
  overrideAuthPassword?: string | undefined;
};

/**
 * As of writing, this is used by:
 * - `npx convex dev`
 * - `npx convex init`
 *
 * But is not used by `npx convex deploy`, `npx convex codegen`,
 * `npx convex env`, or other commands.
 */
export async function deploymentCredentialsOrConfigure(
  ctx: Context,
  deploymentSelection: DeploymentSelection,
  chosenConfiguration: ChosenConfiguration,
  cmdOptions: ConfigureCmdOptions,
): Promise<
  DeploymentCredentials & {
    deploymentFields: {
      deploymentName: DeploymentName;
      deploymentType: DeploymentType;
      projectSlug: string | null;
      teamSlug: string | null;
      siteUrl: string | null;
      reference: string | null;
      isDefault: boolean;
    } | null;
  }
> {
  const selectedDeployment = await _deploymentCredentialsOrConfigure(
    ctx,
    deploymentSelection,
    chosenConfiguration,
    cmdOptions,
  );
  const { convexCloudUrl, convexSiteUrl: siteUrl } =
    await fetchDeploymentCanonicalUrls(ctx, {
      adminKey: selectedDeployment.adminKey,
      deploymentUrl: selectedDeployment.url,
    });
  const applicationUrl = deploymentUrlForApplicationEnv(
    selectedDeployment,
    convexCloudUrl,
  );

  if (selectedDeployment.deploymentFields !== null) {
    // Set the `CONVEX_DEPLOYMENT` env var + the `CONVEX_URL` env var
    await updateEnvAndConfigForDeploymentSelection(
      ctx,
      {
        url: applicationUrl,
        siteUrl,
        deploymentName: selectedDeployment.deploymentFields.deploymentName,
        teamSlug: selectedDeployment.deploymentFields.teamSlug,
        projectSlug: selectedDeployment.deploymentFields.projectSlug,
        deploymentType: selectedDeployment.deploymentFields.deploymentType,
      },
      deploymentNameFromSelection(deploymentSelection),
    );
  } else {
    // Clear the `CONVEX_DEPLOYMENT` env var + set the `CONVEX_URL` and
    // `CONVEX_SITE_URL` env vars.
    await handleManuallySetUrlAndAdminKey(ctx, {
      url: selectedDeployment.url,
      siteUrl,
      adminKey: selectedDeployment.adminKey,
    });
  }
  return {
    url: selectedDeployment.url,
    adminKey: selectedDeployment.adminKey,
    deploymentFields:
      selectedDeployment.deploymentFields === null
        ? null
        : { ...selectedDeployment.deploymentFields, siteUrl: siteUrl },
  };
}

export function deploymentUrlForApplicationEnv(
  selectedDeployment: {
    url: string;
    deploymentFields: { deploymentType: DeploymentType } | null;
  },
  canonicalCloudUrl: string,
): string {
  const deploymentType = selectedDeployment.deploymentFields?.deploymentType;
  return deploymentType !== undefined &&
    deploymentType !== "local" &&
    deploymentType !== "anonymous"
    ? canonicalCloudUrl
    : selectedDeployment.url;
}

export async function _deploymentCredentialsOrConfigure(
  ctx: Context,
  deploymentSelection: DeploymentSelection,
  chosenConfiguration: ChosenConfiguration,
  cmdOptions: ConfigureCmdOptions,
): Promise<
  DeploymentCredentials & {
    deploymentFields: {
      deploymentName: DeploymentName;
      deploymentType: DeploymentType;
      projectSlug: string | null;
      teamSlug: string | null;
      reference: string | null;
      isDefault: boolean;
    } | null;
  }
> {
  switch (deploymentSelection.kind) {
    case "existingDeployment":
      await assertLocalOptionsAreDefault(ctx, cmdOptions.localOptions);
      return {
        url: deploymentSelection.deploymentToActOn.url,
        adminKey: deploymentSelection.deploymentToActOn.adminKey,
        deploymentFields:
          deploymentSelection.deploymentToActOn.deploymentFields,
      };
    case "chooseProject": {
      await ensureLoggedIn(ctx, {
        overrideAuthUrl: cmdOptions.overrideAuthUrl,
        overrideAuthClient: cmdOptions.overrideAuthClient,
        overrideAuthUsername: cmdOptions.overrideAuthUsername,
        overrideAuthPassword: cmdOptions.overrideAuthPassword,
      });
      return await handleChooseProject(
        ctx,
        chosenConfiguration,
        deploymentSelection.selectionWithinProject,
        cmdOptions,
      );
    }
    case "preview":
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: "Use `npx convex deploy` to use preview deployments.",
      });
    case "deploymentWithinProject": {
      return await handleDeploymentWithinProject(ctx, {
        chosenConfiguration,
        deploymentSelection,
        cmdOptions,
      });
    }
    case "anonymous": {
      const hasAuth = ctx.bigBrainAuth() !== null;
      const isAgentMode = process.env.CONVEX_AGENT_MODE === "anonymous";
      if (
        !isAgentMode &&
        process.stdin.isTTY &&
        hasAuth &&
        deploymentSelection.deploymentName !== null
      ) {
        const shouldConfigure =
          chosenConfiguration !== null ||
          (await promptYesNo(ctx, {
            message: `${CONVEX_DEPLOYMENT_ENV_VAR_NAME} is configured with deployment ${deploymentSelection.deploymentName}, which is not linked with your account. Would you like to link it now?`,
          }));
        if (shouldConfigure) {
          return await handleChooseProject(
            ctx,
            chosenConfiguration,
            deploymentSelection.selectionWithinProject,
            cmdOptions,
          );
        }
      }
      const alreadyHasConfiguredAnonymousDeployment =
        deploymentSelection.deploymentName !== null &&
        chosenConfiguration === null;
      if (isAgentMode) {
        logWarning(
          chalkStderr.yellow.bold(
            "CONVEX_AGENT_MODE=anonymous mode is in beta, functionality may change in the future.",
          ),
        );
      }

      const shouldPromptForLogin =
        isAgentMode || !process.stdin.isTTY
          ? "no"
          : alreadyHasConfiguredAnonymousDeployment
            ? "no"
            : await promptOptions(ctx, {
                message:
                  "Welcome to Convex! Would you like to login to your account?",
                choices: [
                  {
                    name: "Start without an account (run Convex locally)",
                    value: "no",
                  },
                  { name: "Login or create an account", value: "yes" },
                ],
                default: "no",
              });
      if (shouldPromptForLogin === "no") {
        const result = await handleAnonymousDeployment(ctx, {
          chosenConfiguration,
          deploymentName: deploymentSelection.deploymentName,
          ...cmdOptions.localOptions,
        });
        return {
          adminKey: result.adminKey,
          url: result.deploymentUrl,
          deploymentFields: {
            deploymentName: result.deploymentName,
            deploymentType: "anonymous",
            projectSlug: null,
            teamSlug: null,
            reference: null,
            isDefault: false,
          },
        };
      }
      return await handleChooseProject(
        ctx,
        chosenConfiguration,
        deploymentSelection.selectionWithinProject,
        cmdOptions,
      );
    }
  }
}

async function handleDeploymentWithinProject(
  ctx: Context,
  {
    chosenConfiguration,
    deploymentSelection,
    cmdOptions,
  }: {
    chosenConfiguration: ChosenConfiguration;
    deploymentSelection: DeploymentSelection & {
      kind: "deploymentWithinProject";
    };
    cmdOptions: ConfigureCmdOptions;
  },
) {
  const hasAuth = ctx.bigBrainAuth() !== null;
  const loginMessage =
    hasAuth && shouldAllowAnonymousDevelopment()
      ? undefined
      : "Tip: You can try out Convex without creating an account by clearing the " +
        `${CONVEX_DEPLOYMENT_ENV_VAR_NAME} environment variable (often in .env.local).`;
  await ensureLoggedIn(ctx, {
    message: loginMessage,
    overrideAuthUrl: cmdOptions.overrideAuthUrl,
    overrideAuthClient: cmdOptions.overrideAuthClient,
    overrideAuthUsername: cmdOptions.overrideAuthUsername,
    overrideAuthPassword: cmdOptions.overrideAuthPassword,
  });
  if (chosenConfiguration !== null) {
    const result = await handleChooseProject(
      ctx,
      chosenConfiguration,
      deploymentSelection.selectionWithinProject,
      cmdOptions,
    );
    return result;
  }

  const accessResult = await checkAccessToSelectedProject(
    ctx,
    deploymentSelection.targetProject,
  );
  let access = accessResult;
  while (access.kind === "noAccess") {
    // With --team and --project, the user already said which project to use.
    const next =
      cmdOptions.team !== undefined && cmdOptions.project !== undefined
        ? "chooseProject"
        : await handleNoAccessToConfiguredProject(
            ctx,
            deploymentSelection.targetProject,
          );
    if (next === "chooseProject") {
      logMessage("You don't have access to the selected project.");
      const result = await handleChooseProject(
        ctx,
        chosenConfiguration,
        deploymentSelection.selectionWithinProject,
        cmdOptions,
      );
      return result;
    }
    access = await checkAccessToSelectedProject(
      ctx,
      deploymentSelection.targetProject,
    );
  }

  const selectedDeployment = await loadSelectedDeploymentCredentials(
    ctx,
    deploymentSelection,
    // We'll start running it below
    { ensureLocalRunning: false },
  );
  if (
    selectedDeployment.deploymentFields !== null &&
    selectedDeployment.deploymentFields.deploymentType === "local"
  ) {
    // Start running the local backend, which may bind to different ports
    // than what was saved from a previous run.
    const localDeployment = await handleLocalDeployment(ctx, {
      teamSlug: selectedDeployment.deploymentFields.teamSlug!,
      projectSlug: selectedDeployment.deploymentFields.projectSlug!,
      forceUpgrade: cmdOptions.localOptions.forceUpgrade,
      ports: cmdOptions.localOptions.ports,
      backendVersion: cmdOptions.localOptions.backendVersion,
    });
    return {
      url: localDeployment.deploymentUrl,
      adminKey: localDeployment.adminKey,
      deploymentFields: selectedDeployment.deploymentFields,
    };
  }
  await assertLocalOptionsAreDefault(ctx, cmdOptions.localOptions);
  return {
    url: selectedDeployment.url,
    adminKey: selectedDeployment.adminKey,
    deploymentFields: selectedDeployment.deploymentFields,
  };
}

async function handleChooseProject(
  ctx: Context,
  chosenConfiguration: ChosenConfiguration,
  selectionWithinProject: DeploymentSelectionWithinProject,
  cmdOptions: ConfigureCmdOptions,
): Promise<
  DeploymentCredentials & {
    deploymentFields: {
      deploymentName: DeploymentName;
      deploymentType: DeploymentType;
      projectSlug: string;
      teamSlug: string;
      reference: string | null;
      isDefault: boolean;
    };
  }
> {
  await ensureLoggedIn(ctx, {
    overrideAuthUrl: cmdOptions.overrideAuthUrl,
    overrideAuthClient: cmdOptions.overrideAuthClient,
    overrideAuthUsername: cmdOptions.overrideAuthUsername,
    overrideAuthPassword: cmdOptions.overrideAuthPassword,
  });
  const project = await selectProject(ctx, chosenConfiguration, {
    team: cmdOptions.team,
    project: cmdOptions.project,
    devDeployment: cmdOptions.devDeployment,
  });
  if (
    selectionWithinProject.kind === "prod" ||
    project.devDeployment !== "local"
  ) {
    await assertLocalOptionsAreDefault(ctx, cmdOptions.localOptions);
  }
  const deploymentOptions: DeploymentOptions =
    selectionWithinProject.kind === "prod"
      ? { kind: "prod" }
      : project.devDeployment === "local"
        ? { kind: "local", ...cmdOptions.localOptions }
        : { kind: "dev" };
  const {
    deploymentName,
    deploymentUrl: url,
    adminKey,
    reference,
    isDefault,
  } = await ensureDeploymentProvisioned(ctx, {
    teamSlug: project.teamSlug,
    projectSlug: project.projectSlug,
    deploymentOptions,
  });
  return {
    url,
    adminKey,
    deploymentFields: {
      deploymentName,
      deploymentType: deploymentOptions.kind,
      projectSlug: project.projectSlug,
      teamSlug: project.teamSlug,
      reference,
      isDefault,
    },
  };
}

async function handleManuallySetUrlAndAdminKey(
  ctx: Context,
  cmdOptions: { url: string; siteUrl: string; adminKey: string },
) {
  const { url, siteUrl, adminKey } = cmdOptions;
  const didErase = await eraseDeploymentEnvVar(ctx);
  if (didErase) {
    logMessage(
      chalkStderr.yellowBright(
        `Removed the CONVEX_DEPLOYMENT environment variable from .env.local`,
      ),
    );
  }
  const envFileConfig = await writeUrlsToEnvFile(ctx, {
    convexUrl: url,
    siteUrl,
  });
  if (
    envFileConfig !== null &&
    (envFileConfig.convexUrlEnvVar || envFileConfig.siteUrlEnvVar)
  ) {
    // Join both names with " and " if both exist, otherwise just use one of them.
    const updatedVars = [
      envFileConfig.convexUrlEnvVar,
      envFileConfig.siteUrlEnvVar,
    ]
      .filter(Boolean)
      .join(" and ");
    logMessage(
      chalkStderr.green(`Saved ${updatedVars} to ${envFileConfig.envFile}`),
    );
  }
  return { url, adminKey };
}

export async function selectProject(
  ctx: Context,
  chosenConfiguration: ChosenConfiguration,
  cmdOptions: {
    team?: string | undefined;
    project?: string | undefined;
    devDeployment?: "cloud" | "local" | undefined;
    defaultProjectName?: string | undefined;
  },
): Promise<{
  teamSlug: string;
  projectSlug: string;
  devDeployment: "cloud" | "local";
}> {
  // Prompt the user to select a project.
  const choice =
    chosenConfiguration !== "ask" && chosenConfiguration !== null
      ? chosenConfiguration
      : await askToConfigure(ctx);
  switch (choice) {
    case "new":
      return selectNewProject(ctx, chosenConfiguration, cmdOptions);
    case "existing":
      return selectExistingProject(ctx, chosenConfiguration, cmdOptions);
    default:
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: "No project selected.",
      });
  }
}

const cwd = path.basename(process.cwd());
async function selectNewProject(
  ctx: Context,
  chosenConfiguration: ChosenConfiguration,
  config: {
    team?: string | undefined;
    project?: string | undefined;
    devDeployment?: "cloud" | "local" | undefined;
    defaultProjectName?: string | undefined;
  },
) {
  const { team: selectedTeam, chosen: didChooseBetweenTeams } =
    await validateOrSelectTeam(ctx, config.team, "Team:");
  let projectName: string = config.project || cwd;
  let choseProjectInteractively = false;
  if (!config.project) {
    projectName = await promptString(ctx, {
      message: "Project name:",
      default: config.defaultProjectName || cwd,
    });
    choseProjectInteractively = true;
  }

  const { devDeployment } = await selectDevDeploymentType(ctx, {
    chosenConfiguration,
    newOrExisting: "new",
    teamSlug: selectedTeam.slug,
    userHasChosenSomethingInteractively:
      didChooseBetweenTeams || choseProjectInteractively,
    projectSlug: undefined,
    devDeploymentFromFlag: config.devDeployment,
  });

  const region =
    devDeployment === "cloud"
      ? await selectRegionOrUseDefault(ctx, selectedTeam, "dev")
      : null;

  showSpinner("Creating new Convex project...");

  const deploymentToProvision =
    devDeployment === "cloud"
      ? {
          deploymentType: "dev" as const,
          region,
        }
      : null;

  let projectSlug;
  try {
    ({ projectSlug } = await createProject(ctx, {
      teamId: selectedTeam.id,
      projectName,
      deploymentToProvision,
    }));
  } catch (err) {
    logFailure("Unable to create project.");
    return await logAndHandleFetchError(ctx, err);
  }
  const teamSlug = selectedTeam.slug;
  const teamMessage = didChooseBetweenTeams
    ? " in team " + chalkStderr.bold(teamSlug)
    : "";
  logFinishedStep(
    `Created project ${chalkStderr.bold(
      projectSlug,
    )}${teamMessage}, manage it at ${chalkStderr.bold(
      projectDashboardUrl(teamSlug, projectSlug),
    )}`,
  );

  await doInitConvexFolder(ctx);
  const { configPath, projectConfig } = await readProjectConfig(ctx);
  const folder = functionsDir(configPath, projectConfig);
  await attemptSetupAiFiles({
    ctx,
    aiFilesConfig: projectConfig.aiFiles,
    convexDir: path.resolve(folder),
    projectDir: path.resolve(path.dirname(configPath)),
  });
  return { teamSlug, projectSlug, devDeployment };
}

async function selectExistingProject(
  ctx: Context,
  chosenConfiguration: ChosenConfiguration,
  config: {
    team?: string | undefined;
    project?: string | undefined;
    devDeployment?: "cloud" | "local" | undefined;
  },
): Promise<{
  teamSlug: string;
  projectSlug: string;
  devDeployment: "cloud" | "local";
}> {
  const {
    team: { id: teamId, slug: teamSlug },
    chosen,
  } = await validateOrSelectTeam(ctx, config.team, "Team:");

  const projectSlug = await validateOrSelectProject(
    ctx,
    config.project,
    teamId,
    "Configure project",
    "Project:",
  );
  if (projectSlug === null) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: "Run the command again to create a new project instead.",
    });
  }
  const { devDeployment } = await selectDevDeploymentType(ctx, {
    chosenConfiguration,
    newOrExisting: "existing",
    teamSlug,
    projectSlug,
    userHasChosenSomethingInteractively: chosen || !config.project,
    devDeploymentFromFlag: config.devDeployment,
  });

  logFinishedStep(`Reinitialized project ${chalkStderr.bold(projectSlug)}`);

  return { teamSlug, projectSlug, devDeployment };
}

async function askToConfigure(ctx: Context): Promise<"new" | "existing"> {
  if (!(await hasProjects(ctx))) {
    return "new";
  }
  return await promptOptions(ctx, {
    message: "What would you like to configure?",
    default: "new",
    choices: [
      { name: "create a new project", value: "new" },
      { name: "choose an existing project", value: "existing" },
    ],
  });
}

type DeploymentOptions =
  | {
      kind: "prod";
    }
  | { kind: "dev" }
  | {
      kind: "local";
      ports: {
        cloud: number | undefined;
        site: number | undefined;
      };
      backendVersion?: string | undefined;
      forceUpgrade: boolean;
    };

/**
 * This method assumes that the member has access to the selected project.
 */
async function ensureDeploymentProvisioned(
  ctx: Context,
  options: {
    teamSlug: string;
    projectSlug: string;
    deploymentOptions: DeploymentOptions;
  },
): Promise<DeploymentDetails> {
  switch (options.deploymentOptions.kind) {
    case "dev":
    case "prod": {
      const credentials =
        await fetchDeploymentCredentialsProvisioningDevOrProdMaybeThrows(
          ctx,
          {
            kind: "teamAndProjectSlugs",
            teamSlug: options.teamSlug,
            projectSlug: options.projectSlug,
          },
          options.deploymentOptions.kind,
        );
      return credentials;
    }
    case "local": {
      const credentials = await handleLocalDeployment(ctx, {
        teamSlug: options.teamSlug,
        projectSlug: options.projectSlug,
        ...options.deploymentOptions,
      });
      return credentials;
    }
    default:
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `Invalid deployment type: ${(options.deploymentOptions as any).kind}`,
        errForSentry: `Invalid deployment type: ${(options.deploymentOptions as any).kind}`,
      });
  }
}

export async function updateEnvAndConfigForDeploymentSelection(
  ctx: Context,
  options: {
    url: string;
    siteUrl?: string | null;
    deploymentName: string;
    teamSlug: string | null;
    projectSlug: string | null;
    deploymentType: DeploymentType;
  },
  existingValue: string | null,
) {
  const { configPath, projectConfig } = await readProjectConfig(ctx);

  const { wroteToGitIgnore, changedDeploymentEnvVar } =
    options.deploymentType !== "prod"
      ? await writeDeploymentEnvVar(
          ctx,
          options.deploymentType,
          {
            team: options.teamSlug,
            project: options.projectSlug,
            deploymentName: options.deploymentName,
          },
          existingValue,
        )
      : { wroteToGitIgnore: false, changedDeploymentEnvVar: false };
  await ensureConvexFunctionsDir(ctx, projectConfig);
  await finalizeConfiguration(ctx, {
    deploymentType: options.deploymentType,
    deploymentName: options.deploymentName,
    url: options.url,
    siteUrl: options.siteUrl,
    wroteToGitIgnore,
    changedDeploymentEnvVar,
    functionsPath: functionsDir(configPath, projectConfig),
  });
}

async function assertLocalOptionsAreDefault(
  ctx: Context,
  localOptions: ConfigureCmdOptions["localOptions"],
) {
  if (localOptions.ports.cloud !== undefined) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage:
        "`--local-cloud-port` can only be used when developing with a local deployment. " +
        "Use `npx convex deployment select local` to use a local deployment.",
    });
  }
  if (localOptions.ports.site !== undefined) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage:
        "`--local-site-port` can only be used when developing with a local deployment. " +
        "Use `npx convex deployment select local` to use a local deployment.",
    });
  }
  if (localOptions.backendVersion !== undefined) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage:
        "`--local-backend-version` can only be used when developing with a local deployment. " +
        "Use `npx convex deployment select local` to use a local deployment.",
    });
  }
  if (localOptions.dashboardVersion !== undefined) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage:
        "`--local-dashboard-version` can only be used when developing with a local deployment. " +
        "Use `npx convex deployment select local` to use a local deployment.",
    });
  }
  if (localOptions.forceUpgrade === true) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage:
        "`--local-force-upgrade` can only be used when developing with a local deployment. " +
        "Use `npx convex deployment select local` to use a local deployment.",
    });
  }
}
