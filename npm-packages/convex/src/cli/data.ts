import { chalkStderr } from "chalk";
import { oneoffContext } from "../bundler/context.js";
import { loadSelectedDeploymentCredentials } from "./lib/api.js";
import { Command } from "@commander-js/extra-typings";
import { actionDescription } from "./lib/command.js";
import { dataInDeployment } from "./lib/data.js";
import { getDeploymentSelection } from "./lib/deploymentSelection.js";

export const data = new Command("data")
  .summary("List tables and print data from your database")
  .description(
    [
      "Inspect your Convex deployment's database.",
      "",
      "• List tables: `npx convex data`",
      "• List documents in a table: `npx convex data tableName`",
      "",
      "By default, this inspects your dev deployment.",
      "",
      "This works with system tables, such as `_storage`, in addition to your own tables.",
    ].join("\n"),
  )
  .allowExcessArguments(false)
  .addDataOptions()
  .addDeploymentSelectionOptions(actionDescription("Inspect the database in"))
  .showHelpAfterError()
  .action(async (tableName, options) => {
    const ctx = await oneoffContext(options);

    const deploymentSelection = await getDeploymentSelection(ctx, options);
    const deployment = await loadSelectedDeploymentCredentials(
      ctx,
      deploymentSelection,
    );

    const deploymentNotice = deployment.deploymentFields?.deploymentName
      ? `${chalkStderr.bold(deployment.deploymentFields.deploymentName)} deployment's `
      : "";

    await dataInDeployment(ctx, {
      deploymentUrl: deployment.url,
      adminKey: deployment.adminKey,
      deploymentNotice,
      tableName,
      ...options,
    });
  });
