import { Command } from "@commander-js/extra-typings";
import { chalkStderr } from "chalk";
import { oneoffContext } from "../bundler/context.js";
import { logFinishedStep, showSpinner } from "../bundler/log.js";
import { loadSelectedDeploymentCredentials } from "./lib/api.js";
import { actionDescription } from "./lib/command.js";
import {
  ensureLoggedInWithAccessToken,
  getDeploymentSelection,
} from "./lib/deploymentSelection.js";
import { typedPlatformClient } from "./lib/utils/utils.js";

export const deploymentTokenDelete = new Command("delete")
  .summary("Delete an access token")
  .description(
    [
      "Delete an access token. Currently only deploy keys (deployment-scoped access tokens) are supported.",
      "",
      "The positional `<nameOrToken>` can be the unique name of the deploy key (as passed to `token create`) or the deploy key value itself. The target deployment defaults to the currently-selected one; pass `--deployment` to target a different deployment.",
      "",
      "• Delete by name: `npx convex deployment token delete my-token`",
      "• Delete by value: `npx convex deployment token delete 'dev:happy-animal-123|ey...'`",
      "• Target prod: `npx convex deployment token delete ci-token --deployment prod`",
    ].join("\n"),
  )
  .argument(
    "<nameOrToken>",
    "The unique name of the deploy key, or the deploy key value itself.",
  )
  .allowExcessArguments(false)
  .addDeploymentSelectionOptions(actionDescription("Delete a deploy key for"))
  .showHelpAfterError()
  .action(async (nameOrToken, options) => {
    const ctx = await oneoffContext(options);

    await ensureLoggedInWithAccessToken(ctx, "Deleting a deploy key");

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
          "Cannot delete a deploy key for a self-hosted deployment.",
      });
    }

    const { deploymentName, deploymentType } = deployment.deploymentFields;
    if (deploymentType === "local" || deploymentType === "anonymous") {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `Cannot delete a deploy key for a ${deploymentType} deployment.`,
      });
    }

    // A full deploy key has the form `<type>:<deployment-name>|<token>`. If we
    // see the prefix without the `|`, the user almost certainly forgot to
    // quote: the shell ate `|` and everything after.
    if (/^(dev|prod|preview|local):[^|]*$/.test(nameOrToken)) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `"${nameOrToken}" looks like a partial deploy key — your shell likely consumed the \`|\` and everything after it. Wrap the value in single quotes (e.g. ${chalkStderr.bold(`npx convex deployment token delete '${nameOrToken}|...'`)}) and try again.`,
      });
    }
    // The server matches against just the token portion, so strip the prefix
    // when present so users can paste the value of CONVEX_DEPLOY_KEY directly.
    const pipeIdx = nameOrToken.indexOf("|");
    const id = pipeIdx >= 0 ? nameOrToken.slice(pipeIdx + 1) : nameOrToken;

    showSpinner(`Deleting deploy key for ${deploymentName}...`);
    await typedPlatformClient(ctx).POST(
      "/deployments/{deployment_name}/delete_deploy_key",
      {
        params: { path: { deployment_name: deploymentName } },
        body: { id },
      },
    );

    logFinishedStep(`Deleted deploy key for ${deploymentName}.`);
  });
