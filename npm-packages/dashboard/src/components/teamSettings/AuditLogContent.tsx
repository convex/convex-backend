import { Sheet } from "@ui/Sheet";
import { AuditLogEventResponse } from "@convex-dev/platform/managementApi";
import { MemberResponse, TeamResponse } from "generatedApi";
import { AuditLogItem } from "./AuditLogItem";

export function AuditLogContent({
  team,
  members,
  entries,
}: {
  team: TeamResponse;
  members: MemberResponse[];
  entries: AuditLogEventResponse[];
}) {
  return (
    <Sheet
      // Account for the load more button below the content
      className="flex-col overflow-y-auto py-4"
      padding={false}
    >
      {entries.length === 0 ? (
        <NoEntries />
      ) : (
        entries.map((entry) => (
          <AuditLogItem
            team={team}
            entry={entry}
            key={entry.createTime}
            memberId={entryMemberId(entry)}
            members={members}
          />
        ))
      )}
    </Sheet>
  );
}

function entryMemberId(entry: AuditLogEventResponse) {
  switch (entry.actor.kind) {
    case "member":
      return entry.actor.member_id;
    case "token":
      return entry.actor.member_id ?? null;
    default:
      return null;
  }
}

function NoEntries() {
  return (
    <div
      className="flex h-full flex-1 flex-col items-center justify-center"
      data-testid="no-entries"
    >
      <div className="mx-2 flex flex-col items-center gap-2 text-content-secondary">
        No audit log entries matching the selected date range and filters.
      </div>
    </div>
  );
}
