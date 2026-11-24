import { Checkbox } from "@ui/Checkbox";
import { Modal } from "@ui/Modal";
import { TextInput } from "@ui/TextInput";
import { ListTable, ListTableHeaderDetail } from "elements/ListTable";
import {
  listMinHeight,
  PendingRoleChangeBadge,
  RoleChangesFooter,
} from "elements/ProjectAdminRoleChanges";
import { ProjectMemberRoleResponse } from "generatedApi";
import { TeamMember } from "@convex-dev/platform/managementApi";
import difference from "lodash/difference";
import sortBy from "lodash/sortBy";
import { Link } from "@ui/Link";
import { useState } from "react";
import type { ProjectDetails } from "generatedApi";

export function ProjectAdminFormModal({
  project,
  members,
  projectRoles,
  onUpdateProjectRoles,
  onClose,
}: {
  project: ProjectDetails;
  members: TeamMember[];
  projectRoles: ProjectMemberRoleResponse[];
  onUpdateProjectRoles: (body: {
    updates: {
      memberId: number;
      projectId: number;
      role?: "admin";
    }[];
  }) => Promise<void>;
  onClose: () => void;
}) {
  const originalAdmins = members
    .filter((member) =>
      projectRoles.find((role) => role.memberId === member.id),
    )
    .map((member) => member.id);
  const [newAdmins, setNewAdmins] = useState(originalAdmins);

  const addedAdmins = difference(newAdmins, originalAdmins);
  const removedAdmins = difference(originalAdmins, newAdmins);
  const [isSaving, setIsSaving] = useState(false);

  const [search, setSearch] = useState("");
  const normalizedSearch = search.trim().toLowerCase();
  const filteredMembers = sortBy(
    members.filter(
      (member) =>
        member.name?.toLowerCase().includes(normalizedSearch) ||
        member.email.toLowerCase().includes(normalizedSearch),
    ),
    (member) => (member.name || member.email).toLocaleLowerCase(),
  );

  const hasChanges = addedAdmins.length > 0 || removedAdmins.length > 0;
  const memberDisplayName = (memberId: number) => {
    const member = members.find((m) => m.id === memberId);
    return member ? member.name || member.email : `Member ${memberId}`;
  };

  // Runs as the Modal's onBeforeClose so declining keeps the modal open;
  // the Modal's onClose only fires after it has animated shut.
  const confirmClose = () => {
    if (isSaving) {
      return false;
    }
    return (
      !hasChanges ||
      window.confirm(
        "Closing the popup will clear your unsaved changes. Are you sure you want to continue?",
      )
    );
  };
  return (
    <Modal
      title="Manage Project Admins"
      size="md"
      description={
        <div>
          Add or remove project admins for{" "}
          <span className="font-semibold">{project.name}</span>.
        </div>
      }
      onBeforeClose={confirmClose}
      onClose={onClose}
    >
      <form
        className="flex w-full flex-col gap-4 pt-2"
        onSubmit={async (e) => {
          e.preventDefault();
          setIsSaving(true);
          try {
            await onUpdateProjectRoles({
              updates: [
                ...addedAdmins.map((added) => ({
                  memberId: added,
                  projectId: project.id,
                  role: "admin" as const,
                })),
                ...removedAdmins.map((removed) => ({
                  memberId: removed,
                  projectId: project.id,
                })),
              ],
            });
            onClose();
          } finally {
            setIsSaving(false);
          }
        }}
      >
        <TextInput
          type="search"
          id="projectAdminSearch"
          labelHidden
          label="Search members"
          placeholder="Search members"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          onKeyDown={(e) => {
            // Enter would otherwise implicitly submit the form and save.
            if (e.key === "Enter") {
              e.preventDefault();
            }
          }}
        />
        <ListTable
          className="max-h-[50vh]"
          // Reserve the unfiltered list's height so the list keeps its size as
          // search narrows the results.
          style={{ minHeight: listMinHeight(members.length) }}
          header={
            <>
              {/* Aligns the label with the member names below the checkboxes. */}
              <span className="size-3.5 shrink-0" aria-hidden="true" />
              Member
              <ListTableHeaderDetail>
                {newAdmins.length} project admin
                {newAdmins.length === 1 ? "" : "s"}
              </ListTableHeaderDetail>
            </>
          }
        >
          <div
            className="scrollbar flex min-h-0 grow flex-col divide-y overflow-y-auto"
            // Allow scrollable zone to be focused
            // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- https://dequeuniversity.com/rules/axe/4.11/scrollable-region-focusable?application=axeAPI
            tabIndex={0}
          >
            {filteredMembers.length === 0 ? (
              <div className="m-auto py-8 text-sm text-content-secondary">
                No members match your search.
              </div>
            ) : (
              filteredMembers.map((member) => {
                const checkboxId = `projectAdmin-${member.id}`;
                const isOriginalAdmin = originalAdmins.includes(member.id);
                const isNewAdmin = newAdmins.includes(member.id);
                return (
                  <div
                    key={member.id}
                    className="flex items-center gap-3 px-4 py-3 transition-colors hover:bg-background-tertiary"
                  >
                    <Checkbox
                      id={checkboxId}
                      aria-label={member.name || member.email}
                      checked={isNewAdmin}
                      onChange={() => {
                        setNewAdmins((prev) =>
                          prev.includes(member.id)
                            ? prev.filter((id) => id !== member.id)
                            : [...prev, member.id],
                        );
                      }}
                    />
                    <label
                      htmlFor={checkboxId}
                      className="flex min-w-0 grow cursor-pointer flex-col"
                    >
                      {member.name && (
                        <span className="truncate text-sm text-content-primary">
                          {member.name}
                        </span>
                      )}
                      <span
                        className={
                          member.name
                            ? "truncate text-xs text-content-secondary"
                            : "truncate text-sm text-content-primary"
                        }
                      >
                        {member.email}
                      </span>
                    </label>
                    <PendingRoleChangeBadge
                      isOriginalAdmin={isOriginalAdmin}
                      isNewAdmin={isNewAdmin}
                    />
                  </div>
                );
              })
            )}
          </div>
        </ListTable>
        <p className="text-xs text-content-secondary">
          Pro-tip! You can manage the Project Admin role for multiple projects
          at the same time on the{" "}
          <Link href="https://docs.convex.dev/dashboard/teams#members">
            Team Member Settings
          </Link>{" "}
          page.{" "}
        </p>
        <RoleChangesFooter
          added={addedAdmins.map(memberDisplayName)}
          removed={removedAdmins.map(memberDisplayName)}
          noun="admin"
          isSaving={isSaving}
          onCancel={() => {
            if (confirmClose()) {
              onClose();
            }
          }}
        />
      </form>
    </Modal>
  );
}
