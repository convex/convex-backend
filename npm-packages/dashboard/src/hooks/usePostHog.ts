import { useCallback, useRef } from "react";
import { usePostHog as usePostHogOriginal } from "posthog-js/react";
import { FiltersAppliedProperties } from "@common/features/data/lib/filterAnalytics";

// Map of event names to their properties (use `never` if no properties).
export type PostHogEventMap = {
  created_project: never;
  deleted_projects: never;
  created_table: never;
  add_documents: {
    count: number;
  };
  added_environment_variables: {
    count: number;
  };
  uploaded_files: {
    count: number;
  };
  ran_custom_query: never;
  copied_query_result: never;
  upgraded_to_pro: never;
  added_integration: {
    kind: string;
  };
  paused_deployment: never;
  generated_deploy_key: {
    type: string;
  };
  deleted_deploy_key: {
    type: string;
  };
  created_backup: {
    includedStorage: boolean;
  };
  // How the command palette was opened.
  command_palette_opened: {
    via:
      | "hotkey"
      | "slash"
      | "button"
      | "project-selector"
      | "deployment-selector"
      | "backup-restore-from";
  };
  command_palette_item_selected: {
    kind: string;
  };
  data_filters_applied: FiltersAppliedProperties;
};

export type PostHogEvent = keyof PostHogEventMap;

// Type-safe wrapper around PostHog's capture function, only allows capturing
// events with predefined event names and their specific properties.
export function usePostHog() {
  const posthog = usePostHogOriginal();
  const posthogRef = useRef(posthog);
  posthogRef.current = posthog;

  // Captures a custom event by name, with properties as required.
  const capture = useCallback(
    <E extends PostHogEvent>(
      event: E,
      ...args: PostHogEventMap[E] extends never
        ? []
        : [properties: PostHogEventMap[E]]
    ) => {
      posthogRef.current?.capture(event, args[0]);
    },
    [],
  );

  return {
    capture,
    posthog, // Expose the original PostHog instance for advanced use cases.
  };
}
