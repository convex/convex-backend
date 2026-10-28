import { DotsVerticalIcon } from "@radix-ui/react-icons";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { Checkbox } from "@ui/Checkbox";
import { Modal } from "@ui/Modal";
import { Menu, MenuItem } from "@ui/Menu";
import { HelpTooltip } from "@ui/HelpTooltip";
import { PlatformDeployKeyResponse } from "@convex-dev/platform/managementApi";
import { useCurrentTeam, useTeamMembers } from "api/teams";
import { useState } from "react";
import { TeamMemberLink } from "elements/TeamMemberLink";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { usePostHog } from "hooks/usePostHog";
import {
  ACTION_GROUPS,
  DeployKeyAction,
} from "components/deploymentSettings/GenerateDeployKeyButton";

export function DeployKeyListItem({
  deployKey,
  deploymentType,
  onDelete,
  canDelete = true,
}: {
  deployKey: PlatformDeployKeyResponse;
  deploymentType: string;
  onDelete: (args: { id: string }) => Promise<unknown>;
  canDelete?: boolean;
}) {
  const team = useCurrentTeam();
  const members = useTeamMembers(team?.id);
  const [showDeleteConfirmation, setShowDeleteConfirmation] = useState(false);
  const [showOperations, setShowOperations] = useState(false);
  const { capture } = usePostHog();

  const member = members?.find((m) => m.id === deployKey.creator);

  const allowedActions: DeployKeyAction[] | undefined =
    deployKey.allowedActions;

  const knownActionKeys = new Set<string>(
    ACTION_GROUPS.flatMap((g) => g.actions.map((a) => a.key)),
  );
  const unknownActions = allowedActions?.filter(
    (action) => !knownActionKeys.has(action),
  );

  return (
    <div key={deployKey.name} className="flex w-full flex-col">
      <div className="my-2 flex flex-wrap items-center justify-between gap-2">
        <div>
          {deployKey.name || (
            <span className="text-content-secondary italic">Unnamed</span>
          )}
        </div>
        <div className="flex flex-wrap items-center gap-4">
          <div className="flex flex-col items-end">
            {deployKey.lastUsedTime !== null &&
            deployKey.lastUsedTime !== undefined ? (
              <TimestampDistance
                prefix="Last used "
                date={new Date(deployKey.lastUsedTime)}
              />
            ) : (
              <div className="text-xs text-content-secondary">Never used</div>
            )}
            <div className="flex gap-1">
              <TimestampDistance
                prefix="Created "
                date={new Date(deployKey.creationTime)}
              />
              <div className="flex items-center gap-1 text-xs text-content-secondary">
                by{" "}
                {member ? (
                  <TeamMemberLink
                    memberId={deployKey.creator}
                    name={member?.name || member?.email}
                  />
                ) : (
                  "Unknown member"
                )}
              </div>
            </div>
            {deployKey.expiresAt !== null &&
              deployKey.expiresAt !== undefined && (
                <TimestampDistance
                  prefix="Expires "
                  date={new Date(deployKey.expiresAt)}
                  className="text-left text-content-errorSecondary"
                />
              )}
          </div>
          <Menu
            placement="bottom-end"
            buttonProps={{
              variant: "neutral",
              size: "xs",
              icon: <DotsVerticalIcon />,
              "aria-label": "Deploy key options",
            }}
          >
            {allowedActions !== undefined ? (
              <MenuItem action={() => setShowOperations(true)}>
                View allowed actions
              </MenuItem>
            ) : null}
            <MenuItem
              variant="danger"
              action={() => setShowDeleteConfirmation(true)}
              disabled={!canDelete}
              tip={
                canDelete
                  ? undefined
                  : permissionDeniedTip(
                      "You do not have permission to delete this deploy key.",
                      "deployment:token:delete",
                    )
              }
              tipSide="right"
            >
              Delete
            </MenuItem>
          </Menu>
        </div>
      </div>
      {showDeleteConfirmation && (
        <ConfirmationDialog
          onClose={() => {
            setShowDeleteConfirmation(false);
          }}
          onConfirm={async () => {
            await onDelete({ id: deployKey.name });
            capture("deleted_deploy_key", {
              type: deploymentType,
            });
          }}
          confirmText="Delete"
          dialogTitle="Delete Deploy Key"
          dialogBody={
            <>
              Are you sure you want to delete:{" "}
              <span className="font-semibold">{deployKey.name}</span>?
            </>
          }
        />
      )}
      {showOperations && allowedActions !== undefined && (
        <Modal
          title="Allowed Actions"
          onClose={() => setShowOperations(false)}
          size="md"
        >
          <div className="scrollbar max-h-[60dvh] overflow-y-auto">
            <div className="flex flex-col gap-3">
              {ACTION_GROUPS.map((group) => {
                const groupActions = group.actions.filter((a) =>
                  allowedActions.includes(a.key),
                );
                if (groupActions.length === 0) return null;
                return (
                  <div key={group.label}>
                    <div className="mb-1 text-sm font-semibold text-content-secondary">
                      {group.label}
                    </div>
                    <div className="grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-x-4 gap-y-1">
                      {groupActions.map((a) => (
                        <label
                          key={a.key}
                          className="flex items-center gap-2 rounded-sm p-1 text-xs"
                        >
                          <Checkbox checked disabled onChange={() => {}} />
                          <span className="font-mono">{a.key}</span>
                          <HelpTooltip>{a.description}</HelpTooltip>
                        </label>
                      ))}
                    </div>
                  </div>
                );
              })}
              {unknownActions && unknownActions.length > 0 && (
                <div>
                  <div className="mb-1 text-sm font-semibold text-content-secondary">
                    Other
                  </div>
                  <div className="grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-x-4 gap-y-1">
                    {unknownActions.map((action) => (
                      <label
                        key={action}
                        className="flex items-center gap-2 rounded-sm p-1 text-xs"
                      >
                        <Checkbox checked disabled onChange={() => {}} />
                        <span className="font-mono">{action}</span>
                      </label>
                    ))}
                  </div>
                </div>
              )}
            </div>
          </div>
        </Modal>
      )}
    </div>
  );
}
