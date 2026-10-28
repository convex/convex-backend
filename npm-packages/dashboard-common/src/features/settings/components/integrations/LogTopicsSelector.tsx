import { useContext } from "react";
import { Checkbox } from "@ui/Checkbox";
import { HelpTooltip } from "@ui/HelpTooltip";
import { Link } from "@ui/Link";
import type { LogTopic } from "@convex-dev/platform/deploymentApi";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";

// The subscribable log stream topics, rendered by their canonical name (matching
// the `topic` field in log events). Keep in sync with `LogTopic::SUBSCRIBABLE`
// in `crates/common/src/log_streaming.rs`.
//
// `requiresCustomAuditEntitlement` marks topics that are only available with the
// corresponding team entitlement; the backend rejects subscribing to them
// otherwise.
const TOPICS: {
  key: LogTopic;
  description: string;
  docsUrl?: string;
  requiresCustomAuditEntitlement?: boolean;
}[] = [
  {
    key: "function_execution",
    description:
      "Emitted whenever a function runs, including timing, status, and usage stats.",
  },
  {
    key: "console",
    description: "Reports console.* logs produced by your functions.",
  },
  {
    key: "audit_log",
    description:
      "Reports deployment changes such as deploys, environment variable edits, and state changes.",
  },
  {
    key: "scheduler_stats",
    description:
      "Reports periodic statistics from the scheduled function executor.",
  },
  {
    key: "scheduled_job_lag",
    description: "Reports lag of the oldest overdue scheduled job.",
  },
  {
    key: "concurrency_stats",
    description: "Reports per-minute function concurrency statistics.",
  },
  {
    key: "current_storage_usage",
    description:
      "Reports periodic snapshots of your deployment's storage usage.",
  },
  {
    key: "storage_api_bandwidth",
    description:
      "Reports bytes served directly from the storage HTTP API (file downloads).",
  },
  {
    key: "ai_gateway_usage",
    description:
      "Reports AI Gateway inference cost, token usage, model, and function attribution.",
  },
  {
    key: "log_stream_egress",
    description: "Reports bytes sent by any log stream to its destination.",
  },
  {
    key: "custom_audit",
    description:
      "Custom audit events emitted via log.audit() in your functions.",
    docsUrl: "https://docs.convex.dev/production/integrations/audit-logging",
    requiresCustomAuditEntitlement: true,
  },
];

const ALL_LOG_TOPICS: LogTopic[] = TOPICS.map((t) => t.key);

/**
 * Multi-select for the topics a log stream is subscribed to, modeled after the
 * deploy key "allowed actions" UI.
 *
 * `value === null` means "subscribed to all topics, including ones added in the
 * future" (which excludes the opt-in `custom_audit` topic). For display, a
 * `null` value shows every selectable topic as checked. Any interaction emits an
 * explicit array of the checked topics.
 *
 * The `custom_audit` topic is gated behind a team entitlement.
 */
export function LogTopicsSelector({
  value,
  onChange,
  error,
}: {
  value: LogTopic[] | null;
  onChange: (value: LogTopic[] | null) => void;
  error?: string;
}) {
  const { useCurrentTeam, useTeamEntitlements } = useContext(
    DeploymentInfoContext,
  );
  const team = useCurrentTeam();
  const entitlements = useTeamEntitlements(team?.id);
  const customAuditEnabled =
    entitlements?.customAuditLogsInLogStreamsConfigEnabled ?? false;

  // A `null` value means "subscribed to all topics", which (matching the
  // backend) excludes the opt-in `custom_audit` topic. Render it as every
  // non-opt-in topic checked, regardless of entitlement.
  const subscribeAllTopics = TOPICS.filter(
    (t) => !t.requiresCustomAuditEntitlement,
  ).map((t) => t.key);
  const selected = new Set(value ?? subscribeAllTopics);

  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-col gap-1">
        Topics
        <div className="max-w-prose text-xs text-content-secondary">
          Choose which event topics are sent to this log stream.
        </div>
      </div>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-x-4 gap-y-1">
        {TOPICS.map((topic) => {
          const isChecked = selected.has(topic.key);
          // Block adding entitlement-gated topics, but still allow removing
          // one that was somehow already subscribed.
          const missingCustomAuditEntitlement =
            !!topic.requiresCustomAuditEntitlement && !customAuditEnabled;
          const disabled = missingCustomAuditEntitlement && !isChecked;
          const customAuditTooltip = missingCustomAuditEntitlement
            ? isChecked
              ? " This topic is selected but ignored because your plan doesn't have access to custom audit logs."
              : " Your plan doesn't have access to custom audit logs."
            : "";
          return (
            <label
              key={topic.key}
              htmlFor={`topic-${topic.key}`}
              className="flex cursor-pointer items-center gap-2 rounded-sm p-1 text-xs hover:bg-background-secondary aria-disabled:cursor-not-allowed aria-disabled:opacity-60"
              aria-disabled={disabled}
            >
              <Checkbox
                id={`topic-${topic.key}`}
                checked={isChecked}
                disabled={disabled}
                onChange={() => {
                  const next = new Set(selected);
                  if (next.has(topic.key)) {
                    next.delete(topic.key);
                  } else {
                    next.add(topic.key);
                  }
                  // Preserve the canonical topic order.
                  onChange(ALL_LOG_TOPICS.filter((t) => next.has(t)));
                }}
              />
              <span className="font-mono">{topic.key}</span>
              <HelpTooltip>
                {topic.description}
                {customAuditTooltip}
                {topic.docsUrl && (
                  <>
                    {" "}
                    <Link href={topic.docsUrl} target="_blank">
                      Learn more
                    </Link>
                  </>
                )}
              </HelpTooltip>
            </label>
          );
        })}
      </div>
      {error && (
        <p className="text-xs text-content-errorSecondary" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
