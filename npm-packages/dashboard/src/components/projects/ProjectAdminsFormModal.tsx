import { Button } from "@ui/Button";
import { Checkbox } from "@ui/Checkbox";
import { Modal } from "@ui/Modal";
import { TeamMemberLink } from "elements/TeamMemberLink";
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

  const closeWithConfirmation = () => {
    if (addedAdmins.length > 0 || removedAdmins.length > 0) {
      const shouldClose = window.confirm(
        "Closing the popup will clear your unsaved changes. Are you sure you want to continue?",
      );
      if (!shouldClose) {
        return;
      }
    }
    onClose();
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
      onClose={closeWithConfirmation}
    >
      <form
        className="flex w-full flex-col gap-2"
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
        <div className="scrollbar max-h-[60vh] overflow-auto">
          {sortBy(members, (member) =>
            (member.name || member.email).toLocaleLowerCase(),
          ).map((member) => (
            <div
              className="flex h-12 items-center gap-4 border-b px-1 py-2 last:border-b-0"
              key={member.id}
            >
              <Checkbox
                checked={newAdmins.includes(member.id)}
                onChange={() => {
                  setNewAdmins((prev) =>
                    newAdmins.includes(member.id)
                      ? prev.filter((id) => id !== member.id)
                      : [...prev, member.id],
                  );
                }}
              />
              <div className="text-sm">
                <TeamMemberLink
                  memberId={member.id}
                  name={member.name || member.email}
                />
              </div>
              <div className="ml-auto rounded-sm p-1 text-xs">
                {originalAdmins.includes(member.id) &&
                  !newAdmins.includes(member.id) && (
                    <div className="rounded-sm bg-background-error p-1 text-xs text-content-error">
                      Role will be removed
                    </div>
                  )}
                {!originalAdmins.includes(member.id) &&
                  newAdmins.includes(member.id) && (
                    <div className="rounded-sm bg-background-success p-1 text-xs text-content-success">
                      Role will be added
                    </div>
                  )}
              </div>
            </div>
          ))}
        </div>
        <p className="text-xs text-content-secondary">
          Pro-tip! You can manage the Project Admin role for multiple projects
          at the same time on the{" "}
          <Link href="https://docs.convex.dev/dashboard/teams#members">
            Team Member Settings
          </Link>{" "}
          page.{" "}
        </p>
        <div className="ml-auto flex items-center gap-2 text-right">
          <div className="text-xs">
            {addedAdmins.length > 0 && (
              <div className="text-content-success">
                Add {addedAdmins.length} role
                {addedAdmins.length > 1 ? "s" : ""}
              </div>
            )}
            {removedAdmins.length > 0 && (
              <div className="text-content-error">
                Remove {removedAdmins.length} role
                {removedAdmins.length > 1 ? "s" : ""}
              </div>
            )}
          </div>

          <Button
            type="submit"
            disabled={addedAdmins.length === 0 && removedAdmins.length === 0}
            loading={isSaving}
          >
            Save
          </Button>
        </div>
      </form>
    </Modal>
  );
}
