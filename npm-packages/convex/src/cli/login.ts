import { Command, Option } from "@commander-js/extra-typings";
import * as dotenv from "dotenv";
import { BigBrainAuth, Context, oneoffContext } from "../bundler/context.js";
import { logFinishedStep, logMessage, logWarning } from "../bundler/log.js";
import {
  checkAuthorization,
  isAuthorizedHeader,
  performLogin,
} from "./lib/login.js";
import {
  loadProjectLocalConfig,
  loadUuidForAnonymousUser,
} from "./lib/localDeployment/filePaths.js";
import {
  handleLinkToProject,
  listLegacyAnonymousDeployments,
} from "./lib/localDeployment/anonymous.js";
import {
  DASHBOARD_HOST,
  deploymentDashboardUrlPage,
  teamDashboardUrl,
} from "./lib/dashboard.js";
import { promptSearch, promptYesNo } from "./lib/utils/prompts.js";
import {
  CONVEX_DEPLOY_KEY_ENV_VAR_NAME,
  CONVEX_DEPLOYMENT_TOKEN_ENV_VAR_NAME,
  ENV_VAR_FILE_PATH,
  validateOrSelectTeam,
} from "./lib/utils/utils.js";
import {
  selectProject,
  updateEnvAndConfigForDeploymentSelection,
} from "./configure.js";
import {
  getDeploymentSelection,
  shouldAllowAnonymousDevelopment,
} from "./lib/deploymentSelection.js";
import {
  isAnonymousDeployment,
  removeAnonymousPrefix,
} from "./lib/deployment.js";
import {
  readGlobalConfig,
  globalConfigPath,
} from "./lib/utils/globalConfig.js";
import { getTeamsForUser } from "./lib/api.js";

/**
 * The env file a deploy key came from, or null if it came from the shell.
 * `dotenv` doesn't overwrite variables that are already set, so a value from
 * the shell outranks these files -- match on the value to tell them apart.
 */
function deployKeyEnvFile(
  ctx: Context,
  envVarName: string,
  value: string,
): string | null {
  for (const file of [ENV_VAR_FILE_PATH, ".env"]) {
    if (!ctx.fs.exists(file)) {
      continue;
    }
    if (dotenv.parse(ctx.fs.readUtf8File(file))[envVarName] === value) {
      return file;
    }
  }
  return null;
}

/**
 * A project or deployment key in the environment outranks the account token for
 * commands run from this directory, and an expired one makes the CLI look
 * logged out, so report it separately from the account.
 */
async function reportDeployKeyInWorkingDirectory(
  ctx: Context,
  auth: BigBrainAuth | null,
) {
  // A preview deploy key is only used when there's no account token at all.
  if (
    auth === null ||
    auth.kind === "accessToken" ||
    auth.kind === "previewDeployKey"
  ) {
    return;
  }
  const key = auth.kind === "projectKey" ? auth.projectKey : auth.deploymentKey;
  const envVarName = process.env[CONVEX_DEPLOY_KEY_ENV_VAR_NAME]
    ? CONVEX_DEPLOY_KEY_ENV_VAR_NAME
    : CONVEX_DEPLOYMENT_TOKEN_ENV_VAR_NAME;
  const envFile = deployKeyEnvFile(ctx, envVarName, key);
  const source = envFile ?? "shell environment";
  // Big Brain only strips a `project:`/`team:` prefix itself, so a deployment
  // key authorizes only without its `dev:`/`prod:` prefix. Split on the first
  // `|` like the server does, so a secret containing one survives.
  const secret = key.slice(key.indexOf("|") + 1);
  if (await isAuthorizedHeader(ctx, `Bearer ${secret}`)) {
    logMessage(`Working Directory: Valid ${envVarName} in ${source}`);
    return;
  }
  logWarning(`Working Directory: Invalid ${envVarName} in ${source}`);
}

const loginStatus = new Command("status")
  .description("Check login status and list accessible teams")
  .allowExcessArguments(false)
  .action(async () => {
    const ctx = await oneoffContext({
      url: undefined,
      adminKey: undefined,
      envFile: undefined,
    });

    // `_updateBigBrainAuth` below replaces any deploy key in `ctx` with the
    // account token, so read the deploy key first.
    const auth = ctx.bigBrainAuth();

    const globalConfig = readGlobalConfig(ctx);
    if (globalConfig === null) {
      logMessage(`No Convex account token found in: ${globalConfigPath()}`);
      logMessage("Status: Not logged in");
      await reportDeployKeyInWorkingDirectory(ctx, auth);
      return;
    }
    logMessage(`Convex account token found in: ${globalConfigPath()}`);

    const accessToken = globalConfig.accessToken;
    if (!(await isAuthorizedHeader(ctx, `Bearer ${accessToken}`))) {
      logMessage("Status: Not logged in");
      await reportDeployKeyInWorkingDirectory(ctx, auth);
      return;
    }

    logMessage("Status: Logged in");
    // Describe the account, even if a deploy key outranks it for other commands.
    ctx._updateBigBrainAuth({
      kind: "accessToken",
      header: `Bearer ${accessToken}`,
      accessToken,
    });
    const teams = await getTeamsForUser(ctx);
    logMessage(
      `Teams: ${teams.length} team${teams.length === 1 ? "" : "s"} accessible`,
    );
    for (const team of teams) {
      logMessage(`  - ${team.name} (${team.slug})`);
    }
    await reportDeployKeyInWorkingDirectory(ctx, auth);
  });

export const login = new Command("login")
  .description("Login to Convex")
  .allowExcessArguments(false)
  .option(
    "--device-name <name>",
    "Provide a name for the device being authorized",
  )
  .option(
    "-f, --force",
    "Proceed with login even if a valid access token already exists for this device",
  )
  .option(
    "--no-open",
    "Don't automatically open the login link in the default browser",
  )
  .addOption(
    new Option(
      "--login-flow <mode>",
      `How to log in; defaults to guessing based on the environment.`,
    )
      .choices(["paste", "auto", "poll"] as const)
      .default("auto" as const),
  )
  .addOption(new Option("--link-deployments").hideHelp())
  // These options are hidden from the help/usage message, but allow overriding settings for testing.
  // Change the auth credentials with the auth provider
  .addOption(new Option("--override-auth-url <url>").hideHelp())
  .addOption(new Option("--override-auth-client <id>").hideHelp())
  .addOption(new Option("--override-auth-username <username>").hideHelp())
  .addOption(new Option("--override-auth-password <password>").hideHelp())
  // Skip the auth provider login and directly use this access token
  .addOption(new Option("--override-access-token <token>").hideHelp())
  // Automatically accept opt ins without prompting
  .addOption(new Option("--accept-opt-ins").hideHelp())
  // Dump the access token from the auth provider and skip authorization with Convex
  .addOption(new Option("--dump-access-token").hideHelp())
  // Hidden option for tests to check if the user is logged in.
  .addOption(new Option("--check-login").hideHelp())
  // Redirect to Vercel SSO integration URL
  .addOption(
    new Option(
      "--vercel",
      "Redirect to Vercel SSO integration for login",
    ).hideHelp(),
  )
  // Override the Vercel URL slug (defaults to 'convex')
  .addOption(new Option("--vercel-override <slug>").hideHelp())
  .addCommand(loginStatus)
  .addHelpCommand(false)
  .action(async (options, cmd: Command) => {
    const ctx = await oneoffContext({
      url: undefined,
      adminKey: undefined,
      envFile: undefined,
    });
    if (
      !options.force &&
      (await checkAuthorization(ctx, !!options.acceptOptIns))
    ) {
      logFinishedStep(
        "This device has previously been authorized and is ready for use with Convex.",
      );
      await handleLinkingDeployments(ctx, {
        interactive: !!options.linkDeployments,
      });
      return;
    }
    if (!options.force && options.checkLogin) {
      const isLoggedIn = await checkAuthorization(ctx, !!options.acceptOptIns);
      if (!isLoggedIn) {
        return ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          errForSentry: "You are not logged in.",
          printedMessage: "You are not logged in.",
        });
      }
    }
    if (!!options.overrideAuthUsername !== !!options.overrideAuthPassword) {
      cmd.error(
        "If overriding credentials, both username and password must be provided",
      );
    }

    const uuid = loadUuidForAnonymousUser(ctx);
    await performLogin(ctx, {
      ...options,
      anonymousId: uuid,
      vercel: options.vercel,
      vercelOverride: options.vercelOverride,
    });

    await handleLinkingDeployments(ctx, {
      interactive: !!options.linkDeployments,
    });
  });

async function handleLinkingDeployments(
  ctx: Context,
  args: {
    interactive: boolean;
  },
) {
  if (!shouldAllowAnonymousDevelopment()) {
    return;
  }

  // Check for project-local anonymous deployment first - this takes priority
  const projectLocal = loadProjectLocalConfig(ctx);
  if (
    projectLocal !== null &&
    isAnonymousDeployment(projectLocal.deploymentName)
  ) {
    const shouldLink = await promptYesNo(ctx, {
      message: `Would you like to link your existing deployment to your account? ("${projectLocal.deploymentName}")`,
      default: true,
    });
    if (!shouldLink) {
      logMessage(
        "Not linking your existing deployment. If you want to link it later, run `npx convex login --link-deployments`.",
      );
      logMessage(
        `Visit ${DASHBOARD_HOST} or run \`npx convex dev\` to get started with your new account.`,
      );
      return;
    }

    const { dashboardUrl } = await linkSingleDeployment(
      ctx,
      projectLocal.deploymentName,
      projectLocal.deploymentName,
    );
    logFinishedStep(`Visit ${dashboardUrl} to get started.`);
    return;
  }

  // No project-local deployment - check for legacy deployments
  const legacyDeployments = listLegacyAnonymousDeployments(ctx);
  if (legacyDeployments.length === 0) {
    if (args.interactive) {
      logMessage(
        "It doesn't look like you have any deployments to link. You can run `npx convex dev` to set up a new project or select an existing one.",
      );
    }
    return;
  }

  // Get the currently configured deployment (if any) for env var updates
  const deploymentSelection = await getDeploymentSelection(ctx, {
    url: undefined,
    adminKey: undefined,
    envFile: undefined,
  });
  const configuredDeployment =
    deploymentSelection.kind === "anonymous"
      ? deploymentSelection.deploymentName
      : null;

  if (!args.interactive) {
    // Non-interactive: link all legacy deployments automatically
    const message = getMessage(legacyDeployments.map((d) => d.deploymentName));
    const createProjects = await promptYesNo(ctx, {
      message,
      default: true,
    });
    if (!createProjects) {
      logMessage(
        "Not linking your existing deployments. If you want to link them later, run `npx convex login --link-deployments`.",
      );
      logMessage(
        `Visit ${DASHBOARD_HOST} or run \`npx convex dev\` to get started with your new account.`,
      );
      return;
    }

    const {
      team: { slug: teamSlug },
    } = await validateOrSelectTeam(
      ctx,
      undefined,
      "Choose a team for your deployments:",
    );
    let dashboardUrl = teamDashboardUrl(teamSlug);
    for (const deployment of legacyDeployments) {
      const result = await linkSingleDeployment(
        ctx,
        deployment.deploymentName,
        configuredDeployment,
        { teamSlug, projectSlug: null },
      );
      if (deployment.deploymentName === configuredDeployment) {
        dashboardUrl = result.dashboardUrl;
      }
    }
    logFinishedStep(
      `Successfully linked your deployments! Visit ${dashboardUrl} to get started.`,
    );
    return;
  }

  // Interactive mode: let user choose which legacy deployments to link
  while (true) {
    const currentLegacyDeployments = listLegacyAnonymousDeployments(ctx);
    if (currentLegacyDeployments.length === 0) {
      logMessage("All deployments have been linked.");
      break;
    }
    logMessage(
      getDeploymentListMessage(
        currentLegacyDeployments.map((d) => d.deploymentName),
      ),
    );
    const deploymentToLink = await promptSearch(ctx, {
      message: "Which deployment would you like to link to your account?",
      choices: currentLegacyDeployments.map((d) => ({
        name: d.deploymentName,
        value: d.deploymentName,
      })),
    });

    await linkSingleDeployment(ctx, deploymentToLink, configuredDeployment);

    const shouldContinue = await promptYesNo(ctx, {
      message: "Would you like to link another deployment?",
      default: true,
    });
    if (!shouldContinue) {
      break;
    }
  }
}

/**
 * Link a single deployment to a project, prompting for team and project selection.
 * Updates env vars if this is the currently configured deployment.
 */
async function linkSingleDeployment(
  ctx: Context,
  deploymentName: string,
  configuredDeployment: string | null,
  options?: {
    teamSlug?: string;
    projectSlug?: string | null;
  },
): Promise<{ dashboardUrl: string }> {
  const { team } = await validateOrSelectTeam(
    ctx,
    options?.teamSlug,
    "Choose a team for your deployment:",
  );

  const projectSlug =
    options?.projectSlug ??
    (
      await selectProject(ctx, "ask", {
        team: team.slug,
        devDeployment: "local",
        defaultProjectName: removeAnonymousPrefix(deploymentName),
      })
    ).projectSlug;

  const linkedDeployment = await handleLinkToProject(ctx, {
    deploymentName,
    teamSlug: team.slug,
    teamId: team.id,
    projectSlug,
  });

  if (deploymentName === configuredDeployment) {
    await updateEnvAndConfigForDeploymentSelection(
      ctx,
      {
        url: linkedDeployment.deploymentUrl,
        deploymentName: linkedDeployment.deploymentName,
        teamSlug: team.slug,
        projectSlug: linkedDeployment.projectSlug,
        deploymentType: "local",
      },
      configuredDeployment,
    );
  }

  return {
    dashboardUrl: deploymentDashboardUrlPage(
      linkedDeployment.deploymentName,
      "",
    ),
  };
}

function getDeploymentListMessage(anonymousDeploymentNames: string[]) {
  let message = `You have ${anonymousDeploymentNames.length} existing deployments.`;
  message += `\n\nDeployments:`;
  for (const deploymentName of anonymousDeploymentNames) {
    message += `\n- ${deploymentName}`;
  }
  return message;
}

function getMessage(anonymousDeploymentNames: string[]) {
  if (anonymousDeploymentNames.length === 1) {
    return `Would you like to link your existing deployment to your account? ("${anonymousDeploymentNames[0]}")`;
  }
  let message = `You have ${anonymousDeploymentNames.length} existing deployments. Would you like to link them to your account?`;
  message += `\n\nDeployments:`;
  for (const deploymentName of anonymousDeploymentNames) {
    message += `\n- ${deploymentName}`;
  }
  message += `\n\nYou can alternatively run \`npx convex login --link-deployments\` to interactively choose which deployments to add.`;
  return message;
}
