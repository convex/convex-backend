import path from "path";
import { Command } from "@commander-js/extra-typings";
import { chalkStderr } from "chalk";
import { logMessage } from "../bundler/log.js";
import { oneoffContext } from "../bundler/context.js";
import { readProjectConfig } from "./lib/config.js";
import { functionsDir } from "./lib/utils/utils.js";
import {
  installAiFiles,
  enableAiFiles,
  disableAiFiles,
  removeAiFiles,
} from "./lib/aiFiles/index.js";
import { statusAiFiles } from "./lib/aiFiles/status.js";
import { writeAiFilesConfig } from "./lib/config.js";

async function resolveProjectPaths() {
  const ctx = await oneoffContext({});
  const { configPath, projectConfig } = await readProjectConfig(ctx);
  const convexDir = path.resolve(functionsDir(configPath, projectConfig));
  const projectDir = path.resolve(path.dirname(configPath));
  const aiFilesConfig = projectConfig.aiFiles;
  return { ctx, projectDir, convexDir, aiFilesConfig, projectConfig };
}

const aiInstall = new Command("install")
  .summary("Install or refresh Convex AI files")
  .description(
    "Installs the following (or refreshes them if already present):\n" +
      "  - convex/_generated/ai/guidelines.md\n" +
      "  - AGENTS.md (Convex section only)\n" +
      "  - CLAUDE.md (Convex section only)\n" +
      "  - Agent skills (installed to each coding agent's native path, configured via convex.json)",
  )
  .allowExcessArguments(false)
  .action(async () => {
    const { ctx, projectDir, convexDir, aiFilesConfig } =
      await resolveProjectPaths();

    const result = await installAiFiles({
      projectDir,
      convexDir,
      aiFilesConfig,
    });

    if (result.kind === "error") {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: result.message,
      });
    }

    logMessage(`${chalkStderr.green("✔")} Convex AI files installed.`);
  });

const aiEnable = new Command("enable")
  .summary("Enable Convex AI files")
  .description(
    "Re-enables Convex AI files by writing `aiFiles.enabled: true` to\n" +
      "`convex.json`, then installs or refreshes the managed AI files.",
  )
  .allowExcessArguments(false)
  .action(async () => {
    const { projectDir, convexDir, aiFilesConfig } =
      await resolveProjectPaths();

    const newAiFilesConfig = await enableAiFiles({
      projectDir,
      convexDir,
      aiFilesConfig,
    });

    await writeAiFilesConfig({ projectDir, aiFiles: newAiFilesConfig });
  });

const aiUpdate = new Command("update")
  .summary("Update Convex AI files to the latest version")
  .description(
    "Updates the following to their latest versions:\n" +
      "  - convex/_generated/ai/guidelines.md\n" +
      "  - AGENTS.md (Convex section only)\n" +
      "  - CLAUDE.md (Convex section only)\n" +
      "  - Agent skills (installed to each coding agent's native path, configured via convex.json)",
  )
  .allowExcessArguments(false)
  .action(async () => {
    const { ctx, projectDir, convexDir, aiFilesConfig } =
      await resolveProjectPaths();

    const result = await installAiFiles({
      projectDir,
      convexDir,
      aiFilesConfig,
    });

    if (result.kind === "error") {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: result.message,
      });
    }

    logMessage(`${chalkStderr.green("✔")} Convex AI files updated.`);
  });

const aiDisable = new Command("disable")
  .summary("Disable Convex AI files without removing them")
  .description(
    "Writes `aiFiles.enabled: false` to `convex.json` so `npx convex dev`\n" +
      "stops prompting to install AI files and suppresses staleness messages.\n\n" +
      "Files already installed are left untouched - use `npx convex ai-files remove`\n" +
      "if you also want to delete them.\n\n" +
      "Run `npx convex ai-files enable` to re-enable at any time.",
  )
  .allowExcessArguments(false)
  .action(async () => {
    const { projectDir, aiFilesConfig } = await resolveProjectPaths();

    const newAiFilesConfig = disableAiFiles(aiFilesConfig);

    await writeAiFilesConfig({
      projectDir,
      aiFiles: newAiFilesConfig,
    });

    logMessage(
      `${chalkStderr.green(`✔`)} Convex AI files disabled. Run ${chalkStderr.bold(`npx convex ai-files enable`)} to re-enable.`,
    );
  });

const aiStatus = new Command("status")
  .summary("Show the current status of Convex AI files")
  .description(
    "Prints whether Convex AI files are enabled, and for each component:\n" +
      "  - convex/_generated/ai/guidelines.md\n" +
      "  - AGENTS.md (Convex section)\n" +
      "  - CLAUDE.md (if installed by Convex)\n" +
      "  - Agent skills\n\n" +
      "Fetches the latest hashes from version.convex.dev to report whether\n" +
      "each file is up to date. If the network is unavailable the staleness\n" +
      "check is skipped silently.",
  )
  .allowExcessArguments(false)
  .action(async () => {
    const { projectDir, convexDir, aiFilesConfig } =
      await resolveProjectPaths();

    await statusAiFiles({ projectDir, convexDir, aiFilesConfig });
  });

const aiRemove = new Command("remove")
  .summary("Remove all Convex AI files from the project")
  .description(
    "Removes the following:\n" +
      "  - convex/_generated/ai/ directory (guidelines.md, ai-files.state.json)\n" +
      "  - Convex sections from AGENTS.md and CLAUDE.md\n" +
      "  - Agent skills installed by `npx convex ai-files install`\n\n" +
      "If removing the managed section leaves AGENTS.md or CLAUDE.md empty, the\n" +
      "empty file is deleted. Otherwise the rest of the file is kept.\n\n" +
      "Skills installed from other sources are not affected.\n\n" +
      "Note: after `remove`, `npx convex dev` will suggest reinstalling AI files.\n" +
      "Use `npx convex ai-files disable` to opt out entirely without deleting files.",
  )
  .allowExcessArguments(false)
  .action(async () => {
    const { ctx, projectDir, convexDir } = await resolveProjectPaths();
    const result = await removeAiFiles({ projectDir, convexDir });
    if (result.kind === "error") {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: result.message,
      });
    }
  });

export const aiFiles = new Command("ai-files")
  .summary("Manage Convex AI files")
  .description(
    "Convex AI files help AI coding assistants (Cursor, Claude Code, etc.) understand\n" +
      "Convex patterns and APIs. They are set up during your first `npx convex dev`\n" +
      "and can be managed at any time with the commands below.",
  )
  .addCommand(aiStatus)
  .addCommand(aiInstall)
  .addCommand(aiEnable)
  .addCommand(aiUpdate)
  .addCommand(aiDisable)
  .addCommand(aiRemove)
  .addHelpCommand(false);
