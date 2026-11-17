import { SWRConfiguration } from "swr";
import { useCallback } from "react";
import { UpdateDefaultEnvironmentVariablesArgs } from "@convex-dev/platform/managementApi";
import { toast } from "@common/lib/utils";

import { ProjectEnvVarConfig } from "@common/features/settings/lib/types";
import { useManagementApiMutation, useManagementApiQuery } from "api/api";

export function useProjectEnvironmentVariables(
  projectId?: number,
  refreshInterval?: SWRConfiguration["refreshInterval"],
): { configs: ProjectEnvVarConfig[] } | undefined {
  const { data } = useManagementApiQuery({
    path: "/projects/{project_id}/list_default_environment_variables",
    pathParams: { project_id: projectId ?? 0 },
    swrOptions: { refreshInterval },
  });
  if (data?.pagination.hasMore) {
    throw new Error("Unexpected pagination in default environment variables");
  }
  return data ? { configs: data.items } : undefined;
}

export function useUpdateProjectEnvVars(projectId?: number) {
  const update = useManagementApiMutation({
    path: "/projects/{project_id}/update_default_environment_variables",
    pathParams: { project_id: projectId ?? 0 },
    mutateKey:
      "/projects/{project_id}/list_default_environment_variables" as const,
    mutatePathParams: { project_id: projectId ?? 0 },
  });
  return useCallback(
    async (args: UpdateDefaultEnvironmentVariablesArgs) => {
      await update(args);
      const rolloutRequested = args.changes.some(
        (change) => change.applyToExistingDeployments,
      );
      toast(
        "success",
        rolloutRequested
          ? "Environment variables updated. Rollouts to matching deployments have been scheduled."
          : "Environment variables updated.",
      );
    },
    [update],
  );
}
