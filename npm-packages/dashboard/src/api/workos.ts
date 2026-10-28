import { mutate as globalMutate } from "swr";
import type {
  DeleteWorkOsEnvironmentRequest,
  DisconnectWorkOsTeamRequest,
  GetOrProvisionEnvironmentRequest,
  ProvisionWorkOsTeamRequest,
} from "generatedApi";
import { useBBMutation, useBBQuery } from "./api";

// Helper function to invalidate all WorkOS-related cache keys for a team
async function invalidateWorkOSTeamCache(teamId: string) {
  await globalMutate(
    (key) => {
      if (!Array.isArray(key) || key.length < 3) return false;
      const [prefix, keyPath] = key;
      if (prefix !== "big-brain") return false;

      // Invalidate any WorkOS-related endpoints for this team
      // Also invalidate deployment-level endpoints since they include team data
      return (
        keyPath === `/teams/${teamId}/workos_integration` ||
        keyPath === `/teams/${teamId}/workos_team_health` ||
        keyPath === `/teams/${teamId}/workos_invitation_eligible_emails` ||
        keyPath === "/workos/available_workos_team_emails" ||
        (keyPath.startsWith("/deployments/") &&
          keyPath.includes("/workos_environment"))
      );
    },
    undefined,
    { revalidate: true },
  );
}

// Helper function to invalidate all WorkOS-related cache keys for a deployment
async function invalidateWorkOSEnvironmentCache(deploymentName: string) {
  await globalMutate(
    (key) => {
      if (!Array.isArray(key) || key.length < 3) return false;
      const [prefix, keyPath] = key;
      if (prefix !== "big-brain") return false;

      // Invalidate any WorkOS-related endpoints for this deployment
      return (
        keyPath === `/deployments/${deploymentName}/workos_environment` ||
        keyPath === `/deployments/${deploymentName}/workos_environment_health`
      );
    },
    undefined,
    { revalidate: true },
  );
}

export function useDeploymentWorkOSEnvironment(deploymentName?: string) {
  const { data, error } = useBBQuery({
    path: "/deployments/{deployment_name}/workos_environment",
    pathParams: { deployment_name: deploymentName || "" },
  });

  return { data, error };
}

export function useTeamWorkOSIntegration(teamId?: string) {
  const { data } = useBBQuery({
    path: "/teams/{team_id}/workos_integration",
    pathParams: { team_id: teamId || "" },
  });
  return data;
}

export function useWorkOSTeamHealth(teamId?: string) {
  const { data, error } = useBBQuery({
    path: "/teams/{team_id}/workos_team_health",
    pathParams: { team_id: teamId || "" },
  });
  return { data, error };
}

export function useWorkOSEnvironmentHealth(deploymentName?: string) {
  const { data, error } = useBBQuery({
    path: "/deployments/{deployment_name}/workos_environment_health",
    pathParams: { deployment_name: deploymentName || "" },
  });
  return { data, error };
}

export function useDisconnectWorkOSTeam(teamId?: string) {
  const mutation = useBBMutation({
    path: "/workos/disconnect_workos_team",
    pathParams: undefined,
    method: "post",
    successToast: "WorkOS team disconnected successfully",
    mutateKey: "/teams/{team_id}/workos_integration",
    mutatePathParams: { team_id: teamId || "" },
  });

  return async (body: DisconnectWorkOsTeamRequest) => {
    const result = await mutation(body);
    // Invalidate all related WorkOS caches for this team
    if (teamId) {
      await invalidateWorkOSTeamCache(teamId);
    }
    return result;
  };
}

export function useInviteWorkOSTeamMember() {
  return useBBMutation({
    path: "/workos/invite_team_member",
    pathParams: undefined,
    method: "post",
    successToast: "WorkOS invitation sent successfully",
    toastOnError: false,
  });
}

export function useWorkOSInvitationEligibleEmails(teamId?: string) {
  const { data } = useBBQuery({
    path: "/teams/{team_id}/workos_invitation_eligible_emails",
    pathParams: { team_id: teamId || "" },
  });
  return data;
}

export function useAvailableWorkOSTeamEmails() {
  const { data } = useBBQuery({
    path: "/workos/available_workos_team_emails",
    pathParams: undefined,
  });
  return data;
}

export function useProvisionWorkOSTeam(teamId?: string) {
  const mutation = useBBMutation({
    path: "/workos/provision_associated_workos_team",
    pathParams: undefined,
    method: "post",
    successToast: "WorkOS team created successfully",
    mutateKey: "/teams/{team_id}/workos_integration",
    mutatePathParams: { team_id: teamId || "" },
  });

  return async (body: ProvisionWorkOsTeamRequest) => {
    const result = await mutation(body);
    // Invalidate all related WorkOS caches for this team
    if (teamId) {
      await invalidateWorkOSTeamCache(teamId);
    }
    return result;
  };
}

export function useProvisionWorkOSEnvironment(deploymentName?: string) {
  const mutation = useBBMutation({
    path: "/workos/get_or_provision_workos_environment",
    pathParams: undefined,
    method: "post",
    // No successToast - the component handles success messaging with more context
    // (knows whether env vars were set, whether it's a new vs existing environment, etc.)
    mutateKey: "/deployments/{deployment_name}/workos_environment",
    mutatePathParams: { deployment_name: deploymentName || "" },
  });

  return async (body: GetOrProvisionEnvironmentRequest) => {
    const result = await mutation(body);
    // Invalidate all related WorkOS caches for this deployment
    if (deploymentName) {
      await invalidateWorkOSEnvironmentCache(deploymentName);
    }
    return result;
  };
}

export function useDeleteWorkOSEnvironment(deploymentName?: string) {
  const mutation = useBBMutation({
    path: "/workos/delete_environment",
    pathParams: undefined,
    method: "post",
    successToast: "WorkOS environment deleted successfully",
    mutateKey: "/deployments/{deployment_name}/workos_environment",
    mutatePathParams: { deployment_name: deploymentName || "" },
  });

  return async (body: DeleteWorkOsEnvironmentRequest) => {
    const result = await mutation(body);
    // Invalidate all related WorkOS caches for this deployment
    if (deploymentName) {
      await invalidateWorkOSEnvironmentCache(deploymentName);
    }
    return result;
  };
}

// Project environment hooks
export function useProjectWorkOSEnvironments(projectId?: number) {
  // Skip the query if no projectId
  const shouldFetch = !!projectId;
  const { data } = useBBQuery({
    path: "/projects/{project_id}/workos_environments",
    pathParams: { project_id: projectId || 0 },
    swrOptions: {
      // Disable fetching if no projectId
      revalidateIfStale: shouldFetch,
      revalidateOnFocus: shouldFetch,
      revalidateOnReconnect: shouldFetch,
    },
  });

  if (!projectId) {
    return undefined;
  }
  return data?.environments;
}

export function useGetProjectWorkOSEnvironment(
  projectId?: number,
  clientId?: string,
) {
  // Only fetch if we have both projectId and clientId
  const shouldFetch = !!projectId && !!clientId;

  const { data, error: _error } = useBBQuery({
    path: "/projects/{project_id}/workos_environments/{client_id}",
    pathParams: {
      project_id: projectId || 0,
      client_id: clientId || "",
    },
    swrOptions: {
      revalidateIfStale: shouldFetch,
      revalidateOnFocus: shouldFetch,
      revalidateOnReconnect: shouldFetch,
    },
  });

  if (!shouldFetch) {
    return undefined;
  }

  // Response is now flat (no wrapper)
  return data;
}

export function useCheckProjectEnvironmentHealth(
  projectId?: number,
  clientId?: string,
) {
  const mutation = useBBMutation({
    path: "/workos/check_project_environment_health",
    pathParams: undefined,
    method: "post",
  });

  return async () => {
    if (!projectId || !clientId) return null;
    try {
      const result = await mutation({
        projectId,
        clientId,
      });
      return result;
    } catch {
      return null;
    }
  };
}

export function useProvisionProjectWorkOSEnvironment(projectId?: number) {
  const mutation = useBBMutation({
    path: "/projects/{project_id}/workos_environments",
    pathParams: { project_id: projectId || 0 },
    method: "post",
    successToast: "Project WorkOS environment created successfully",
    mutateKey: "/projects/{project_id}/workos_environments",
    mutatePathParams: { project_id: projectId || 0 },
  });

  return mutation;
}

export function useDeleteProjectWorkOSEnvironment(projectId?: number) {
  const mutation = useBBMutation({
    path: "/workos/delete_project_environment",
    pathParams: undefined,
    method: "post",
    successToast: "Project WorkOS environment deleted successfully",
    mutateKey: "/projects/{project_id}/workos_environments",
    mutatePathParams: { project_id: projectId || 0 },
  });

  return (clientId: string) => mutation({ projectId: projectId!, clientId });
}
