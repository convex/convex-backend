import { createGlobalState } from "react-use";
import type { PlatformDeploymentResponse, ProjectDetails } from "generatedApi";

// Puts the palette in picker mode: choosing a deployment hands it back to the
// control that opened the palette instead of navigating to it (e.g. the
// backups page's "Restore from" menu re-points its list). Set when such a
// control opens the palette and cleared when the palette closes, so a ⌘K open
// is never a picker.
export type DeploymentPicker = {
  onSelect: (deployment: PlatformDeploymentResponse) => void;
  // Why this deployment can't be chosen. When it returns a reason the row is
  // disabled and shows the reason as a tip.
  unavailableReason?: (
    deployment: PlatformDeploymentResponse,
  ) => string | undefined;
  // The deployment the control currently points at, marked in the list.
  selectedDeploymentName?: string;
  // Its project, pinned to the top of the project list so the project you're
  // already pointing at stays one keystroke away however deep the team's
  // project list is.
  selectedProject?: ProjectDetails;
};

export const useCommandPaletteDeploymentPicker =
  createGlobalState<DeploymentPicker | null>(null);
