import { Button } from "@ui/Button";
import { Combobox } from "@ui/Combobox";
import { Link } from "@ui/Link";
import { Modal } from "@ui/Modal";
import { Tooltip } from "@ui/Tooltip";
import { useState } from "react";
import type {
  CustomRoleResponse,
  TeamMember,
} from "@convex-dev/platform/managementApi";
import type { TeamResponse } from "generatedApi";
import { useHasCustomRolePermission } from "api/roles";
import { CUSTOM_ROLE_RESOURCE } from "lib/permissions";
import { CustomRolesSelector } from "./CustomRolesSelector";

type RoleChoice = "admin" | "developer" | "custom";

function sameIds(a: number[], b: number[]) {
  if (a.length !== b.length) return false;
  const aSorted = [...a].sort((x, y) => x - y);
  const bSorted = [...b].sort((x, y) => x - y);
  return aSorted.every((v, i) => v === bSorted[i]);
}

export function EditTeamRoleDialog({
  team,
  member,
  customRoles,
  customRolesEnabled,
  onSave,
  onClose,
}: {
  team: TeamResponse;
  member: TeamMember;
  customRoles: CustomRoleResponse[];
  customRolesEnabled: boolean;
  onSave: (body: {
    memberId: number;
    role?: "admin" | "developer";
    customRoles?: number[];
  }) => Promise<unknown>;
  onClose: () => void;
}) {
  const memberCustomRoleIds = (member.customRoles ?? []).map((c) => c.id);
  const [choice, setChoice] = useState<RoleChoice>(
    member.role === "custom" ? "custom" : member.role,
  );
  const [selectedCustomRoleIds, setSelectedCustomRoleIds] =
    useState<number[]>(memberCustomRoleIds);
  const [isSaving, setIsSaving] = useState(false);
  const [didAttemptSave, setDidAttemptSave] = useState(false);

  const canViewCustomRoles = useHasCustomRolePermission(
    team.id,
    "customRole:view",
    CUSTOM_ROLE_RESOURCE,
    true,
  );
  const customDisabledReason = !customRolesEnabled
    ? "Custom roles are not enabled for this team."
    : canViewCustomRoles === false
      ? "You do not have permission to view custom roles."
      : undefined;
  const roleOptions = [
    { label: "Admin", value: "admin" as const, disabled: false },
    { label: "Developer", value: "developer" as const, disabled: false },
    {
      label: "Custom",
      value: "custom" as const,
      disabled: customDisabledReason !== undefined,
    },
  ];

  const isUnchanged =
    choice === "custom"
      ? member.role === "custom" &&
        sameIds(selectedCustomRoleIds, memberCustomRoleIds)
      : member.role === choice;
  const customSelectionEmpty =
    choice === "custom" && selectedCustomRoleIds.length === 0;
  const noCustomRolesAvailable =
    choice === "custom" && customRoles.length === 0;

  return (
    <Modal
      title="Edit team role"
      description={`Change the team role for ${member.name || member.email}.`}
      onClose={onClose}
    >
      <form
        className="flex flex-col gap-4"
        onSubmit={async (e) => {
          e.preventDefault();
          setDidAttemptSave(true);
          if (
            isUnchanged ||
            customSelectionEmpty ||
            noCustomRolesAvailable ||
            isSaving
          ) {
            return;
          }
          setIsSaving(true);
          try {
            if (choice === "custom") {
              await onSave({
                memberId: member.id,
                customRoles: selectedCustomRoleIds,
              });
            } else {
              await onSave({ memberId: member.id, role: choice });
            }
            onClose();
          } finally {
            setIsSaving(false);
          }
        }}
      >
        <div className="flex flex-col gap-1">
          <Combobox
            label="Role"
            labelHidden={false}
            options={roleOptions}
            selectedOption={choice}
            setSelectedOption={(value) => {
              if (value) setChoice(value);
            }}
            disableSearch
            Option={({ label, disabled }) =>
              disabled && customDisabledReason ? (
                <Tooltip tip={customDisabledReason} side="left">
                  <span>{label}</span>
                </Tooltip>
              ) : (
                <span>{label}</span>
              )
            }
          />
        </div>

        {choice === "custom" && (
          <div className="flex flex-col gap-1">
            <p className="mb-2 text-xs text-content-secondary">
              Custom roles let you assign granular permissions to team members.
              Manage them in{" "}
              <Link
                href={`/t/${team.slug}/settings/custom-roles`}
                className="items-center"
              >
                Team Settings → Custom Roles
              </Link>
              .
            </p>
            <CustomRolesSelector
              availableRoles={customRoles}
              selectedIds={selectedCustomRoleIds}
              onChange={setSelectedCustomRoleIds}
            />
            {didAttemptSave &&
              customSelectionEmpty &&
              !noCustomRolesAvailable && (
                <span className="text-xs text-content-error">
                  Select at least one custom role.
                </span>
              )}
          </div>
        )}

        <div className="mt-2 flex justify-end gap-2">
          <Button variant="neutral" onClick={onClose} disabled={isSaving}>
            Cancel
          </Button>
          <Button
            type="submit"
            loading={isSaving}
            disabled={isUnchanged || noCustomRolesAvailable}
          >
            Save
          </Button>
        </div>
      </form>
    </Modal>
  );
}
