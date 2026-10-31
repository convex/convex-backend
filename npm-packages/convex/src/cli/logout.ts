import { Command } from "@commander-js/extra-typings";
import { chalkStderr } from "chalk";
import { oneoffContext } from "../bundler/context.js";
import { logFinishedStep } from "../bundler/log.js";
import { recursivelyDelete } from "./lib/fsUtils.js";
import {
  globalConfigPath,
  defaultAccountId,
  readGlobalConfig,
  updateGlobalConfig,
  withoutAccount,
} from "./lib/utils/globalConfig.js";
import { promptOptions } from "./lib/utils/prompts.js";
import {
  accountLabel,
  accountLabelSentence,
  formatAccountName,
} from "./account.js";

export const logout = new Command("logout")
  .description("Log out of Convex on this machine")
  .option(
    "--account <member-id>",
    "Log out of one account and stop using it for the directories bound to it",
  )
  .option("--all", "Log out of every account")
  .allowExcessArguments(false)
  .action(async (options, cmd) => {
    const ctx = await oneoffContext({
      url: undefined,
      adminKey: undefined,
      envFile: undefined,
    });
    if (options.account !== undefined && options.all) {
      cmd.error("Pass either --account or --all, not both.");
    }

    const config = readGlobalConfig(ctx);
    let accountId = options.account;
    if (
      accountId === undefined &&
      !options.all &&
      config?.accounts !== undefined
    ) {
      const accounts = config.accounts;
      if (!process.stdin.isTTY) {
        return await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage: `Several Convex accounts are logged in. Pass ${chalkStderr.bold(
            "--account <member-id>",
          )} or ${chalkStderr.bold("--all")}.`,
        });
      }
      accountId =
        (await promptOptions<string | null>(ctx, {
          message: "Which account do you want to log out of?",
          choices: [
            ...Object.entries(accounts).map(([id, account]) => ({
              name: formatAccountName(id, account),
              value: id,
            })),
            { name: "All accounts", value: null },
          ],
        })) ?? undefined;
    }

    if (accountId === undefined) {
      if (ctx.fs.exists(globalConfigPath())) {
        recursivelyDelete(ctx, globalConfigPath());
      }
      logFinishedStep(
        "You have been logged out of Convex.\n  Run `npx convex dev` to log in.",
      );
      return;
    }

    if (config?.accounts?.[accountId] === undefined) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `No logged-in account for member "${accountId}".`,
      });
    }
    const id = accountId;
    const updated = await updateGlobalConfig(ctx, (current) =>
      withoutAccount(current ?? {}, id),
    );
    const newDefault = defaultAccountId(updated);
    logFinishedStep(
      `Logged out of ${accountLabel(id, config.accounts[id])}.${
        newDefault !== null && newDefault !== defaultAccountId(config)
          ? ` ${accountLabelSentence(newDefault, updated.accounts?.[newDefault])} is now the default account.`
          : ""
      }`,
    );
  });
