import { Command } from "@commander-js/extra-typings";
// eslint-disable-next-line no-restricted-imports -- stdout output uses default chalk
import chalk from "chalk";
import { oneoffContext } from "../bundler/context.js";
import { logOutput } from "../bundler/log.js";
import { loadSelectedDeploymentCredentials } from "./lib/api.js";
import { actionDescription } from "./lib/command.js";
import { deploymentDashboardUrlPage } from "./lib/dashboard.js";
import { getDeploymentSelection } from "./lib/deploymentSelection.js";
import { type Insight, fetchInsights } from "./lib/insights.js";

function formatInsightKind(kind: string): string {
  switch (kind) {
    case "occRetried":
      return "OCC Retried";
    case "occFailedPermanently":
      return "OCC Failed Permanently";
    case "bytesReadLimit":
      return "Bytes Read Limit Exceeded";
    case "bytesReadThreshold":
      return "Bytes Read Near Limit";
    case "documentsReadLimit":
      return "Documents Read Limit Exceeded";
    case "documentsReadThreshold":
      return "Documents Read Near Limit";
    default:
      return kind;
  }
}

function formatFunctionName(insight: Insight): string {
  if (insight.componentPath) {
    return `${insight.componentPath}:${insight.functionId}`;
  }
  return insight.functionId;
}

function formatInsight(insight: Insight, details: boolean): string {
  const severity =
    insight.severity === "error"
      ? chalk.red(`[ERROR]`)
      : chalk.yellow(`[WARNING]`);
  const kind = formatInsightKind(insight.kind);
  const fn = chalk.bold(formatFunctionName(insight));

  let detail: string;
  if ("occCalls" in insight) {
    const table = insight.occTableName
      ? ` on table ${chalk.cyan(insight.occTableName)}`
      : "";
    detail = `${insight.occCalls} OCC conflict${insight.occCalls !== 1 ? "s" : ""}${table}`;
  } else {
    detail = `${insight.count} occurrence${insight.count !== 1 ? "s" : ""}`;
  }

  let output = `${severity} ${kind}: ${fn} — ${detail}`;

  if (details && insight.recentEvents && insight.recentEvents.length > 0) {
    output += "\n";
    for (const event of insight.recentEvents) {
      const time = chalk.dim(new Date(event.timestamp).toLocaleString());
      const reqId = chalk.dim(`req:${event.request_id}`);

      if ("occ_retry_count" in event) {
        const docId = event.occ_document_id
          ? ` doc:${event.occ_document_id}`
          : "";
        const source = event.occ_write_source
          ? ` source:${event.occ_write_source}`
          : "";
        output += `    ${time}  ${reqId}  retries:${event.occ_retry_count}${docId}${source}\n`;
      } else {
        const status = event.success ? chalk.green("ok") : chalk.red("fail");
        const calls = event.calls
          .map(
            (c) =>
              `${c.table_name}(${c.documents_read} docs, ${c.bytes_read} bytes)`,
          )
          .join(", ");
        output += `    ${time}  ${reqId}  ${status}  ${calls}\n`;
      }
    }
  }

  return output;
}

export const insights = new Command("insights")
  .summary("Show health insights for your deployment")
  .description(
    [
      "Show health insights for a Convex deployment over the last 72 hours.",
      "Displays OCC conflicts and resource limit issues that may indicate performance problems.",
      "",
      "• Show insights: `npx convex insights`",
      "• Include recent events for each insight: `npx convex insights --details`",
      "• Check the production deployment: `npx convex insights --prod`",
      "",
      "This command is only available for Convex cloud deployments when logged in as a user.",
    ].join("\n"),
  )
  .allowExcessArguments(false)
  .option("--details", "Show recent events for each insight", false)
  .option("--json", "Output insights as JSON", false)
  .addDeploymentSelectionOptions(actionDescription("Show insights for"))
  .showHelpAfterError()
  .action(async (cmdOptions) => {
    const ctx = await oneoffContext(cmdOptions);

    const deploymentSelection = await getDeploymentSelection(ctx, cmdOptions);
    const credentials = await loadSelectedDeploymentCredentials(
      ctx,
      deploymentSelection,
    );

    const deploymentName = credentials.deploymentFields?.deploymentName ?? null;
    if (deploymentName === null) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage:
          "Insights are only available for cloud deployments. Local deployments do not have insights data.",
      });
    }

    const auth = ctx.bigBrainAuth();
    if (
      auth === null ||
      auth.kind === "deploymentKey" ||
      auth.kind === "projectKey"
    ) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage:
          "Insights require to be logged in as a user. Deploy keys and project keys cannot access team usage data.",
      });
    }

    const insightsList = await fetchInsights(ctx, deploymentName, {
      includeRecentEvents: cmdOptions.details,
    });

    const dashboardUrl = deploymentDashboardUrlPage(
      deploymentName,
      "?view=insights",
    );

    if (cmdOptions.json) {
      logOutput(
        JSON.stringify(
          { deploymentName, dashboardUrl, insights: insightsList },
          null,
          2,
        ),
      );
      return;
    }

    if (insightsList.length === 0) {
      logOutput(
        chalk.green(
          "No issues found. The deployment is healthy over the last 72 hours.",
        ),
      );
    } else {
      const errorCount = insightsList.filter(
        (i) => i.severity === "error",
      ).length;
      const warningCount = insightsList.filter(
        (i) => i.severity === "warning",
      ).length;

      const parts: string[] = [];
      if (errorCount > 0)
        parts.push(
          chalk.red(`${errorCount} error${errorCount > 1 ? "s" : ""}`),
        );
      if (warningCount > 0)
        parts.push(
          chalk.yellow(`${warningCount} warning${warningCount > 1 ? "s" : ""}`),
        );
      logOutput(`Found ${parts.join(" and ")} in the last 72 hours:\n`);

      for (const insight of insightsList) {
        logOutput(formatInsight(insight, cmdOptions.details));
      }
    }

    logOutput(`\nDashboard: ${chalk.cyan(dashboardUrl)}`);
  });
