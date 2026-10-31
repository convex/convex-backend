import { Command, Option } from "@commander-js/extra-typings";
import { Context, oneoffContext } from "../bundler/context.js";
import { logFinishedStep, logMessage, logWarning } from "../bundler/log.js";
import {
  checkAuthorization,
  identifyLegacyAccounts,
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
import {
  promptOptions,
  promptSearch,
  promptYesNo,
} from "./lib/utils/prompts.js";
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
  ResolvedAccessToken,
  findInstanceForDirectory,
  formatPathForPrinting,
  readGlobalConfig,
  globalConfigPath,
  resolveAccessToken,
  accessTokenAuth,
  UNIDENTIFIED_ACCOUNT,
  isMemberId,
  updateGlobalConfig,
  withAccount,
} from "./lib/utils/globalConfig.js";
import {
  accountLabel,
  accountLabelSentence,
  bindDirectoryToAccount,
  deploymentAccessWarning,
  formatSavedAccount,
  listSavedAccounts,
  logAccountsInEffect,
} from "./account.js";
import { chalkStderr } from "chalk";
import { getTeamsForUser } from "./lib/api.js";
import { printDeployKeyBanner } from "./lib/deployKeyWarning.js";

/**
 * The account token commands in this directory would use if no deploy key
 * were set. Unlike `ctx.bigBrainAuth()`, a deploy key doesn't shadow it, so
 * checking it tells whether this device is logged in.
 */
function accountTokenForDirectory(ctx: Context): ResolvedAccessToken | null {
  if (process.env.CONVEX_OVERRIDE_ACCESS_TOKEN) {
    return {
      accessToken: process.env.CONVEX_OVERRIDE_ACCESS_TOKEN,
      accountId: null,
      source: "legacy",
    };
  }
  const config = readGlobalConfig(ctx);
  return config !== null ? resolveAccessToken(config, process.cwd()) : null;
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
    await printDeployKeyBanner(ctx);

    const globalConfig = readGlobalConfig(ctx);
    const resolved =
      globalConfig !== null
        ? resolveAccessToken(globalConfig, process.cwd())
        : null;
    if (resolved === null) {
      logMessage(`No Convex account token found in: ${globalConfigPath()}`);
      logMessage("Status: Not logged in");
      return;
    }
    logMessage(`Convex account token found in: ${globalConfigPath()}`);
    if (resolved.accountId !== null && globalConfig !== null) {
      logAccountsInEffect(ctx, globalConfig);
    }

    const accessToken = resolved.accessToken;
    if (!(await isAuthorizedHeader(ctx, `Bearer ${accessToken}`))) {
      logMessage("Status: Not logged in");
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
  .option(
    "--account [member-id]",
    "List the Convex accounts logged in on this device, then pick one for this directory or log in to another. Accounts are saved by Convex member id. Existing accounts stay logged in.",
  )
  .option(
    "--description <text>",
    "A note about the account, shown by `npx convex account list`",
  )
  .option(
    "-y, --yes",
    "Accept the default answer to every prompt. With --account and no name, logs in to a new account.",
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
      typeof options.account === "string" &&
      !isMemberId(options.account) &&
      options.account !== UNIDENTIFIED_ACCOUNT
    ) {
      cmd.error(
        `--account takes a Convex member id, not "${options.account}". To see the saved accounts, run \`npx convex account list\`.`,
      );
    }
    if (!!options.overrideAuthUsername !== !!options.overrideAuthPassword) {
      cmd.error(
        "If overriding credentials, both username and password must be provided",
      );
    }

    if (options.account !== undefined) {
      await loginToNamedAccount(ctx, {
        ...options,
        account: options.account === true ? undefined : options.account,
      });
      await handleLinkingDeployments(ctx, {
        interactive: !!options.linkDeployments,
      });
      return;
    }

    await printDeployKeyBanner(ctx);

    // Check the account token itself: a deploy key outranks it in `ctx`, and
    // checking the key instead made every login look logged out.
    const accountToken = accountTokenForDirectory(ctx);
    ctx._updateBigBrainAuth(
      accountToken === null ? null : accessTokenAuth(accountToken.accessToken),
    );
    const isLoggedIn = await checkAuthorization(ctx, !!options.acceptOptIns);
    if (!options.force && isLoggedIn) {
      logFinishedStep(
        "This device has previously been authorized and is ready for use with Convex.",
      );
      const config = readGlobalConfig(ctx);
      if (accountToken?.accountId && config !== null) {
        logAccountsInEffect(ctx, config);
      }
      logMessage(
        `To log in to another Convex account as well, run ${chalkStderr.bold("npx convex login --account")}.`,
      );
      await handleLinkingDeployments(ctx, {
        interactive: !!options.linkDeployments,
      });
      return;
    }
    if (!options.force && options.checkLogin) {
      return ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        errForSentry: "You are not logged in.",
        printedMessage: "You are not logged in.",
      });
    }

    const uuid = loadUuidForAnonymousUser(ctx);
    await performLogin(ctx, {
      ...options,
      account: undefined,
      acceptDefaults: !!options.yes,
      anonymousId: uuid,
      vercel: options.vercel,
      vercelOverride: options.vercelOverride,
    });

    await handleLinkingDeployments(ctx, {
      interactive: !!options.linkDeployments,
    });
  });

/**
 * `npx convex login --account [name]`: show the accounts already logged in,
 * then use one of them or log in to a new one, keeping the others.
 */
async function loginToNamedAccount(
  ctx: Context,
  options: Parameters<typeof performLogin>[1] & {
    account: string | undefined;
    description?: string | undefined;
    force?: boolean | undefined;
    yes?: boolean | undefined;
  },
) {
  await printDeployKeyBanner(ctx);
  const acceptDefaults = !!options.yes;
  await identifyLegacyAccounts(ctx);

  const config = readGlobalConfig(ctx);
  const saved = await listSavedAccounts(ctx, config);
  const current =
    config !== null ? resolveAccessToken(config, process.cwd()) : null;
  const currentId =
    current?.accountId ?? (current !== null ? UNIDENTIFIED_ACCOUNT : null);
  // Only say what bears on this login: the account asked for, or the
  // accounts to pick from when there's no terminal to show the picker.
  if (options.account !== undefined) {
    const entry = saved.find((account) => account.id === options.account);
    if (entry === undefined) {
      logMessage(`Member ${options.account} isn't saved on this device yet.`);
    } else if (!entry.isLoggedIn) {
      logMessage(
        `${accountLabelSentence(entry.id, entry.account)}: token revoked or expired, logging in again.`,
      );
    }
  }

  let accountId = options.account;
  let chosenFromList = false;
  if (accountId === undefined && !acceptDefaults && saved.length > 0) {
    if (!process.stdin.isTTY) {
      logMessage("Convex accounts on this device (* used in this directory):");
      for (const account of saved) {
        logMessage(
          `  ${account.id === currentId ? "*" : " "} ${formatSavedAccount(account)}`,
        );
      }
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `Pass ${chalkStderr.bold("--account <member-id>")} to use a saved account, or ${chalkStderr.bold("--account --yes")} to log in to a new one.`,
      });
    }
    const choice = await promptOptions<string | null>(ctx, {
      message: `Which Convex account should ${formatPathForPrinting(process.cwd())} use?`,
      choices: [
        ...saved.map((account) => ({
          name: formatSavedAccount(account),
          value: account.id,
        })),
        { name: "Log in to another account", value: null },
      ],
      ...(currentId !== null ? { default: currentId } : {}),
    });
    if (choice !== null) {
      accountId = choice;
      chosenFromList = true;
    }
  }

  const existing = saved.find((account) => account.id === accountId);
  // A working token saved before its member was recorded still needs one
  // browser login to find out whose it is.
  const needsIdentifying =
    accountId !== undefined && existing?.isLoggedIn && !isMemberId(accountId);
  if (needsIdentifying) {
    logMessage(
      `${accountLabelSentence(accountId!, existing!.account)} works, but its member isn't recorded. Log in once to record it.`,
    );
  }
  if (
    accountId !== undefined &&
    !options.force &&
    existing?.isLoggedIn &&
    !needsIdentifying
  ) {
    const id = accountId;
    // Converts a single-token config to named accounts, keeping the token.
    await updateGlobalConfig(ctx, (current) =>
      withAccount(current, id, {
        accessToken: existing.account.accessToken,
        description: options.description,
      }),
    );
    logFinishedStep(
      `Already logged in as ${accountLabel(accountId, existing.account)}.`,
    );
  } else {
    const savedAs = await performLogin(ctx, {
      ...options,
      anonymousId: loadUuidForAnonymousUser(ctx),
      account: accountId,
      accountDescription: options.description,
      nameNewAccount: accountId === undefined,
      acceptDefaults,
    });
    accountId = savedAs ?? accountId;
  }
  if (accountId === undefined) {
    return;
  }

  if (chosenFromList) {
    await bindDirectoryToAccount(ctx, accountId, { acceptDefaults });
  } else {
    await offerToBindDirectory(ctx, accountId, acceptDefaults);
  }
}

/**
 * After logging in to an account, offer to use it for this directory. The
 * default is yes only when the directory looks like a project, isn't bound
 * to another account, and the account can reach the deployment it's
 * configured for.
 */
async function offerToBindDirectory(
  ctx: Context,
  accountId: string,
  acceptDefaults: boolean,
) {
  const config = readGlobalConfig(ctx);
  const account = config?.accounts?.[accountId];
  const current =
    config !== null ? findInstanceForDirectory(config, process.cwd()) : null;
  if (current?.account === accountId || account === undefined) {
    return;
  }
  const label = accountLabel(accountId, account);
  const accessWarning = await deploymentAccessWarning(ctx, accountId, account);
  if (accessWarning !== null) {
    logWarning(chalkStderr.yellow(accessWarning));
  }
  const suggestBinding =
    current === null &&
    accessWarning === null &&
    (ctx.fs.exists("convex.json") || ctx.fs.exists("package.json"));
  const bind =
    acceptDefaults || !process.stdin.isTTY
      ? suggestBinding
      : await promptYesNo(ctx, {
          message:
            current === null
              ? `Use ${label} for ${formatPathForPrinting(process.cwd())}?`
              : `${formatPathForPrinting(current.directory)} uses ${accountLabel(current.account, config?.accounts?.[current.account])}. Switch it to ${label}?`,
          default: suggestBinding,
        });
  if (bind) {
    // Any access problem was shown above and the user chose to go ahead.
    await bindDirectoryToAccount(ctx, accountId, { force: true });
  } else {
    logMessage(
      `To use ${label} for a project, run ${chalkStderr.bold(`npx convex account use ${accountId}`)} in its directory.`,
    );
  }
}

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
