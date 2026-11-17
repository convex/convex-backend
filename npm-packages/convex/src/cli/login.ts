import { Command, Option } from "@commander-js/extra-typings";
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
import { validateOrSelectTeam } from "./lib/utils/utils.js";
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
  globalConfigPath,
  readGlobalConfig,
} from "./lib/utils/globalConfig.js";
import { getTeamsForUser } from "./lib/api.js";
import {
  getAccountSelection,
  initializeDefaultAccount,
  legacyAuthForFirstAccount,
  accountConfigPath,
  accountNotLoggedInMessage,
  readAccountAuth,
  writeAccountCredentials,
} from "./lib/accounts.js";
import {
  getDeployKeySource,
  loadProjectEnvironmentVariables,
} from "./lib/envvars.js";
import { bindAccountToDirectory } from "./lib/accountManagement.js";
import { warnIfDeployKeyOverridesLogin } from "./lib/deployKeyWarning.js";

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
  const { envVarName, source } = getDeployKeySource(key);
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

function createLoginStatusCommand() {
  return new Command("status")
    .description(
      "Check login status and list accessible teams. Exits with code 1 when not logged in.",
    )
    .allowExcessArguments(false)
    .action(async () => {
      const ctx = await oneoffContext({
        url: undefined,
        adminKey: undefined,
        envFile: undefined,
      });

      // Keep the directory's deploy key for separate reporting after selecting
      // the account whose teams will be listed.
      const auth = ctx.bigBrainAuth();
      let accountAuth = auth?.kind === "accessToken" ? auth : null;
      if (accountAuth === null) {
        const account = await getAccountSelection(ctx);
        if (account !== null) {
          accountAuth = await readAccountAuth(ctx, account, true);
          if (accountAuth === null) {
            logMessage(accountNotLoggedInMessage(account));
            logMessage("Status: Not logged in");
            await reportDeployKeyInWorkingDirectory(ctx, auth);
            return await ctx.crash({
              exitCode: 1,
              errorType: "fatal",
              printedMessage: null,
            });
          }
        } else {
          const globalConfig = readGlobalConfig(ctx);
          if (globalConfig !== null) {
            accountAuth = {
              kind: "accessToken",
              accessToken: globalConfig.accessToken,
              header: `Bearer ${globalConfig.accessToken}`,
              account: null,
            };
          }
        }
      }
      if (accountAuth === null) {
        logMessage(`No Convex account token found in: ${globalConfigPath()}`);
        logMessage("Status: Not logged in");
        await reportDeployKeyInWorkingDirectory(ctx, auth);
        return await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage: null,
        });
      }
      warnIfDeployKeyOverridesLogin(ctx, true);
      if (accountAuth.account) {
        logMessage(`Account: ${accountAuth.account.name}`);
        logMessage(`Selected by: ${accountAuth.account.source}`);
        logMessage(
          `Convex account token found in: ${accountConfigPath(accountAuth.account.name)}`,
        );
      } else if (process.env.CONVEX_OVERRIDE_ACCESS_TOKEN) {
        logMessage("Using CONVEX_OVERRIDE_ACCESS_TOKEN from the environment");
      } else {
        logMessage(`Convex account token found in: ${globalConfigPath()}`);
      }

      if (!(await isAuthorizedHeader(ctx, accountAuth.header))) {
        logMessage("Status: Not logged in");
        await reportDeployKeyInWorkingDirectory(ctx, auth);
        return await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage: null,
        });
      }

      logMessage("Status: Logged in");
      ctx._updateBigBrainAuth(accountAuth);
      const teams = await getTeamsForUser(ctx);
      logMessage(
        `Teams: ${teams.length} team${teams.length === 1 ? "" : "s"} accessible`,
      );
      for (const team of teams) {
        logMessage(`  - ${team.name} (${team.slug})`);
      }
      await reportDeployKeyInWorkingDirectory(ctx, auth);
    });
}

export function createLoginCommand() {
  return (
    new Command("login")
      .description("Login to Convex")
      .allowExcessArguments(false)
      .option("--account <name>", "Log in to a named account")
      .option(
        "--select",
        "Select this account for this directory and its subdirectories",
      )
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
      .addCommand(createLoginStatusCommand())
      .addHelpCommand(false)
      .action(async (options, cmd: Command) => {
        const ctx = await oneoffContext({
          url: undefined,
          adminKey: undefined,
          envFile: undefined,
          skipAuth: true,
        });
        const account = await getAccountSelection(ctx, {
          name: options.account,
        });
        if (options.select && account === null) {
          cmd.error("Pass --account <name> with --select to name this login.");
        }
        loadProjectEnvironmentVariables();
        let namingLegacyAccount = false;
        if (account !== null) {
          let auth = options.force
            ? null
            : await readAccountAuth(ctx, account, true);
          if (
            auth === null &&
            !options.force &&
            !options.overrideAccessToken &&
            !options.overrideAuthUsername &&
            !process.env.CONVEX_OVERRIDE_ACCESS_TOKEN
          ) {
            auth = await legacyAuthForFirstAccount(ctx, account);
            namingLegacyAccount = auth !== null;
          }
          ctx._updateBigBrainAuth(auth);
        } else {
          const accessToken =
            process.env.CONVEX_OVERRIDE_ACCESS_TOKEN ||
            readGlobalConfig(ctx)?.accessToken;
          ctx._updateBigBrainAuth(
            accessToken
              ? {
                  kind: "accessToken",
                  accessToken,
                  header: `Bearer ${accessToken}`,
                  account: null,
                }
              : null,
          );
        }
        warnIfDeployKeyOverridesLogin(ctx, ctx.bigBrainAuth() !== null);
        const alreadyLoggedIn =
          !options.force &&
          (await checkAuthorization(ctx, !!options.acceptOptIns));
        if (alreadyLoggedIn) {
          const auth = ctx.bigBrainAuth();
          if (
            namingLegacyAccount &&
            account !== null &&
            auth?.kind === "accessToken"
          ) {
            await writeAccountCredentials(
              ctx,
              account.name,
              auth.accessToken,
              {},
              null,
            );
          }
          logFinishedStep(
            account !== null
              ? namingLegacyAccount
                ? `Saved account "${account.name}" using your existing login.`
                : `Account "${account.name}" is already logged in.`
              : "This device has previously been authorized and is ready for use with Convex.",
          );
        } else {
          if (!options.force && options.checkLogin) {
            const isLoggedIn = await checkAuthorization(
              ctx,
              !!options.acceptOptIns,
            );
            if (!isLoggedIn) {
              return ctx.crash({
                exitCode: 1,
                errorType: "fatal",
                errForSentry: "You are not logged in.",
                printedMessage: "You are not logged in.",
              });
            }
          }
          if (
            !!options.overrideAuthUsername !== !!options.overrideAuthPassword
          ) {
            cmd.error(
              "If overriding credentials, both username and password must be provided",
            );
          }

          const uuid = loadUuidForAnonymousUser(ctx);
          await performLogin(ctx, {
            ...options,
            account,
            anonymousId: uuid,
            vercel: options.vercel,
            vercelOverride: options.vercelOverride,
          });
        }

        const auth = ctx.bigBrainAuth();
        if (account !== null && auth?.kind === "accessToken") {
          await initializeDefaultAccount(
            ctx,
            account,
            namingLegacyAccount,
            !!options.select,
          );
        }
        if (options.select && account !== null) {
          if (!(await bindAccountToDirectory(ctx, account.name))) {
            return await ctx.crash({
              exitCode: 1,
              errorType: "fatal",
              printedMessage: `Logged in as "${account.name}", but the directory selection could not be changed.`,
            });
          }
        }
        if (!alreadyLoggedIn) warnIfDeployKeyOverridesLogin(ctx, true);

        await handleLinkingDeployments(ctx, {
          interactive: !!options.linkDeployments,
        });
      })
  );
}

export const login = createLoginCommand();

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
