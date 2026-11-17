import { Command } from "@commander-js/extra-typings";
import path from "path";
import { createLoginCommand } from "./login.js";
import { createLogoutCommand } from "./logout.js";
import { Context, oneoffContext } from "../bundler/context.js";
import {
  logFinishedStep,
  logMessage,
  logOutput,
  logWarning,
} from "../bundler/log.js";
import { loadProjectEnvironmentVariables } from "./lib/envvars.js";
import { globalConfigPath } from "./lib/utils/globalConfig.js";
import { isAuthorizedHeader, performLogin } from "./lib/login.js";
import {
  findAccountDirectory,
  getAccountSelection,
  initializeDefaultAccount,
  loadAccountCredentials,
  readAccountAuth,
  readAccountCredentials,
  readAccountSettings,
  savedAccountNames,
  validateAccountName,
  updateAccountSettings,
} from "./lib/accounts.js";
import {
  bindAccountToDirectory,
  projectAccess,
} from "./lib/accountManagement.js";
import { warnIfDeployKeyOverridesLogin } from "./lib/deployKeyWarning.js";
import { ProjectSelection } from "./lib/deploymentSelection.js";
import { promptOptions, promptString } from "./lib/utils/prompts.js";

async function accountContext() {
  const ctx = await oneoffContext({ skipAuth: true });
  loadProjectEnvironmentVariables();
  return ctx;
}

const accountList = new Command("list")
  .description(
    "List saved accounts, credential status, and directory selections",
  )
  .allowExcessArguments(false)
  .action(async () => {
    const ctx = await accountContext();
    const settings = await readAccountSettings(ctx);
    const selected = await getAccountSelection(ctx);
    const names = new Set([
      ...savedAccountNames(ctx),
      ...Object.values(settings.directories ?? {}),
      ...(settings.defaultAccount === undefined
        ? []
        : [settings.defaultAccount]),
      ...(selected === null ? [] : [selected.name]),
    ]);
    warnIfDeployKeyOverridesLogin(
      ctx,
      typeof settings.accessToken === "string" ||
        savedAccountNames(ctx).length > 0,
    );
    if (names.size === 0) {
      logMessage(
        typeof settings.accessToken === "string"
          ? (await isAuthorizedHeader(ctx, `Bearer ${settings.accessToken}`))
            ? "Logged in without a named account. Name this login with `npx convex login --account personal --select`."
            : "Your saved login is no longer valid. Run `npx convex login --account <name>`."
          : "No saved accounts. Run `npx convex login --account <name>`.",
      );
      return;
    }
    if (selected !== null)
      logMessage(`Selected account: ${selected.name} (${selected.source})`);
    if (settings.defaultAccount !== undefined)
      logMessage(`Default account: ${settings.defaultAccount}`);
    for (const name of [...names].sort()) {
      await validateAccountName(ctx, name, globalConfigPath());
      const credentials = loadAccountCredentials(ctx, name);
      const email =
        typeof credentials === "string" ? undefined : credentials.email;
      const label = `${name}${email ? ` (${email})` : ""}`;
      const status =
        credentials === "missing"
          ? " ⚠️ Not logged in"
          : credentials === "unreadable"
            ? " ⚠️ Unreadable credentials"
            : (await isAuthorizedHeader(
                  ctx,
                  `Bearer ${credentials.accessToken}`,
                ))
              ? ""
              : " ⚠️ Invalid credentials";
      logOutput(`${label}${status}`);
      for (const [directory, account] of Object.entries(
        settings.directories ?? {},
      )) {
        if (account === name) logOutput(`    ${directory}`);
      }
    }
  });

const accountSelect = new Command("select")
  .description("Use a saved account in this directory and its subdirectories")
  .argument("<account>", "Account name, such as work or personal")
  .option(
    "--force",
    "Select this account even if deployment access cannot be verified",
  )
  .allowExcessArguments(false)
  .action(async (name, options) => {
    const ctx = await accountContext();
    if (!(await bindAccountToDirectory(ctx, name, options.force))) {
      await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: null,
      });
    }
  });

const accountDefault = new Command("default")
  .description(
    "Show or set the default account for directories without a directory selection",
  )
  .argument("[account]", "Account name")
  .allowExcessArguments(false)
  .action(async (name) => {
    const ctx = await accountContext();
    if (name === undefined) {
      const settings = await readAccountSettings(ctx);
      logOutput(
        settings.defaultAccount === undefined
          ? "No default account is set."
          : `Default account: ${settings.defaultAccount}`,
      );
      return;
    }
    await updateAccountSettings(ctx, async (settings) => {
      const credentials = await readAccountCredentials(ctx, {
        name,
        source: "account default",
      });
      settings.defaultAccount = name;
      settings.accessToken = credentials!.accessToken;
    });
    logFinishedStep(`Default account: ${name}`);
  });

const accountDeselect = new Command("deselect")
  .description(
    "Clear this directory's account selection, inheriting its parent's selection or the default",
  )
  .allowExcessArguments(false)
  .action(async () => {
    const ctx = await accountContext();
    const directory = path.resolve(process.cwd());
    const { removed, inherited } = await updateAccountSettings(
      ctx,
      (settings) => {
        if (
          settings.directories &&
          Object.hasOwn(settings.directories, directory)
        ) {
          delete settings.directories[directory];
          return { removed: true, inherited: null };
        }
        return {
          removed: false,
          inherited: findAccountDirectory(
            settings.directories ?? {},
            directory,
          ),
        };
      },
    );
    if (removed)
      logFinishedStep(`Removed the directory selection for ${directory}.`);
    else logMessage(`No directory selection is saved for ${directory}.`);
    const selected = await getAccountSelection(ctx);
    if (selected !== null)
      logMessage(`Selected account: ${selected.name} (${selected.source})`);
    if (inherited !== null)
      logMessage(
        `To remove the inherited account selection, run \`npx convex@latest account deselect\` from ${inherited.directory}.`,
      );
  });

export const account = new Command("account")
  .summary("Manage saved accounts and their directory selections")
  .description(
    "Manage saved accounts and their directory selections.\n\n" +
      "Save logins for multiple Convex accounts under names such as work and personal. `npx convex@latest account login` and `npx convex@latest account logout` are aliases for the top-level login and logout commands, with the same options. Select the appropriate account for each personal, work, or client project.\n\n" +
      "In your personal project directory, save an account:\n\n" +
      "  `npx convex@latest login --account personal --select`\n\n" +
      "In your work project directory, log in with your work account and check its teams:\n\n" +
      "  `npx convex@latest login --account work --select`\n" +
      "  `npx convex@latest login status`\n\n" +
      "Choose the appropriate account in the browser; sign out of the dashboard first if it is using another account. Upgrade each project's `convex` dependency to a version that supports saved accounts, then run `npx convex dev` to develop with its selected account.\n\n" +
      "`login --account` saves credentials without changing directory selections. Add `--select` to select the account for this directory and its subdirectories after checking deployment access. The first saved account becomes the default if no other account is saved. To select a saved account in another directory, run `npx convex@latest account select work`. Run `npx convex@latest account deselect` to remove this directory's selection and inherit its parent or default. Run `npx convex@latest account default` to show the default, or `npx convex@latest account default work` to change it. The closest directory selection wins. Selections use absolute paths, so select the account again after moving a project.\n\n" +
      "The first account name is assigned to your existing login if you are already logged in, and becomes the default for directories without a selection. Use `--force` to authenticate another account instead, keeping the existing login as the default. Names start with a lowercase letter or number and contain only lowercase letters, numbers, hyphens, and underscores.\n\n" +
      "`CONVEX_ACCOUNT` in the shell or project env file overrides directory selections and the default account. Deployment and project keys take priority over saved accounts.\n\n" +
      "Use `npx convex@latest logout --account work` to remove one account's credentials, or `npx convex@latest logout --all` to remove all saved credentials. Logout preserves directory selections; a logged-out account requires login before use.\n\n" +
      "After saving accounts, use `npx convex@latest` for login, logout, and account commands in every project directory on this machine. Plain `npx convex` can run the older version installed in that project. Specifying `@latest` runs the latest published CLI without updating the project's dependency.\n\n" +
      "Older CLIs ignore directory selections and `CONVEX_ACCOUNT`, using the shared login instead (normally the default account). Upgrade the project's `convex` dependency to use saved accounts with project commands such as `npx convex dev` and `npx convex deploy`.\n\n" +
      "Logging in with an older CLI can change the shared login independently of your saved accounts, or discard account settings if the shared login has been removed. Logging out with an older CLI deletes directory selections and the default selection but leaves named account credentials behind. Recreate any lost selections with `npx convex@latest account select <name>` and `npx convex@latest account default <name>`.",
  )
  .addCommand(accountList)
  .addCommand(accountSelect)
  .addCommand(accountDefault)
  .addCommand(accountDeselect)
  .addCommand(createLoginCommand())
  .addCommand(createLogoutCommand())
  .addHelpCommand(false);

export async function handleNoAccessToConfiguredProject(
  ctx: Context,
  target: ProjectSelection,
  envFile?: string,
): Promise<"retry" | "chooseProject"> {
  const auth = ctx.bigBrainAuth();
  const current = auth?.kind === "accessToken" ? auth.account?.name : undefined;
  const project =
    target.kind === "deploymentName"
      ? `deployment "${target.deploymentName}"`
      : target.kind === "teamAndProjectSlugs"
        ? `project "${target.teamSlug}/${target.projectSlug}"`
        : "the configured project";
  logWarning(
    `${current ? `Account "${current}"` : "Your account"} cannot access ${project}. It may belong to another account.`,
  );
  const alternatives: string[] = [];
  if (target.kind !== "projectDeployKey") {
    for (const name of savedAccountNames(ctx)) {
      if (name === current) continue;
      const credentials = loadAccountCredentials(ctx, name);
      if (
        typeof credentials !== "string" &&
        (await projectAccess(credentials.accessToken, target)) === "yes"
      )
        alternatives.push(name);
    }
  }
  if (!process.stdin.isTTY) {
    const selection = await getAccountSelection(ctx, { envFile });
    const overrideHint = selection?.source.startsWith("CONVEX_ACCOUNT")
      ? ` Update or unset CONVEX_ACCOUNT (${selection.source}) before retrying.`
      : "";
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `${
        alternatives.length > 0
          ? `A saved account can access it: ${alternatives.map((name) => `\`npx convex account select ${name}\``).join(" or ")}.`
          : "Log in to its account with `npx convex login --account <name> --select`."
      }${overrideHint} To select a different project, run \`npx convex dev --configure\` (this changes CONVEX_DEPLOYMENT).`,
    });
  }
  type Choice =
    | { kind: "use"; name: string }
    | { kind: "login" }
    | { kind: "chooseProject" };
  const choice = await promptOptions<Choice>(ctx, {
    message: "Which account should this project use?",
    choices: [
      ...alternatives.map((name) => ({
        name: `Use account "${name}"`,
        value: { kind: "use", name } as const,
      })),
      { name: "Log in to another account", value: { kind: "login" } as const },
      {
        name: "Configure a different project (changes CONVEX_DEPLOYMENT)",
        value: { kind: "chooseProject" } as const,
      },
    ],
  });
  if (choice.kind === "chooseProject") return "chooseProject";
  const name =
    choice.kind === "use"
      ? choice.name
      : await promptString(ctx, {
          message: "Account name (for example, work):",
        });
  await validateAccountName(ctx, name);
  if (choice.kind === "login") {
    const selection = { name, source: "account recovery" };
    await performLogin(ctx, { account: selection });
    await initializeDefaultAccount(ctx, selection, false);
  }
  if (!(await bindAccountToDirectory(ctx, name, false, target)))
    return "chooseProject";
  const selection = await getAccountSelection(ctx, { envFile });
  if (selection?.name !== name) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem or env vars",
      printedMessage: `Remove the account override from ${selection?.source} before retrying.`,
    });
  }
  ctx._updateBigBrainAuth(await readAccountAuth(ctx, selection));
  return "retry";
}
