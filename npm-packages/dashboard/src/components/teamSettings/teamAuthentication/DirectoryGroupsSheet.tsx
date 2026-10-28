import { HelpTooltip } from "@ui/HelpTooltip";
import { LoadingTransition } from "@ui/Loading";
import { Sheet } from "@ui/Sheet";
import { cn } from "@ui/cn";
import type { DirectoryGroupResponse, TeamResponse } from "generatedApi";
import {
  DIRECTORY_GROUPS_PAGE_SIZE,
  useDirectorySyncGroups,
} from "api/directorySync";
import { PaginationControls } from "elements/PaginationControls";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import {
  useCursorPagination,
  useSnapBackOnEmptyPage,
} from "hooks/useCursorPagination";
import { RoleDisplay } from "../RoleDisplay";
import { EditGroupRolePopover } from "./EditGroupRolePopover";
import { EmptyStateRow, TABLE_CELL, TABLE_HEADER_CELL } from "./SettingsSheet";

const RESERVED_ADMIN_GROUP_NAME = "convex-team-admins";

export const MULTI_GROUP_EXPLANATION = (
  <div className="flex flex-col gap-2 text-left">
    <p>
      Each group in your identity provider maps to a role or a set of custom
      roles in Convex.
    </p>
    <p>A member of the directory receives:</p>
    <ul className="flex list-disc flex-col gap-1 pl-4">
      <li>
        <span className="font-semibold">Admin</span>, if any of their groups
        maps to Admin.
      </li>
      <li>
        Otherwise, if any group grants a custom role, the union of all custom
        roles granted.
      </li>
      <li>
        Otherwise, <span className="font-semibold">Developer</span>, if any of
        their groups maps to Developer.
      </li>
      <li>
        Otherwise <span className="font-semibold">no access</span>: a user no
        mapped group covers is not offered the team, and a member the directory
        reaches this way is removed from it.
      </li>
    </ul>
  </div>
);

export const INITIAL_SYNC_NOTE =
  "It may take up to an hour to finish the initial sync of your directory. Groups appear here as they are synced.";

export const NO_GROUPS_NOTE =
  "Your identity provider has not sent any groups for this directory.";

export function DirectoryGroupsSheet({
  team,
  canEdit,
  customRolesEnabled,
  awaitingInitialSync,
}: {
  team: TeamResponse;
  canEdit: boolean;
  customRolesEnabled: boolean;
  awaitingInitialSync: boolean;
}) {
  const pagination = useCursorPagination();
  const { data, isLoading, error } = useDirectorySyncGroups(
    team.id,
    pagination.currentCursor,
  );
  const groups = data?.groups;
  useSnapBackOnEmptyPage(pagination, {
    isLoading,
    currentPageItems: groups,
  });
  const hasMore = data?.pagination.hasMore ?? false;
  const showPagination = hasMore || pagination.currentPage > 1;

  return (
    <section
      className="flex flex-col gap-2"
      data-testid="directory-groups-sheet"
    >
      <div className="flex items-center gap-2">
        <h4>Directory group roles</h4>
        <HelpTooltip tipSide="right" maxWidthClassName="max-w-xs">
          {MULTI_GROUP_EXPLANATION}
        </HelpTooltip>
      </div>
      <Sheet className="flex flex-col" padding={false}>
        {error ? (
          <EmptyStateRow message="Unable to load directory groups. Please try again." />
        ) : (
          <LoadingTransition
            loadingProps={{ fullHeight: false, className: "m-3 h-10" }}
          >
            {groups &&
              (groups.length > 0 ? (
                <table className="w-full border-collapse">
                  <thead>
                    <tr className="border-b">
                      <th className={cn(TABLE_HEADER_CELL, "w-1/3")}>Group</th>
                      <th className={TABLE_HEADER_CELL}>Role</th>
                      <th className={cn(TABLE_HEADER_CELL, "w-0")}>
                        <span className="sr-only">Actions</span>
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {groups.map((group) => (
                      <GroupRow
                        key={group.workosGroupId}
                        team={team}
                        group={group}
                        canEdit={canEdit}
                        customRolesEnabled={customRolesEnabled}
                      />
                    ))}
                  </tbody>
                </table>
              ) : (
                <EmptyStateRow
                  message={
                    awaitingInitialSync ? INITIAL_SYNC_NOTE : NO_GROUPS_NOTE
                  }
                />
              ))}
          </LoadingTransition>
        )}
      </Sheet>
      {showPagination && (
        <PaginationControls
          isCursorBasedPagination
          className="justify-end"
          currentPage={pagination.currentPage}
          hasMore={hasMore}
          pageSize={DIRECTORY_GROUPS_PAGE_SIZE}
          onPageSizeChange={() => {}}
          showPageSize={false}
          onPreviousPage={pagination.onPreviousPage}
          onNextPage={() => pagination.onNextPage(data?.pagination.nextCursor)}
          canGoPrevious={pagination.canGoPrevious}
        />
      )}
    </section>
  );
}

function GroupRow({
  team,
  group,
  canEdit,
  customRolesEnabled,
}: {
  team: TeamResponse;
  group: DirectoryGroupResponse;
  canEdit: boolean;
  customRolesEnabled: boolean;
}) {
  const isReserved = group.name.toLowerCase() === RESERVED_ADMIN_GROUP_NAME;

  return (
    <tr className="border-b last:border-b-0">
      <td className={cn(TABLE_CELL, "truncate")}>{group.name}</td>
      <td className={TABLE_CELL}>
        {group.mapping ? (
          // The mapping carries its custom roles' names, so the cell reads
          // like the members table without waiting on the team's role list.
          <RoleDisplay
            role={group.mapping.role}
            customRoles={group.mapping.customRoles}
            teamSlug={team.slug}
          />
        ) : (
          // An unmapped group gives its members no place on the team.
          <div className="text-sm text-content-secondary">No access</div>
        )}
      </td>
      <td className={cn(TABLE_CELL, "text-right")}>
        <EditGroupRolePopover
          team={team}
          group={group}
          customRolesEnabled={customRolesEnabled}
          disabled={isReserved || !canEdit}
          disabledTip={
            isReserved
              ? "Members of convex-team-admins are always team admins. This mapping cannot be changed."
              : canEdit
                ? undefined
                : permissionDeniedTip(
                    "You do not have permission to change group roles.",
                    "directorySync:updateGroupMapping",
                  )
          }
        />
      </td>
    </tr>
  );
}
