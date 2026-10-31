import { Command } from "@commander-js/extra-typings";
import { chalkStderr } from "chalk";
import { Context, oneoffContext } from "../bundler/context.js";
import {
  logFinishedStep,
  logMessage,
  logOutput,
  logWarning,
} from "../bundler/log.js";
import {
  AccountConfig,
  GlobalConfig,
  SETTINGS,
  withSetting,
  accessTokenAuth,
  savedAccounts,
  defaultAccountId,
  findInstanceForDirectory,
  formatPathForPrinting,
  readGlobalConfig,
  isMemberId,
  resolveAccessToken,
  updateGlobalConfig,
  withAccount,
  withInstance,
  withDefaultAccount,
} from "./lib/utils/globalConfig.js";
import {
  convexApi,
  identifyLegacyAccounts,
  isAuthorizedHeader,
  performLogin,
} from "./lib/login.js";
import type { ProjectSelection } from "./lib/deploymentSelection.js";
import { loadUuidForAnonymousUser } from "./lib/localDeployment/filePaths.js";
import { printDeployKeyBanner } from "./lib/deployKeyWarning.js";
import {
  getDeploymentTypeFromConfiguredDeployment,
  isAnonymousDeployment,
  stripDeploymentTypePrefix,
} from "./lib/deployment.js";
import {
  CONVEX_DEPLOYMENT_ENV_VAR_NAME,
  ENV_VAR_FILE_PATH,
} from "./lib/utils/utils.js";
import { promptOptions, promptYesNo } from "./lib/utils/prompts.js";

/** How an account is referred to in messages. */
export function accountLabel(id: string, account?: AccountConfig) {
  if (!isMemberId(id)) {
    return "the unidentified token (saved before its member was recorded)";
  }
  return `member ${id}${account?.email !== undefined ? ` <${account.email}>` : ""}`;
}

/** `accountLabel`, capitalized to start a sentence. */
export function accountLabelSentence(id: string, account?: AccountConfig) {
  const label = accountLabel(id, account);
  return label.charAt(0).toUpperCase() + label.slice(1);
}

export function formatAccountName(id: string, account: AccountConfig) {
  return `${accountLabel(id, account)}${
    account.description ? ` - ${account.description}` : ""
  }${account.isDefault ? " (default)" : ""}`;
}

export type SavedAccount = {
  id: string;
  account: AccountConfig;
  isLoggedIn: boolean;
};

/**
 * The accounts saved on this device and whether each token works right now.
 * A token revoked later only shows up the next time it's checked, so commands
 * can still fail after this. A single-token config shows up as the
 * unidentified token, the key it gets when converted.
 */
export async function listSavedAccounts(
  ctx: Context,
  config: GlobalConfig | null,
): Promise<SavedAccount[]> {
  return await Promise.all(
    savedAccounts(config).map(async ([id, account]) => ({
      id,
      account,
      isLoggedIn: await isAuthorizedHeader(
        ctx,
        `Bearer ${account.accessToken}`,
      ),
    })),
  );
}

export function formatSavedAccount(saved: SavedAccount) {
  return `${formatAccountName(saved.id, saved.account)}${
    saved.isLoggedIn
      ? chalkStderr.green(" - logged in")
      : chalkStderr.yellow(" - token revoked or expired, log in again")
  }`;
}

/**
 * Which account is the default, and which one this directory uses, for the
 * top of account listings.
 */
export function logAccountsInEffect(ctx: Context, config: GlobalConfig) {
  const defaultId = defaultAccountId(config);
  if (defaultId !== null) {
    logMessage(
      `Default account: ${chalkStderr.bold(accountLabel(defaultId, config.accounts?.[defaultId]))}. Directories not bound to an account, and older Convex CLI versions, use it.`,
    );
  }
  const current = resolveAccessToken(config, process.cwd());
  const instance = findInstanceForDirectory(config, process.cwd());
  if (current?.accountId && instance !== null) {
    logMessage(
      `This directory: ${chalkStderr.bold(accountLabel(current.accountId, config.accounts?.[current.accountId]))}, bound at ${formatPathForPrinting(instance.directory)}.`,
    );
  } else if (current !== null) {
    logMessage("This directory: the default account.");
  }
}

/**
 * The cloud deployment this directory's `CONVEX_DEPLOYMENT` points to, which
 * an account must be able to reach for commands here to work. Null for local
 * and anonymous deployments, or when it isn't set.
 */
export function configuredCloudDeployment(): string | null {
  const raw = process.env[CONVEX_DEPLOYMENT_ENV_VAR_NAME]?.trim();
  if (!raw || getDeploymentTypeFromConfiguredDeployment(raw) === "local") {
    return null;
  }
  const name = stripDeploymentTypePrefix(raw);
  return isAnonymousDeployment(name) ? null : name;
}

/**
 * Whether an account's token can see a deployment. "unknown" when the token
 * itself no longer works.
 */
export async function deploymentAccess(
  ctx: Context,
  accessToken: string,
  deploymentName: string,
): Promise<"yes" | "no" | "unknown"> {
  if (!(await isAuthorizedHeader(ctx, `Bearer ${accessToken}`))) {
    return "unknown";
  }
  try {
    const resp = await convexApi(
      accessToken,
      `/v1/deployments/${encodeURIComponent(deploymentName)}`,
    );
    if (resp.status === 200) {
      return "yes";
    }
    return resp.status === 401 || resp.status === 403 || resp.status === 404
      ? "no"
      : "unknown";
  } catch {
    return "unknown";
  }
}

/**
 * Check an account against the deployment this directory is configured for.
 * Returns a warning to show, or null if the account can reach it or there's
 * nothing to check.
 */
export async function deploymentAccessWarning(
  ctx: Context,
  accountId: string,
  account: AccountConfig,
): Promise<string | null> {
  const deployment = configuredCloudDeployment();
  if (deployment === null) {
    return null;
  }
  const access = await deploymentAccess(ctx, account.accessToken, deployment);
  if (access === "yes") {
    return null;
  }
  const label = accountLabel(accountId, account);
  return access === "no"
    ? `${accountLabelSentence(accountId, account)} can't access the deployment "${deployment}" that ${CONVEX_DEPLOYMENT_ENV_VAR_NAME} points to here, so commands in this directory would fail with it.`
    : `Couldn't check whether ${label} can access the deployment "${deployment}" that ${CONVEX_DEPLOYMENT_ENV_VAR_NAME} points to here: its token doesn't work. Log in to it again first.`;
}

/**
 * The team and project the CLI noted next to `CONVEX_DEPLOYMENT` in
 * .env.local (`# team: cwc, project: cwc-website`), if present.
 */
function configuredTeamAndProject(
  ctx: Context,
): { team: string; project: string } | null {
  if (!ctx.fs.exists(ENV_VAR_FILE_PATH)) {
    return null;
  }
  const line = ctx.fs
    .readUtf8File(ENV_VAR_FILE_PATH)
    .split("\n")
    .find((l) =>
      l.trimStart().startsWith(`${CONVEX_DEPLOYMENT_ENV_VAR_NAME}=`),
    );
  const match = line?.match(/#\s*team:\s*([^,\s]+),\s*project:\s*(\S+)/);
  return match ? { team: match[1], project: match[2] } : null;
}

function describeProjectSelection(
  ctx: Context,
  target: ProjectSelection,
): string {
  switch (target.kind) {
    case "deploymentName": {
      const noted = configuredTeamAndProject(ctx);
      return `the deployment "${target.deploymentName}"${
        noted ? ` (team: ${noted.team}, project: ${noted.project})` : ""
      }`;
    }
    case "teamAndProjectSlugs":
      return `the project "${target.projectSlug}" in the team "${target.teamSlug}"`;
    default:
      return "the configured project";
  }
}

/**
 * `npx convex dev` found a project configured for this directory that the
 * account it's using can't access, usually because another Convex account
 * owns it. Explain that, then let the user switch to a saved account that can
 * reach it, log in to another account, or set up a different project.
 *
 * Returns "retry" once `ctx` uses a different account, or "chooseProject" to
 * continue with the usual project setup, which rewrites CONVEX_DEPLOYMENT.
 */
export async function handleNoAccessToConfiguredProject(
  ctx: Context,
  target: ProjectSelection,
): Promise<"retry" | "chooseProject"> {
  const config = readGlobalConfig(ctx);
  const current =
    config !== null ? resolveAccessToken(config, process.cwd()) : null;
  const currentLabel =
    current?.accountId && isMemberId(current.accountId)
      ? accountLabel(current.accountId, config?.accounts?.[current.accountId])
      : "the Convex account you're logged in with";
  const what = describeProjectSelection(ctx, target);
  logWarning(
    chalkStderr.yellow(
      [
        `This directory is set up for ${what} by ${CONVEX_DEPLOYMENT_ENV_VAR_NAME} in ${ENV_VAR_FILE_PATH},`,
        `but ${currentLabel} can't access it. It probably belongs to another Convex account.`,
      ].join("\n"),
    ),
  );

  // Saved accounts that can reach the deployment, other than the current one.
  const others = Object.entries(config?.accounts ?? {}).filter(
    ([id]) => id !== current?.accountId,
  );
  const withAccess =
    target.kind === "deploymentName"
      ? (
          await Promise.all(
            others.map(async ([id, account]) => ({
              id,
              account,
              access: await deploymentAccess(
                ctx,
                account.accessToken,
                target.deploymentName,
              ),
            })),
          )
        ).filter((entry) => entry.access === "yes")
      : [];

  if (!process.stdin.isTTY) {
    const useSaved = withAccess
      .map((entry) => `\`npx convex account use ${entry.id}\``)
      .join(" or ");
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: [
        useSaved
          ? `A saved account can access it: run ${useSaved}.`
          : `Log in to the account that owns it with \`npx convex login --account\`.`,
        `To set up a different project here instead, run \`npx convex dev --configure\` (this replaces ${CONVEX_DEPLOYMENT_ENV_VAR_NAME}).`,
      ].join("\n"),
    });
  }

  type Choice =
    { kind: "use"; id: string } | { kind: "login" } | { kind: "chooseProject" };
  const choice = await promptOptions<Choice>(ctx, {
    message: "What would you like to do?",
    choices: [
      ...withAccess.map((entry) => ({
        name: `Use ${accountLabel(entry.id, entry.account)}, which can access it`,
        value: { kind: "use", id: entry.id } as Choice,
      })),
      {
        name: "Log in to another Convex account",
        value: { kind: "login" },
      },
      {
        name: `Set up a different project here (replaces ${CONVEX_DEPLOYMENT_ENV_VAR_NAME} in ${ENV_VAR_FILE_PATH})`,
        value: { kind: "chooseProject" },
      },
    ],
  });

  if (choice.kind === "chooseProject") {
    return "chooseProject";
  }
  if (choice.kind === "use") {
    const account = config!.accounts![choice.id];
    await bindDirectoryToAccount(ctx, choice.id, { force: true });
    ctx._updateBigBrainAuth(accessTokenAuth(account.accessToken));
    return "retry";
  }
  // `performLogin` points `ctx` at the new account's token.
  const savedAs = await performLogin(ctx, {
    anonymousId: loadUuidForAnonymousUser(ctx),
    nameNewAccount: true,
  });
  if (savedAs !== null) {
    await bindDirectoryToAccount(ctx, savedAs, { force: true });
  }
  return "retry";
}

/** The context for `npx convex account` commands, which take no deployment flags. */
function accountCommandContext() {
  return oneoffContext({
    url: undefined,
    adminKey: undefined,
    envFile: undefined,
  });
}

/** The config and the saved account `accountId`, or a crash explaining why not. */
async function readAccountOrCrash(
  ctx: Context,
  accountId: string,
): Promise<{ config: GlobalConfig; account: AccountConfig }> {
  const config = await readAccountsOrCrash(ctx);
  await ensureAccountExists(ctx, config, accountId);
  return { config, account: config.accounts![accountId] };
}

async function readAccountsOrCrash(ctx: Context): Promise<GlobalConfig> {
  const config = readGlobalConfig(ctx);
  if (config?.accounts === undefined) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `No Convex accounts are saved by member yet. Run ${chalkStderr.bold(
        "npx convex login --account",
      )} to add one.`,
    });
  }
  return config;
}

async function ensureAccountExists(
  ctx: Context,
  config: GlobalConfig,
  accountId: string,
) {
  if (config.accounts?.[accountId] !== undefined) {
    return;
  }
  const known = Object.keys(config.accounts ?? {});
  return await ctx.crash({
    exitCode: 1,
    errorType: "fatal",
    printedMessage: `No logged-in account for member "${accountId}". ${
      known.length > 0 ? `Logged-in accounts: ${known.join(", ")}. ` : ""
    }Run ${chalkStderr.bold("npx convex login --account")} to add one.`,
  });
}

/**
 * Make commands run from the current directory use `accountId`. Unless
 * `force` is set, first checks the account can reach the deployment this
 * directory is configured for, and asks before binding one that can't.
 */
export async function bindDirectoryToAccount(
  ctx: Context,
  accountId: string,
  opts: { force?: boolean; acceptDefaults?: boolean } = {},
): Promise<boolean> {
  const config = readGlobalConfig(ctx);
  const account = config?.accounts?.[accountId];
  const directory = formatPathForPrinting(process.cwd());
  if (!opts.force && account !== undefined) {
    const warning = await deploymentAccessWarning(ctx, accountId, account);
    if (warning !== null) {
      logWarning(chalkStderr.yellow(warning));
      const proceed =
        opts.acceptDefaults || !process.stdin.isTTY
          ? false
          : await promptYesNo(ctx, {
              message: `Use ${accountLabel(accountId, account)} for ${directory} anyway?`,
              default: false,
            });
      if (!proceed) {
        logMessage(
          `Left ${directory} unchanged. To use it anyway, run ${chalkStderr.bold(
            `npx convex account use ${accountId} --force`,
          )}.`,
        );
        return false;
      }
    }
  }
  const before =
    config !== null ? resolveAccessToken(config, process.cwd()) : null;
  await updateGlobalConfig(ctx, (current) =>
    withInstance(current ?? {}, process.cwd(), accountId),
  );
  logFinishedStep(`${directory} now uses ${accountLabel(accountId, account)}.`);
  if (before?.accountId === accountId && before.source === "default") {
    logMessage(
      "It already used this account as the default; binding it keeps this directory on it if the default changes.",
    );
  }
  if (!isMemberId(accountId)) {
    logMessage(
      `Its member isn't recorded. To record it, run ${chalkStderr.bold(`npx convex login --account ${accountId}`)}.`,
    );
  }
  return true;
}

const accountList = new Command("list")
  .description(
    "List logged-in Convex accounts, whether each token still works, and the directories that use each one",
  )
  .allowExcessArguments(false)
  .action(async () => {
    const ctx = await accountCommandContext();
    await printDeployKeyBanner(ctx);
    await identifyLegacyAccounts(ctx);
    const config = readGlobalConfig(ctx);
    if (config?.accounts === undefined) {
      logMessage(
        config === null
          ? "Not logged in."
          : `Logged in to a single account. Run ${chalkStderr.bold(
              "npx convex login --account",
            )} to add another.`,
      );
      return;
    }
    logAccountsInEffect(ctx, config);
    const current = resolveAccessToken(config, process.cwd());
    const currentAccount =
      current?.accountId !== null && current?.accountId !== undefined
        ? config.accounts[current.accountId]
        : undefined;
    if (current?.accountId && currentAccount !== undefined) {
      const warning = await deploymentAccessWarning(
        ctx,
        current.accountId,
        currentAccount,
      );
      if (warning !== null) {
        logWarning(chalkStderr.yellow(warning));
      }
    }
    logMessage("");
    const saved = await listSavedAccounts(ctx, config);
    for (const entry of saved) {
      const marker = current?.accountId === entry.id ? "*" : " ";
      logOutput(`${marker} ${formatSavedAccount(entry)}`);
      for (const [directory, instance] of Object.entries(
        config.instances ?? {},
      )) {
        if (instance.account === entry.id) {
          logOutput(`      ${formatPathForPrinting(directory)}`);
        }
      }
    }
    logMessage(
      "\n* used in this directory. Token state is checked now; a token revoked later makes commands fail until you log in to it again.",
    );
  });

const accountUse = new Command("use")
  .description(
    "Use a logged-in Convex account for all commands run from the current directory and its subdirectories",
  )
  .argument(
    "<member-id>",
    "The account's Convex member id, from `npx convex account list`",
  )
  .option(
    "--force",
    "Use the account even if it can't access the deployment CONVEX_DEPLOYMENT points to",
  )
  .allowExcessArguments(false)
  .action(async (accountId, options) => {
    const ctx = await accountCommandContext();
    await readAccountOrCrash(ctx, accountId);
    await bindDirectoryToAccount(ctx, accountId, { force: !!options.force });
  });

const accountUnbind = new Command("unbind")
  .description(
    "Stop using a specific account for the current directory; it falls back to the default account",
  )
  .allowExcessArguments(false)
  .action(async () => {
    const ctx = await accountCommandContext();
    const config = await readAccountsOrCrash(ctx);
    const instance = findInstanceForDirectory(config, process.cwd());
    if (instance === null) {
      logMessage(
        `${formatPathForPrinting(process.cwd())} already uses the default account.`,
      );
      return;
    }
    await updateGlobalConfig(ctx, (current) =>
      withInstance(current ?? {}, instance.directory, null),
    );
    logFinishedStep(
      `${formatPathForPrinting(instance.directory)} no longer uses ${accountLabel(instance.account, config.accounts?.[instance.account])}; it now uses the default account.`,
    );
  });

const accountDefault = new Command("default")
  .description(
    "Set the account used by directories that aren't bound to one, and by older CLI versions",
  )
  .argument(
    "<member-id>",
    "The account's Convex member id, from `npx convex account list`",
  )
  .allowExcessArguments(false)
  .action(async (accountId) => {
    const ctx = await accountCommandContext();
    const { account } = await readAccountOrCrash(ctx, accountId);
    await updateGlobalConfig(ctx, (current) =>
      withDefaultAccount(current ?? {}, accountId),
    );
    logFinishedStep(
      `${accountLabelSentence(accountId, account)} is now the default account.`,
    );
  });

const accountDescribe = new Command("describe")
  .description("Set the note shown next to an account")
  .argument("<member-id>", "The account's Convex member id")
  .argument("<text>", "The note, such as the client it belongs to")
  .allowExcessArguments(false)
  .action(async (accountId, description) => {
    const ctx = await accountCommandContext();
    const { account } = await readAccountOrCrash(ctx, accountId);
    await updateGlobalConfig(ctx, (current) =>
      withAccount(current, accountId, {
        accessToken: account.accessToken,
        description,
      }),
    );
    logFinishedStep(`Updated the description of member ${accountId}.`);
  });

const accountSettings = new Command("settings")
  .description(
    "Show or change settings for the accounts on this machine. With no arguments, lists them.",
  )
  .argument("[name]", `The setting: ${Object.keys(SETTINGS).join(", ")}`)
  .argument("[value]", "true or false")
  .allowExcessArguments(false)
  .action(async (name, value) => {
    const ctx = await accountCommandContext();
    const config = readGlobalConfig(ctx);
    if (name === undefined) {
      for (const [settingName, setting] of Object.entries(SETTINGS)) {
        const current = config?.settings?.[setting.key];
        logOutput(
          `${settingName} = ${current ?? setting.default}${current === undefined ? " (default)" : ""}`,
        );
        logMessage(`  ${setting.description}`);
      }
      return;
    }
    const setting = SETTINGS[name as keyof typeof SETTINGS];
    if (setting === undefined) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `Unknown setting "${name}". Settings: ${Object.keys(SETTINGS).join(", ")}.`,
      });
    }
    if (value === undefined) {
      logOutput(
        `${name} = ${config?.settings?.[setting.key] ?? setting.default}`,
      );
      return;
    }
    if (value !== "true" && value !== "false") {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `${name} must be true or false.`,
      });
    }
    if (config === null) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `Not logged in. Log in first with ${chalkStderr.bold("npx convex login")}.`,
      });
    }
    await updateGlobalConfig(ctx, (current) =>
      withSetting(current ?? {}, setting.key, value === "true"),
    );
    logFinishedStep(`Set ${name} to ${value}.`);
  });

export const account = new Command("account")
  .description(
    "Manage the Convex accounts logged in on this machine and which project directories use each one",
  )
  .addCommand(accountList)
  .addCommand(accountUse)
  .addCommand(accountUnbind)
  .addCommand(accountDefault)
  .addCommand(accountDescribe)
  .addCommand(accountSettings)
  .addHelpCommand(false);
