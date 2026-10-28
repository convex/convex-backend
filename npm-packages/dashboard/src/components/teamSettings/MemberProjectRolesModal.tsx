import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";
import { Checkbox } from "@ui/Checkbox";
import { Modal } from "@ui/Modal";
import { TextInput } from "@ui/TextInput";
import { LoadingLogo } from "@ui/Loading";
import difference from "lodash/difference";
import React, { useState, useEffect } from "react";
import type {
  TeamResponse,
  ProjectMemberRoleResponse,
  ProjectDetails,
  UpdateProjectRolesArgs,
} from "generatedApi";
import type { TeamMember } from "@convex-dev/platform/managementApi";
import { Link } from "@ui/Link";
import { useHasProjectAdminPermissions } from "api/roles";
import { usePaginatedProjects } from "api/projects";
import sortBy from "lodash/sortBy";
import { TeamMemberLink } from "elements/TeamMemberLink";
import { PaginationControls } from "elements/PaginationControls";
import { useDebounce } from "react-use";
import {
  useProjectsPageSize,
  PROJECT_PAGE_SIZES,
} from "hooks/useProjectsPageSize";
import {
  useCursorPagination,
  useSnapBackOnEmptyPage,
} from "hooks/useCursorPagination";
import { ProjectLink } from "./AuditLogItem";

export function MemberProjectRolesModal({
  team,
  member,
  projectRoles,
  onUpdateProjectRoles,
  onClose,
}: {
  team: TeamResponse;
  member: TeamMember;
  projectRoles: ProjectMemberRoleResponse[];
  onUpdateProjectRoles: (body: UpdateProjectRolesArgs) => Promise<undefined>;
  onClose: () => void;
}) {
  const originalProjectRoles = projectRoles.map(
    (projectRole) => projectRole.projectId,
  );
  const [newProjectRoles, setNewProjectRoles] = useState(originalProjectRoles);

  const addedProjects = difference(newProjectRoles, originalProjectRoles);
  const removedProjects = difference(originalProjectRoles, newProjectRoles);

  const [isSaving, setIsSaving] = useState(false);

  // Pagination and search state
  const [projectQuery, setProjectQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const {
    currentCursor,
    currentPage: currentPageNumber,
    canGoPrevious,
    onNextPage,
    onPreviousPage,
    resetPagination,
  } = useCursorPagination();
  const { pageSize, setPageSize } = useProjectsPageSize();

  // Debounce search query (300ms delay)
  useDebounce(
    () => {
      setDebouncedQuery(projectQuery);
    },
    300,
    [projectQuery],
  );

  // Fetch paginated projects with debounced query
  const paginatedData = usePaginatedProjects(
    team.id,
    {
      cursor: currentCursor,
      q: debouncedQuery.trim() || undefined,
    },
    30000,
  );

  const projects = paginatedData?.items ?? [];
  const hasMore = paginatedData?.pagination.hasMore ?? false;
  const nextCursor = paginatedData?.pagination.nextCursor;
  const isLoading = paginatedData === undefined;

  useSnapBackOnEmptyPage(
    { canGoPrevious, onPreviousPage },
    {
      isLoading: paginatedData?.isLoading ?? true,
      currentPageItems: paginatedData?.items,
    },
  );

  const handlePageSizeChange = (newPageSize: number) => {
    setPageSize(newPageSize);
    // Reset to first page when page size changes
    resetPagination();
  };

  // Reset cursor when debounced search query changes
  useEffect(() => {
    resetPagination();
  }, [debouncedQuery, resetPagination]);

  const closeWithConfirmation = () => {
    if (addedProjects.length > 0 || removedProjects.length > 0) {
      const shouldClose = window.confirm(
        "Closing the popup will clear your unsaved changes. Are you sure you want to continue?",
      );
      if (!shouldClose) {
        return;
      }
    }
    onClose();
  };
  return (
    <Modal
      title="Manage Project Roles"
      size="md"
      description={
        <div className="flex flex-col gap-2 text-sm">
          <p>
            Manage Project Admin access for{" "}
            <TeamMemberLink
              memberId={member.id}
              name={member.name || member.email}
            />
            .
          </p>
          <p>
            Project Admins have administrative access to a project, including
            the ability to delete the project and write to production.
          </p>
        </div>
      }
      onClose={closeWithConfirmation}
    >
      <form
        className="flex w-full flex-col gap-2"
        onSubmit={async (e) => {
          e.preventDefault();
          setIsSaving(true);
          try {
            await onUpdateProjectRoles({
              updates: [
                ...addedProjects.map((added) => ({
                  memberId: member.id,
                  projectId: added,
                  role: "admin" as const,
                })),
                ...removedProjects.map((removed) => ({
                  memberId: member.id,
                  projectId: removed,
                })),
              ],
            });
            onClose();
          } finally {
            setIsSaving(false);
          }
        }}
      >
        {/* Search input */}
        <TextInput
          placeholder="Search projects"
          value={projectQuery}
          onChange={(e) => setProjectQuery(e.target.value)}
          type="search"
          id="Search projects in modal"
          isSearchLoading={isLoading && debouncedQuery === projectQuery}
        />

        {/* Loading state */}
        {projects.length === 0 && isLoading && (
          <div className="my-12 flex flex-col items-center gap-2">
            <LoadingLogo />
          </div>
        )}

        {/* Empty search results */}
        {projects.length === 0 && !isLoading && debouncedQuery.trim() && (
          <div className="my-12 flex animate-fadeInFromLoading flex-col items-center gap-2 text-content-secondary">
            No projects match your search.
          </div>
        )}

        {/* Empty state - no projects */}
        {projects.length === 0 && !isLoading && !debouncedQuery.trim() && (
          <div className="my-12 flex flex-col items-center gap-2 text-content-secondary">
            This team doesn't have any projects yet.
          </div>
        )}

        {/* Project list */}
        {projects.length > 0 && (
          <div className="scrollbar max-h-[40vh] overflow-auto">
            {sortBy(projects, (project) =>
              project.name.toLocaleLowerCase(),
            ).map((project) => (
              <ProjectRoleItem
                key={project.id}
                project={project}
                team={team}
                originalProjectRoles={originalProjectRoles}
                newProjectRoles={newProjectRoles}
                setNewProjectRoles={setNewProjectRoles}
              />
            ))}
          </div>
        )}

        {/* Bottom pagination controls with page size */}
        {projects.length > 0 && (
          <PaginationControls
            showPageSize
            isCursorBasedPagination
            currentPage={currentPageNumber}
            hasMore={hasMore}
            pageSize={pageSize}
            onPageSizeChange={handlePageSizeChange}
            onPreviousPage={onPreviousPage}
            onNextPage={() => onNextPage(nextCursor)}
            canGoPrevious={canGoPrevious}
            pageSizeOptions={PROJECT_PAGE_SIZES}
          />
        )}
        <p className="mt-1 text-xs text-content-secondary">
          Pro-tip! You can manage the Project Admin role for multiple members at
          the same time on the{" "}
          <Link href="https://docs.convex.dev/dashboard/projects#project-settings">
            Project Settings
          </Link>{" "}
          page.{" "}
        </p>
        <div className="ml-auto flex items-center gap-2 text-right">
          <div className="text-xs">
            {addedProjects.length > 0 && (
              <div className="text-content-success">
                Add {addedProjects.length} role
                {addedProjects.length > 1 ? "s" : ""}
              </div>
            )}
            {removedProjects.length > 0 && (
              <div className="text-content-error">
                Remove {removedProjects.length} role
                {removedProjects.length > 1 ? "s" : ""}
              </div>
            )}
          </div>

          <Button
            type="submit"
            disabled={
              addedProjects.length === 0 && removedProjects.length === 0
            }
            loading={isSaving}
          >
            Save
          </Button>
        </div>
      </form>
    </Modal>
  );
}

function ProjectRoleItem({
  project,
  team,
  originalProjectRoles,
  newProjectRoles,
  setNewProjectRoles,
}: {
  project: ProjectDetails;
  team: TeamResponse;
  originalProjectRoles: number[];
  newProjectRoles: number[];
  setNewProjectRoles: React.Dispatch<React.SetStateAction<number[]>>;
}) {
  const hasAdminPermissions = useHasProjectAdminPermissions(project.id);
  return (
    <div className="flex h-12 items-center gap-4 border-b px-1 py-2 last:border-b-0">
      <Tooltip
        tip={
          !hasAdminPermissions &&
          `You do not have permission to manage roles for ${project.name}`
        }
        side="left"
      >
        <Checkbox
          checked={newProjectRoles.includes(project.id)}
          disabled={!hasAdminPermissions}
          onChange={() => {
            setNewProjectRoles((prev) =>
              newProjectRoles.includes(project.id)
                ? prev.filter((id) => id !== project.id)
                : [...prev, project.id],
            );
          }}
        />
      </Tooltip>
      <ProjectLink metadata={{}} projectId={project.id} team={team} />
      <div className="ml-auto rounded-sm p-1 text-xs">
        {originalProjectRoles.includes(project.id) &&
          !newProjectRoles.includes(project.id) && (
            <div className="rounded-sm bg-background-error p-1 text-xs text-content-error">
              Role will be removed
            </div>
          )}
        {!originalProjectRoles.includes(project.id) &&
          newProjectRoles.includes(project.id) && (
            <div className="rounded-sm bg-background-success p-1 text-xs text-content-success">
              Role will be added
            </div>
          )}
      </div>
    </div>
  );
}
