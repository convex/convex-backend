import { SWRConfiguration } from "swr";

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
  return useManagementApiMutation({
    path: "/projects/{project_id}/update_default_environment_variables",
    pathParams: { project_id: projectId ?? 0 },
    mutateKey:
      "/projects/{project_id}/list_default_environment_variables" as const,
    mutatePathParams: { project_id: projectId ?? 0 },
    successToast: "Environment variables updated.",
  });
}
