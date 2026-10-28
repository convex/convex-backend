import { usePostHog } from "hooks/usePostHog";

// Product analytics for the command palette. Centralizes the PostHog event
// names so the many call sites (open triggers, item selections) stay in sync.
export function usePaletteAnalytics() {
  const { capture } = usePostHog();
  return {
    trackOpened: (
      via:
        | "hotkey"
        | "slash"
        | "button"
        | "project-selector"
        | "deployment-selector"
        | "backup-restore-from",
    ) => capture("command_palette_opened", { via }),
    trackSelected: (kind: string) =>
      capture("command_palette_item_selected", { kind }),
  };
}
