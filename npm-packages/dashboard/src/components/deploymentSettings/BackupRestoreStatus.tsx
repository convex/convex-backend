import { Button } from "@ui/Button";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { Callout } from "@ui/Callout";
import { Modal } from "@ui/Modal";
import { formatNumberCompact } from "@common/lib/format";
import { useQuery } from "convex/react";
import { useConfirmImport } from "hooks/deploymentApi";
import { useEffect, useState } from "react";
import { Doc } from "system-udfs/convex/_generated/dataModel";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import udfs from "@common/udfs";
import { CheckCircledIcon, CrossCircledIcon } from "@radix-ui/react-icons";
import { ProgressBar } from "@ui/ProgressBar";
import { useGetCloudBackup, BackupResponse } from "api/backups";
import { TransferSummary } from "./BackupListItem";
import { ImportSummary } from "./SnapshotImport";

export function BackupRestoreStatus({
  deployment,
}: {
  deployment: PlatformDeploymentResponse;
}) {
  const currentRestore = useLatestRestore();
  const requestor = currentRestore?.requestor;
  if (requestor && requestor.type !== "cloudRestore") {
    throw new Error("Unexpected requestor for restore");
  }
  const sourceCloudBackupId = requestor?.sourceCloudBackupId;

  const backup = useGetCloudBackup(
    sourceCloudBackupId !== undefined ? Number(sourceCloudBackupId) : undefined,
  );

  // Automatically call confirmImport when the current backup is waiting for confirmation.
  // This is necessary because the snapshot import flow has a confirmation step, but for backups
  // the user doesn’t confirm the import after it’s created.
  const confirmImport = useConfirmImport();
  useEffect(() => {
    if (currentRestore?.state.state === "waiting_for_confirmation") {
      void confirmImport(currentRestore._id);
    }
  }, [currentRestore?.state.state, currentRestore?._id, confirmImport]);

  if (!currentRestore || backup === undefined) return null;

  const { state } = currentRestore;

  switch (state.state) {
    case "in_progress":
      return <BackupRestoreOngoing progressMessage={state.progress_message} />;
    case "uploaded":
    case "waiting_for_confirmation":
      return <BackupRestoreOngoing progressMessage="Starting the restore…" />;
    case "completed":
      return (
        <BackupRestoreSuccess
          completedTime={new Date(Number(state.timestamp / BigInt(1000000)))}
          restoredRowsCount={state.num_rows_written}
          deployment={deployment}
          backup={backup}
          snapshotImportCheckpoints={currentRestore.checkpoints}
        />
      );
    case "failed":
      return (
        <BackupRestoreFail
          errorMessage={state.error_message}
          restoreStartTime={new Date(currentRestore._creationTime)}
          deployment={deployment}
          backup={backup}
        />
      );
    default: {
      state satisfies never;
      return null;
    }
  }
}

export function useLatestRestore():
  | Doc<"_snapshot_imports">
  | null
  | undefined {
  const allImports = useQuery(udfs.snapshotImport.list);
  return allImports?.find((i) => i.requestor.type === "cloudRestore") ?? null;
}

export function BackupRestoreFail({
  errorMessage,
  restoreStartTime,
  deployment,
  backup,
}: {
  errorMessage: string;
  restoreStartTime: Date;
  deployment: PlatformDeploymentResponse;
  backup: BackupResponse | null;
}) {
  const [isModalOpen, setIsModalOpen] = useState(false);

  return (
    <>
      <div className="flex min-h-16 flex-wrap items-center gap-2 rounded-lg border bg-background-secondary px-4 py-2">
        <div className="flex grow gap-2">
          <CrossCircledIcon className="size-5 shrink-0 text-content-errorSecondary" />
          <p className="grow text-sm/tight text-balance text-content-primary">
            The restore started{" "}
            <TimestampDistance
              date={restoreStartTime}
              className="text-sm text-inherit"
            />{" "}
            failed.
          </p>
        </div>
        <Button
          size="sm"
          variant="neutral"
          onClick={() => setIsModalOpen(true)}
        >
          More Details
        </Button>
      </div>

      {isModalOpen && (
        <Modal
          onClose={() => setIsModalOpen(false)}
          title={
            <div className="flex items-center gap-1">
              Restore Failed <CrossCircledIcon className="text-content-error" />
            </div>
          }
          size="md"
        >
          <TransferSummary
            backup={backup}
            targetDeployment={deployment}
            latestBackupInTargetDeployment={undefined}
          />

          <p className="my-2">Encountered an error while restoring:</p>
          <Callout variant="error">{errorMessage}</Callout>
        </Modal>
      )}
    </>
  );
}

export function BackupRestoreSuccess({
  completedTime,
  restoredRowsCount,
  deployment,
  backup,
  snapshotImportCheckpoints,
}: {
  completedTime: Date;

  // `number` is used from stories because Storybook crashes when using bigint
  restoredRowsCount: bigint | number;

  deployment: PlatformDeploymentResponse;
  backup: BackupResponse | null;
  snapshotImportCheckpoints: Doc<"_snapshot_imports">["checkpoints"] | null;
}) {
  const [isModalOpen, setIsModalOpen] = useState(false);

  return (
    <>
      <div className="flex min-h-16 flex-wrap items-center gap-2 rounded-lg border bg-background-secondary px-4 py-2">
        <div className="flex grow items-center gap-2">
          <CheckCircledIcon className="shrink-0 text-content-success" />
          <p className="grow text-sm/tight text-balance text-content-primary">
            <strong>{`${formatNumberCompact(restoredRowsCount)} ${restoredRowsCount === BigInt(1) ? "document" : "documents"}`}</strong>{" "}
            {restoredRowsCount === BigInt(1) ? "was" : "were"} restored from a
            backup{" "}
            <TimestampDistance
              date={completedTime}
              className="text-sm text-inherit"
            />
            .
          </p>
        </div>
        <Button
          size="sm"
          variant="neutral"
          onClick={() => setIsModalOpen(true)}
        >
          More Details
        </Button>
      </div>

      {isModalOpen && (
        <Modal
          onClose={() => setIsModalOpen(false)}
          title={
            <div className="flex items-center gap-1">
              Restore Succeeded{" "}
              <CheckCircledIcon className="text-content-success" />
            </div>
          }
          size="md"
        >
          <TransferSummary
            backup={backup}
            targetDeployment={deployment}
            latestBackupInTargetDeployment={undefined}
          />

          <ImportSummary
            snapshotImportCheckpoints={snapshotImportCheckpoints}
          />
        </Modal>
      )}
    </>
  );
}

export function BackupRestoreOngoing({
  progressMessage,
}: {
  progressMessage: string;
}) {
  return (
    <div className="flex min-h-16 flex-col flex-wrap justify-center gap-2 rounded-lg border bg-background-secondary px-4 py-2 text-sm">
      <div className="flex flex-wrap justify-end gap-4">
        <div className="grow font-semibold">Restoring from a backup</div>
        <div className="min-w-56 text-right text-content-secondary">
          {progressMessage}
        </div>
      </div>
      <ProgressBar fraction={undefined} ariaLabel="In progress" />
    </div>
  );
}
