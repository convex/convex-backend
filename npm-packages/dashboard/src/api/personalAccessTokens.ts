import { useMemo } from "react";
import { useManagementApiMutation, useManagementApiQuery } from "./api";

export function usePaginatedPersonalAccessTokens(cursor?: string) {
  const queryParams = useMemo(() => ({ cursor }), [cursor]);

  const { data, isLoading } = useManagementApiQuery({
    path: "/list_personal_access_tokens",
    pathParams: undefined as never,
    queryParams,
  });

  return { data, isLoading };
}

export function useCreatePersonalAccessToken() {
  return useManagementApiMutation({
    path: "/create_personal_access_token",
    pathParams: undefined as never,
    mutateKey: "/list_personal_access_tokens",
    mutatePathParams: undefined as never,
    successToast: "Personal access token created.",
  });
}

export function useDeletePersonalAccessToken() {
  return useManagementApiMutation({
    path: "/delete_personal_access_token",
    pathParams: undefined as never,
    mutateKey: "/list_personal_access_tokens",
    mutatePathParams: undefined as never,
    successToast: "Personal access token deleted.",
  });
}
