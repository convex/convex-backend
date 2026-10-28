import { ProjectDetails, TeamResponse } from "generatedApi";

import classNames from "classnames";
import React from "react";
import { Button } from "@ui/Button";
import { CaretSortIcon } from "@radix-ui/react-icons";
import { Avatar } from "elements/Avatar";
import { cn } from "@ui/cn";
import { useWindowSize } from "react-use";
import {
  useCommandPaletteAnchor,
  useCommandPaletteOpen,
  useOpenCommandPalette,
} from "elements/CommandPalette";
import type { PalettePage } from "elements/CommandPalette/pages";
import { usePaletteAnalytics } from "elements/CommandPalette/analytics";

export function ProjectSelector({
  teams,
  selectedTeamSlug,
  selectedProject,
}: {
  teams?: TeamResponse[];
  selectedTeamSlug?: string;
  selectedProject?: ProjectDetails;
}) {
  const team = teams?.find((t) => t.slug === selectedTeamSlug) ?? null;

  const { width } = useWindowSize();
  const openCommandPalette = useOpenCommandPalette();
  const [isPaletteOpen] = useCommandPaletteOpen();
  const [paletteAnchor] = useCommandPaletteAnchor();
  const { trackOpened } = usePaletteAnalytics();
  const teamActive = isPaletteOpen && paletteAnchor?.source === "team-switcher";
  const projectActive =
    isPaletteOpen && paletteAnchor?.source === "project-switcher";

  const triggerClassName = classNames(
    "flex items-center h-full ml-1",
    "w-fit select-none",
    "text-content-primary group",
    "cursor-pointer",
    "outline-none",
  );

  const openAnchored = (
    event: React.MouseEvent,
    page: PalettePage,
    source: string,
  ) => {
    trackOpened("project-selector");
    const rect = event.currentTarget.getBoundingClientRect();
    openCommandPalette({
      pages: [page],
      anchor: { left: rect.left, top: rect.bottom + 8, source },
    });
  };

  const segmentClassName = cn(
    "flex h-full items-center rounded-full outline-none",
    "hover:bg-background-tertiary",
    "focus-visible:ring-2 focus-visible:ring-border-selected focus-visible:ring-inset",
  );
  const nameStyle = {
    maxWidth: width > 1024 ? "14rem" : width > 640 ? "10rem" : "6rem",
  };

  return (
    <div className={triggerClassName}>
      <div className="flex h-10 items-center rounded-full bg-(--project-selector-bg)">
        {team && selectedProject ? (
          <>
            <Button
              aria-label="Switch team"
              variant="unstyled"
              type="button"
              className={cn(
                segmentClassName,
                "px-2",
                teamActive && "bg-background-tertiary",
              )}
              onClick={(event) =>
                openAnchored(event, { type: "teams" }, "team-switcher")
              }
            >
              <Avatar name={team.name} hashKey={team.id.toString()} />
            </Button>
            <span className="text-content-secondary" role="separator">
              /
            </span>
            <Button
              aria-label="Switch project"
              variant="unstyled"
              type="button"
              className={cn(
                segmentClassName,
                "gap-2 px-2",
                projectActive && "bg-background-tertiary",
              )}
              onClick={(event) =>
                openAnchored(event, { type: "projects" }, "project-switcher")
              }
            >
              <div className="truncate font-semibold" style={nameStyle}>
                {selectedProject.name}
              </div>
              <CaretSortIcon className="size-5" />
            </Button>
          </>
        ) : (
          <Button
            aria-label="Switch team"
            variant="unstyled"
            type="button"
            className={cn(
              segmentClassName,
              "gap-2 px-3",
              teamActive && "bg-background-tertiary",
            )}
            onClick={(event) =>
              openAnchored(event, { type: "teams" }, "team-switcher")
            }
          >
            <Avatar name={team?.name} hashKey={team?.id.toString() ?? ""} />
            <span className="grow truncate" style={nameStyle}>
              {team?.name}
            </span>
            <CaretSortIcon className="size-5" />
          </Button>
        )}
      </div>
    </div>
  );
}
