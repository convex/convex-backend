import { TrashIcon } from "@radix-ui/react-icons";
import { useState, useContext } from "react";
import { Id } from "system-udfs/convex/_generated/dataModel";
import { Button } from "@ui/Button";
import { useNents } from "@common/lib/useNents";
import { PermissionsContext } from "@common/lib/deploymentContext";
import { DeleteFileModal } from "./DeleteFileModal";

export function DeleteFilesButton({
  selectedFiles,
}: {
  selectedFiles: Id<"_storage">[];
}) {
  const [showDeleteModal, setShowDeleteModal] = useState(false);
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canDeleteFiles = useIsOperationAllowed("WriteData");

  const { selectedNent } = useNents();
  const isInUnmountedComponent = !!(
    selectedNent && selectedNent.state !== "active"
  );

  const { length } = selectedFiles;
  if (length === 0) return null;

  return (
    <>
      <Button
        onClick={() => setShowDeleteModal(true)}
        variant="danger"
        icon={<TrashIcon aria-hidden="true" />}
        disabled={!canDeleteFiles || isInUnmountedComponent}
        tip={
          isInUnmountedComponent
            ? "Cannot delete files in an unmounted component."
            : !canDeleteFiles &&
              "You do not have permission to delete files in this deployment."
        }
      >
        Delete {`${length} file${length > 1 ? "s" : ""}`}
      </Button>

      {showDeleteModal && (
        <DeleteFileModal
          storageIds={selectedFiles}
          onClose={() => setShowDeleteModal(false)}
        />
      )}
    </>
  );
}
