import { Command } from "@commander-js/extra-typings";
import { oneoffContext } from "../bundler/context.js";
import { logWarning } from "../bundler/log.js";
import { loadSelectedDeploymentCredentials } from "./lib/api.js";
import { actionDescription } from "./lib/command.js";
import { logsForDeployment } from "./lib/logs.js";
import { getDeploymentSelection } from "./lib/deploymentSelection.js";
import { announceDeploymentTarget } from "./lib/announceDeploymentTarget.js";

export const logs = new Command("logs")
  .summary("Watch logs from your deployment")
  .description(
    "Stream function logs from your Convex deployment.\nBy default, this streams from your project's dev deployment.",
  )
  .allowExcessArguments(false)
  .addLogsOptions()
  .addDeploymentSelectionOptions(actionDescription("Watch logs from"))
  .showHelpAfterError()
  .action(async (cmdOptions) => {
    const ctx = await oneoffContext(cmdOptions);

    const deploymentSelection = await getDeploymentSelection(ctx, cmdOptions);
    const deployment = await loadSelectedDeploymentCredentials(
      ctx,
      deploymentSelection,
    );
    announceDeploymentTarget("Showing logs of deployment:", deployment);
    const deploymentName = deployment.deploymentFields?.deploymentName
      ? ` ${deployment.deploymentFields.deploymentName}`
      : "";
    const deploymentNotice = ` for ${cmdOptions.prod ? "production" : "dev"} deployment${deploymentName}`;
    let history = cmdOptions.history;
    if (cmdOptions.tail !== undefined) {
      logWarning(
        "`--tail` is unnecessary: `convex logs` already tails by default. Treating it as `--history`.",
      );
      if (typeof cmdOptions.tail === "number" && history === undefined) {
        history = cmdOptions.tail;
      }
    }
    await logsForDeployment(ctx, deployment, {
      history,
      success: cmdOptions.success,
      jsonl: cmdOptions.jsonl,
      deploymentNotice,
    });
  });
