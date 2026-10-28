import { Command, Option } from "@commander-js/extra-typings";
import { Context } from "../bundler/context.js";
import { logFinishedStep, logMessage, logOutput } from "../bundler/log.js";
import { DeploymentSelectionOptions } from "./lib/api.js";
import { actionDescription } from "./lib/command.js";
import { ensureHasConvexDependency } from "./lib/utils/utils.js";
import { withRunningBackend } from "./lib/localDeployment/run.js";
import { selectEnvDeployment } from "./env.js";
import {
  compareMetricNames,
  createUsageLimit,
  deleteUsageLimit,
  findUsageLimitByKey,
  getCurrentUsage,
  listUsageLimits,
  listUsageLimitsWithStatus,
  metricLabel,
  updateUsageLimit,
  USAGE_LIMIT_METRICS,
  USAGE_LIMIT_TYPES,
  USAGE_LIMIT_WINDOWS,
  UsageLimitStatus,
} from "./lib/usageLimits.js";

async function parseLimit(ctx: Context, value: string): Promise<number> {
  const limit = Number(value);
  if (!Number.isInteger(limit) || limit < 1) {
    return ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `error: --limit must be a positive integer, got "${value}".`,
    });
  }
  return limit;
}

function formatTable(
  header: string[],
  rows: string[][],
  rightAlign: number[] = [],
): string {
  const right = new Set(rightAlign);
  const widths = header.map((cell, i) =>
    Math.max(cell.length, ...rows.map((row) => (row[i] ?? "").length)),
  );
  const pad = (cell: string, i: number) =>
    right.has(i) ? cell.padStart(widths[i]) : cell.padEnd(widths[i]);
  const rule = (left: string, mid: string, r: string) =>
    left + widths.map((w) => "\u2500".repeat(w + 2)).join(mid) + r;
  const line = (cells: string[]) =>
    "\u2502 " +
    cells.map((cell, i) => pad(cell ?? "", i)).join(" \u2502 ") +
    " \u2502";
  return [
    rule("\u250c", "\u252c", "\u2510"),
    line(header),
    rule("\u251c", "\u253c", "\u2524"),
    ...rows.map(line),
    rule("\u2514", "\u2534", "\u2518"),
  ].join("\n");
}

const NUMBER_FORMAT_COMPACT = new Intl.NumberFormat("en-US", {
  notation: "compact",
  compactDisplay: "short",
  maximumFractionDigits: 3,
});
function formatNumberCompact(value: number): string {
  const formatted = NUMBER_FORMAT_COMPACT.format(value);
  return formatted === "-0" ? "0" : formatted;
}

// Exact, thousands-separated amount for prose (success messages), where a
// precise value reads clearer than the compact table form.
const LIMIT_FORMAT = new Intl.NumberFormat("en-US");
function formatLimitAmount(value: number): string {
  return LIMIT_FORMAT.format(value);
}

const NUMBER_FORMAT = new Intl.NumberFormat("en-US");
function formatNumber(value: number): string {
  const formatted = NUMBER_FORMAT.format(value);
  return formatted === "-0" ? "0" : formatted;
}

function unitFor(amount: number, unit: string): string {
  return amount === 1 && unit === "calls" ? "call" : unit;
}

function formatAmount(value: number, unit: string | null): string {
  return unit === null
    ? formatNumberCompact(value)
    : `${formatNumberCompact(value)} ${unitFor(value, unit)}`;
}

function usageLimitRow(limit: UsageLimitStatus): string[] {
  const currentUsage =
    limit.currentUsage === null
      ? "\u2014"
      : `${formatAmount(limit.currentUsage, limit.unit)} (${formatNumber(
          Math.round((limit.currentUsage / limit.limit) * 100),
        )}%)`;
  return [
    metricLabel(limit.metric),
    limit.window,
    limit.limitType,
    formatAmount(limit.limit, limit.unit),
    currentUsage,
    limit.enabled ? "yes" : "no",
    limit.triggered ? "yes" : "no",
  ];
}

// The (metric, window, type) triple uniquely identifies a usage limit. `set`
// requires all three plus a limit; `update`/`remove` reuse them to locate an
// existing limit without needing its opaque id.
const metricOption = new Option(
  "--metric <metric>",
  "The metric to limit.",
).choices(USAGE_LIMIT_METRICS);
const windowOption = new Option(
  "--window <window>",
  "The window the limit is measured over.",
).choices(USAGE_LIMIT_WINDOWS);
const typeOption = new Option(
  "--type <type>",
  "`warning` only notifies; `disable` pauses the deployment when exceeded.",
).choices(USAGE_LIMIT_TYPES);

function seedStatusMessage(
  seedStatus: "pending" | "partial" | "failed",
): string {
  return seedStatus === "failed"
    ? "We couldn't load this deployment's historical usage, so the usage shown below may understate its actual usage. Limits are still enforced going forward."
    : "Historical usage is still being loaded, so the usage shown below may understate this deployment's actual usage. Check back shortly for accurate totals.";
}

const listCmd = new Command("list")
  .summary("List configured usage limits")
  .description(
    [
      "List the usage limits configured on your deployment.",
      "",
      "• List all usage limits: `npx convex deployment usage-limits list`",
      "• Print as JSON: `npx convex deployment usage-limits list --json`",
    ].join("\n"),
  )
  .option("--json", "Output the usage limits as JSON.")
  .configureHelp({ showGlobalOptions: true })
  .allowExcessArguments(false)
  .action(async (cmdOptions, cmd) => {
    const options = cmd.optsWithGlobals() as DeploymentSelectionOptions & {
      json?: boolean;
    };
    const { ctx, deployment } = await selectEnvDeployment(options);
    await ensureHasConvexDependency(ctx, "deployment usage-limits list");
    await withRunningBackend({
      ctx,
      deployment,
      action: async () => {
        const { limits, seedStatus } = await listUsageLimitsWithStatus(
          ctx,
          deployment,
        );
        if (cmdOptions.json) {
          logOutput(JSON.stringify(limits, null, 2));
          return;
        }
        if (limits.length === 0) {
          logMessage(
            `No usage limits configured${deployment.deploymentNotice}.`,
          );
          return;
        }
        // "Triggered" is derived from reported usage, which a non-`complete`
        // backfill may understate.
        if (seedStatus !== "complete") {
          logMessage(seedStatusMessage(seedStatus));
        }
        logOutput(
          formatTable(
            [
              "Metric",
              "Window",
              "Type",
              "Limit",
              "Current Usage",
              "Active",
              "Triggered",
            ],
            limits.map(usageLimitRow),
            [3, 4],
          ),
        );
      },
    });
  });

const setCmd = new Command("set")
  .summary("Create or update a usage limit")
  .description(
    [
      "Create a usage limit, or update the existing one for the same",
      "(metric, window, type). At most one limit exists per combination.",
      "",
      "• Set the amount (creates or replaces it):",
      "  `npx convex deployment usage-limits set --metric functionCalls --window day --type disable --limit 1000000`",
      "• Deactivate without deleting: add `--inactive` (use `--active` to re-enable).",
      "• Toggle active state without changing the amount: omit `--limit`.",
    ].join("\n"),
  )
  .addOption(metricOption.makeOptionMandatory())
  .addOption(windowOption.makeOptionMandatory())
  .addOption(typeOption.makeOptionMandatory())
  .option(
    "--limit <limit>",
    "The limit amount, in the metric's native units. Required when creating; kept as-is when omitted while updating.",
  )
  .option("--active", "Enforce the limit (the default for a new limit).")
  .option("--inactive", "Create or leave the limit unenforced.")
  .configureHelp({ showGlobalOptions: true })
  .allowExcessArguments(false)
  .action(async (cmdOptions, cmd) => {
    const options = cmd.optsWithGlobals() as DeploymentSelectionOptions;
    const { ctx, deployment } = await selectEnvDeployment(options);
    await ensureHasConvexDependency(ctx, "deployment usage-limits set");
    if (cmdOptions.active && cmdOptions.inactive) {
      return ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: "error: Pass at most one of --active and --inactive.",
      });
    }
    const newLimit =
      cmdOptions.limit === undefined
        ? undefined
        : await parseLimit(ctx, cmdOptions.limit);
    await withRunningBackend({
      ctx,
      deployment,
      action: async () => {
        const existing = (await listUsageLimits(ctx, deployment)).find(
          (l) =>
            l.metric === cmdOptions.metric &&
            l.window === cmdOptions.window &&
            l.limitType === cmdOptions.type,
        );
        const label = `${cmdOptions.type} usage limit on ${metricLabel(cmdOptions.metric)} per ${cmdOptions.window}`;

        // No existing limit: create one. A limit amount is required.
        if (existing === undefined) {
          if (newLimit === undefined) {
            return ctx.crash({
              exitCode: 1,
              errorType: "fatal",
              printedMessage:
                "error: --limit is required when creating a usage limit.",
            });
          }
          const enabled = !cmdOptions.inactive;
          const created = await createUsageLimit(ctx, deployment, {
            metric: cmdOptions.metric,
            window: cmdOptions.window,
            limitType: cmdOptions.type,
            limit: newLimit,
            enabled,
          });
          logFinishedStep(
            `Created ${label}: ${formatLimitAmount(created.limit)}, ${created.enabled ? "active" : "inactive"}${deployment.deploymentNotice}.`,
          );
          return;
        }

        // Existing limit: update it, surfacing exactly which fields changed.
        const enabled = cmdOptions.active
          ? true
          : cmdOptions.inactive
            ? false
            : existing.enabled;
        const limit = newLimit ?? existing.limit;
        const changes: string[] = [];
        if (limit !== existing.limit) {
          changes.push(
            `limit ${formatLimitAmount(existing.limit)} \u2192 ${formatLimitAmount(limit)}`,
          );
        }
        if (enabled !== existing.enabled) {
          changes.push(
            `${existing.enabled ? "active" : "inactive"} \u2192 ${enabled ? "active" : "inactive"}`,
          );
        }
        if (changes.length === 0) {
          logFinishedStep(
            `No changes to ${label} (${formatLimitAmount(existing.limit)}, ${existing.enabled ? "active" : "inactive"})${deployment.deploymentNotice}.`,
          );
          return;
        }
        await updateUsageLimit(ctx, deployment, existing.id, {
          metric: existing.metric,
          window: existing.window,
          limitType: existing.limitType,
          limit,
          enabled,
        });
        logFinishedStep(
          `Updated ${label}: ${changes.join(", ")}${deployment.deploymentNotice}.`,
        );
      },
    });
  });

const removeCmd = new Command("remove")
  .alias("rm")
  .alias("delete")
  .summary("Delete a usage limit")
  .description(
    [
      "Delete a usage limit, identified by its (metric, window, type).",
      "",
      "• `npx convex deployment usage-limits remove --metric functionCalls --window day --type warning`",
    ].join("\n"),
  )
  .addOption(metricOption.makeOptionMandatory())
  .addOption(windowOption.makeOptionMandatory())
  .addOption(typeOption.makeOptionMandatory())
  .configureHelp({ showGlobalOptions: true })
  .allowExcessArguments(false)
  .action(async (cmdOptions, cmd) => {
    const options = cmd.optsWithGlobals() as DeploymentSelectionOptions;
    const { ctx, deployment } = await selectEnvDeployment(options);
    await ensureHasConvexDependency(ctx, "deployment usage-limits remove");
    await withRunningBackend({
      ctx,
      deployment,
      action: async () => {
        const existing = await findUsageLimitByKey(ctx, deployment, {
          metric: cmdOptions.metric,
          window: cmdOptions.window,
          limitType: cmdOptions.type,
        });
        await deleteUsageLimit(ctx, deployment, existing.id);
        logFinishedStep(
          `Deleted ${existing.limitType} usage limit on ${metricLabel(existing.metric)} per ${existing.window}${deployment.deploymentNotice}.`,
        );
      },
    });
  });

export const usage = new Command("usage")
  .summary("Show current usage for each metric")
  .description(
    [
      "Show usage so far in the current day and calendar month for every metric.",
      "",
      "• Show current usage: `npx convex deployment usage`",
      "• Print as JSON: `npx convex deployment usage --json`",
    ].join("\n"),
  )
  .option("--json", "Output the usage as JSON.")
  .configureHelp({ showGlobalOptions: true })
  .allowExcessArguments(false)
  .addDeploymentSelectionOptions(actionDescription("Show current usage for"))
  .action(async (cmdOptions, cmd) => {
    const options = cmd.optsWithGlobals() as DeploymentSelectionOptions & {
      json?: boolean;
    };
    const { ctx, deployment } = await selectEnvDeployment(options);
    await ensureHasConvexDependency(ctx, "deployment usage");
    await withRunningBackend({
      ctx,
      deployment,
      action: async () => {
        const usage = await getCurrentUsage(ctx, deployment);
        if (cmdOptions.json) {
          logOutput(JSON.stringify(usage, null, 2));
          return;
        }
        // A non-`complete` backfill means the numbers can understate reality.
        if (usage.seedStatus !== "complete") {
          logMessage(seedStatusMessage(usage.seedStatus));
        }
        logOutput(
          formatTable(
            ["Metric", "Day", "Month"],
            Object.entries(usage.metrics)
              .sort(([a], [b]) => compareMetricNames(a, b))
              .map(([metric, m]) => [
                metricLabel(metric),
                formatAmount(m.usage.current_day, m.unit),
                formatAmount(m.usage.current_month, m.unit),
              ]),
          ),
        );
      },
    });
  });

export const usageLimits = new Command("usage-limits")
  .summary("List and configure deployment usage limits")
  .description(
    [
      "List and configure usage limits on your deployment.",
      "",
      "A usage limit either warns or pauses your deployment when a metric",
      "(function calls, database bandwidth, …) crosses a threshold within a",
      "daily or monthly window. Each limit is identified by its",
      "(metric, window, type).",
      "",
      "• List usage limits: `npx convex deployment usage-limits list`",
      "• Create or update one: `npx convex deployment usage-limits set --metric functionCalls --window day --type disable --limit 1000000`",
      "• Delete one: `npx convex deployment usage-limits remove --metric functionCalls --window day --type disable`",
    ].join("\n"),
  )
  .addCommand(listCmd)
  .addCommand(setCmd)
  .addCommand(removeCmd)
  .helpCommand(false)
  .addDeploymentSelectionOptions(
    actionDescription("List and configure usage limits on"),
  );
