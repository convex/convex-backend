import { Command } from "@commander-js/extra-typings";
import { chalkStderr } from "chalk";
import { oneoffContext } from "../bundler/context.js";
import {
  logFailure,
  logFinishedStep,
  logMessage,
  logOutput,
  logWarning,
  showSpinner,
} from "../bundler/log.js";
import { claudeCode } from "./lib/plugins/claudeCode.js";
import { codex } from "./lib/plugins/codex.js";
import { getNewestHarnessLocations } from "./lib/plugins/harness.js";
import { logMarketplaces } from "./lib/plugins/marketplaces.js";
import { logConvexSkills } from "./lib/plugins/skills.js";
import { capture, run } from "./lib/plugins/utils.js";

const harnesses = [claudeCode, codex];

/**
 * Puts the latest Convex plugin in each harness on this machine, so the
 * harness knows Convex the next time it starts. It works through each
 * harness's own CLI and never edits the harnesses' files.
 *
 * Supported harnesses, on macOS and Linux:
 * - Claude Code: terminal, desktop app (macOS), VS Code extension.
 * - Codex: terminal, ChatGPT desktop app (macOS), VS Code extension.
 *
 * Windows is currently not supported.
 *
 * Success, exits 0:
 * - The harness has the latest Convex plugin: prints the version.
 * - The harness isn't installed: says so.
 *
 * Failure, exits 1:
 * - The install failed: says why and what to do.
 * - The machine runs Windows: says it isn't supported yet.
 */
const pluginsInstall = new Command("install")
  .summary("Install or upgrade the Convex plugin for Claude Code and Codex")
  .description(
    "Installs or upgrades the Convex plugin for Claude Code and Codex, in the\n" +
      "terminal, the desktop app and the VS Code extension:\n" +
      harnesses.map((h) => `  - ${h.name}: ${h.plugin}\n`).join("") +
      "\nClaude Code or Codex is skipped when it isn't installed. Windows isn't\n" +
      "supported yet.",
  )
  .allowExcessArguments(false)
  .action(async () => {
    const ctx = await oneoffContext({});
    if (process.platform === "win32") {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage:
          "`npx convex plugins install` doesn't support Windows yet.",
      });
    }
    const locations = await getNewestHarnessLocations();
    let failed = false;
    for (const {
      id,
      name,
      plugin,
      upgradeStep,
      installSteps,
      plugins,
    } of harnesses) {
      const cli = locations[id]?.path;
      if (cli === undefined) {
        logWarning(`${chalkStderr.yellow("⚠")} ${name}: not installed`);
        continue;
      }
      showSpinner(`Installing the Convex plugin for ${name}…`);
      let output = "";
      let upgraded = true;
      for (const args of installSteps) {
        const step = await run(cli, args);
        output += step.output;
        // Only the upgrade step's failure matters. Others can fail harmlessly,
        // like adding a marketplace that's already added from another source.
        if (args === upgradeStep && !step.ok) upgraded = false;
      }
      const list = await capture(cli, ["plugin", "list", "--json"]);
      const version =
        list === null
          ? undefined
          : plugins(list)?.find((p) => `${p.name}@${p.marketplace}` === plugin)
              ?.version;
      if (version === undefined || !upgraded) {
        failed = true;
        logFailure(`${name}: couldn't install the Convex plugin`);
        // The last lines of the harness's own output say why.
        const tail = output.trim().split(/\r?\n/).slice(-10).join("\n");
        if (tail !== "") logMessage(chalkStderr.gray(tail));
        logMessage(
          "Run `npx convex plugins install` again after fixing the error above.",
        );
      } else {
        logFinishedStep(`${name}: Convex plugin ${version}`);
      }
    }
    if (failed) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: null,
      });
    }
  });

// When status is picked up again, check it against install. Install now
// upgrades the plugin and reports the harnesses it didn't find.
const pluginsStatus = new Command("status")
  .summary("Show the Convex plugins and skills installed")
  .description(
    "Lists the Convex setup your coding agents load:\n" +
      "  - Convex skills in the hidden folders of your home folder and this folder\n" +
      "  - Convex marketplaces and plugins in Codex and Claude Code\n\n" +
      "It only reads, and changes nothing.",
  )
  .allowExcessArguments(false)
  .action(async () => {
    await logConvexSkills();

    const locations = await getNewestHarnessLocations();
    for (const harness of harnesses) {
      const cli = locations[harness.id]?.path;
      const marketplaceList =
        cli === undefined
          ? null
          : await capture(cli, ["plugin", "marketplace", "list", "--json"]);
      const pluginList =
        cli === undefined
          ? null
          : await capture(cli, ["plugin", "list", "--json"]);
      if (marketplaceList === null || pluginList === null) {
        logOutput(`${harness.name}: not installed`);
        continue;
      }
      const marketplaces = harness.marketplaces(marketplaceList, pluginList);
      if (marketplaces === null) {
        logOutput(`${harness.name}: couldn't read its plugin lists`);
        continue;
      }
      logMarketplaces(harness.name, marketplaces, await harness.latest());
    }
  });

export const plugins = new Command("plugins")
  .summary("Manage Convex plugins for coding agents")
  .addCommand(pluginsInstall)
  .addCommand(pluginsStatus, { hidden: true });
