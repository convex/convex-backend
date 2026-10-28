import { z } from "zod";
import { ConvexTool } from "./index.js";
import { loadSelectedDeploymentCredentials } from "../../api.js";
import { deploymentDashboardUrlPage } from "../../../lib/dashboard.js";
import { fetchInsights } from "../../insights.js";
import { getMcpDeploymentSelection } from "../requestContext.js";

const inputSchema = z.object({
  deploymentSelector: z
    .string()
    .describe(
      "Deployment selector (from the status tool) to fetch insights for.",
    ),
});

const occRecentEventSchema = z.object({
  timestamp: z.string(),
  id: z.string(),
  request_id: z.string(),
  occ_document_id: z.string().optional(),
  occ_write_source: z.string().optional(),
  occ_retry_count: z.number(),
});

const resourceRecentEventSchema = z.object({
  timestamp: z.string(),
  id: z.string(),
  request_id: z.string(),
  calls: z.array(
    z.object({
      table_name: z.string(),
      bytes_read: z.number(),
      documents_read: z.number(),
    }),
  ),
  success: z.boolean(),
});

const insightSchema = z.discriminatedUnion("kind", [
  z.object({
    kind: z.literal("occRetried"),
    severity: z.literal("warning"),
    functionId: z.string(),
    componentPath: z.string().nullable(),
    occCalls: z.number(),
    occTableName: z.string().optional(),
    recentEvents: z.array(occRecentEventSchema),
  }),
  z.object({
    kind: z.literal("occFailedPermanently"),
    severity: z.literal("error"),
    functionId: z.string(),
    componentPath: z.string().nullable(),
    occCalls: z.number(),
    occTableName: z.string().optional(),
    recentEvents: z.array(occRecentEventSchema),
  }),
  z.object({
    kind: z.literal("bytesReadLimit"),
    severity: z.literal("error"),
    functionId: z.string(),
    componentPath: z.string().nullable(),
    count: z.number(),
    recentEvents: z.array(resourceRecentEventSchema),
  }),
  z.object({
    kind: z.literal("bytesReadThreshold"),
    severity: z.literal("warning"),
    functionId: z.string(),
    componentPath: z.string().nullable(),
    count: z.number(),
    recentEvents: z.array(resourceRecentEventSchema),
  }),
  z.object({
    kind: z.literal("documentsReadLimit"),
    severity: z.literal("error"),
    functionId: z.string(),
    componentPath: z.string().nullable(),
    count: z.number(),
    recentEvents: z.array(resourceRecentEventSchema),
  }),
  z.object({
    kind: z.literal("documentsReadThreshold"),
    severity: z.literal("warning"),
    functionId: z.string(),
    componentPath: z.string().nullable(),
    count: z.number(),
    recentEvents: z.array(resourceRecentEventSchema),
  }),
]);

const outputSchema = z.object({
  insights: z.array(insightSchema),
  summary: z.string(),
  dashboardUrl: z.string(),
});

const description = `
Fetch health insights for a Convex deployment over the last 72 hours.

Returns OCC (Optimistic Concurrency Control) conflicts and resource limit issues
that may indicate performance problems or failing functions.

**OCC insights** (occRetried, occFailedPermanently):
  Mutations that conflict on the same document. To fix: restructure mutations to
  touch fewer shared documents, split hot documents, or reduce transaction scope.

**Resource limit insights** (bytesReadLimit, documentsReadLimit, bytesReadThreshold, documentsReadThreshold):
  Functions reading too much data. To fix: add indexes to avoid full table scans,
  use pagination, or filter data more precisely in queries.

Severity levels:
  - "error": Function executions are failing (permanent OCC failures or hard limits hit)
  - "warning": Function executions succeed but are at risk (retried OCCs or approaching limits)

Use the logs tool with status "failure" to see individual error messages and stack traces.

Only available for Convex cloud deployments when logged in as a user.
`.trim();

export const InsightsTool: ConvexTool<typeof inputSchema, typeof outputSchema> =
  {
    name: "insights",
    description,
    inputSchema,
    outputSchema,
    handler: async (ctx, args) => {
      const { projectDir, deployment } = ctx.decodeDeploymentSelectorUnchecked(
        args.deploymentSelector,
      );
      process.chdir(projectDir);
      const deploymentSelection = await getMcpDeploymentSelection(
        ctx,
        deployment,
      );
      const credentials = await loadSelectedDeploymentCredentials(
        ctx,
        deploymentSelection,
      );

      const deploymentName =
        credentials.deploymentFields?.deploymentName ?? null;
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

      const insights = await fetchInsights(ctx, deploymentName, {
        includeRecentEvents: true,
      });

      const errorCount = insights.filter((i) => i.severity === "error").length;
      const warningCount = insights.filter(
        (i) => i.severity === "warning",
      ).length;

      let summary: string;
      if (insights.length === 0) {
        summary =
          "No issues found. The deployment is healthy over the last 72 hours.";
      } else {
        const parts: string[] = [];
        if (errorCount > 0)
          parts.push(`${errorCount} error${errorCount > 1 ? "s" : ""}`);
        if (warningCount > 0)
          parts.push(`${warningCount} warning${warningCount > 1 ? "s" : ""}`);
        summary = `Found ${parts.join(" and ")} in the last 72 hours.`;
      }

      const dashboardUrl = deploymentDashboardUrlPage(
        deploymentName,
        "?view=insights",
      );

      // Cast needed: fetchInsights returns Insight[] with optional recentEvents,
      // but the zod schema requires them. They're always present when
      // includeRecentEvents is true.
      return {
        insights: insights as z.infer<typeof insightSchema>[],
        summary,
        dashboardUrl,
      };
    },
  };
