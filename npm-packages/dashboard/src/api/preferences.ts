import { useCallback } from "react";
import { MemberPreferences, SetPreferenceArgs } from "generatedApi";
import { useBBMutation, useBBQuery, useMutate } from "./api";

const PREFERENCES_PATH = "/preferences" as const;

export function useMemberPreferences(): MemberPreferences | undefined {
  const { data } = useBBQuery({
    path: PREFERENCES_PATH,
    pathParams: undefined,
    swrOptions: {
      revalidateOnMount: false,
      revalidateIfStale: false,
      revalidateOnFocus: false,
      revalidateOnReconnect: false,
    },
  });
  return data?.preferences;
}

export function useSetPreference() {
  const preferences = useMemberPreferences();
  const setPreference = useBBMutation({
    method: "put",
    path: "/set_preference",
    pathParams: undefined,
  });
  const mutate = useMutate();

  return useCallback(
    async ({ name, value }: SetPreferenceArgs) => {
      await setPreference({ name, value });
      await mutate(
        [PREFERENCES_PATH],
        { preferences: { ...preferences, [name]: value } },
        { revalidate: true },
      );
    },
    [setPreference, mutate, preferences],
  );
}
