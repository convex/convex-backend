import { Command } from "@commander-js/extra-typings";
import { oneoffContext } from "../bundler/context.js";
import { logFinishedStep, logOutput, showSpinner } from "../bundler/log.js";
import { loadSelectedDeploymentCredentials } from "./lib/api.js";
import { actionDescription } from "./lib/command.js";
import {
  ensureLoggedInWithAccessToken,
  getDeploymentSelection,
} from "./lib/deploymentSelection.js";
import { changedEnvVarFile } from "./lib/envvars.js";
import {
  CONVEX_DEPLOY_KEY_ENV_VAR_NAME,
  ENV_VAR_FILE_PATH,
  typedPlatformClient,
} from "./lib/utils/utils.js";

export const deploymentTokenCreate = new Command("create")
  .summary("Create an access token")
  .description(
    [
      `Creates a deploy key that, when set as \`${CONVEX_DEPLOY_KEY_ENV_VAR_NAME}\`, scopes all commands to the target deployment.`,
      "",
      "• Print a new deploy key to stdout: `npx convex deployment token create my-token`",
      `• Save a new deploy key in \`${ENV_VAR_FILE_PATH}\`: \`npx convex deployment token create my-token --save-env\``,
      "• Save a new deploy key in a custom env file: `npx convex deployment token create ci-token --save-env .env.production`",
      "• Create a key for the project's prod: `npx convex deployment token create ci-token --deployment prod`",
    ].join("\n"),
  )
  .argument("<name>", "Name for the new deploy key")
  .allowExcessArguments(false)
  .option(
    "--save-env [path]",
    `Save the new key as ${CONVEX_DEPLOY_KEY_ENV_VAR_NAME} in an env file instead of printing it. Defaults to ${ENV_VAR_FILE_PATH}.`,
  )
  .addDeploymentSelectionOptions(actionDescription("Create a deploy key for"))
  .showHelpAfterError()
  .action(async (name, options) => {
    const ctx = await oneoffContext(options);

    await ensureLoggedInWithAccessToken(ctx, "Creating a deploy key");

    const deploymentSelection = await getDeploymentSelection(ctx, options);
    const deployment = await loadSelectedDeploymentCredentials(
      ctx,
      deploymentSelection,
      { ensureLocalRunning: false },
    );

    if (deployment.deploymentFields === null) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage:
          "Cannot create a deploy key for a self-hosted deployment.",
      });
    }

    const { deploymentName, deploymentType } = deployment.deploymentFields;
    if (deploymentType === "local" || deploymentType === "anonymous") {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `Cannot create a deploy key for a ${deploymentType} deployment.`,
      });
    }

    showSpinner(`Creating deploy key for ${deploymentName}...`);
    const response = await typedPlatformClient(ctx).POST(
      "/deployments/{deployment_name}/create_deploy_key",
      {
        params: { path: { deployment_name: deploymentName } },
        body: { name },
      },
    );
    const deployKey = response.data!.deployKey;

    if (options.saveEnv === undefined) {
      logFinishedStep(`Created deploy key "${name}" for ${deploymentName}.`);
      logOutput(deployKey);
      return;
    }

    const envFile =
      typeof options.saveEnv === "string" ? options.saveEnv : ENV_VAR_FILE_PATH;
    const existingFileContent = ctx.fs.exists(envFile)
      ? ctx.fs.readUtf8File(envFile)
      : null;
    const updatedContent = changedEnvVarFile({
      existingFileContent,
      envVarName: CONVEX_DEPLOY_KEY_ENV_VAR_NAME,
      envVarValue: deployKey,
      commentAfterValue: null,
      commentOnPreviousLine: null,
    });

    if (updatedContent === null) {
      logFinishedStep(
        `Deploy key for ${deploymentName} already present in ${envFile}; no changes made.`,
      );
      return;
    }

    ctx.fs.writeUtf8File(envFile, updatedContent);
    logFinishedStep(
      `Saved deploy key "${name}" for ${deploymentName} as ${CONVEX_DEPLOY_KEY_ENV_VAR_NAME} in ${envFile}.`,
    );
  });
