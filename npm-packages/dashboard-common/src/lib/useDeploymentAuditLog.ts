import { usePaginatedQuery } from "convex/react";
import udfs from "@common/udfs";
import { Doc } from "system-udfs/convex/_generated/dataModel";
import { useEffect, useContext, useRef } from "react";
import {
  DeploymentInfoContext,
  PermissionsContext,
} from "@common/lib/deploymentContext";

export type DeploymentAuditLogEvent = Doc<"_deployment_audit_log"> & {
  memberName: string;
  metadata?: {
    schema?: null | {
      previous_schema: null | string;
      next_schema: null | string;
    };
  };
};

function getMemberName(
  auditLogEvent: Doc<"_deployment_audit_log">,
  teamMembers: { id: number; name?: string | null; email?: string }[],
): string {
  if (auditLogEvent.member_id === null) {
    return "Convex";
  }

  const member = teamMembers.find(
    (m) => BigInt(m.id) === auditLogEvent.member_id,
  );
  return member?.name || member?.email || "Unknown member";
}

function processDeploymentAuditLogEvent(
  auditLogEvent: Doc<"_deployment_audit_log">,
  teamMembers: {
    id: number;
    name?: string | null;
    email?: string;
  }[],
): DeploymentAuditLogEvent | null {
  switch (auditLogEvent.action) {
    case "create_environment_variable":
    case "update_environment_variable":
    case "delete_environment_variable":
    case "replace_environment_variable":
    case "create_usage_limit":
    case "update_usage_limit":
    case "delete_usage_limit":
    case "usage_limit_exceeded":
    case "change_usage_limit_stop_state":
    case "update_canonical_url":
    case "delete_canonical_url":
    case "push_config":
    case "push_config_with_components":
    case "change_deployment_state":
    case "pause_deployment":
    case "unpause_deployment":
    case "change_system_stop_state":
    case "build_indexes":
    case "clear_tables":
    case "snapshot_import":
    case "delete_scheduled_jobs_table":
    case "delete_tables":
    case "delete_component":
    case "cancel_all_scheduled_functions":
    case "cancel_scheduled_function":
    case "request_export":
    case "download_export":
    case "cancel_export":
    case "set_export_expiration":
    case "create_integration":
    case "update_integration":
    case "delete_integration":
    case "add_documents":
    case "delete_documents":
    case "update_documents":
    case "create_table":
    case "delete_files":
    case "generate_upload_url":
    case "create_data_sync":
      break;
    default:
      return null;
  }

  return {
    ...auditLogEvent,
    memberName: getMemberName(auditLogEvent, teamMembers),
  } as DeploymentAuditLogEvent;
}

function processDeploymentEvents(
  events: Doc<"_deployment_audit_log">[],
  teamMembers: {
    id: number;
    name?: string | null;
    email?: string;
  }[],
): DeploymentAuditLogEvent[] {
  return events
    .map((event) => processDeploymentAuditLogEvent(event, teamMembers))
    .filter((v): v is DeploymentAuditLogEvent => v !== null);
}

export function useDeploymentAuditLogs(
  fromTimestamp?: number,
  filters?: {
    actions: string[];
  },
): DeploymentAuditLogEvent[] | undefined {
  const { useCurrentTeam, useTeamMembers } = useContext(DeploymentInfoContext);
  const team = useCurrentTeam();
  const teamMembers = useTeamMembers(team?.id) || [];

  const initialNumItems = 50;
  const { results, loadMore, status } = usePaginatedDeploymentEvents(
    fromTimestamp
      ? {
          minDate: fromTimestamp,
          actions: filters ? filters.actions : undefined,
        }
      : undefined,
    teamMembers,
    initialNumItems,
  );

  // Cache the longest (most complete) result set we've seen for the current
  // team, time window, and filters. After a deploy the paginated query may
  // briefly return only the newest page, making older events disappear and then
  // reappear as pagination catches up; keeping the longest results avoids this
  // flicker. When any of those inputs change we clear the cache and start over.
  const longestResultsRef = useRef<DeploymentAuditLogEvent[] | undefined>(
    undefined,
  );
  const resetKeyRef = useRef<string | undefined>(undefined);
  const resetKey = [
    team?.id ?? "no-team",
    fromTimestamp ?? "no-timestamp",
    filters?.actions?.join(",") ?? "no-actions",
  ].join("|");
  if (resetKeyRef.current !== resetKey) {
    longestResultsRef.current = undefined;
    resetKeyRef.current = resetKey;
  }

  // Load items until we've exhausted the results.
  useEffect(() => {
    if (status === "CanLoadMore") {
      loadMore(initialNumItems);
    }
  }, [loadMore, status]);

  // Update the longest results if current results are the same or longer.
  if (results) {
    const currentLongest = longestResultsRef.current;
    if (!currentLongest || results.length >= currentLongest.length) {
      // New results are at least as complete as what we had before.
      longestResultsRef.current = results;
    }
  }

  // Return the longest results we've seen so far, with `build_indexes` events removed.
  return longestResultsRef.current
    ? longestResultsRef.current
        .slice()
        .reverse()
        .filter(
          (event: DeploymentAuditLogEvent) => event.action !== "build_indexes",
        )
    : undefined;
}

export type DeploymentAuditLogFilters = {
  minDate: number;
  maxDate?: number;
  authorMemberIds?: bigint[];
  actions?: string[];
};

export function usePaginatedDeploymentEvents(
  filters: DeploymentAuditLogFilters | undefined,
  teamMembers: {
    id: number;
    name?: string | null;
    email?: string;
  }[],
  initialNumItems = 10,
) {
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canViewAuditLog = useIsOperationAllowed("ViewAuditLog");
  const { results, ...rest } = usePaginatedQuery(
    udfs.paginatedDeploymentEvents.default,
    filters && canViewAuditLog ? { filters } : "skip",
    {
      initialNumItems,
    },
  );
  return {
    results: processDeploymentEvents(results, teamMembers),
    ...rest,
  };
}
