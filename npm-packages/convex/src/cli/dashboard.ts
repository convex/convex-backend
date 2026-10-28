import { Command } from "@commander-js/extra-typings";
import { chalkStderr } from "chalk";
import open from "open";
import { Context, oneoffContext } from "../bundler/context.js";
import { logMessage, logOutput, logWarning } from "../bundler/log.js";
import { loadSelectedDeploymentCredentials } from "./lib/api.js";
import { actionDescription } from "./lib/command.js";
import { getDeploymentSelection } from "./lib/deploymentSelection.js";
import { DASHBOARD_HOST, getDashboardUrl } from "./lib/dashboard.js";
import { isAnonymousDeployment } from "./lib/deployment.js";

export const dashboard = new Command("dashboard")
  .alias("dash")
  .description("Open the dashboard in the browser")
  .allowExcessArguments(false)
  .option(
    "--no-open",
    "Don't automatically open the dashboard in the default browser",
  )
  .addDeploymentSelectionOptions(actionDescription("Open the dashboard for"))
  .showHelpAfterError()
  .action(async (options) => {
    const ctx = await oneoffContext(options);

    const deploymentSelection = await getDeploymentSelection(ctx, options);
    const deployment = await loadSelectedDeploymentCredentials(
      ctx,
      deploymentSelection,
      { ensureLocalRunning: false },
    );

    if (deployment.deploymentFields === null) {
      const msg = `Self-hosted deployment configured.\n\`${chalkStderr.bold("npx convex dashboard")}\` is not supported for self-hosted deployments.\nSee self-hosting instructions for how to self-host the dashboard.`;
      logMessage(chalkStderr.yellow(msg));
      return;
    }
    const dashboardUrl = await getDashboardUrl(
      ctx,
      deployment.deploymentFields,
    );
    if (isAnonymousDeployment(deployment.deploymentFields.deploymentName)) {
      const warningMessage = `You are not currently running the dashboard locally. Make sure \`npx convex dev\` is running and try again.`;
      if (dashboardUrl === null) {
        logWarning(warningMessage);
        return;
      }
      // The anonymous-mode dashboard is a separate local HTTP server; confirm
      // it's actually serving by hitting its API rather than inferring from the
      // backend.
      if (
        !(await isAnonymousDashboardRunning(
          dashboardUrl,
          deployment.deploymentFields.deploymentName,
        ))
      ) {
        logWarning(warningMessage);
        return;
      }
      await logOrOpenUrl(ctx, dashboardUrl, options.open);
      return;
    }

    await logOrOpenUrl(ctx, dashboardUrl ?? DASHBOARD_HOST, options.open);
  });

async function isAnonymousDashboardRunning(
  dashboardUrl: string,
  deploymentName: string,
): Promise<boolean> {
  try {
    // `dashboardUrl` ends with a trailing slash.
    const resp = await fetch(`${dashboardUrl}api/current_deployment`);
    if (resp.status !== 200) {
      return false;
    }
    // The dashboard port is stored per deployment but can be reused by a
    // different anonymous dev session, so confirm this dashboard is actually
    // serving the selected deployment.
    const currentDeployment = (await resp.json()) as { name?: unknown };
    return currentDeployment.name === deploymentName;
  } catch {
    return false;
  }
}

async function logOrOpenUrl(ctx: Context, url: string, shouldOpen: boolean) {
  if (shouldOpen) {
    logMessage(chalkStderr.gray(`Opening ${url} in the default browser...`));
    try {
      // This can fail e.g. on a headless dev machine.
      await open(url);
    } catch {
      logWarning(
        `⚠️ Could not open dashboard in the default browser.\nPlease visit: ${url}`,
      );
    }
  } else {
    logOutput(url);
  }
}
