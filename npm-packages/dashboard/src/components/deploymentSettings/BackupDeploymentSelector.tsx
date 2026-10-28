import { CaretSortIcon } from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { useProjectById } from "api/projects";
import { cn } from "@ui/cn";
import { ReactNode, useRef } from "react";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import {
  useCommandPaletteAnchor,
  useCommandPaletteOpen,
  useOpenCommandPalette,
} from "elements/CommandPalette";
import { usePaletteAnalytics } from "elements/CommandPalette/analytics";
import type { PalettePage } from "elements/CommandPalette/pages";
import { FullDeploymentName } from "./BackupListItem";

const PALETTE_SOURCE = "backup-restore-from";

const MENU_WIDTH = 448;

export function BackupDeploymentSelector({
  selectedDeployment,
  onChange,
  targetDeployment,
  children,
}: {
  selectedDeployment: PlatformDeploymentResponse;
  onChange: (newDeployment: PlatformDeploymentResponse) => void;
  targetDeployment: PlatformDeploymentResponse;
  children?: ReactNode;
}) {
  // Restoring zip backups into dedicated deployments isn't supported, so the
  // cross-deployment "Restore from" dropdown is hidden — backups are always
  // scoped to the current deployment.
  const isDedicated =
    targetDeployment.kind === "cloud" && targetDeployment.class.startsWith("d");

  return (
    <div className="flex w-full flex-wrap items-center justify-between gap-2 p-4">
      <h4 className="text-content-primary">Existing Backups</h4>
      <div className="flex flex-wrap items-center justify-end gap-3">
        {children}
        {!isDedicated && (
          <RestoreFromButton
            selectedDeployment={selectedDeployment}
            onChange={onChange}
            targetDeployment={targetDeployment}
          />
        )}
      </div>
    </div>
  );
}

// Opens the command palette as a deployment picker anchored beneath itself,
// and re-points the backup list at whatever the user chooses.
function RestoreFromButton({
  selectedDeployment,
  onChange,
  targetDeployment,
}: {
  selectedDeployment: PlatformDeploymentResponse;
  onChange: (newDeployment: PlatformDeploymentResponse) => void;
  targetDeployment: PlatformDeploymentResponse;
}) {
  const buttonRef = useRef<HTMLButtonElement>(null);
  const openCommandPalette = useOpenCommandPalette();
  const [isPaletteOpen] = useCommandPaletteOpen();
  const [anchor] = useCommandPaletteAnchor();
  const { trackOpened } = usePaletteAnalytics();
  const { project } = useProjectById(selectedDeployment.projectId);

  const isOpen = isPaletteOpen && anchor?.source === PALETTE_SOURCE;
  const isTargetDeployment =
    selectedDeployment.kind === "cloud" &&
    targetDeployment.kind === "cloud" &&
    selectedDeployment.id === targetDeployment.id;

  const openMenu = () => {
    const rect = buttonRef.current?.getBoundingClientRect();
    if (!rect) {
      return;
    }
    trackOpened(PALETTE_SOURCE);
    // Open on the selected deployment's own project, with the project list
    // behind it to step back to. Until that project loads, open on the list.
    const pages: PalettePage[] = [{ type: "pickProject" }];
    if (project) {
      pages.push({ type: "pickDeployment", project });
    }
    openCommandPalette({
      pages,
      anchor: {
        // Right-aligned under the trigger, which sits at the right edge of the
        // backup list (the header switchers hang off their left edge instead).
        left: rect.right - MENU_WIDTH,
        top: rect.bottom + 8,
        source: PALETTE_SOURCE,
        width: MENU_WIDTH,
      },
      picker: {
        onSelect: onChange,
        selectedDeploymentName: selectedDeployment.name,
        selectedProject: project,
        // Cross-region restores are CLI-only, so deployments outside the
        // target's region are listed but disabled.
        unavailableReason: (deployment) =>
          targetDeployment.kind === "cloud" &&
          deployment.kind === "cloud" &&
          deployment.region !== targetDeployment.region
            ? "Use the CLI to restore a backup from a deployment in a different region."
            : undefined,
      },
    });
  };

  return (
    <Button
      variant="unstyled"
      ref={buttonRef}
      aria-haspopup="menu"
      aria-expanded={isOpen}
      onClick={openMenu}
      className={cn(
        "flex items-center gap-1",
        "truncate rounded-sm text-left text-content-primary",
        "border bg-background-secondary px-2 py-1.5 text-sm focus:border-border-selected focus:outline-hidden",
        "cursor-pointer hover:bg-background-tertiary",
        isOpen && "border-border-selected bg-background-tertiary",
      )}
    >
      <span className="font-semibold">Restore from:</span>
      {isTargetDeployment ? (
        "Current Deployment"
      ) : (
        <FullDeploymentName deployment={selectedDeployment} />
      )}
      <CaretSortIcon
        className="ml-auto size-5 text-content-primary"
        aria-hidden="true"
      />
    </Button>
  );
}
