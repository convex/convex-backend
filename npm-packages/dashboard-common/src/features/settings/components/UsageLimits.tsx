import { useCallback, useEffect, useState, type ReactNode } from "react";
import { useRouter } from "next/router";
import { Link } from "@ui/Link";
import {
  PlusCircledIcon,
  DotsVerticalIcon,
  ExclamationTriangleIcon,
  QuestionMarkCircledIcon,
} from "@radix-ui/react-icons";
import { Sheet } from "@ui/Sheet";
import { Button } from "@ui/Button";
import { Menu, MenuItem } from "@ui/Menu";
import { Checkbox } from "@ui/Checkbox";
import { TextInput } from "@ui/TextInput";
import { Tooltip } from "@ui/Tooltip";
import { Loading } from "@ui/Loading";
import { SegmentedControl } from "@ui/SegmentedControl";
import { Donut } from "@ui/Donut";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { cn } from "@ui/cn";
import type { DeploymentType } from "@convex-dev/platform/managementApi";
import type {
  SeedStatusResponse,
  UsageLimitConfigRequest,
  UsageLimitConfigResponse,
} from "@convex-dev/platform/deploymentApi";
import { formatNumberCompact } from "@common/lib/format";

// The usage metrics that a deployment-level usage limit can be applied to.
// These match the `UsageLimitMetric` enum on the backend (serialized with
// serde/strum `camelCase`), so the string values are sent to the usage limits
// API verbatim.
export type UsageMetric =
  | "functionCalls"
  | "queryMutationComputeGbHours"
  | "actionComputeConvexGbHours"
  | "actionComputeNodeJsGbHours"
  | "actionComputeCpuGbHours"
  | "databaseIoGb"
  | "searchQueryGb"
  | "dataEgressGb"
  | "aiGatewayCostDollars";

export type UsageLimitWindow = "day" | "month";

export type UsageLimitType = "warning" | "disable";

// The editable configuration of a usage limit.
export type UsageLimitConfig = Omit<
  UsageLimitConfigRequest,
  "metric" | "window" | "limitType"
> & {
  metric: UsageMetric;
  window: UsageLimitWindow;
  limitType: UsageLimitType;
};

export type UsageLimit = UsageLimitConfig &
  Pick<UsageLimitConfigResponse, "id">;

export type CurrentUsage = Partial<
  Record<UsageMetric, Partial<Record<UsageLimitWindow, number>>>
>;

export type UsageSeedStatus = SeedStatusResponse;

export const MAX_USAGE_LIMIT_VALUE = 100_000_000_000_000;

type MetricConfig = {
  name: string;
  description: string;
  // Long unit label used in raw mode (e.g. "GB-hours").
  rawUnit: string;
  // Compact unit label shown inline next to inputs (e.g. "GBh").
  rawUnitShort: string;
  // Singular form of `rawUnitShort`, for amounts of exactly 1 (e.g. "call" vs
  // "calls"). Omitted for unit symbols that don't inflect (GB, GBh, qGB).
  rawUnitShortSingular?: string;
  // Increment used by the numeric input, and the placeholder amount hint.
  rawStep: number;
  defaultAmount: number;
};

export const METRIC_CONFIG: Record<UsageMetric, MetricConfig> = {
  functionCalls: {
    name: "Function calls",
    description:
      "Total number of query, mutation, action, HTTP action, and file storage calls.",
    rawUnit: "function calls",
    rawUnitShort: "calls",
    rawUnitShortSingular: "call",
    rawStep: 1_000_000,
    defaultAmount: 50_000_000,
  },
  queryMutationComputeGbHours: {
    name: "Query/Mutation compute",
    description: "Compute consumed running queries and mutations.",
    rawUnit: "GB-hours",
    rawUnitShort: "GBh",
    rawStep: 1,
    defaultAmount: 100,
  },
  actionComputeConvexGbHours: {
    name: "Action compute",
    description: "Compute consumed running actions in the Convex runtime.",
    rawUnit: "GB-hours",
    rawUnitShort: "GBh",
    rawStep: 1,
    defaultAmount: 100,
  },
  actionComputeNodeJsGbHours: {
    name: "Action compute (Node.js)",
    description: "Compute consumed running actions in the Node.js runtime.",
    rawUnit: "GB-hours",
    rawUnitShort: "GBh",
    rawStep: 1,
    defaultAmount: 100,
  },
  actionComputeCpuGbHours: {
    name: "Action compute (CPU)",
    description: "CPU time consumed running actions.",
    rawUnit: "GB-hours",
    rawUnitShort: "GBh",
    rawStep: 1,
    // $0.30 per GB-hour → ~$100/mo.
    defaultAmount: 100,
  },
  databaseIoGb: {
    name: "Database I/O",
    description: "Bandwidth used reading from and writing to the database.",
    rawUnit: "GB",
    rawUnitShort: "GB",
    rawStep: 1,
    defaultAmount: 1000,
  },
  searchQueryGb: {
    name: "Search queries",
    description: "Bandwidth used serving text and vector search queries.",
    rawUnit: "query-GB",
    rawUnitShort: "qGB",
    rawStep: 1,
    defaultAmount: 1_000_000,
  },
  dataEgressGb: {
    name: "Data egress",
    description:
      "Bandwidth used serving file downloads, outgoing fetch requests, log streams, and streaming export.",
    rawUnit: "GB",
    rawUnitShort: "GB",
    rawStep: 1,
    defaultAmount: 1000,
  },
  aiGatewayCostDollars: {
    name: "AI Gateway",
    description:
      "Raw USD cost of Convex-managed AI models through the AI Gateway.",
    rawUnit: "USD",
    rawUnitShort: "USD",
    rawStep: 1,
    defaultAmount: 100,
  },
};

const METRIC_ORDER: UsageMetric[] = [
  "functionCalls",
  "queryMutationComputeGbHours",
  "actionComputeConvexGbHours",
  "actionComputeNodeJsGbHours",
  "actionComputeCpuGbHours",
  "databaseIoGb",
  "searchQueryGb",
  "dataEgressGb",
  "aiGatewayCostDollars",
];

// Whether Convex sends a notification email when a usage limit is exceeded.
// Dev deployments never do (they're personal to the member who created them);
// prod/preview/custom deployments email all team members. An unknown type
// (e.g. self-hosted) is treated like the latter.
function sendsEmail(deploymentType: DeploymentType | undefined) {
  return deploymentType !== "dev";
}

// What each limit type does when the limit is exceeded, shown as a tooltip on
// the row's type label. Dev deployments send no email, so their "warning"
// threshold is disabled and their "disable" threshold notes no email is sent.
function actionDescription(
  limitType: UsageLimitType,
  deploymentType: DeploymentType | undefined,
): string {
  const emails = sendsEmail(deploymentType);
  switch (limitType) {
    case "warning":
      return emails
        ? "When exceeded, Convex emails all team members."
        : "Development deployments don't receive email notifications, so this threshold isn't available.";
    case "disable":
      return emails
        ? "When exceeded, Convex emails all team members and disables the deployment for the rest of the window, and all function calls will fail."
        : "When exceeded, Convex disables the deployment for the rest of the window, and all function calls will fail. Development deployments don't receive email notifications.";
    default: {
      const _exhaustive: never = limitType;
      return _exhaustive;
    }
  }
}

// Short label for each limit type. A metric card shows one row per type.
export const LIMIT_TYPE_LABEL: Record<UsageLimitType, string> = {
  warning: "Warning threshold",
  disable: "Disable threshold",
};

const LIMIT_TYPE_ORDER: UsageLimitType[] = ["warning", "disable"];

// The window segmented control, ordered coarsest-first per the design.
const WINDOW_ORDER: UsageLimitWindow[] = ["month", "day"];
const WINDOW_LABEL: Record<UsageLimitWindow, string> = {
  month: "Monthly",
  day: "Daily",
};

// Suffix shown after a limit's unit to convey the window it's enforced over
// (e.g. "10 GB / month").
export const WINDOW_SUFFIX: Record<UsageLimitWindow, string> = {
  month: "/ month",
  day: "/ day",
};

// Explains when usage resets for the selected window, shown beneath the window
// segmented control.
const WINDOW_RESET_DESCRIPTION: Record<UsageLimitWindow, string> = {
  month: "Monthly usage resets on the first of the month, at midnight UTC.",
  day: "Daily usage resets at midnight UTC.",
};

// Whether a limit is currently triggered: it's enforced (enabled) and this
// window's usage has reached its threshold. Derived from the reported current
// usage, so it only reflects usage the backfill has hydrated (see seed status).
function isLimitTriggered(limit: UsageLimit, currentUsage: CurrentUsage) {
  const used = currentUsage[limit.metric]?.[limit.window];
  return limit.enabled && used !== undefined && used >= limit.limit;
}

// How many triggered warning- and disable-type limits a window currently has.
function windowTriggerCounts(
  usageLimits: UsageLimit[],
  currentUsage: CurrentUsage,
  window: UsageLimitWindow,
): { warning: number; disable: number } {
  const triggered = usageLimits.filter(
    (limit) => limit.window === window && isLimitTriggered(limit, currentUsage),
  );
  return {
    warning: triggered.filter((limit) => limit.limitType === "warning").length,
    disable: triggered.filter((limit) => limit.limitType === "disable").length,
  };
}

// Truncate rather than round to 2 decimals so amounts match the compact figure
// shown inline (formatNumberCompact also uses roundingMode: "trunc"): a value
// like 0.0375 reads as "0.03" in both the inline usage and its tooltip, not
// "0.03" inline and "0.04" in the tooltip.
const AMOUNT_FORMAT = new Intl.NumberFormat("en-US", {
  maximumFractionDigits: 2,
  roundingMode: "trunc",
});

// Format a usage amount. Intl renders -0 (and tiny negatives that round to
// zero) as "-0"; never surface a signed zero.
export function formatAmount(value: number): string {
  const formatted = AMOUNT_FORMAT.format(value);
  return formatted === "-0" ? "0" : formatted;
}

// The compact unit label for an amount, using the singular form when the amount
// is exactly 1 (e.g. "1 call" vs "5 calls"). Unit symbols that don't inflect
// (GB, GBh, qGB) have no singular form and are returned unchanged.
function rawUnitShortFor(config: MetricConfig, amount: number): string {
  return amount === 1 && config.rawUnitShortSingular !== undefined
    ? config.rawUnitShortSingular
    : config.rawUnitShort;
}

// Column template shared by the header row and every metric row so the table's
// columns line up: metric name, current usage, then a Warning and a Disable
// threshold column. The threshold columns have a 16rem floor so the inline
// editor always fits; they flex wider when there's room so a configured
// limit's gauge, amount, and pills stay on one line.
const TABLE_GRID =
  "grid grid-cols-[minmax(10rem,1fr)_minmax(7rem,0.5fr)_minmax(16rem,1.2fr)_minmax(16rem,1.2fr)] gap-x-6";

// This page leans on tooltips for the finer print (what each threshold does,
// what "enabled" means, exact usage figures). A small hover delay keeps them
// from flashing open as the pointer crosses the dense grid of labels, badges,
// and progress bars.
const TOOLTIP_DELAY_MS = 150;

// Which compute metrics a team is billed for depends on plan tier and
// deployment class (see convex.dev/pricing and convex.dev/enterprise/pricing):
// - Node.js action compute is billed on every plan.
// - Convex-runtime action compute is billed only on non-Business/Enterprise
//   plans.
// - CPU action compute is billed only on Business/Enterprise plans.
// - Query/Mutation compute is billed only on dedicated (DXXXX) deployments.
// Returns a map from each metric the team ISN'T billed for to a short
// explanation; billed metrics are absent from the map. A limit on an unbilled
// metric is still enforced when active; the team just isn't charged for that
// metric's usage.
export function computeUnbilledMetrics({
  isBusinessPlan,
  isDedicated,
}: {
  isBusinessPlan: boolean;
  isDedicated: boolean;
}): Partial<Record<UsageMetric, string>> {
  const unbilled: Partial<Record<UsageMetric, string>> = {};
  if (isBusinessPlan) {
    unbilled.actionComputeConvexGbHours =
      "Your plan isn't billed for Convex runtime compute (Business and Enterprise plans are billed for CPU time instead), but this limit is still enforced when active.";
  } else {
    unbilled.actionComputeCpuGbHours =
      "Your plan isn't billed for CPU time (only Business and Enterprise plans are), but this limit is still enforced when active.";
  }
  if (!isDedicated) {
    unbilled.queryMutationComputeGbHours =
      "Your deployment isn't billed for Query/Mutation compute (only dedicated deployments are), but this limit is still enforced when active.";
  }
  return unbilled;
}

// What each non-complete backfill status means for the accuracy of the usage
// figures shown on the page.
const SEED_STATUS_MESSAGE: Record<
  Exclude<UsageSeedStatus, "complete">,
  string
> = {
  pending:
    "Historical usage is still being loaded, so the usage shown below may understate this deployment's actual usage. Check back shortly for accurate totals.",
  partial:
    "Historical usage is still being loaded, so the usage shown below may understate this deployment's actual usage. Check back shortly for accurate totals.",
  failed:
    "We couldn't load this deployment's historical usage, so the usage shown below may understate its actual usage. Limits are still enforced going forward.",
};

const SEED_STATUS_GRACE_MS = 90 * 60 * 1000;

// A callout shown while the historical-usage backfill is incomplete, warning
// that the usage figures below may understate actual usage.
function SeedStatusNote({
  seedStatus,
}: {
  seedStatus: Exclude<UsageSeedStatus, "complete">;
}) {
  return (
    <div className="flex w-fit items-start gap-2 rounded-md bg-background-warning p-2 text-xs text-content-warning">
      <ExclamationTriangleIcon className="mt-0.5 shrink-0" />
      <span className="max-w-prose">{SEED_STATUS_MESSAGE[seedStatus]}</span>
    </div>
  );
}

// A count badge on a window segment for currently triggered limits. Disable
// triggers use the error palette; warning triggers the warning palette. Both
// clear the 4.5:1 contrast bar (content-error/warning on their tinted
// background).
function TriggerBadge({
  limitType,
  count,
}: {
  limitType: UsageLimitType;
  count: number;
}) {
  const isDisable = limitType === "disable";
  const noun = count === 1 ? "threshold" : "thresholds";
  return (
    <Tooltip
      asChild
      delayDuration={TOOLTIP_DELAY_MS}
      tip={
        isDisable
          ? `${count} disable ${noun} triggered — the deployment is disabled for the rest of this window, and all function calls will fail.`
          : `${count} warning ${noun} triggered in this window.`
      }
      side="bottom"
    >
      <span
        className={cn(
          "flex items-center gap-1 rounded-full px-1.5 py-0.5 text-xs font-medium",
          isDisable
            ? "bg-background-error text-content-error"
            : "bg-background-warning text-content-warning",
        )}
      >
        <ExclamationTriangleIcon className="size-3 shrink-0" />
        <span className="tabular-nums">{count}</span>
      </span>
    </Tooltip>
  );
}

// Warn the user before they navigate away — via browser unload (close/refresh)
// or a client-side route change — while any inline editor has unsaved edits.
function useUnsavedChangesWarning(when: boolean) {
  const router = useRouter();
  useEffect(() => {
    if (!when) {
      return undefined;
    }
    const message =
      "You have unsaved usage limit changes. Leave without saving?";
    const beforeUnload = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = message;
      return message;
    };
    const onRouteChangeStart = (url: string) => {
      if (url === router.asPath) {
        return;
      }

      if (!window.confirm(message)) {
        router.events.emit("routeChangeError");
        // Throwing aborts the in-flight route change (Next.js pages router).

        throw `Route change to "${url}" aborted (unsaved usage limit changes).`;
      }
    };
    window.addEventListener("beforeunload", beforeUnload);
    router.events.on("routeChangeStart", onRouteChangeStart);
    return () => {
      window.removeEventListener("beforeunload", beforeUnload);
      router.events.off("routeChangeStart", onRouteChangeStart);
    };
  }, [when, router]);
}

export function UsageLimits({
  usageLimits,
  onCreate,
  onUpdate,
  onDelete,
  canWrite = true,
  isLoading = false,
  title = "Usage Limits",
  unbilledMetrics = {},
  currentUsage = {},
  seedStatus,
  deploymentCreateTime,
  deploymentType,
  billingUri,
  writePermissionTip = "You do not have permission to modify usage limits.",
}: {
  usageLimits: UsageLimit[];
  // Persist a brand new usage limit.
  onCreate: (config: UsageLimitConfig) => Promise<void> | void;
  // Persist the full configuration of an existing usage limit.
  onUpdate: (id: string, config: UsageLimitConfig) => Promise<void> | void;
  // Delete an existing usage limit.
  onDelete: (id: string) => Promise<void> | void;
  // Whether the current member may modify usage limits.
  canWrite?: boolean;
  // Whether the usage limits are still loading.
  isLoading?: boolean;
  // Heading for the card. Defaults to the deployment-level title.
  title?: string;
  // Metrics that aren't billed on the current plan/deployment, mapped to a
  // short explanation. See `computeUnbilledMetrics`. Absent = billed.
  unbilledMetrics?: Partial<Record<UsageMetric, string>>;
  // Current usage per metric/window, shown even for metrics without a limit.
  currentUsage?: CurrentUsage;
  // Progress of the historical-usage backfill. When not "complete", the current
  // usage figures may understate actual usage, so we note that to the user.
  seedStatus?: UsageSeedStatus;
  deploymentCreateTime?: number;
  // The current deployment's type. Dev deployments send no email when a limit
  // is exceeded, so their warning threshold is disabled and their disable
  // threshold notes no email is sent; prod/preview/custom email all team
  // members. Omit when unknown (e.g. self-hosted), in which case email is
  // assumed sent.
  deploymentType?: DeploymentType;
  // Provided only for cloud deployments, where it drives the reminder that these
  // windows don't follow the billing cycle. Omitted for self-hosted deployments,
  // which have no billing.
  billingUri?: string;
  // Tooltip shown on disabled write controls when `canWrite` is false. Callers
  // pass a `PermissionDeniedTip` so custom-role members see the missing action.
  writePermissionTip?: ReactNode;
}) {
  const [selectedWindow, setSelectedWindow] =
    useState<UsageLimitWindow>("month");
  // Keys (`${metric}|${window}|${limitType}`) of the threshold columns whose
  // inline editor is currently open. Several may be open at once.
  const [editingKeys, setEditingKeys] = useState<Set<string>>(new Set());
  // The subset of open editors that have unsaved changes.
  const [dirtyKeys, setDirtyKeys] = useState<Set<string>>(new Set());

  useUnsavedChangesWarning(dirtyKeys.size > 0);

  const startEdit = useCallback((key: string) => {
    setEditingKeys((prev) => {
      const next = new Set(prev);
      next.add(key);
      return next;
    });
  }, []);
  const stopEdit = useCallback((key: string) => {
    setEditingKeys((prev) => {
      const next = new Set(prev);
      next.delete(key);
      return next;
    });
  }, []);
  const setDirty = useCallback((key: string, dirty: boolean) => {
    setDirtyKeys((prev) => {
      if (dirty === prev.has(key)) {
        return prev;
      }
      const next = new Set(prev);
      if (dirty) {
        next.add(key);
      } else {
        next.delete(key);
      }
      return next;
    });
  }, []);

  // Each window segment is labelled with a badge showing how many of its limits
  // are active out of how many are configured (e.g. "Monthly 3/4 Active"); the
  // badge is hidden when nothing is configured for that window.
  const windowOptions = WINDOW_ORDER.map((w) => {
    const inWindow = usageLimits.filter((limit) => limit.window === w);
    const enabled = inWindow.filter((limit) => limit.enabled).length;
    const triggers = windowTriggerCounts(usageLimits, currentUsage, w);
    return {
      value: w,
      label: (
        <span className="flex items-center gap-1.5">
          {WINDOW_LABEL[w]}
          {inWindow.length > 0 && (
            <span className="rounded-full bg-blue-100 px-1.5 py-0.5 text-xs font-medium text-blue-700 dark:bg-util-accent dark:text-white">
              <span className="tabular-nums">
                {enabled}/{inWindow.length}
              </span>{" "}
              Active
            </span>
          )}
          {/* Triggered limits take precedence: their badges call out how many
              warning and disable thresholds this window has currently tripped. */}
          {triggers.disable > 0 && (
            <TriggerBadge limitType="disable" count={triggers.disable} />
          )}
          {triggers.warning > 0 && (
            <TriggerBadge limitType="warning" count={triggers.warning} />
          )}
        </span>
      ),
    };
  });

  const limitFor = (metric: UsageMetric, limitType: UsageLimitType) =>
    usageLimits.find(
      (limit) =>
        limit.metric === metric &&
        limit.window === selectedWindow &&
        limit.limitType === limitType,
    );

  // Unbilled metrics that have a limit configured in the selected window are
  // pinned to the top so they're noticeable; applicable metrics always follow.
  const unbilledConfigured = METRIC_ORDER.filter(
    (m) =>
      unbilledMetrics[m] && (limitFor(m, "warning") || limitFor(m, "disable")),
  );
  const applicable = METRIC_ORDER.filter((m) => !unbilledMetrics[m]);
  const shownMetrics = [...unbilledConfigured, ...applicable];

  return (
    <Sheet className="mb-6 flex flex-col gap-4">
      <div className="flex flex-col gap-2">
        <h3>{title}</h3>
        <p className="max-w-prose text-sm text-content-secondary">
          Limit how much usage this deployment can consume in a given timeframe.{" "}
          <Link href="https://docs.convex.dev/production/usage-limits">
            Learn more about deployment usage limits
          </Link>
        </p>
      </div>

      {seedStatus !== undefined &&
        seedStatus !== "complete" &&
        !(
          deploymentCreateTime !== undefined &&
          Date.now() - deploymentCreateTime < SEED_STATUS_GRACE_MS
        ) && <SeedStatusNote seedStatus={seedStatus} />}

      <div className="flex flex-col gap-2">
        <Tooltip
          asChild
          delayDuration={TOOLTIP_DELAY_MS}
          tip="Configure limits at a monthly or daily granularity. Each window's usage is tracked and enforced separately."
          side="right"
        >
          <span className="inline-flex w-fit items-center gap-1 text-sm text-content-secondary">
            Window
            <QuestionMarkCircledIcon className="text-content-tertiary" />
          </span>
        </Tooltip>

        <SegmentedControl
          className="w-fit"
          options={windowOptions}
          value={selectedWindow}
          onChange={(w) => {
            // Switching windows unmounts the current window's editors, so warn
            // before discarding any unsaved edits (mirrors the nav guard).
            if (
              dirtyKeys.size > 0 &&
              !window.confirm(
                "You have unsaved usage limit changes. Switch window without saving?",
              )
            ) {
              return;
            }
            setSelectedWindow(w);
            setEditingKeys(new Set());
          }}
        />

        <p className="text-xs text-content-secondary">
          {WINDOW_RESET_DESCRIPTION[selectedWindow]}
          {billingUri &&
            " This is a fixed calendar schedule and may not match your billing cycle."}
        </p>
        <p className="text-sm text-content-secondary">
          <WindowResetCountdown window={selectedWindow} />
        </p>
      </div>

      {isLoading ? (
        <Loading fullHeight={false} className="h-24 w-full rounded-lg" />
      ) : (
        // The table has a fixed minimum width (the threshold columns alone are
        // 32rem), so narrow viewports scroll horizontally rather than crushing
        // the columns.
        <div className="overflow-x-auto">
          <div className="flex min-w-208 flex-col divide-y divide-border-transparent">
            <div className={cn(TABLE_GRID, "pb-2")}>
              <span className="text-sm text-content-secondary">Metric</span>
              <span className="text-sm text-content-secondary">
                Current usage
              </span>
              <ThresholdLabel
                limitType="warning"
                deploymentType={deploymentType}
              />
              <ThresholdLabel
                limitType="disable"
                deploymentType={deploymentType}
              />
            </div>
            {shownMetrics.map((metric) => (
              <UsageLimitMetricRow
                key={metric}
                metric={metric}
                window={selectedWindow}
                warningLimit={limitFor(metric, "warning")}
                disableLimit={limitFor(metric, "disable")}
                currentUsage={currentUsage[metric]?.[selectedWindow]}
                unbilledReason={unbilledMetrics[metric]}
                deploymentType={deploymentType}
                canWrite={canWrite}
                writePermissionTip={writePermissionTip}
                editingKeys={editingKeys}
                onStartEdit={startEdit}
                onStopEdit={stopEdit}
                onDirtyChange={setDirty}
                onCreate={onCreate}
                onUpdate={onUpdate}
                onDelete={onDelete}
              />
            ))}
          </div>
        </div>
      )}
    </Sheet>
  );
}

// One table row per metric: the metric's name (with its description in a
// tooltip), its current usage in the selected window, and a Warning and a
// Disable threshold cell.
function UsageLimitMetricRow({
  metric,
  window,
  warningLimit,
  disableLimit,
  currentUsage,
  unbilledReason,
  deploymentType,
  canWrite,
  writePermissionTip,
  editingKeys,
  onStartEdit,
  onStopEdit,
  onDirtyChange,
  onCreate,
  onUpdate,
  onDelete,
}: {
  metric: UsageMetric;
  window: UsageLimitWindow;
  warningLimit?: UsageLimit;
  disableLimit?: UsageLimit;
  // Current usage of this metric in the selected window, if known.
  currentUsage?: number;
  unbilledReason?: string;
  deploymentType: DeploymentType | undefined;
  canWrite: boolean;
  writePermissionTip: ReactNode;
  editingKeys: Set<string>;
  onStartEdit: (key: string) => void;
  onStopEdit: (key: string) => void;
  onDirtyChange: (key: string, dirty: boolean) => void;
  onCreate: (config: UsageLimitConfig) => Promise<void> | void;
  onUpdate: (id: string, config: UsageLimitConfig) => Promise<void> | void;
  onDelete: (id: string) => Promise<void> | void;
}) {
  const config = METRIC_CONFIG[metric];
  const limitByType: Record<UsageLimitType, UsageLimit | undefined> = {
    warning: warningLimit,
    disable: disableLimit,
  };
  return (
    <div className={cn(TABLE_GRID, "items-start py-3")}>
      {/* pt-0.5 vertically centers these one-line cells against the threshold
          cells' min-h-6 first line. */}
      <div className="flex min-w-0 items-center gap-1.5 pt-0.5">
        <Tooltip
          tip={
            <>
              <div className="font-medium">{config.name}</div>
              {config.description}
            </>
          }
          side="right"
          delayDuration={TOOLTIP_DELAY_MS}
          className="min-w-0"
        >
          <span className="flex min-w-0 items-center gap-1 text-left text-sm font-medium text-content-primary">
            <span className="truncate">{config.name}</span>
            <QuestionMarkCircledIcon className="shrink-0 text-content-tertiary" />
          </span>
        </Tooltip>
        {unbilledReason && (
          <Tooltip
            tip={unbilledReason}
            side="right"
            delayDuration={TOOLTIP_DELAY_MS}
            className="shrink-0"
          >
            <span className="flex items-center">
              <ExclamationTriangleIcon className="size-3.5 shrink-0 text-content-warning" />
              <span className="sr-only">Not billed on this plan</span>
            </span>
          </Tooltip>
        )}
      </div>

      <div className="pt-0.5">
        {currentUsage !== undefined ? (
          <Tooltip
            asChild
            delayDuration={TOOLTIP_DELAY_MS}
            tip={`${formatAmount(currentUsage)} ${config.rawUnit} used this ${window}.`}
            side="bottom"
          >
            <span className="w-fit text-sm text-content-primary tabular-nums">
              {formatNumberCompact(currentUsage, 2)}{" "}
              {rawUnitShortFor(config, currentUsage)}
            </span>
          </Tooltip>
        ) : (
          <span className="text-sm text-content-tertiary">–</span>
        )}
      </div>

      {LIMIT_TYPE_ORDER.map((limitType) => {
        const rowKey = `${metric}|${window}|${limitType}`;
        // The other threshold's amount, used to warn when this one is set
        // such that the deployment would be disabled before the warning fires.
        const counterpartAmount =
          limitByType[limitType === "warning" ? "disable" : "warning"]?.limit;
        return (
          <UsageLimitThreshold
            key={limitType}
            metric={metric}
            window={window}
            limitType={limitType}
            limit={limitByType[limitType]}
            counterpartAmount={counterpartAmount}
            currentUsage={currentUsage}
            deploymentType={deploymentType}
            canWrite={canWrite}
            writePermissionTip={writePermissionTip}
            isEditing={editingKeys.has(rowKey)}
            onStartEdit={() => onStartEdit(rowKey)}
            onStopEdit={() => onStopEdit(rowKey)}
            onDirtyChange={onDirtyChange}
            onCreate={onCreate}
            onUpdate={onUpdate}
            onDelete={onDelete}
          />
        );
      })}
    </div>
  );
}

// A threshold column's label ("Warning threshold"/"Disable threshold") with a
// tooltip describing what happens when the limit is exceeded.
function ThresholdLabel({
  limitType,
  deploymentType,
}: {
  limitType: UsageLimitType;
  deploymentType: DeploymentType | undefined;
}) {
  return (
    <Tooltip
      tip={actionDescription(limitType, deploymentType)}
      side="right"
      delayDuration={TOOLTIP_DELAY_MS}
      // The trigger is a full-width button when this label is a grid cell, so
      // shrink it to its content to keep the label flush with the column.
      className="w-fit"
    >
      <span className="inline-flex items-center gap-1 text-sm text-content-secondary">
        {LIMIT_TYPE_LABEL[limitType]}
        <QuestionMarkCircledIcon className="text-content-tertiary" />
      </span>
    </Tooltip>
  );
}

// A single (metric, window, limitType) threshold cell. Read-only when not
// editing (the configured amount with an overflow menu for edit/delete, then a
// status line), swapping to an inline editor while editing. The column headers
// above the table carry the "Warning/Disable threshold" labels.
function UsageLimitThreshold({
  metric,
  window,
  limitType,
  limit,
  counterpartAmount,
  currentUsage,
  deploymentType,
  canWrite,
  writePermissionTip,
  isEditing,
  onStartEdit,
  onStopEdit,
  onDirtyChange,
  onCreate,
  onUpdate,
  onDelete,
}: {
  metric: UsageMetric;
  window: UsageLimitWindow;
  limitType: UsageLimitType;
  limit?: UsageLimit;
  // The amount of the other threshold (disable for a warning row, warning for a
  // disable row) in this metric/window, if configured. Used to hint when the
  // two are ordered such that the deployment would be disabled before the
  // warning fires.
  counterpartAmount?: number;
  // Current usage of this metric in the window, used to fill the progress bar.
  currentUsage?: number;
  deploymentType: DeploymentType | undefined;
  canWrite: boolean;
  writePermissionTip: ReactNode;
  isEditing: boolean;
  onStartEdit: () => void;
  onStopEdit: () => void;
  onDirtyChange: (key: string, dirty: boolean) => void;
  onCreate: (config: UsageLimitConfig) => Promise<void> | void;
  onUpdate: (id: string, config: UsageLimitConfig) => Promise<void> | void;
  onDelete: (id: string) => Promise<void> | void;
}) {
  const config = METRIC_CONFIG[metric];

  // A warning limit's only effect is the email, which dev deployments don't
  // send, so the warning threshold can't be configured there (the backend also
  // rejects warning limits on dev deployments). The slot still renders like any
  // other empty threshold, but its configure button is disabled with an
  // explanation rather than replaced by a bare "Not available".
  const warningUnavailableOnDev =
    limitType === "warning" && !sendsEmail(deploymentType);
  const warningUnavailableTip =
    "Development deployments don't receive email notifications, so warning thresholds aren't available.";

  if (isEditing) {
    return (
      <UsageLimitThresholdEditor
        metric={metric}
        window={window}
        limitType={limitType}
        limit={limit}
        counterpartAmount={counterpartAmount}
        currentUsage={currentUsage}
        deploymentType={deploymentType}
        onDone={onStopEdit}
        onDirtyChange={onDirtyChange}
        onCreate={onCreate}
        onUpdate={onUpdate}
      />
    );
  }

  if (!limit) {
    return (
      <div className="flex min-h-6 items-center gap-x-2">
        <span className="text-sm text-content-tertiary">–</span>
        <ConfigureLimitButton
          disabled={!canWrite || warningUnavailableOnDev}
          disabledTip={
            warningUnavailableOnDev ? warningUnavailableTip : writePermissionTip
          }
          onStartEdit={onStartEdit}
        />
      </div>
    );
  }

  const isTriggered =
    limit.enabled && currentUsage !== undefined && currentUsage >= limit.limit;

  // The whole limit fits on one line: usage gauge, amount, then status pills.
  // Status is only called out when it's the exception: an Inactive pill for an
  // unenforced limit, a triggered pill for an exceeded one. flex-wrap lets the
  // pills drop to a second line rather than overflow if the column runs out of
  // room.
  return (
    <div className="flex min-h-6 flex-wrap items-center gap-x-2 gap-y-1">
      {currentUsage !== undefined && (
        <UsageDonut
          metric={metric}
          window={window}
          current={currentUsage}
          limit={limit}
        />
      )}
      <span className="text-sm text-content-primary tabular-nums">
        {formatAmount(limit.limit)} {rawUnitShortFor(config, limit.limit)}
      </span>
      {!limit.enabled && <InactivePill />}
      {isTriggered && <TriggeredBadge limitType={limit.limitType} />}
      <ThresholdOverflowMenu
        canWrite={canWrite}
        writePermissionTip={writePermissionTip}
        onEdit={onStartEdit}
        onDelete={() => onDelete(limit.id)}
      />
    </div>
  );
}

// The "Configure limit" button shown in an empty slot. When it can't be used
// (the member lacks write access, or the threshold isn't available on this
// deployment) it's disabled with an explanatory tooltip rather than hidden.
function ConfigureLimitButton({
  disabled,
  disabledTip,
  onStartEdit,
}: {
  disabled: boolean;
  disabledTip: ReactNode;
  onStartEdit: () => void;
}) {
  return (
    <Button
      className="w-fit"
      size="xs"
      variant="neutral"
      inline
      icon={<PlusCircledIcon />}
      aria-label="Configure limit"
      tip={disabled ? disabledTip : "Configure limit"}
      disabled={disabled}
      onClick={onStartEdit}
    />
  );
}

// The overflow menu for a configured threshold: Edit opens the inline editor,
// Delete removes the limit. Both are disabled with a tooltip when the member
// can't write.
function ThresholdOverflowMenu({
  canWrite,
  writePermissionTip,
  onEdit,
  onDelete,
}: {
  canWrite: boolean;
  writePermissionTip: ReactNode;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const disabledTip = !canWrite ? writePermissionTip : undefined;
  return (
    <Menu
      placement="bottom-end"
      buttonProps={{
        "aria-label": "Limit options",
        icon: <DotsVerticalIcon />,
        size: "xs",
        variant: "neutral",
        inline: true,
      }}
    >
      <MenuItem
        action={onEdit}
        disabled={!canWrite}
        tip={disabledTip}
        tipSide="left"
      >
        Edit
      </MenuItem>
      <MenuItem
        action={onDelete}
        variant="danger"
        disabled={!canWrite}
        tip={disabledTip}
        tipSide="left"
      >
        Delete
      </MenuItem>
    </Menu>
  );
}

// Inline editor for a single threshold column. Mounted fresh when a column
// enters edit mode, so its local state is seeded from the current limit (or
// blank for a new one).
function UsageLimitThresholdEditor({
  metric,
  window,
  limitType,
  limit,
  counterpartAmount,
  currentUsage,
  deploymentType,
  onDone,
  onDirtyChange,
  onCreate,
  onUpdate,
}: {
  metric: UsageMetric;
  window: UsageLimitWindow;
  limitType: UsageLimitType;
  limit?: UsageLimit;
  // The other threshold's amount in this metric/window; see UsageLimitThreshold.
  counterpartAmount?: number;
  // Current usage of this metric in the window, used to warn when the entered
  // amount is below it.
  currentUsage?: number;
  // Used to confirm before adding an enforced disable threshold on prod.
  deploymentType: DeploymentType | undefined;
  onDone: () => void;
  onDirtyChange: (key: string, dirty: boolean) => void;
  onCreate: (config: UsageLimitConfig) => Promise<void> | void;
  onUpdate: (id: string, config: UsageLimitConfig) => Promise<void> | void;
}) {
  const config = METRIC_CONFIG[metric];
  // A new limit starts with a blank amount (the ~$100/mo default is only a
  // placeholder hint, see the TextInput below) and defaults to enforced.
  const initialAmount = limit ? String(limit.limit) : "";
  const initialEnabled = limit ? limit.enabled : true;
  const [amount, setAmount] = useState(initialAmount);
  const [enabled, setEnabled] = useState(initialEnabled);
  const [isSaving, setIsSaving] = useState(false);
  const [showDisableConfirmation, setShowDisableConfirmation] = useState(false);

  const parsedLimit = Math.floor(Number(amount));
  const hasAmount = amount.trim() !== "";
  // The UI enforces 1 <= limit <= 100 trillion.
  const isInRange =
    Number.isFinite(parsedLimit) &&
    parsedLimit >= 1 &&
    parsedLimit <= MAX_USAGE_LIMIT_VALUE;
  const isValid = hasAmount && isInRange;
  const isDraftAmount = limit ? parsedLimit !== limit.limit : true;
  const isBelowCurrentUsage =
    isValid && currentUsage !== undefined && parsedLimit < currentUsage;
  const belowCurrentUsageBlocks = isBelowCurrentUsage && enabled;
  const belowCurrentUsageWarning =
    isBelowCurrentUsage && !enabled && isDraftAmount;
  const canSave = isValid && !belowCurrentUsageBlocks;

  // Warnings are meant to fire before the deployment is disabled, so the warning
  // threshold should sit below the disable threshold. Flag an inverted (or
  // equal) ordering, where the deployment would be disabled before this warning
  // could ever be sent. Both rows surface the same soft hint.
  const disableBeforeWarning =
    isValid &&
    counterpartAmount !== undefined &&
    (limitType === "warning"
      ? parsedLimit >= counterpartAmount
      : parsedLimit <= counterpartAmount);

  // Report whether this editor has unsaved changes so the page can warn before
  // navigating away. Dirty means a field diverges from where it started — the
  // saved value for an existing limit, or the seeded default for a new one (so
  // an untouched default isn't treated as an unsaved edit).
  const rowKey = `${metric}|${window}|${limitType}`;
  const isDirty = amount !== initialAmount || enabled !== initialEnabled;
  useEffect(() => {
    onDirtyChange(rowKey, isDirty);
    return () => onDirtyChange(rowKey, false);
  }, [rowKey, isDirty, onDirtyChange]);

  // An enforced disable threshold on a production deployment takes the
  // deployment offline the moment usage crosses the limit, so confirm the
  // intent (and point at warnings) before creating one. Editing an existing
  // limit, an inactive one, or any non-disable / non-prod threshold saves
  // directly, since none of them newly arm a production shutdown.
  const needsDisableConfirmation =
    !limit && limitType === "disable" && enabled && deploymentType === "prod";

  const performSave = async () => {
    setIsSaving(true);
    try {
      if (limit) {
        await onUpdate(limit.id, {
          metric,
          limit: parsedLimit,
          window,
          limitType,
          enabled,
        });
      } else {
        await onCreate({
          metric,
          limit: parsedLimit,
          window,
          limitType,
          enabled,
        });
      }
      // Only close the editor once the save actually succeeded. onCreate/onUpdate
      // reject on API failure (after toasting), so a failed save keeps the editor
      // open and the in-progress edits intact.
      onDone();
    } finally {
      setIsSaving(false);
    }
  };

  const handleSave = () => {
    if (!canSave) {
      return;
    }
    if (needsDisableConfirmation) {
      setShowDisableConfirmation(true);
      return;
    }
    void performSave().catch(() => {
      // The mutation hook already surfaced the error via toast; swallow it here
      // so the editor stays open rather than dropping the user's edits.
    });
  };

  return (
    <>
      <form
        // Capped at the threshold columns' 16rem floor so the input doesn't
        // stretch to fill a flexed-wider column.
        className="flex max-w-64 flex-col gap-2"
        // We validate the amount ourselves (see `isValid`), so suppress native
        // HTML5 validation — otherwise the number input's min/step would pop
        // "Please enter a valid value" on submit for non-multiples of the step.
        noValidate
        onSubmit={(e) => {
          e.preventDefault();
          void handleSave();
        }}
      >
        <div className="flex flex-col gap-1">
          <TextInput
            autoFocus
            id={`usage-limit-${metric}-${limitType}`}
            label="Amount"
            labelHidden
            type="number"
            min={1}
            max={MAX_USAGE_LIMIT_VALUE}
            step={config.rawStep}
            // Hint the ~$100/mo default amount for this metric/window.
            placeholder={formatAmount(config.defaultAmount)}
            value={amount}
            onChange={(e) => setAmount(e.target.value)}
            rightAddon={
              <span className="text-xs text-content-secondary">
                {rawUnitShortFor(config, parsedLimit)} {WINDOW_SUFFIX[window]}
              </span>
            }
          />
          {hasAmount && !isInRange && (
            <p className="text-xs text-content-errorSecondary">
              Enter an amount between 1 and 100 trillion.
            </p>
          )}
          {belowCurrentUsageBlocks && currentUsage !== undefined && (
            <p className="text-xs text-content-errorSecondary">
              Limit must be above the current usage for this window (
              {formatNumberCompact(currentUsage, 2)}{" "}
              {rawUnitShortFor(config, currentUsage)}).
            </p>
          )}
          {belowCurrentUsageWarning && currentUsage !== undefined && (
            <p className="text-xs text-content-warning">
              This is below the current usage for this window (
              {formatNumberCompact(currentUsage, 2)}{" "}
              {rawUnitShortFor(config, currentUsage)}), so it will take effect
              immediately once you make it active.
            </p>
          )}
          {disableBeforeWarning && counterpartAmount !== undefined && (
            <p className="text-xs text-content-warning">
              {limitType === "warning"
                ? `This is at or above the disable threshold (${formatNumberCompact(
                    counterpartAmount,
                    2,
                  )} ${rawUnitShortFor(
                    config,
                    counterpartAmount,
                  )}), so the deployment is disabled before this warning is sent.`
                : `This is at or below the warning threshold (${formatNumberCompact(
                    counterpartAmount,
                    2,
                  )} ${rawUnitShortFor(
                    config,
                    counterpartAmount,
                  )}), so the deployment is disabled before that warning is sent.`}
            </p>
          )}
        </div>
        <Tooltip
          tip="When active, this limit is enforced. If usage exceeds the allotted amount, the limit takes effect."
          side="left"
          delayDuration={TOOLTIP_DELAY_MS}
        >
          <label className="flex w-fit items-center gap-2 text-xs text-content-secondary">
            <Checkbox
              checked={enabled}
              onChange={() => setEnabled((prev) => !prev)}
            />
            Active
          </label>
        </Tooltip>
        <div className="flex items-center gap-2">
          <Button
            type="submit"
            size="xs"
            inline
            loading={isSaving}
            disabled={!canSave}
          >
            Save
          </Button>
          <Button
            type="button"
            size="xs"
            variant="neutral"
            inline
            onClick={onDone}
            disabled={isSaving}
          >
            Cancel
          </Button>
        </div>
      </form>
      {showDisableConfirmation && (
        <ConfirmationDialog
          onClose={() => setShowDisableConfirmation(false)}
          onConfirm={performSave}
          confirmText="Add disable threshold"
          dialogTitle="Add disable threshold?"
          variant="danger"
          dialogBody={
            <div className="flex flex-col gap-3 text-sm text-content-primary">
              <p>
                When {config.name} usage reaches{" "}
                <span className="font-medium">
                  {formatAmount(parsedLimit)}{" "}
                  {rawUnitShortFor(config, parsedLimit)} {WINDOW_SUFFIX[window]}
                </span>
                , the deployment will be disabled for the rest of the {window}{" "}
                and stop running functions.
              </p>
              <p className="text-content-secondary">
                Consider a warning threshold instead to be notified without
                interrupting your deployment.
              </p>
            </div>
          }
        />
      )}
    </>
  );
}

// A pill marking a limit that exists but isn't enforced. Enforced limits get no
// pill: active is the default state, so only the exception is called out.
function InactivePill() {
  return (
    <Tooltip
      asChild
      delayDuration={TOOLTIP_DELAY_MS}
      tip="This limit is inactive: it will not be enforced even if usage exceeds the allotted amount."
      side="bottom"
    >
      <span className="inline-flex w-fit items-center rounded-full bg-background-tertiary px-2 py-0.5 text-xs font-medium text-content-primary">
        Inactive
      </span>
    </Tooltip>
  );
}

// A pill shown, in place of the progress bar, on a triggered limit. A triggered
// "disable" limit has disabled the deployment (error palette); a triggered
// "warning" limit has emailed the team (warning palette).
function TriggeredBadge({ limitType }: { limitType: UsageLimitType }) {
  const isDisable = limitType === "disable";
  return (
    <Tooltip
      asChild
      delayDuration={TOOLTIP_DELAY_MS}
      tip={
        isDisable
          ? "This limit was exceeded, so the deployment is disabled for the rest of this window, and all function calls will fail. Raise or deactivate the limit to resume the deployment."
          : "This limit was exceeded, so all team members were emailed."
      }
      side="bottom"
    >
      <span
        className={cn(
          "flex w-fit items-center rounded-full p-1",
          isDisable
            ? "bg-background-error text-content-error"
            : "bg-background-warning text-content-warning",
        )}
      >
        <ExclamationTriangleIcon
          className="size-3"
          aria-label="Limit exceeded"
        />
      </span>
    </Tooltip>
  );
}

// A donut gauge shown left of a configured limit's amount, filling as the
// window's usage approaches the limit; its tooltip spells out where the
// deployment is at. Only rendered when live usage is available. A disabled
// (not enforced) limit's gauge is muted, since nothing happens even when usage
// is over it, and a triggered limit's gauge is tinted to match its pill (error
// for a disable limit, warning for a warning limit).
function UsageDonut({
  metric,
  window,
  current,
  limit,
}: {
  metric: UsageMetric;
  window: UsageLimitWindow;
  current: number;
  limit: UsageLimit;
}) {
  const config = METRIC_CONFIG[metric];
  const ratio = limit.limit > 0 ? current / limit.limit : 0;
  const percent = Math.round(ratio * 100);
  const isTriggered = limit.enabled && current >= limit.limit;
  return (
    <Tooltip
      asChild
      delayDuration={TOOLTIP_DELAY_MS}
      tip={`${formatNumberCompact(current, 2)} of ${formatNumberCompact(limit.limit, 2)} ${config.rawUnit} used this ${window} (${percent}%).`}
      side="bottom"
    >
      <div
        role="img"
        aria-label={`${config.name} usage: ${percent}% of limit`}
        className={cn("flex items-center", !limit.enabled && "opacity-60")}
      >
        <Donut
          current={current}
          max={limit.limit}
          strokeClassName={
            isTriggered
              ? limit.limitType === "disable"
                ? "stroke-content-error"
                : "stroke-content-warning"
              : undefined
          }
        />
      </div>
    </Tooltip>
  );
}

const DAY_MS = 86_400_000;

// A live "Resets at <date> (in …)" line for when the selected window next
// resets. The countdown ticks every second and shows seconds ("0:59:33") when
// under a day to go; with more than a day left, seconds are noise, so it ticks
// every minute and drops them ("2d 03:04"). The reset boundary is derived from
// the window itself (see `nextWindowResetMs`), so no backend data is needed. The
// boundary is a UTC midnight, so its date is rendered in UTC to match.
function WindowResetCountdown({ window }: { window: UsageLimitWindow }) {
  const [now, setNow] = useState(() => Date.now());
  const resetMs = nextWindowResetMs(window, now);
  const remainingMs = Math.max(0, resetMs - now);
  const underOneDay = remainingMs < DAY_MS;
  useEffect(() => {
    const id = setInterval(
      () => setNow(Date.now()),
      underOneDay ? 1000 : 60_000,
    );
    return () => clearInterval(id);
  }, [underOneDay]);
  return (
    <span className="tabular-nums">
      Resets on {formatResetDate(resetMs)} (in{" "}
      {formatCountdown(remainingMs, underOneDay)})
    </span>
  );
}

// The reset boundary as a UTC calendar date, e.g. "Aug 1, 2026". Rendered in UTC
// because the boundary itself is a UTC midnight.
function formatResetDate(resetMs: number): string {
  return new Date(resetMs).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
    timeZone: "UTC",
  });
}

// Epoch ms of the next reset boundary for a window, per the reset rules shown to
// the user: the next UTC midnight, or the first of the next month at UTC
// midnight. `Date.UTC` normalizes the rolled-over field.
function nextWindowResetMs(window: UsageLimitWindow, now: number): number {
  const d = new Date(now);
  switch (window) {
    case "day":
      return Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate() + 1);
    case "month":
    default:
      return Date.UTC(d.getUTCFullYear(), d.getUTCMonth() + 1, 1);
  }
}

// Format a positive duration. With `showSeconds`, "H:MM:SS" (always under a day,
// so no day prefix): "0:59:33". Otherwise minute resolution with a day prefix:
// "2d 03:04".
function formatCountdown(ms: number, showSeconds: boolean): string {
  const totalSeconds = Math.floor(ms / 1000);
  const days = Math.floor(totalSeconds / 86400);
  const hours = Math.floor((totalSeconds % 86400) / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  if (showSeconds) {
    return `${hours}:${pad(minutes)}:${pad(seconds)}`;
  }
  const hm = `${pad(hours)}:${pad(minutes)}`;
  return days > 0 ? `${days}d ${hm}` : hm;
}
