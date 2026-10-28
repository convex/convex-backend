import { Command } from "cmdk";
import React, {
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useRouter } from "next/router";
import { Checkbox } from "@ui/Checkbox";
import { KEYCAP_CLASSES, KeyboardShortcut } from "@ui/KeyboardShortcut";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import {
  useCurrentProject,
  useDeleteProjects,
  useInfiniteProjects,
} from "api/projects";
import { useCurrentTeam } from "api/teams";
import type { ProjectDetails } from "generatedApi";
import {
  HighlightedText,
  LoadingSignal,
  PaletteConfirmContext,
  PaletteStatusContext,
} from "./items";
import { useCopyAction } from "./copy";
import { InfiniteScrollSentinel } from "./InfiniteScrollSentinel";
import { REMOTE_VALUE_PREFIX } from "./navigation";

const MAX_SELECTED = 100;

// The drilled-into "Delete Projects" page: a searchable, multi-select list of
// the team's projects with a destructive action that deletes every selected
// project at once.
export function DeleteProjectsCommands({
  search,
  onClose,
}: {
  search: string;
  onClose: () => void;
}) {
  const router = useRouter();
  const team = useCurrentTeam();
  const currentProject = useCurrentProject();

  const {
    projects,
    isLoading,
    isLoadingMore,
    hasMore,
    loadMore,
    debouncedQuery,
  } = useInfiniteProjects(team?.id ?? 0, search, false);
  const deleteProjects = useDeleteProjects(team?.id);

  const [selectedIds, setSelectedIds] = useState<number[]>([]);
  // Anchor for Shift+click range selection.
  const [lastSelectedId, setLastSelectedId] = useState<number | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);
  // Set by an item's onMouseDown just before cmdk fires its onSelect, so a
  // click can extend the selection as a range instead of toggling one project.
  const shiftHeld = useRef(false);

  const shown = useMemo(() => projects ?? [], [projects]);
  const atLimit = selectedIds.length >= MAX_SELECTED;

  // Show the selection count and the (deliberately awkward) confirm shortcut in
  // the footer gutter — deletion happens via the chord, not a list item, so it
  // can't be triggered by an accidental Enter on a project row.
  const setFooterStatus = useContext(PaletteStatusContext);
  const registerConfirm = useContext(PaletteConfirmContext);
  useEffect(() => {
    setFooterStatus?.(
      selectedIds.length > 0 ? (
        <span className="flex items-center gap-3">
          <span>{`${selectedIds.length} of ${MAX_SELECTED} selected`}</span>
          <span className="flex items-center gap-1 text-content-error">
            <KeyboardShortcut
              value={["CtrlOrCmd", "Shift", "Return"]}
              className={KEYCAP_CLASSES}
            />
            Delete
          </span>
        </span>
      ) : null,
    );
    return () => setFooterStatus?.(null);
  }, [setFooterStatus, selectedIds.length]);

  const toggleProject = useCallback(
    (index: number) => {
      const project = shown[index];
      if (!project) {
        return;
      }
      const anchorIndex =
        lastSelectedId !== null
          ? shown.findIndex((p) => p.id === lastSelectedId)
          : -1;
      const rangeSelect = shiftHeld.current && anchorIndex !== -1;
      shiftHeld.current = false;
      setSelectedIds((current) => {
        if (rangeSelect) {
          // Fill (or clear) the whole span between the anchor and this row,
          // matching whether this row is being turned on or off.
          const start = Math.min(anchorIndex, index);
          const end = Math.max(anchorIndex, index);
          const turningOn = !current.includes(project.id);
          const next = new Set(current);
          for (let i = start; i <= end; i++) {
            const id = shown[i]?.id;
            if (id === undefined) {
              continue;
            }
            if (turningOn) {
              // Stop filling the range once the cap is reached.
              if (next.size >= MAX_SELECTED) {
                break;
              }
              next.add(id);
            } else {
              next.delete(id);
            }
          }
          return Array.from(next);
        }
        if (current.includes(project.id)) {
          return current.filter((id) => id !== project.id);
        }
        if (current.length >= MAX_SELECTED) {
          return current;
        }
        return [...current, project.id];
      });
      setLastSelectedId(project.id);
    },
    [shown, lastSelectedId],
  );

  const handleDelete = useCallback(async () => {
    if (selectedIds.length === 0 || isSubmitting) {
      return;
    }
    setIsSubmitting(true);
    try {
      // Deleting the project you're currently viewing would leave you on a
      // dead page, so step back up to the team first.
      if (currentProject && selectedIds.includes(currentProject.id)) {
        await router.push(`/t/${team?.slug}`);
      }
      await deleteProjects({ projectIds: selectedIds });
      onClose();
    } finally {
      setIsSubmitting(false);
    }
  }, [
    selectedIds,
    isSubmitting,
    currentProject,
    router,
    team?.slug,
    deleteProjects,
    onClose,
  ]);

  // Deletion is only reachable through the confirm chord (Cmd/Ctrl+Shift+Enter),
  // registered while there's a selection to delete.
  useEffect(() => {
    const canDelete = selectedIds.length > 0 && !isSubmitting;
    registerConfirm?.(canDelete ? handleDelete : null);
    return () => registerConfirm?.(null);
  }, [registerConfirm, handleDelete, selectedIds.length, isSubmitting]);

  const stale = isLoading || debouncedQuery.trim() !== search.trim();

  if (!team) {
    return <LoadingSignal />;
  }

  return (
    <Command.Group heading="Select projects to delete">
      {stale ? (
        <LoadingSignal />
      ) : (
        shown.map((project, index) => (
          <DeleteProjectItem
            key={project.id}
            project={project}
            index={index}
            selected={selectedIds.includes(project.id)}
            // At the cap, only already-selected rows stay interactive so the
            // selection can still be trimmed but not grown.
            disabled={atLimit && !selectedIds.includes(project.id)}
            onToggle={toggleProject}
            shiftHeld={shiftHeld}
          />
        ))
      )}
      {!stale && (
        <InfiniteScrollSentinel
          hasMore={hasMore}
          isLoadingMore={!!isLoadingMore}
          loadMore={loadMore}
        />
      )}
    </Command.Group>
  );
}

function DeleteProjectItem({
  project,
  index,
  selected,
  disabled,
  onToggle,
  shiftHeld,
}: {
  project: ProjectDetails;
  index: number;
  selected: boolean;
  disabled: boolean;
  onToggle: (index: number) => void;
  shiftHeld: React.MutableRefObject<boolean>;
}) {
  const value = `${REMOTE_VALUE_PREFIX}delete-project:${project.id}`;
  useCopyAction(value, { label: "slug", getText: () => project.slug });
  return (
    <Command.Item
      value={value}
      className="animate-fadeInFromLoading"
      disabled={disabled}
      onMouseDown={(event) => {
        shiftHeld.current = event.shiftKey;
      }}
      onSelect={() => onToggle(index)}
    >
      {/* The row itself toggles selection; the checkbox is a visual indicator. */}
      <Checkbox
        checked={selected}
        onChange={() => {}}
        disabled={disabled}
        className="pointer-events-none"
      />
      <span className="flex min-w-0 flex-col">
        <span className="flex min-w-0 items-baseline gap-1.5">
          <span className="truncate">
            <HighlightedText text={project.name || project.slug} />
          </span>
          <span className="truncate text-xs text-content-tertiary">
            <HighlightedText text={project.slug} />
          </span>
        </span>
      </span>
      <TimestampDistance
        className="ml-auto shrink-0"
        date={new Date(project.createTime)}
        prefix="Created"
      />
    </Command.Item>
  );
}
