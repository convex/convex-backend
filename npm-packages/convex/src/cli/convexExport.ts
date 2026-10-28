import { Command } from "@commander-js/extra-typings";
import { chalkStderr } from "chalk";
import { ensureHasConvexDependency } from "./lib/utils/utils.js";
import { oneoffContext } from "../bundler/context.js";
import { loadSelectedDeploymentCredentials } from "./lib/api.js";
import { deploymentDashboardUrlPage } from "./lib/dashboard.js";
import { actionDescription } from "./lib/command.js";
import { exportFromDeployment } from "./lib/convexExport.js";
import { getDeploymentSelection } from "./lib/deploymentSelection.js";
export const convexExport = new Command("export")
  .summary("Export data from your deployment to a ZIP file")
  .description(
    [
      "Export data, and optionally file storage, from your Convex deployment to a ZIP file.",
      "",
      "• Export to a directory: `npx convex export --path dir/`",
      "• Export to a ZIP file: `npx convex export --path snapshot.zip`",
      "• Include file storage: `npx convex export --include-file-storage --path dir/`",
      "",
      "By default, this exports from your dev deployment.",
      "",
      "See the data export guide (https://docs.convex.dev/database/import-export/export) for details and use cases.",
    ].join("\n"),
  )
  .allowExcessArguments(false)
  .addExportOptions()
  .addDeploymentSelectionOptions(actionDescription("Export data from"))
  .showHelpAfterError()
  .action(async (options) => {
    const ctx = await oneoffContext(options);
    await ensureHasConvexDependency(ctx, "export");

    const deploymentSelection = await getDeploymentSelection(ctx, options);
    const deployment = await loadSelectedDeploymentCredentials(
      ctx,
      deploymentSelection,
    );

    const deploymentNotice = options.prod
      ? ` in your ${chalkStderr.bold("prod")} deployment`
      : "";
    await exportFromDeployment(ctx, {
      ...options,
      deploymentUrl: deployment.url,
      adminKey: deployment.adminKey,
      deploymentNotice,
      snapshotExportDashboardLink: deploymentDashboardUrlPage(
        deployment.deploymentFields?.deploymentName ?? null,
        "/settings/snapshots",
      ),
    });
  });
