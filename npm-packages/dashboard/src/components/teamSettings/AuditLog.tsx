import { Button } from "@ui/Button";
import { endOfDay } from "date-fns";
import { LoadingTransition } from "@ui/Loading";
import { useDateFilters } from "@common/elements/DateRangePicker";
import { AuditLogAction, useTeamAuditLog } from "api/auditLog";
import { useHasCustomRolePermission } from "api/roles";
import { useTeamEntitlements, useTeamMembers } from "api/teams";
import { TeamResponse } from "generatedApi";
import { NoPermissionMessage } from "elements/NoPermissionMessage";
import { TEAM_RESOURCE } from "lib/permissions";
import { useRouter } from "next/router";
import { AuditLogContent } from "./AuditLogContent";
import { AuditLogToolbar } from "./AuditLogToolbar";

export function AuditLog({ team }: { team: TeamResponse }) {
  const canViewAuditLog = useHasCustomRolePermission(
    team.id,
    "team:auditLog:view",
    TEAM_RESOURCE,
    true,
  );

  if (canViewAuditLog === false) {
    return (
      <>
        <h2>Audit Log</h2>
        <NoPermissionMessage
          message="You do not have permission to view the audit log."
          missingPermission="team:auditLog:view"
        />
      </>
    );
  }

  return <AuditLogContents team={team} />;
}

function AuditLogContents({ team }: { team: TeamResponse }) {
  const members = useTeamMembers(team.id);
  const auditLogRetentionDays =
    useTeamEntitlements(team?.id)?.auditLogRetentionDays ?? 0;

  const router = useRouter();

  // Filter state management
  const { startDate, endDate, setDate } = useDateFilters(router);

  const { member, action } = router.query;
  const selectedMember = Array.isArray(member) ? member[0] : member || null;
  const selectedAction = Array.isArray(action)
    ? (action[0] as AuditLogAction)
    : (action as AuditLogAction) || null;

  const setSelectedMember = (m: string) => {
    void router.push(
      {
        query: { ...router.query, member: m === "all_members" ? undefined : m },
      },
      undefined,
      { shallow: true },
    );
  };
  const setSelectedAction = (a: AuditLogAction | "all_actions") => {
    void router.push(
      {
        query: { ...router.query, action: a === "all_actions" ? undefined : a },
      },
      undefined,
      { shallow: true },
    );
  };

  // Load the data for the audit log
  const { entries, isLoading, loadNextPage, hasMore } = useTeamAuditLog(
    team.id,
    {
      from: startDate.getTime(),
      to: endOfDay(endDate).getTime(),
      memberId: selectedMember === "all_members" ? null : selectedMember,
      action: selectedAction,
    },
  );

  return (
    <>
      <h2>Audit Log</h2>
      <div className="flex grow flex-col gap-4 overflow-y-hidden">
        {members && (
          <AuditLogToolbar
            selectedMember={
              selectedMember === null ? "all_members" : selectedMember
            }
            setSelectedMember={setSelectedMember}
            selectedAction={
              selectedAction === null ? "all_actions" : selectedAction
            }
            setSelectedAction={setSelectedAction}
            selectedStartDay={startDate}
            selectedEndDay={endDate}
            setDate={setDate}
            members={members}
            auditLogRetentionDays={auditLogRetentionDays}
          />
        )}
        <LoadingTransition>
          {members && !isLoading && entries !== undefined && (
            <div className="flex w-full flex-col gap-4 overflow-y-auto">
              <AuditLogContent
                {...{
                  team,
                  members,
                  entries,
                }}
              />
              <Button
                onClick={loadNextPage}
                className="ml-auto w-fit"
                variant="neutral"
                disabled={!hasMore || isLoading}
                tip={hasMore ? undefined : "There are no more entries to load."}
                loading={isLoading}
              >
                Load more
              </Button>
            </div>
          )}
        </LoadingTransition>
      </div>
    </>
  );
}
