import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { ArchiveIcon } from "@radix-ui/react-icons";
import { useState } from "react";
import { useDownloadZipExport } from "hooks/deploymentApi";
import { BackupResponse, useListCloudBackupsIfAvailable } from "api/backups";
import { Loading } from "@ui/Loading";
import { EmptySection } from "@common/elements/EmptySection";
import { useQuery } from "convex/react";
import udfs from "@common/udfs";
import { BackupListItem, progressMessageForBackup } from "./BackupListItem";
import { BackupDeploymentSelector } from "./BackupDeploymentSelector";
import { useLatestRestore } from "./BackupRestoreStatus";
import { BackupNowButton } from "./BackupNowButton";

export function BackupList({
  teamId,
  targetDeployment,
  canCreate,
  canImport,
  canDelete,
  maxCloudBackups,
}: {
  teamId: number;
  targetDeployment: PlatformDeploymentResponse; // = deployment the settings page is open for
  canCreate: boolean;
  canImport: boolean;
  canDelete: boolean;
  maxCloudBackups: number;
}) {
  const [selectedDeployment, setSelectedDeployment] =
    useState(targetDeployment);

  const backups = useListCloudBackupsIfAvailable(selectedDeployment); // order: latest to oldest

  const latestRestore = useLatestRestore();
  const requestor = latestRestore?.requestor;
  if (requestor && requestor.type !== "cloudRestore") {
    throw new Error("Unexpected requestor for restore");
  }

  const sourceCloudBackupId = requestor?.sourceCloudBackupId ?? null;

  const restoringBackupId =
    !!latestRestore &&
    !["completed", "failed"].includes(latestRestore.state.state)
      ? sourceCloudBackupId
      : null;

  return (
    <div className="flex h-full flex-col">
      <div className="border-b text-content-secondary">
        <BackupDeploymentSelector
          selectedDeployment={selectedDeployment}
          onChange={setSelectedDeployment}
          targetDeployment={targetDeployment}
        >
          <BackupNowButton
            teamId={teamId}
            deployment={targetDeployment}
            maxCloudBackups={maxCloudBackups}
            canCreate={canCreate}
          />
        </BackupDeploymentSelector>
      </div>
      <div className="scrollbar grow overflow-auto">
        {!backups ? (
          <Loading />
        ) : (
          <BackupListForDeployment
            teamId={teamId}
            backups={backups}
            targetDeployment={targetDeployment}
            restoringBackupId={restoringBackupId}
            canCreate={canCreate}
            canImport={canImport}
            canDelete={canDelete}
            maxCloudBackups={maxCloudBackups}
          />
        )}
      </div>
    </div>
  );
}

function BackupListForDeployment({
  teamId,
  backups,
  targetDeployment,
  restoringBackupId,
  canCreate,
  canImport,
  canDelete,
  maxCloudBackups,
}: {
  teamId: number;
  backups: BackupResponse[];
  targetDeployment: PlatformDeploymentResponse;
  restoringBackupId: bigint | null;
  canCreate: boolean;
  canImport: boolean;
  canDelete: boolean;
  maxCloudBackups: number;
}) {
  const existingCloudBackup = useQuery(udfs.latestExport.latestCloudExport);

  // Backups are already scoped to the selected deployment by the server.
  // For target deployment stats, use a separate query (SWR deduplicates when
  // selectedDeployment === targetDeployment, the common case).
  const targetBackups = useListCloudBackupsIfAvailable(targetDeployment);

  const latestBackupInTargetDeployment =
    (targetDeployment.kind === "cloud" && targetBackups?.[0]) || null;

  const someBackupInProgress =
    targetDeployment.kind === "cloud" &&
    (targetBackups ?? []).some(
      (backup) => backup.state === "requested" || backup.state === "inProgress",
    );

  const downloadZipExport = useDownloadZipExport({
    format: "zip",
    include_storage: true,
  });

  return backups.length === 0 ? (
    <EmptySection
      Icon={ArchiveIcon}
      header="No backups in this deployment."
      body="With backups, you can periodically generate snapshots of your deployment data to restore later."
      learnMoreButton={{
        href: "https://docs.convex.dev/database/backup-restore",
        children: "Learn more about backups.",
      }}
      sheet={false}
    />
  ) : (
    <div className="flex flex-col divide-y px-4 py-2">
      {backups.map((backup) => (
        <BackupListItem
          key={backup.id}
          teamId={teamId}
          backup={backup}
          restoring={BigInt(backup.id) === restoringBackupId}
          someBackupInProgress={someBackupInProgress}
          someRestoreInProgress={restoringBackupId !== null}
          latestBackupInTargetDeployment={latestBackupInTargetDeployment}
          targetDeployment={targetDeployment}
          downloadZipExport={downloadZipExport}
          canCreate={canCreate}
          canImport={canImport}
          canDelete={canDelete}
          maxCloudBackups={maxCloudBackups}
          progressMessage={progressMessageForBackup(
            backup,
            existingCloudBackup,
          )}
        />
      ))}
    </div>
  );
}
