import React, { useContext, useState } from "react";
import { Link } from "@ui/Link";
import { TrashIcon } from "@radix-ui/react-icons";
import { useDeleteComponent } from "@common/features/settings/lib/api";
import { Sheet } from "@ui/Sheet";
import { Nent } from "@common/lib/useNents";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { Button } from "@ui/Button";
import { PermissionsContext } from "@common/lib/deploymentContext";
import { PermissionDeniedTip } from "@common/elements/NoPermissionMessage";

export function Components({ nents }: { nents: Nent[] }) {
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canWriteData = useIsOperationAllowed("WriteData");
  const sortedNents = [...nents]
    .filter((nent) => nent.name !== null)
    .sort((a, b) => {
      if (a.state === "active" && b.state !== "active") {
        return 1;
      }
      if (a.state !== "active" && b.state === "active") {
        return -1;
      }
      return 0;
    });

  return (
    <Sheet className="h-fit">
      <div>
        <h3 className="mb-4">Components</h3>
        <p className="flex flex-wrap gap-1">
          This page lists all of the components that are configured in your
          project's <pre>convex.config.ts</pre> file. You may delete components
          that have been unmounted.{" "}
          <Link // TODO(ENG-7301): Update this link to the correct URL
            href="https://docs.convex.dev/components"
          >
            Learn more
          </Link>
        </p>
        {sortedNents.length === 0 ? (
          <div className="my-4 text-content-secondary">
            There are no components installed in this deployment.
          </div>
        ) : (
          <div className="my-4 flex flex-col divide-y">
            {sortedNents.map((nent, i) => (
              <ComponentListItem key={i} nent={nent} canDelete={canWriteData} />
            ))}
          </div>
        )}
      </div>
    </Sheet>
  );
}

function ComponentListItem({
  nent,
  canDelete,
}: {
  nent: Nent;
  canDelete: boolean;
}) {
  const [showConfirmation, setShowConfirmation] = useState(false);
  const deleteComponent = useDeleteComponent();
  return (
    <div
      className="grid w-full items-center gap-4 py-2"
      style={{
        gridTemplateColumns: "1fr auto",
      }}
    >
      {showConfirmation && (
        <ConfirmationDialog
          onConfirm={async () => {
            // You can only delete non-root components
            if (nent.id) {
              await deleteComponent(nent.id);
            }
            setShowConfirmation(false);
          }}
          onClose={() => setShowConfirmation(false)}
          validationText={nent.path}
          confirmText="Delete"
          dialogTitle="Delete Component"
          dialogBody="Deleting this component will destroy all of its functions and data. It will also delete all subcomponents of this component."
        />
      )}
      <span className="flex items-center truncate">
        {nent.path}
        {nent.state !== "active" && (
          <span className="ml-1 text-content-secondary">(unmounted)</span>
        )}
      </span>
      <Button
        size="sm"
        inline
        className="ml-auto"
        tip={
          !canDelete ? (
            <PermissionDeniedTip
              message="You do not have permission to delete components in this deployment."
              action="deployment:data:write"
            />
          ) : (
            nent.state === "active" &&
            "You must unmount your component before it can be deleted."
          )
        }
        tipSide="left"
        onClick={() => {
          setShowConfirmation(true);
        }}
        variant="danger"
        icon={<TrashIcon />}
        disabled={nent.state === "active" || !canDelete}
      >
        Delete
      </Button>
    </div>
  );
}
