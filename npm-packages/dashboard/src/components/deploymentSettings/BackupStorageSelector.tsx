import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { formatBytes } from "@common/lib/format";
import { InfoCircledIcon } from "@radix-ui/react-icons";
import { Checkbox } from "@ui/Checkbox";
import { cn } from "@ui/cn";
import { Link } from "@ui/Link";
import { Loading } from "@ui/Loading";
import { Spinner } from "@ui/Spinner";
import { Tooltip } from "@ui/Tooltip";
import { useBackupStorageSummary } from "hooks/usageMetrics";
import { useId } from "react";

const includeStorageTip =
  "If file storage isn't included, only database tables are backed up and restored. Files in the destination deployment aren't deleted or replaced. Including file storage adds a full copy of your files to every backup and incurs additional storage costs.";
const fileStorageTooLargeTip =
  "Backups can include up to 1 TB of file storage. This deployment has more than 1 TB.";

// Backups including more file storage than this can't be produced.
const MAX_BACKUP_FILE_STORAGE = 1024 ** 4;

export const backupPricingTip = (
  <>
    Backups generation incurs{" "}
    <Link href="https://docs.convex.dev/database/backup-restore#how-are-they-priced">
      storage and bandwidth usage
    </Link>
    .
  </>
);

export function BackupStorageSelector({
  teamId,
  deployment,
  includeStorage,
  setIncludeStorage,
  disabled = false,
  isSubmitting = false,
  showEstimatedSize = true,
  usage,
}: {
  teamId: number;
  deployment: PlatformDeploymentResponse;
  includeStorage: boolean;
  setIncludeStorage: (includeStorage: boolean) => void | Promise<void>;
  disabled?: boolean;
  isSubmitting?: boolean;
  showEstimatedSize?: boolean;
  usage?: BackupStorageUsage;
}) {
  const includeStorageCheckboxId = useId();
  const queriedUsage = useBackupStorageUsage(
    teamId,
    deployment,
    usage !== undefined,
  );
  const resolvedUsage = usage ?? queriedUsage;
  const { fileStorage, error } = resolvedUsage;
  const fileStorageTooLarge =
    fileStorage !== undefined && fileStorage > MAX_BACKUP_FILE_STORAGE;
  const checkboxDisabled =
    disabled || isSubmitting || (fileStorageTooLarge && !includeStorage);

  return (
    <div className="flex w-full flex-col gap-4">
      <div className="ml-px flex items-center gap-2 text-sm">
        <Tooltip
          tip={
            fileStorageTooLarge && !includeStorage
              ? fileStorageTooLargeTip
              : undefined
          }
          asChild
        >
          <label
            className={cn(
              "flex items-center gap-2",
              checkboxDisabled && "cursor-not-allowed text-content-secondary",
            )}
            htmlFor={includeStorageCheckboxId}
          >
            <Checkbox
              id={includeStorageCheckboxId}
              checked={includeStorage}
              disabled={checkboxDisabled}
              onChange={() => setIncludeStorage(!includeStorage)}
            />
            <span>
              Include file storage{" "}
              <EstimatedSize bytes={fileStorage} error={error} sign="+" />
            </span>
          </label>
        </Tooltip>
        <Tooltip
          tip={includeStorageTip}
          aria-label="About including file storage"
        >
          <InfoCircledIcon className="size-3.5 text-content-secondary" />
        </Tooltip>
        {isSubmitting && <Spinner />}
      </div>

      {showEstimatedSize && (
        <div className="flex items-center justify-between gap-4 rounded-sm border p-3 text-sm">
          <span className="flex items-center gap-1.5">
            Backup size
            <Tooltip tip={backupPricingTip} aria-label="Backup usage pricing">
              <InfoCircledIcon className="size-3.5 text-content-secondary" />
            </Tooltip>
          </span>
          <EstimatedSize
            bytes={estimatedBackupSize(resolvedUsage, includeStorage)}
            error={error}
          />
        </div>
      )}
    </div>
  );
}

export function EstimatedSize({
  bytes,
  error,
  sign,
}: {
  bytes: number | undefined;
  error: unknown;
  sign?: "+";
}) {
  return (
    <span className="inline-flex items-center text-xs whitespace-nowrap text-content-secondary">
      {error ? (
        "(Est. unavailable)"
      ) : bytes === undefined ? (
        <>
          (Est.
          <Loading fullHeight={false} className="ml-1 h-3 w-8" />)
        </>
      ) : (
        `(Est. ${sign ?? ""}${formatBytes(bytes)})`
      )}
    </span>
  );
}

export type BackupStorageUsage = {
  databaseStorage: number | undefined;
  fileStorage: number | undefined;
  error: ReturnType<typeof useBackupStorageSummary>["error"];
};

export function useBackupStorageUsage(
  teamId: number,
  deployment: PlatformDeploymentResponse,
  skip = false,
): BackupStorageUsage {
  const { data, error } = useBackupStorageSummary(
    teamId,
    deployment.projectId,
    deployment.kind === "cloud" ? deployment.name : undefined,
    skip || deployment.kind !== "cloud",
  );
  return {
    databaseStorage: data?.databaseStorage,
    fileStorage: data?.fileStorage,
    error,
  };
}

export function estimatedBackupSize(
  { databaseStorage, fileStorage }: BackupStorageUsage,
  includeStorage: boolean,
): number | undefined {
  if (databaseStorage === undefined) {
    return undefined;
  }
  return databaseStorage + (includeStorage ? (fileStorage ?? 0) : 0);
}
