import { Cursor } from "convex/server";
import { useContext, useState } from "react";
import { useMountedState } from "react-use";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { toast } from "@common/lib/utils";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { ProgressBar } from "@ui/ProgressBar";

export function ClearTableConfirmation({
  numRows,
  closePopup,
  tableName,
  isProd,
  clearTable,
  clearSelectedRows,
}: {
  numRows?: number;
  closePopup: () => void;
  clearTable: (cursor: Cursor | null) => Promise<{
    continueCursor: Cursor;
    deleted: number;
    hasMore: boolean;
  }>;
  clearSelectedRows: () => void;
  tableName: string;
  isProd: boolean;
}) {
  const [initialNumRows, setInitialNumRows] = useState(numRows);
  const [numDeleted, setNumDeleted] = useState(0);
  const [isClearing, setIsClearing] = useState(false);
  const progressPct = initialNumRows
    ? Math.min(100, Math.floor((numDeleted / initialNumRows) * 100))
    : 0;

  const confirmClose = () => {
    if (isClearing) {
      const shouldClose = window.confirm(
        "Closing the popup will cancel the clear table operation with the table partially cleared. Are you sure you want to continue?",
      );
      if (!shouldClose) {
        return false;
      }
    }
    return true;
  };

  const isMounted = useMountedState();

  const { captureException } = useContext(DeploymentInfoContext);

  return (
    <ConfirmationDialog
      onClose={closePopup}
      onBeforeClose={confirmClose}
      onConfirm={async () => {
        setInitialNumRows(numRows);
        setIsClearing(true);
        let nextCursor: Cursor | null = null;
        let hasMoreDocuments = true;
        while (isMounted() && hasMoreDocuments) {
          try {
            const {
              hasMore,
              deleted,
              continueCursor,
            }: Awaited<ReturnType<typeof clearTable>> =
              await clearTable(nextCursor);
            hasMoreDocuments = hasMore;
            nextCursor = continueCursor;
            setNumDeleted((prev) => prev + deleted);
          } catch (e) {
            hasMoreDocuments = false;
            captureException(e);
            toast(
              "error",
              "Failed to clear table. Please try again or contact support.",
            );
            setIsClearing(false);
            return;
          }
        }
        setIsClearing(false);
        clearSelectedRows();

        if (isMounted()) {
          toast("success", "Table cleared.");
        } else {
          toast("info", "Table partially cleared.");
        }
      }}
      validationText={
        isProd && !isClearing
          ? `Delete all production documents in ${tableName}`
          : undefined
      }
      confirmText="Confirm"
      variant="danger"
      dialogTitle="Clear table"
      disableCancel={isClearing}
      dialogBody={
        isClearing ? (
          <div className="flex flex-col gap-2">
            <span className="text-xs font-semibold">{progressPct}% done</span>
            <ProgressBar
              fraction={progressPct / 100}
              ariaLabel="Clear table progress"
            />
          </div>
        ) : (
          <div className="flex flex-col gap-2">
            <div>Delete all documents in this table?</div>
            <div>
              Clearing a large table may take a few minutes to complete. Keep
              this dialogue open while clearing is in progress. Documents that
              are created during the clear operation may not be deleted.
            </div>
          </div>
        )
      }
    />
  );
}
