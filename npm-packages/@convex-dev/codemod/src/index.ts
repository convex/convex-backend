#!/usr/bin/env node

import { Argument, Command, Option } from "commander";
import { Project } from "ts-morph";
import { createContext, logFinishedStep, showSpinner } from "./context";
import { explicitIds, CONVEX_VERSION_RANGE } from "./codemods/explicit-ids";
import { hasUncommittedChanges } from "./git";
import { version } from "./version";
import { checkConvexVersion, findTsconfig } from "./load";
import { diffProjects } from "./diff";
import chalk from "chalk";
import * as Sentry from "@sentry/node";
import { initSentry } from "./sentry";

initSentry(version);

const program = new Command();

program
  .name("@convex-dev/codemod")
  .description(
    "Automatic codemods to help you migrate your codebase to newer versions of Convex",
  )
  .version(version)
  .addArgument(
    new Argument("<transform>", "The codemod to run").choices(["explicit-ids"]),
  )
  .addOption(
    new Option("--root <path>", "Root directory path of your project").default(
      process.cwd(),
      "current directory",
    ),
  )
  .addOption(
    new Option(
      "--allow-dirty",
      "Allow the codemod to run even if there are uncommitted changes in the project",
    ),
  )
  .addOption(
    new Option(
      "--skip-convex-check",
      "Skip checking the version of Convex that is used in the project before running",
    ),
  )
  .addOption(
    new Option(
      "--dry-run",
      "Show what changes would be made and don't actually apply them",
    ),
  )
  .action(
    async (
      _transform: string,
      {
        root,
        allowDirty,
        skipConvexCheck,
        dryRun,
      }: {
        root: string;
        allowDirty: boolean;
        skipConvexCheck: boolean;
        dryRun: boolean;
      },
    ) => {
      const ctx = createContext();

      showSpinner(ctx, "Loading your project…");

      if (!allowDirty && !dryRun && (await hasUncommittedChanges(root))) {
        return await ctx.crash({
          printedMessage:
            "There are uncommitted changes in the project. Please commit or stash them before running the codemod, or run with --allow-dirty to override.",
        });
      }

      if (!skipConvexCheck) {
        await checkConvexVersion(ctx, root, CONVEX_VERSION_RANGE);
      }

      const tsConfigFilePath = await findTsconfig(ctx, root);
      if (!tsConfigFilePath) {
        return await ctx.crash({
          printedMessage: "No tsconfig.json found",
        });
      }

      const project = new Project({
        tsConfigFilePath,
      });

      logFinishedStep(ctx, "Loaded project");

      await explicitIds(ctx, project, root);

      if (dryRun) {
        const baseProject = new Project({
          tsConfigFilePath,
        });

        diffProjects(root, baseProject, project);
      } else {
        project.saveSync();
      }

      ctx.printResults(dryRun);
    },
  )
  .exitOverride((err) => {
    if (err.code === "commander.missingArgument") {
      program.outputHelp();
    }
  });

process.on("SIGINT", () => {
  process.exit(0);
});

async function main() {
  try {
    await program.parseAsync();
  } catch (e) {
    Sentry.captureException(e);
    process.exitCode = 1;
    console.error(chalk.red("Unexpected Error: " + e));
  } finally {
    await Sentry.close();
  }
}

void main();
