import { Command, Option } from "@commander-js/extra-typings";
import { chalkStderr } from "chalk";
import { Context, oneoffContext } from "../bundler/context.js";
import {
  DeploymentSelectionOptions,
  DetailedDeploymentCredentials,
  loadSelectedDeploymentCredentials,
} from "./lib/api.js";
import { actionDescription } from "./lib/command.js";
import { ensureHasConvexDependency } from "./lib/utils/utils.js";
import {
  deploymentEnvBackend,
  envGet,
  envList,
  envRemove,
  envSet,
} from "./lib/env.js";
import { getDeploymentSelection } from "./lib/deploymentSelection.js";
import { withRunningBackend } from "./lib/localDeployment/run.js";
import { envDefault } from "./envDefault.js";

const envSetCmd = new Command("set")
  // Pretend value is required
  .usage("[options] <name> <value>")
  .argument("[name]", "The name of the environment variable to set.")
  .argument(
    "[value]",
    "The value to set the variable to. Omit to set it interactively.",
  )
  .summary("Set a variable")
  .description(
    [
      "Set environment variables on your deployment.",
      "",
      "• `npx convex env set NAME 'value'`",
      "• `npx convex env set NAME # omit a value to set one interactively`",
      "• `npx convex env set NAME --from-file value.txt`",
      "• `npx convex env set --from-file .env.defaults`",
      "",
      "When setting multiple values, it will refuse all changes if any variables are already set to different values by default. Pass --force to overwrite the provided values.",
      "",
      "To keep secrets out of your shell history, omit the value to pipe it in via stdin, for instance:",
      "• `pbpaste | npx convex env set API_KEY` (macOS)",
      "• `Get-Clipboard | npx convex env set API_KEY` (Windows PowerShell)",
      "",
      "To update many variables at once, save them with `npx convex env list --include-values > .env.convex`, edit the file, then reapply the changes with `npx convex env set --force < .env.convex`.",
    ].join("\n"),
  )
  .option(
    "--from-file <file>",
    "Read environment variables from a .env file. Without --force, fails if any existing variable has a different value.",
  )
  .option(
    "--force",
    "When setting multiple variables, overwrite existing environment variable values instead of failing on mismatch.",
  )
  .configureHelp({ showGlobalOptions: true })
  .allowExcessArguments(false)
  .action(async (name, value, cmdOptions, cmd) => {
    // Note: We use `as` here because optsWithGlobals() type inference doesn't
    // include global options from the parent command (added via addDeploymentSelectionOptions)
    const options = cmd.optsWithGlobals() as DeploymentSelectionOptions;
    const { ctx, deployment } = await selectEnvDeployment(options);
    await ensureHasConvexDependency(ctx, "env set");
    await withRunningBackend({
      ctx,
      deployment,
      action: async () => {
        const backend = deploymentEnvBackend(ctx, deployment);
        const didAnything = await envSet(ctx, backend, name, value, cmdOptions);
        if (didAnything === false) {
          cmd.outputHelp({ error: true });
          return await ctx.crash({
            exitCode: 1,
            errorType: "fatal",
            printedMessage:
              "error: No environment variables specified to be set.",
          });
        }
      },
    });
  });

export async function selectEnvDeployment(
  options: DeploymentSelectionOptions,
): Promise<{
  ctx: Context;
  deployment: {
    deploymentUrl: string;
    adminKey: string;
    deploymentNotice: string;
    deploymentFields: DetailedDeploymentCredentials["deploymentFields"];
  };
}> {
  const ctx = await oneoffContext(options);
  const deploymentSelection = await getDeploymentSelection(ctx, options);
  const {
    adminKey,
    url: deploymentUrl,
    deploymentFields,
  } = await loadSelectedDeploymentCredentials(ctx, deploymentSelection, {
    ensureLocalRunning: false,
  });

  const deploymentNotice =
    deploymentFields !== null
      ? ` (on ${chalkStderr.bold(deploymentFields.deploymentType)} deployment ${chalkStderr.bold(deploymentFields.deploymentName)})`
      : "";
  const result = {
    ctx,
    deployment: {
      deploymentUrl,
      adminKey,
      deploymentNotice,
      deploymentFields,
    },
  };
  return result;
}

const envGetCmd = new Command("get")
  .argument("<name>", "The name of the environment variable to print.")
  .summary("Print a variable's value")
  .description("Print a variable's value: `npx convex env get NAME`")
  .configureHelp({ showGlobalOptions: true })
  .allowExcessArguments(false)
  .action(async (envVarName, _options, cmd) => {
    const options = cmd.optsWithGlobals();
    const { ctx, deployment } = await selectEnvDeployment(options);
    await ensureHasConvexDependency(ctx, "env get");
    await withRunningBackend({
      ctx,
      deployment,
      action: async () => {
        const backend = deploymentEnvBackend(ctx, deployment);
        await envGet(ctx, backend, envVarName);
      },
    });
  });

const envRemoveCmd = new Command("remove")
  .alias("rm")
  .alias("unset")
  .argument("<name>", "The name of the environment variable to unset.")
  .summary("Unset a variable")
  .description(
    "Unset a variable: `npx convex env remove NAME`\n" +
      "If the variable doesn't exist, the command doesn't do anything and succeeds.",
  )
  .configureHelp({ showGlobalOptions: true })
  .allowExcessArguments(false)
  .action(async (name, _options, cmd) => {
    const options = cmd.optsWithGlobals();
    const { ctx, deployment } = await selectEnvDeployment(options);
    await ensureHasConvexDependency(ctx, "env remove");
    await withRunningBackend({
      ctx,
      deployment,
      action: async () => {
        const backend = deploymentEnvBackend(ctx, deployment);
        await envRemove(ctx, backend, name);
      },
    });
  });

const envListCmd = new Command("list")
  .summary("List all environment variables and their values")
  .description(
    [
      "• List only variable names (no values): `npx convex env list --names-only`",
      "• List all variables and their values: `npx convex env list --include-values`",
      "• Save all variables to a file: `npx convex env list --include-values > .env.convex`",
      "• Append to a file: `npx convex env list --include-values >> .env.convex`",
      "",
      "When an AI agent runs this command (detected from environment variables like AI_AGENT or CLAUDECODE), only names are listed unless --include-values is passed.",
    ].join("\n"),
  )
  .option(
    "--names-only",
    "List only the names of environment variables, without their values",
  )
  .addOption(
    new Option(
      "--include-values",
      "Print each variable's value alongside its name, in .env format. This is the default unless an AI agent is detected, so pass it explicitly in scripts that rely on the values.",
    ).conflicts("namesOnly"),
  )
  .configureHelp({ showGlobalOptions: true })
  .allowExcessArguments(false)
  .action(async (cmdOptions, cmd) => {
    // Note: We use `as` here because optsWithGlobals() type inference doesn't
    // include global options from the parent command (added via addDeploymentSelectionOptions)
    const options = cmd.optsWithGlobals() as DeploymentSelectionOptions;
    const { ctx, deployment } = await selectEnvDeployment(options);
    await ensureHasConvexDependency(ctx, "env list");
    await withRunningBackend({
      ctx,
      deployment,
      action: async () => {
        const backend = deploymentEnvBackend(ctx, deployment);
        await envList(ctx, backend, {
          namesOnly: cmdOptions.namesOnly ?? false,
          includeValues: cmdOptions.includeValues ?? false,
        });
      },
    });
  });

export const env = new Command("env")
  .summary("Set and view environment variables")
  .description(
    [
      "Set and view environment variables on your deployment",
      "",
      "• Set a variable: `npx convex env set NAME 'value'`",
      "• Set interactively: `npx convex env set NAME`",
      "• Set multiple from file: `npx convex env set --from-file .env`",
      "• Unset a variable: `npx convex env remove NAME`",
      "• List only variable names (no values): `npx convex env list --names-only`",
      "• List all variables and their values: `npx convex env list --include-values`",
      "• Print a variable's value: `npx convex env get NAME`",
      "",
      "By default, this sets and views variables on your dev deployment.",
      "",
      "See the environment variables guide (https://docs.convex.dev/production/environment-variables) to learn more.",
    ].join("\n"),
  )
  .addCommand(envSetCmd)
  .addCommand(envGetCmd)
  .addCommand(envRemoveCmd)
  .addCommand(envListCmd)
  .addCommand(envDefault)
  .helpCommand(false)
  .addDeploymentSelectionOptions(
    actionDescription("Set and view environment variables on"),
  );
