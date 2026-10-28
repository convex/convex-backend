import { chalkStderr } from "chalk";
import { ensureHasConvexDependency } from "./lib/utils/utils.js";
import { oneoffContext } from "../bundler/context.js";
import { loadSelectedDeploymentCredentials } from "./lib/api.js";
import { Command } from "@commander-js/extra-typings";
import { actionDescription } from "./lib/command.js";
import { DASHBOARD_HOST, deploymentDashboardUrlPage } from "./lib/dashboard.js";
import { importIntoDeployment } from "./lib/convexImport.js";
import { getDeploymentSelection } from "./lib/deploymentSelection.js";

export const convexImport = new Command("import")
  .summary("Import data from a file to your deployment")
  .description(
    [
      "Import data from a file to your Convex deployment.",
      "",
      "• From a snapshot: `npx convex import snapshot.zip`",
      "• For a single table: `npx convex import --table tableName file.json`",
      "",
      "By default, this imports into your dev deployment.",
      "",
      "See the data import guide (https://docs.convex.dev/database/import-export/import) for details and use cases.",
    ].join("\n"),
  )
  .allowExcessArguments(false)
  .addImportOptions()
  .addDeploymentSelectionOptions(actionDescription("Import data into"))
  .showHelpAfterError()
  .action(async (filePath, options) => {
    const ctx = await oneoffContext(options);

    await ensureHasConvexDependency(ctx, "import");

    const deploymentSelection = await getDeploymentSelection(ctx, options);
    const deployment = await loadSelectedDeploymentCredentials(
      ctx,
      deploymentSelection,
    );

    const deploymentNotice = options.prod
      ? ` in your ${chalkStderr.bold("prod")} deployment`
      : "";

    await importIntoDeployment(ctx, filePath, {
      ...options,
      deploymentUrl: deployment.url,
      adminKey: deployment.adminKey,
      deploymentNotice,
      snapshotImportDashboardLink: snapshotImportDashboardLink(
        deployment.deploymentFields?.deploymentName ?? null,
      ),
    });
  });

function snapshotImportDashboardLink(deploymentName: string | null) {
  return deploymentName === null
    ? `${DASHBOARD_HOST}/deployment/settings/snapshots`
    : deploymentDashboardUrlPage(deploymentName, "/settings/snapshots");
}
