import { useState } from "react";
import { Button } from "@ui/Button";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { Sheet } from "@ui/Sheet";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { useDeletedProjects, useRecoverProject } from "api/projects";
import { useIsCurrentMemberTeamAdmin } from "api/roles";
import { PaginationControls } from "elements/PaginationControls";
import type { DeletedProject } from "@convex-dev/platform/managementApi";
import type { TeamResponse } from "generatedApi";
import {
  useCursorPagination,
  useSnapBackOnEmptyPage,
} from "hooks/useCursorPagination";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";

const PAGE_SIZE = 10;

export function RecentlyDeletedProjects({ team }: { team: TeamResponse }) {
  const { projectRecovery } = useLaunchDarkly();
  if (!projectRecovery || team.managedBy === "vercel") {
    return null;
  }
  return <DeletedProjectsSheet key={team.id} teamId={team.id} />;
}

function DeletedProjectsSheet({ teamId }: { teamId: number }) {
  const pagination = useCursorPagination();
  const { data, isLoading, hasError } = useDeletedProjects(
    teamId,
    pagination.currentCursor,
    PAGE_SIZE,
  );
  const isTeamAdmin = useIsCurrentMemberTeamAdmin();
  const [projectToRecover, setProjectToRecover] = useState<DeletedProject>();
  const items = data?.items;
  useSnapBackOnEmptyPage(pagination, { isLoading, currentPageItems: items });

  if (!items?.length && !pagination.canGoPrevious && !hasError) {
    return null;
  }

  const hasMore = data?.pagination.hasMore ?? false;
  return (
    <Sheet>
      <h3 className="mb-4">Recently deleted projects</h3>
      <div className="flex max-w-prose flex-col gap-2">
        {items?.length ? (
          <ul className="flex flex-col divide-y rounded-md border">
            {items.map((project) => (
              <li
                key={project.id}
                className="flex items-center justify-between gap-4 px-3 py-2"
              >
                <div className="min-w-0 wrap-break-word">
                  <div className="font-medium">{project.name}</div>
                  <div className="mb-1 font-mono text-xs text-content-secondary">
                    {project.slug}
                  </div>
                  <div className="flex flex-wrap gap-x-3">
                    <TimestampDistance
                      prefix="Deleted"
                      date={new Date(project.deletedAt)}
                    />
                    <TimestampDistance
                      prefix="Expires"
                      date={new Date(project.recoverableUntil)}
                    />
                  </div>
                </div>
                <Button
                  variant="neutral"
                  size="sm"
                  onClick={() => setProjectToRecover(project)}
                  disabled={!isTeamAdmin}
                  tip={
                    isTeamAdmin
                      ? undefined
                      : "Only team admins can recover projects."
                  }
                >
                  Recover
                </Button>
              </li>
            ))}
          </ul>
        ) : (
          <p role="status" className="text-sm text-content-secondary">
            {hasError
              ? "Unable to load deleted projects."
              : "Loading deleted projects…"}
          </p>
        )}
        {(hasMore || pagination.canGoPrevious) && (
          <PaginationControls
            isCursorBasedPagination
            currentPage={pagination.currentPage}
            hasMore={hasMore}
            pageSize={PAGE_SIZE}
            onPageSizeChange={() => {}}
            showPageSize={false}
            onPreviousPage={pagination.onPreviousPage}
            onNextPage={() =>
              pagination.onNextPage(data?.pagination.nextCursor)
            }
            canGoPrevious={pagination.canGoPrevious}
          />
        )}
      </div>
      {projectToRecover && (
        <RecoverProjectDialog
          teamId={teamId}
          project={projectToRecover}
          onClose={() => setProjectToRecover(undefined)}
        />
      )}
    </Sheet>
  );
}

function RecoverProjectDialog({
  teamId,
  project,
  onClose,
}: {
  teamId: number;
  project: DeletedProject;
  onClose: () => void;
}) {
  const recover = useRecoverProject(teamId, project.id);
  return (
    <ConfirmationDialog
      variant="primary"
      confirmText="Recover"
      dialogTitle={`Recover ${project.name}?`}
      dialogBody="This recovers the project and its deployments. Recovery excludes custom domains and preview deployments."
      onConfirm={recover}
      onClose={onClose}
    />
  );
}
