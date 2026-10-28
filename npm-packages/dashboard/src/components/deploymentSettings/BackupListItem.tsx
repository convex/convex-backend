import {
  ArchiveIcon,
  ArrowDownIcon,
  ArrowRightIcon,
  CrossCircledIcon,
  DotsVerticalIcon,
  ExclamationTriangleIcon,
  GlobeIcon,
  MinusCircledIcon,
  Pencil2Icon,
} from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";
import { Loading } from "@ui/Loading";
import { Spinner } from "@ui/Spinner";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { toast } from "@common/lib/utils";
import { Callout } from "@ui/Callout";
import { Modal } from "@ui/Modal";
import { Checkbox } from "@ui/Checkbox";
import { Menu, MenuItem } from "@ui/Menu";
import { useEffect, useId, useRef, useState } from "react";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { ProjectDetails } from "generatedApi";
import { useDeploymentByName } from "api/deployments";
import { useTeamMembers } from "api/teams";
import { useProjectById } from "api/projects";
import { useProfile } from "api/profile";
import {
  CommandLineIcon,
  ServerIcon,
  SignalIcon,
} from "@heroicons/react/24/outline";
import {
  useRestoreFromCloudBackup,
  useDeleteCloudBackup,
  BackupResponse,
  useCancelCloudBackup,
} from "api/backups";
import { Doc, Id } from "system-udfs/convex/_generated/dataModel";
import { BackupIdentifier } from "elements/BackupIdentifier";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { cn } from "@ui/cn";
import { getDeploymentLabel } from "elements/DeploymentDisplay";
import { BackupNowButton } from "./BackupNowButton";

export function BackupListItem({
  teamId,
  backup,
  restoring,
  someBackupInProgress,
  someRestoreInProgress,
  latestBackupInTargetDeployment,
  targetDeployment,
  canCreate,
  canImport,
  canDelete,
  downloadZipExport,
  maxCloudBackups,
  progressMessage,
}: {
  teamId: number;
  backup: BackupResponse;
  restoring: boolean;
  someBackupInProgress: boolean;
  someRestoreInProgress: boolean;
  latestBackupInTargetDeployment: BackupResponse | null;
  targetDeployment: PlatformDeploymentResponse;
  canCreate: boolean;
  canImport: boolean;
  canDelete: boolean;
  downloadZipExport: (snapshotId: Id<"_exports">) => Promise<void>;
  maxCloudBackups: number;
  progressMessage: string | null;
}) {
  const [modal, setModal] = useState<
    null | "suggestBackup" | "restoreConfirmation" | "delete" | "cancel"
  >(null);

  const targetDeploymentId =
    targetDeployment.kind === "cloud" ? targetDeployment.id : null;
  const restoreBlockedByDedicated =
    targetDeployment.kind === "cloud" && targetDeployment.class.startsWith("d");

  const previousState = useRef(backup.state);
  useEffect(() => {
    if (
      previousState.current !== "complete" &&
      backup.state === "complete" &&
      targetDeployment.kind === "cloud" &&
      backup.sourceDeploymentId === targetDeploymentId
    ) {
      toast("success", "Backup completed successfully.");
    }
    previousState.current = backup.state;
  }, [
    backup.state,
    backup.sourceDeploymentId,
    targetDeployment.kind,
    targetDeploymentId,
  ]);

  let backupStateDescription;
  if (backup.state === "requested" || backup.state === "inProgress") {
    backupStateDescription = "This backup hasn't completed yet.";
  } else if (backup.state === "complete") {
    backupStateDescription = "This backup is complete.";
  } else if (backup.state === "canceled") {
    backupStateDescription = "This backup was canceled.";
  } else if (backup.state === "failed") {
    backupStateDescription = "This backup failed.";
  } else {
    backup.state satisfies never;
  }

  return (
    <>
      <div className="flex w-full flex-col">
        <div className="my-2 flex flex-wrap items-center gap-4">
          <div className="flex flex-1 flex-col items-start gap-1">
            <span className="min-w-fit text-sm font-medium">
              Backup from {new Date(backup.requestedTime).toLocaleString()}
              {backup.state === "complete" &&
                typeof backup.completedTime === "number" && (
                  <span className="text-xs font-normal text-content-secondary">
                    {" "}
                    (completed in{" "}
                    {formatBackupDuration(
                      backup.completedTime - backup.requestedTime,
                    )}
                    )
                  </span>
                )}
            </span>
            <BackupIdentifier backup={backup} />
            {(backup.state === "requested" ||
              backup.state === "inProgress") && (
              <div className="flex items-center gap-2 text-content-secondary">
                <Spinner />
                {progressMessage || "In progress"}
              </div>
            )}
          </div>
          <div className="flex flex-col items-end gap-1">
            <span className="text-xs text-content-secondary">
              {backup.includeStorage ? "Includes file storage" : "Tables only"}
            </span>
            {backup.expirationTime !== null && (
              <TimestampDistance
                prefix="Expires "
                date={new Date(backup.expirationTime)}
                className="text-left text-content-errorSecondary"
              />
            )}
          </div>
          {backup.state === "failed" && (
            <Tooltip
              tip="This backup couldn’t be completed. Contact support@convex.dev for help."
              side="right"
            >
              <div className="flex items-center gap-1 text-content-errorSecondary">
                <CrossCircledIcon />
                <span className="border-b border-dotted border-b-content-errorSecondary/50">
                  Failed
                </span>
              </div>
            </Tooltip>
          )}
          {backup.state === "canceled" && (
            <div className="flex items-center gap-1 text-content-secondary">
              <MinusCircledIcon />
              Canceled
            </div>
          )}
          <div>
            {restoring ? (
              <div className="flex items-center gap-2 text-content-secondary">
                <Spinner />
                Restoring…
              </div>
            ) : (
              <Menu
                buttonProps={{
                  size: "xs",
                  variant: "neutral",
                  icon: <DotsVerticalIcon />,
                  title: "Backup options",
                }}
                placement="bottom-end"
              >
                {backup.state === "requested" ||
                backup.state === "inProgress" ? (
                  <MenuItem
                    variant="danger"
                    action={() => setModal("cancel")}
                    disabled={backup.state !== "inProgress" || !canDelete}
                    tipSide="left"
                    tip={
                      backup.state !== "inProgress"
                        ? "This backup hasn't started yet."
                        : !canDelete
                          ? permissionDeniedTip(
                              "You do not have permission to cancel backups.",
                              "deployment:backups:delete",
                            )
                          : undefined
                    }
                  >
                    Cancel
                  </MenuItem>
                ) : null}
                <MenuItem
                  disabled={
                    backup.state !== "complete" ||
                    (targetDeployment.kind === "cloud" &&
                      backup.sourceDeploymentId !== targetDeployment.id)
                  }
                  action={() => {
                    if (backup.state !== "complete") return; // only when disabled
                    void downloadZipExport(backup.snapshotId);
                  }}
                  tipSide="left"
                  tip={
                    backup.state !== "complete"
                      ? backupStateDescription
                      : targetDeployment.kind === "cloud" &&
                          backup.sourceDeploymentId !== targetDeployment.id
                        ? "You may download this backup from the Backup & Restore page for the deployment in which this backup was created."
                        : null
                  }
                >
                  Download
                </MenuItem>
                <MenuItem
                  action={() => {
                    setModal(
                      latestBackupInTargetDeployment === null ||
                        !isInLastFiveMinutes(latestBackupInTargetDeployment)
                        ? "suggestBackup"
                        : "restoreConfirmation",
                    );
                  }}
                  disabled={
                    backup.state !== "complete" ||
                    someRestoreInProgress ||
                    someBackupInProgress ||
                    !canImport ||
                    restoreBlockedByDedicated
                  }
                  tipSide="left"
                  tip={
                    restoreBlockedByDedicated
                      ? `Restores into ${targetDeployment.kind === "cloud" ? targetDeployment.class.toUpperCase() : "dedicated"} deployments must use a physical backup. Contact the Convex team to restore from a physical backup.`
                      : backup.state !== "complete"
                        ? backupStateDescription
                        : someRestoreInProgress
                          ? "Another backup is being restored at the moment."
                          : someBackupInProgress
                            ? "Please wait for the ongoing backup to be completed before restoring from a backup."
                            : !canImport
                              ? permissionDeniedTip(
                                  "You do not have permission to restore backups.",
                                  "deployment:backups:import",
                                )
                              : undefined
                  }
                >
                  Restore
                </MenuItem>
                <MenuItem
                  variant="danger"
                  action={() => setModal("delete")}
                  disabled={
                    backup.state === "inProgress" ||
                    backup.state === "requested" ||
                    !canDelete
                  }
                  tipSide="left"
                  tip={
                    backup.state === "inProgress" ||
                    backup.state === "requested"
                      ? backupStateDescription
                      : !canDelete
                        ? permissionDeniedTip(
                            "You do not have permission to delete backups.",
                            "deployment:backups:delete",
                          )
                        : undefined
                  }
                >
                  Delete
                </MenuItem>
              </Menu>
            )}
          </div>
        </div>
      </div>

      {(modal === "restoreConfirmation" || modal === "suggestBackup") && (
        <Modal
          onClose={() => setModal(null)}
          title={
            modal === "suggestBackup"
              ? "Backup before restoring?"
              : "Restore from a backup"
          }
          size="md"
        >
          {modal === "suggestBackup" ? (
            <SuggestBackup
              teamId={teamId}
              targetDeployment={targetDeployment}
              onClose={() => setModal(null)}
              onContinue={() => setModal("restoreConfirmation")}
              latestBackupInTargetDeployment={latestBackupInTargetDeployment}
              maxCloudBackups={maxCloudBackups}
              canCreate={canCreate}
            />
          ) : (
            <RestoreConfirmation
              backup={backup}
              targetDeployment={targetDeployment}
              latestBackupInTargetDeployment={latestBackupInTargetDeployment}
              onClose={() => setModal(null)}
            />
          )}
        </Modal>
      )}

      {(modal === "delete" || modal === "cancel") && (
        <DeleteOrCancelBackupModal
          action={modal}
          backup={backup}
          onClose={() => setModal(null)}
        />
      )}
    </>
  );
}

function SuggestBackup({
  teamId,
  targetDeployment,
  onClose,
  onContinue,
  latestBackupInTargetDeployment,
  maxCloudBackups,
  canCreate,
}: {
  teamId: number;
  targetDeployment: PlatformDeploymentResponse;
  onClose: () => void;
  onContinue: () => void;
  latestBackupInTargetDeployment: BackupResponse | null;
  maxCloudBackups: number;
  canCreate: boolean;
}) {
  return (
    <>
      <p>
        Restoring the backup will erase the data currently in the deployment.
      </p>

      <DeploymentSummary
        deployment={targetDeployment}
        latestBackup={latestBackupInTargetDeployment}
      />

      <div className="flex justify-end gap-2">
        <BackupNowButton
          teamId={teamId}
          deployment={targetDeployment}
          maxCloudBackups={maxCloudBackups}
          canCreate={canCreate}
          onBackupRequested={onClose}
        />
        <Button variant="primary" onClick={onContinue}>
          Continue
        </Button>
      </div>
    </>
  );
}

function RestoreConfirmation({
  backup,
  targetDeployment,
  latestBackupInTargetDeployment,
  onClose,
}: {
  backup: BackupResponse;
  targetDeployment: PlatformDeploymentResponse;
  latestBackupInTargetDeployment: BackupResponse | null;
  onClose: () => void;
}) {
  const requestRestore = useRestoreFromCloudBackup(
    targetDeployment.kind === "cloud" ? targetDeployment.id : 0,
  );

  const [isSubmitting, setIsSubmitting] = useState(false);

  const needsCheckboxConfirmation = targetDeployment.deploymentType === "prod";
  const [checkboxConfirmation, setCheckboxConfirmation] = useState(false);
  const checkboxConfirmationId = useId();

  return (
    <>
      <TransferSummary
        backup={backup}
        targetDeployment={targetDeployment}
        latestBackupInTargetDeployment={latestBackupInTargetDeployment}
      />

      <p className="my-2 text-sm">
        The tables in <code>{targetDeployment.name}</code> will be replaced by
        the contents of the backup.{" "}
        {backup.includeStorage ??
          "Any files in the backup that do not already exist will be uploaded."}
      </p>

      <p className="text-sm text-content-secondary">
        The rest of your deployment configuration (code, environment variables,
        scheduled functions, existing files, etc.) will not be changed.
      </p>

      {needsCheckboxConfirmation && (
        <Callout className="mt-4 w-fit">
          <label className="block" htmlFor={checkboxConfirmationId}>
            <Checkbox
              id={checkboxConfirmationId}
              className="mr-2"
              checked={checkboxConfirmation}
              onChange={() => setCheckboxConfirmation(!checkboxConfirmation)}
            />
            I understand that this will <strong>erase</strong> my current{" "}
            <strong>production data</strong>.
          </label>
        </Callout>
      )}

      <div className="mt-4 flex justify-end">
        <Button
          variant="neutral"
          onClick={async () => {
            setIsSubmitting(true);
            try {
              await requestRestore({
                id: backup.id,
              });
            } finally {
              setIsSubmitting(false);
            }

            onClose();
          }}
          disabled={needsCheckboxConfirmation && !checkboxConfirmation}
          loading={isSubmitting}
        >
          Restore
        </Button>
      </div>
    </>
  );
}

function DeleteOrCancelBackupModal({
  action,
  backup,
  onClose,
}: {
  action: "delete" | "cancel";
  backup: BackupResponse;
  onClose: () => void;
}) {
  const doDelete = useDeleteCloudBackup(backup.sourceDeploymentId, backup.id);
  const doCancel = useCancelCloudBackup(backup.sourceDeploymentId, backup.id);

  const [isSubmitting, setIsSubmitting] = useState(false);

  return (
    <Modal
      onClose={onClose}
      title={action === "delete" ? "Delete backup" : "Cancel backup"}
    >
      <p className="text-content-secondary">This action cannot be undone.</p>

      <BackupSummary backup={backup} sourceDeploymentAppearance="inline" />

      <div className="flex justify-end gap-2">
        <Button
          variant="danger"
          onClick={async () => {
            setIsSubmitting(true);
            try {
              if (action === "delete") {
                await doDelete();
              } else if (action === "cancel") {
                await doCancel();
              } else {
                action satisfies never;
              }
            } finally {
              setIsSubmitting(false);
            }

            onClose();
          }}
          loading={isSubmitting}
        >
          {action === "delete" ? "Delete" : "Cancel"} Backup
        </Button>
      </div>
    </Modal>
  );
}

export type BackupSummaryProps = {
  backup: BackupResponse | null;
  sourceDeploymentAppearance: null | "inline" | "differentDeploymentWarning";
};

function BackupSummary({
  backup,
  sourceDeploymentAppearance,
}: BackupSummaryProps) {
  const backupDeployment = useDeploymentByName(backup?.sourceDeploymentName);
  const sourceDeployment = backupDeployment ? (
    <Tooltip tip={<code>{backupDeployment.name}</code>}>
      <FullDeploymentName deployment={backupDeployment} />
    </Tooltip>
  ) : (
    <div className="h-16 min-w-52">
      <Loading />
    </div>
  );

  return (
    <div className="my-4 flex flex-col items-center gap-2">
      <p className="flex items-center gap-2 text-content-tertiary">
        <ArchiveIcon className="size-6" /> Backup
      </p>

      <div className="flex flex-col items-center gap-1 text-center">
        {backup !== null ? (
          <>
            <p>
              Backup from
              <br />
              {new Date(backup.requestedTime).toLocaleString()}
            </p>
            <p className="text-xs text-content-secondary">
              (<TimestampDistance date={new Date(backup.requestedTime)} />)
            </p>
            <p className="text-xs text-content-secondary">
              {backup.includeStorage ? "Includes file storage" : "Tables only"}
            </p>
          </>
        ) : (
          <em>Unknown backup</em>
        )}
      </div>

      {sourceDeploymentAppearance === "inline" && sourceDeployment}
      {sourceDeploymentAppearance === "differentDeploymentWarning" && (
        <div className="relative mt-4 rounded-md border border-util-warning px-6 py-3">
          <p className="absolute top-0 left-0 w-full -translate-y-1/2 text-center text-xs text-yellow-700 dark:text-util-warning">
            <span className="inline-flex items-center justify-center gap-1 bg-background-secondary px-2 py-1">
              <ExclamationTriangleIcon className="size-4" />
              From a different deployment
            </span>
          </p>

          <div className="mt-2 flex flex-col place-items-center items-center gap-1 text-xs">
            {sourceDeployment}
          </div>
        </div>
      )}
    </div>
  );
}

type DeploymentSummaryProps = {
  deployment: PlatformDeploymentResponse;
  // null = show no backup warning, undefined = show nothing
  latestBackup: BackupResponse | null | undefined;
};

function DeploymentSummary({
  deployment,
  latestBackup,
}: DeploymentSummaryProps) {
  return (
    <div className="my-4 flex flex-col items-center gap-2">
      <p className="flex items-center gap-2 text-content-tertiary">
        <ServerIcon className="size-6" /> Deployment
      </p>
      <Tooltip tip={<code>{deployment.name}</code>}>
        <FullDeploymentName deployment={deployment} />
      </Tooltip>
      {latestBackup !== undefined && <LatestBackup backup={latestBackup} />}
    </div>
  );
}

export function TransferSummary({
  backup,
  targetDeployment,
  latestBackupInTargetDeployment,
}: {
  backup: BackupResponse | null;
  targetDeployment: PlatformDeploymentResponse;
  latestBackupInTargetDeployment: BackupResponse | null | undefined;
}) {
  return (
    <div className="mb-4 grid justify-center gap-2 rounded-lg border p-4 md:flex md:gap-5">
      <BackupSummary
        backup={backup}
        sourceDeploymentAppearance={
          backup &&
          targetDeployment.kind === "cloud" &&
          backup.sourceDeploymentId !== targetDeployment.id
            ? "differentDeploymentWarning"
            : null
        }
      />

      <div className="flex w-full justify-center md:my-4 md:w-fit">
        <ArrowDownIcon className="size-6 text-content-tertiary md:hidden" />
        <ArrowRightIcon className="hidden size-6 text-content-tertiary md:block" />
      </div>

      <DeploymentSummary
        deployment={targetDeployment}
        latestBackup={latestBackupInTargetDeployment}
      />
    </div>
  );
}

export function FullDeploymentName({
  deployment,
  showProjectName = true,
}: {
  deployment: PlatformDeploymentResponse;
  showProjectName?: boolean;
}) {
  const { project, isLoading } = useProjectById(deployment.projectId);

  if (!isLoading && project === null) {
    throw new Error("Unknown project");
  }

  const whoseName = useMemberName(project ?? undefined, deployment);
  return (
    <div className="flex flex-wrap items-center gap-2">
      {showProjectName && (
        <>
          {isLoading ? (
            <span className="inline-block h-6 w-32">
              <Loading />
            </span>
          ) : (
            <span>{project?.name}</span>
          )}
          <span className="text-content-secondary">/</span>
        </>
      )}
      <DeploymentLabel deployment={deployment} whoseName={whoseName ?? null} />
    </div>
  );
}

function useMemberName(
  project: ProjectDetails | undefined,
  deployment: PlatformDeploymentResponse | undefined,
) {
  const teamMembers = useTeamMembers(project?.teamId);
  const whose = teamMembers?.find((tm) => tm.id === deployment?.creator);
  const profile = useProfile();
  const whoseName =
    whose?.email === profile?.email
      ? profile?.name || profile?.email
      : whose?.name || whose?.email || "Teammate";
  return whoseName;
}

function LatestBackup({ backup }: { backup: BackupResponse | null }) {
  return (
    <p>
      {backup === null ? (
        <span className="text-xs text-content-errorSecondary">No backup</span>
      ) : (
        <TimestampDistance
          prefix="Last backup created"
          className="text-inherit"
          date={new Date(backup.requestedTime)}
        />
      )}
    </p>
  );
}

function isInLastFiveMinutes(backup: BackupResponse): boolean {
  const fiveMinutes = 5 * 60 * 1000;
  return backup.requestedTime >= Date.now() - fiveMinutes;
}

export function progressMessageForBackup(
  // Backup response from big-brain
  backup: BackupResponse,
  // Existing cloud backup within the backend
  existingCloudBackup: Doc<"_exports"> | null | undefined,
) {
  return existingCloudBackup?.state === "in_progress" &&
    backup.state === "inProgress" &&
    existingCloudBackup._id === backup.snapshotId
    ? existingCloudBackup.progress_message || null
    : null;
}

function formatBackupDuration(ms: number): string {
  const totalSeconds = Math.round(ms / 1000);
  if (totalSeconds < 60) {
    return `${totalSeconds}s`;
  }
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (minutes < 60) {
    return seconds > 0 ? `${minutes}m ${seconds}s` : `${minutes}m`;
  }
  const hours = Math.floor(minutes / 60);
  const remainingMinutes = minutes % 60;
  return remainingMinutes > 0 ? `${hours}h ${remainingMinutes}m` : `${hours}h`;
}

function DeploymentLabel({
  whoseName,
  deployment,
}: {
  deployment: PlatformDeploymentResponse;
  whoseName: string | null;
}) {
  return (
    <div className={cn("flex items-center gap-2 rounded-md")}>
      {deployment.deploymentType === "dev" ? (
        deployment.kind === "local" ? (
          <CommandLineIcon className="size-4" />
        ) : (
          <GlobeIcon className="size-4" />
        )
      ) : deployment.deploymentType === "prod" ? (
        <SignalIcon className="size-4" />
      ) : deployment.deploymentType === "preview" ? (
        <Pencil2Icon className="size-4" />
      ) : null}
      {getDeploymentLabel({
        deployment,
        whoseName,
      })}
    </div>
  );
}
