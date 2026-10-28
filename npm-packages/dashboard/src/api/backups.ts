import { components, CloudBackupResponse } from "generatedApi";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { Id } from "system-udfs/convex/_generated/dataModel";
import { useBBQuery, useBBMutation } from "./api";

// Annotate the backup response with a bit more information.
// TODO: Figure out how to type the enum better on big brain's end.
export type BackupResponse = Omit<CloudBackupResponse, "snapshotId" | "state"> &
  (
    | {
        state: "complete" | "inProgress";
        // document id of the snapshot stored in the backend
        snapshotId: Id<"_exports">;
      }
    | { state: "requested" | "failed" | "canceled" }
  );

export type PeriodicBackupConfig = {
  sourceDeploymentId: number;
  cronspec: string;
  expirationDeltaSecs: number;
  nextRun: number;
};

export function useListCloudBackups(
  deploymentId: number | undefined | null,
): BackupResponse[] | undefined | null {
  const { data } = useBBQuery({
    path: `/deployments/{deployment_id}/list_cloud_backups`,
    pathParams: {
      deployment_id: (deploymentId ?? undefined)?.toString() || "",
    },
    swrOptions: { refreshInterval: 5000 },
  });
  if (deploymentId === null) return null;
  return data as BackupResponse[] | undefined;
}

// Returns null if backups are unavailable (non-cloud deployments),
// undefined while loading, or the backup list when loaded.
export function useListCloudBackupsIfAvailable(
  deployment: PlatformDeploymentResponse | undefined,
): BackupResponse[] | null | undefined {
  const deploymentId = !deployment
    ? undefined
    : deployment.kind === "cloud"
      ? deployment.id
      : null;
  return useListCloudBackups(deploymentId);
}

export function useGetCloudBackup(cloudBackupId?: number) {
  const { data } = useBBQuery({
    path: `/cloud_backups/{cloud_backup_id}`,
    pathParams: {
      cloud_backup_id: cloudBackupId?.toString() || "",
    },
    swrOptions: {
      refreshInterval: 5000,
    },
  });
  return data as BackupResponse | undefined;
}

export function useRequestCloudBackup(
  deploymentId?: components["schemas"]["DeploymentId"],
) {
  return useBBMutation({
    path: `/deployments/{deployment_id}/request_cloud_backup`,
    pathParams: {
      deployment_id: deploymentId?.toString() || "",
    },
    mutateKey: `/deployments/{deployment_id}/list_cloud_backups`,
    mutatePathParams: {
      deployment_id: deploymentId?.toString() || "",
    },
    successToast: "Started a new backup.",
  });
}

export function useDeleteCloudBackup(
  deploymentId: number,
  cloudBackupId?: number,
) {
  return useBBMutation({
    path: `/cloud_backups/{cloud_backup_id}/delete`,
    pathParams: {
      cloud_backup_id: cloudBackupId?.toString() || "",
    },
    mutateKey: `/deployments/{deployment_id}/list_cloud_backups`,
    mutatePathParams: {
      deployment_id: deploymentId.toString(),
    },
    successToast: "Backup deleted.",
  });
}

export function useCancelCloudBackup(
  deploymentId: number,
  cloudBackupId?: number,
) {
  return useBBMutation({
    path: `/cloud_backups/{cloud_backup_id}/cancel`,
    pathParams: {
      cloud_backup_id: cloudBackupId?.toString() || "",
    },
    mutateKey: `/deployments/{deployment_id}/list_cloud_backups`,
    mutatePathParams: {
      deployment_id: deploymentId.toString(),
    },
    successToast: "Backup canceled.",
  });
}

export function useConfigurePeriodicBackup(deploymentId?: number) {
  return useBBMutation({
    path: `/deployments/{deployment_id}/configure_periodic_backup`,
    pathParams: {
      deployment_id: deploymentId?.toString() || "",
    },
    mutateKey: `/deployments/{deployment_id}/get_periodic_backup_config`,
    mutatePathParams: {
      deployment_id: deploymentId?.toString() || "",
    },
  });
}

export function useDisablePeriodicBackup(deploymentId?: number) {
  return useBBMutation({
    path: `/deployments/{deployment_id}/disable_periodic_backup`,
    pathParams: {
      deployment_id: deploymentId?.toString() || "",
    },
    mutateKey: `/deployments/{deployment_id}/get_periodic_backup_config`,
    mutatePathParams: {
      deployment_id: deploymentId?.toString() || "",
    },
    successToast: "Automatic backups are now disabled.",
  });
}

export function useGetPeriodicBackupConfig(deploymentId?: number) {
  const { data } = useBBQuery({
    path: `/deployments/{deployment_id}/get_periodic_backup_config`,
    pathParams: {
      deployment_id: deploymentId?.toString() || "",
    },
    swrOptions: { refreshInterval: 5000 },
  });
  return data;
}

export function useRestoreFromCloudBackup(deploymentId?: number) {
  return useBBMutation({
    path: `/deployments/{deployment_id}/restore_from_cloud_backup`,
    pathParams: {
      deployment_id: deploymentId?.toString() || "",
    },
    successToast: "Cloud backup restoration successfully requested.",
  });
}
