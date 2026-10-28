import { Cross2Icon } from "@radix-ui/react-icons";
import { useDeleteTeamAccessToken } from "api/teamAccessTokens";
import { Button } from "@ui/Button";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { useState } from "react";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import type {
  TeamAccessTokenResponse,
  TeamId,
} from "@convex-dev/platform/managementApi";

export function AccessTokenListItem({
  token,
  teamId: identifier,
  canDelete,
}: {
  token: TeamAccessTokenResponse;
  teamId: TeamId;
  // `undefined` means the caller hasn't wired a permission check yet —
  // leave the button enabled rather than silently disabling it. Pass a
  // boolean to actually gate it.
  canDelete?: boolean | undefined;
}) {
  const deleteAccessToken = useDeleteTeamAccessToken(identifier);
  const [showDeleteConfirmation, setShowDeleteConfirmation] = useState(false);

  return (
    <div key={token.id} className="flex w-full flex-col py-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>{token.name}</div>
        <div className="flex flex-wrap items-center gap-4">
          <div className="flex flex-col items-end">
            {token.lastUsedTime !== null && token.lastUsedTime !== undefined ? (
              <TimestampDistance
                prefix="Last used "
                date={new Date(token.lastUsedTime)}
              />
            ) : (
              <div className="text-xs text-content-secondary">Never used</div>
            )}
            <div className="flex gap-1">
              <TimestampDistance
                prefix="Created "
                date={new Date(token.creationTime)}
              />
            </div>
            {token.expiresAt !== null && token.expiresAt !== undefined && (
              <TimestampDistance
                prefix="Expires "
                date={new Date(token.expiresAt)}
                className="text-left text-content-errorSecondary"
              />
            )}
          </div>
          <div className="flex gap-2">
            <Button
              variant="danger"
              icon={<Cross2Icon />}
              onClick={() => {
                setShowDeleteConfirmation(true);
              }}
              disabled={canDelete === false}
              tip={
                canDelete === false
                  ? permissionDeniedTip(
                      "You do not have permission to delete this access token.",
                      "team:token:delete",
                    )
                  : undefined
              }
            >
              Delete
            </Button>
          </div>
        </div>
      </div>
      {showDeleteConfirmation && (
        <ConfirmationDialog
          onClose={() => {
            setShowDeleteConfirmation(false);
          }}
          onConfirm={async () => {
            await deleteAccessToken({ id: token.name });
          }}
          confirmText="Delete"
          dialogTitle="Delete Access Token"
          dialogBody={
            <>
              Are you sure you want to delete:{" "}
              <span className="font-semibold">{token.name}</span>?
            </>
          }
        />
      )}
    </div>
  );
}
