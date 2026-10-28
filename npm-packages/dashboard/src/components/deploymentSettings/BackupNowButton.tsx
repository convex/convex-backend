import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { ArchiveIcon } from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { Modal } from "@ui/Modal";
import {
  useListCloudBackupsIfAvailable,
  useRequestCloudBackup,
} from "api/backups";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { usePostHog } from "hooks/usePostHog";
import { useState } from "react";
import { BackupStorageSelector } from "./BackupStorageSelector";

export function BackupNowButton({
  teamId,
  deployment,
  maxCloudBackups,
  canCreate,
  onBackupRequested,
}: {
  teamId: number;
  deployment: PlatformDeploymentResponse;
  maxCloudBackups: number;
  canCreate: boolean;
  onBackupRequested?: () => void;
}) {
  const backups = useListCloudBackupsIfAvailable(deployment);
  const nonFailedBackupsForDeployment = backups?.filter(
    (backup) =>
      backup.state === "requested" ||
      backup.state === "inProgress" ||
      backup.state === "complete",
  );

  const deploymentId = deployment.kind === "cloud" ? deployment.id : undefined;
  const requestBackup = useRequestCloudBackup(deploymentId);
  const [isOngoing, setIsOngoing] = useState(false);
  const [showModal, setShowModal] = useState(false);
  const [includeStorage, setIncludeStorage] = useState(false);
  const { capture } = usePostHog();

  const doBackup = async () => {
    setIsOngoing(true);
    try {
      await requestBackup({ includeStorage });
      capture("created_backup", { includedStorage: includeStorage });
    } finally {
      setIsOngoing(false);
    }
    setShowModal(false);
    if (onBackupRequested) {
      onBackupRequested();
    }
  };

  return (
    <>
      <Button
        variant="neutral"
        className="w-fit"
        loading={isOngoing}
        icon={<ArchiveIcon />}
        onClick={() => setShowModal(true)}
        disabled={
          nonFailedBackupsForDeployment === undefined ||
          nonFailedBackupsForDeployment.length >= maxCloudBackups ||
          !canCreate
        }
        tip={
          isOngoing
            ? "A backup is currently in progress."
            : nonFailedBackupsForDeployment &&
                nonFailedBackupsForDeployment.length >= maxCloudBackups
              ? `You can only have up to ${maxCloudBackups} backups on your current plan. Delete some of your existing backups in this deployment to create a new one.`
              : !canCreate
                ? permissionDeniedTip(
                    "You do not have permission to create backups.",
                    "deployment:backups:create",
                  )
                : undefined
        }
      >
        Backup Now
      </Button>

      {showModal && (
        <RequestBackupModal
          teamId={teamId}
          deployment={deployment}
          includeStorage={includeStorage}
          setIncludeStorage={setIncludeStorage}
          onClose={() => setShowModal(false)}
          onCreate={doBackup}
          isOngoing={isOngoing}
        />
      )}
    </>
  );
}

function RequestBackupModal({
  teamId,
  deployment,
  includeStorage,
  setIncludeStorage,
  onClose,
  onCreate,
  isOngoing,
}: {
  teamId: number;
  deployment: PlatformDeploymentResponse;
  includeStorage: boolean;
  setIncludeStorage: (includeStorage: boolean) => void;
  onClose: () => void;
  onCreate: () => Promise<void>;
  isOngoing: boolean;
}) {
  return (
    <Modal onClose={onClose} title="Request an immediate backup" size="sm">
      <BackupStorageSelector
        teamId={teamId}
        deployment={deployment}
        includeStorage={includeStorage}
        setIncludeStorage={setIncludeStorage}
      />

      <Button
        className="mt-4 ml-auto flex gap-2"
        variant="primary"
        onClick={onCreate}
        loading={isOngoing}
      >
        Create Backup
      </Button>
    </Modal>
  );
}
