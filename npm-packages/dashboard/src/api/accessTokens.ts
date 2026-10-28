import {
  useBBMutation,
  useBBQuery,
  useManagementApiMutation,
  useManagementApiQuery,
} from "./api";

export function useTeamAppAccessTokens(teamId?: number) {
  const { data: accessTokens } = useBBQuery({
    path: "/teams/{team_id}/app_access_tokens",
    pathParams: {
      team_id: teamId?.toString() || "",
    },
  });

  return accessTokens;
}

export function useDeployKeys(deploymentName?: string) {
  const { data: deployKeys } = useManagementApiQuery({
    path: "/deployments/{deployment_name}/list_deploy_keys",
    pathParams: {
      deployment_name: deploymentName || "",
    },
  });

  return deployKeys;
}

export function useCreateDeployKey(deploymentName: string) {
  return useManagementApiMutation({
    path: "/deployments/{deployment_name}/create_deploy_key",
    pathParams: {
      deployment_name: deploymentName,
    },
    mutateKey: "/deployments/{deployment_name}/list_deploy_keys",
    mutatePathParams: {
      deployment_name: deploymentName,
    },
    successToast: "Deploy key created.",
    toastOnError: false,
  });
}

export function useDeleteDeployKey(deploymentName: string) {
  return useManagementApiMutation({
    path: "/deployments/{deployment_name}/delete_deploy_key",
    pathParams: {
      deployment_name: deploymentName,
    },
    mutateKey: "/deployments/{deployment_name}/list_deploy_keys",
    mutatePathParams: {
      deployment_name: deploymentName,
    },
    successToast: "Deploy key deleted.",
  });
}

export function usePreviewDeployKeys(projectId?: number) {
  const { data } = useManagementApiQuery({
    path: "/projects/{project_id}/list_preview_deploy_keys",
    pathParams: {
      project_id: projectId ?? 0,
    },
  });

  return data?.items;
}

export function useCreatePreviewDeployKey(projectId: number) {
  return useManagementApiMutation({
    path: "/projects/{project_id}/create_preview_deploy_key",
    pathParams: {
      project_id: projectId,
    },
    mutateKey: "/projects/{project_id}/list_preview_deploy_keys",
    mutatePathParams: {
      project_id: projectId,
    },
    successToast: "Preview deploy key created.",
    toastOnError: false,
  });
}

export function useDeletePreviewDeployKey(projectId: number) {
  return useManagementApiMutation({
    path: "/projects/{project_id}/delete_preview_deploy_key",
    pathParams: {
      project_id: projectId,
    },
    mutateKey: "/projects/{project_id}/list_preview_deploy_keys",
    mutatePathParams: {
      project_id: projectId,
    },
    successToast: "Preview deploy key deleted.",
  });
}

export function useProjectAppAccessTokens(projectId?: number) {
  const { data: accessTokens } = useBBQuery({
    path: "/projects/{project_id}/app_access_tokens",
    pathParams: {
      project_id: projectId?.toString() || "",
    },
  });

  return accessTokens;
}

export function useDeleteAppAccessTokenByName(
  args: { projectId: number | undefined } | { teamId: number },
) {
  return useBBMutation({
    path: "/delete_access_token",
    pathParams: undefined,
    mutateKey:
      "projectId" in args
        ? "/projects/{project_id}/app_access_tokens"
        : "/teams/{team_id}/app_access_tokens",
    mutatePathParams:
      "projectId" in args
        ? { project_id: args.projectId?.toString() ?? "" }
        : { team_id: args.teamId.toString() },
    successToast: "Application access revoked.",
  });
}

export function useAuthorizeApp() {
  return useBBMutation({
    path: "/authorize_app",
    pathParams: undefined,
    toastOnError: false,
  });
}
