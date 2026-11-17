import { Command } from "@commander-js/extra-typings";
import { oneoffContext } from "../bundler/context.js";
import { logFinishedStep, logMessage, logWarning } from "../bundler/log.js";
import { recursivelyDelete } from "./lib/fsUtils.js";
import { globalConfigPath } from "./lib/utils/globalConfig.js";
import {
  getAccountSelection,
  accountConfigPath,
  readAccountToken,
  readAccountSettings,
  updateAccountSettings,
  savedAccountNames,
} from "./lib/accounts.js";
import path from "path";
import { rootDirectory } from "./lib/utils/utils.js";
import { promptOptions } from "./lib/utils/prompts.js";

export function createLogoutCommand() {
  return new Command("logout")
    .description(
      "Log out of the selected account. Directory selections are preserved.",
    )
    .option("--account <name>", "Remove only this account's saved credentials")
    .option(
      "--all",
      "Remove all saved account credentials, preserving account selections",
    )
    .allowExcessArguments(false)
    .action(async (options, cmd) => {
      if (options.all && options.account !== undefined)
        cmd.error("Pass either --account or --all, not both.");
      const ctx = await oneoffContext({
        url: undefined,
        adminKey: undefined,
        envFile: undefined,
        skipAuth: true,
      });

      let all = !!options.all;
      let account = all
        ? null
        : await getAccountSelection(ctx, { name: options.account });
      if (
        !all &&
        options.account === undefined &&
        process.stdin.isTTY &&
        account !== null
      ) {
        const token = readAccountToken(ctx, account.name);
        const settings = await readAccountSettings(ctx);
        const selectedName = account.name;
        // A matching compatibility token is another copy of this account's login.
        const hasOtherLogins =
          savedAccountNames(ctx).some((name) => name !== selectedName) ||
          (typeof settings.accessToken === "string" &&
            settings.accessToken.length > 0 &&
            settings.accessToken !== token);
        if (hasOtherLogins) {
          const scope = await promptOptions(ctx, {
            message: "Which saved logins would you like to log out of?",
            choices: [
              { name: `Only account "${account.name}"`, value: "account" },
              { name: "All saved logins on this device", value: "all" },
            ],
            default: "account",
          });
          if (scope === "all") {
            account = null;
            all = true;
          }
        }
      }

      let warnAboutLegacyCredentials = false;
      if (account !== null) {
        const selectedName = account.name;
        await updateAccountSettings(
          ctx,
          (settings) => {
            const token = readAccountToken(ctx, selectedName);
            // A migrated account can share its token with old CLI versions.
            if (token !== null && settings.accessToken === token) {
              delete settings.accessToken;
            } else if (
              settings.defaultAccount === selectedName &&
              typeof settings.accessToken === "string" &&
              settings.accessToken.length > 0
            ) {
              // Editing the default or logging in with an older CLI can leave this
              // token belonging to a different account.
              warnAboutLegacyCredentials = true;
            }
          },
          () => {
            const configPath = accountConfigPath(selectedName);
            recursivelyDelete(ctx, configPath, {
              force: true,
              followSymlinks: false,
            });
          },
        );
      } else {
        await updateAccountSettings(
          ctx,
          async (settings) => {
            if (
              !all &&
              savedAccountNames(ctx).length > 0 &&
              settings.accessToken === undefined
            ) {
              return await ctx.crash({
                exitCode: 1,
                errorType: "fatal",
                printedMessage:
                  "No account is selected. Use `npx convex logout --account <name>` or `npx convex logout --all`.",
              });
            }
            delete settings.accessToken;
          },
          () => {
            const accountsDir = path.join(rootDirectory(), "accounts");
            if (all)
              recursivelyDelete(ctx, accountsDir, {
                force: true,
                followSymlinks: false,
              });
          },
        );
      }

      logFinishedStep(
        account !== null
          ? `Removed credentials for account "${account.name}".\n  Run \`npx convex login --account ${account.name}\` to log in.`
          : `${all ? "Removed all saved Convex account credentials." : "Removed your saved Convex login credentials."}\n  Run \`npx convex login\` to log in.`,
      );
      if (warnAboutLegacyCredentials) {
        logWarning(
          `Saved credentials remain in ${globalConfigPath()} for older CLI versions. To remove all saved account credentials, run \`npx convex logout --all\`.`,
        );
      }
      if (process.env.CONVEX_OVERRIDE_ACCESS_TOKEN) {
        logMessage(
          "CONVEX_OVERRIDE_ACCESS_TOKEN still supplies credentials. Unset it to stop using that token.",
        );
      }
    });
}

export const logout = createLogoutCommand();
