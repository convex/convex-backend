import { cn } from "@ui/cn";
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
import {
  listMinHeight,
  PendingRoleChangeBadge,
  RoleChangesFooter,
} from "elements/ProjectAdminRoleChanges";
import { PaginationControls } from "elements/PaginationControls";
import { ListTable, ListTableHeaderDetail } from "elements/ListTable";
import { useDebounce } from "react-use";
import { useProjectsPageSize } from "hooks/useProjectsPageSize";
import {
  useCursorPagination,
  useSnapBackOnEmptyPage,
} from "hooks/useCursorPagination";

// The pagination bar at the foot of the list: py-2 around an sm button.
const PAGINATION_FOOTER_REM = 3;

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
  // Projects can be toggled on any page, so remember their names for the
  // unsaved changes summary rather than looking them up in the current page.
  const [projectNames, setProjectNames] = useState<Record<number, string>>({});
  const projectDisplayName = (projectId: number) =>
    projectNames[projectId] ?? `Project ${projectId}`;
  const toggleProject = (project: ProjectDetails) => {
    setProjectNames((prev) => ({ ...prev, [project.id]: project.name }));
    setNewProjectRoles((prev) =>
      prev.includes(project.id)
        ? prev.filter((id) => id !== project.id)
        : [...prev, project.id],
    );
  };

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
  const { pageSize } = useProjectsPageSize();

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
  // Search runs on the server and pages can be short, so size the list to
  // the largest page shown so far rather than the current one.
  const [largestPageSize, setLargestPageSize] = useState(0);
  useEffect(() => {
    setLargestPageSize((prev) => Math.max(prev, projects.length));
  }, [projects.length]);

  useSnapBackOnEmptyPage(
    { canGoPrevious, onPreviousPage },
    {
      isLoading: paginatedData?.isLoading ?? true,
      currentPageItems: paginatedData?.items,
    },
  );

  // Reset cursor when debounced search query changes
  useEffect(() => {
    resetPagination();
  }, [debouncedQuery, resetPagination]);

  // Runs as the Modal's onBeforeClose so declining keeps the modal open;
  // the Modal's onClose only fires after it has animated shut.
  const confirmClose = () => {
    if (isSaving) {
      return false;
    }
    const hasChanges = addedProjects.length > 0 || removedProjects.length > 0;
    return (
      !hasChanges ||
      window.confirm(
        "Closing the popup will clear your unsaved changes. Are you sure you want to continue?",
      )
    );
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
      onBeforeClose={confirmClose}
      onClose={onClose}
    >
      <form
        className="flex w-full flex-col gap-4 pt-2"
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
        <TextInput
          placeholder="Search projects"
          value={projectQuery}
          onChange={(e) => setProjectQuery(e.target.value)}
          onKeyDown={(e) => {
            // Enter would otherwise implicitly submit the form and save.
            if (e.key === "Enter") {
              e.preventDefault();
            }
          }}
          type="search"
          id="Search projects in modal"
          isSearchLoading={isLoading && debouncedQuery === projectQuery}
        />

        <ListTable
          className="max-h-[50vh]"
          style={{
            minHeight: listMinHeight(
              largestPageSize,
              largestPageSize > 0 ? PAGINATION_FOOTER_REM : 0,
            ),
          }}
          header={
            <>
              {/* Aligns the label with the project names below the checkboxes. */}
              <span className="size-3.5 shrink-0" aria-hidden="true" />
              Project
              <ListTableHeaderDetail>
                Admin of {newProjectRoles.length} project
                {newProjectRoles.length === 1 ? "" : "s"}
              </ListTableHeaderDetail>
            </>
          }
        >
          <div
            className="scrollbar flex min-h-0 grow flex-col divide-y overflow-y-auto"
            // Allow scrollable zone to be focused
            // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- https://dequeuniversity.com/rules/axe/4.11/scrollable-region-focusable?application=axeAPI
            tabIndex={0}
          >
            {projects.length === 0 && isLoading ? (
              <div className="m-auto py-8">
                <LoadingLogo />
              </div>
            ) : projects.length === 0 ? (
              <div className="m-auto py-8 text-sm text-content-secondary">
                {debouncedQuery.trim()
                  ? "No projects match your search."
                  : "This team doesn't have any projects yet."}
              </div>
            ) : (
              sortBy(projects, (project) =>
                project.name.toLocaleLowerCase(),
              ).map((project) => (
                <ProjectRoleItem
                  key={project.id}
                  project={project}
                  originalProjectRoles={originalProjectRoles}
                  newProjectRoles={newProjectRoles}
                  onToggle={() => toggleProject(project)}
                />
              ))
            )}
          </div>
          {projects.length > 0 && (
            <PaginationControls
              className="border-t px-4 py-2"
              isCursorBasedPagination
              currentPage={currentPageNumber}
              hasMore={hasMore}
              pageSize={pageSize}
              onPageSizeChange={() => {}}
              // Uses the page size chosen on the team's projects list.
              showPageSize={false}
              onPreviousPage={onPreviousPage}
              onNextPage={() => onNextPage(nextCursor)}
              canGoPrevious={canGoPrevious}
            />
          )}
        </ListTable>
        <p className="text-xs text-content-secondary">
          Pro-tip! You can manage the Project Admin role for multiple members at
          the same time on the{" "}
          <Link href="https://docs.convex.dev/dashboard/projects#project-settings">
            Project Settings
          </Link>{" "}
          page.{" "}
        </p>
        <RoleChangesFooter
          added={addedProjects.map(projectDisplayName)}
          removed={removedProjects.map(projectDisplayName)}
          noun="project"
          isSaving={isSaving}
          onCancel={() => {
            if (confirmClose()) {
              onClose();
            }
          }}
        />
      </form>
    </Modal>
  );
}

function ProjectRoleItem({
  project,
  originalProjectRoles,
  newProjectRoles,
  onToggle,
}: {
  project: ProjectDetails;
  originalProjectRoles: number[];
  newProjectRoles: number[];
  onToggle: () => void;
}) {
  const hasAdminPermissions = useHasProjectAdminPermissions(project.id);
  const checkboxId = `memberProjectRole-${project.id}`;
  const row = (
    <div
      className={cn(
        "flex items-center gap-3 px-4 py-3",
        hasAdminPermissions
          ? "transition-colors hover:bg-background-tertiary"
          : "cursor-not-allowed opacity-50",
      )}
    >
      <Checkbox
        id={checkboxId}
        aria-label={project.name}
        checked={newProjectRoles.includes(project.id)}
        // aria-disabled rather than disabled keeps the checkbox focusable, and
        // focusing it opens the row's tooltip explaining why it's disabled.
        aria-disabled={!hasAdminPermissions}
        onChange={onToggle}
        // The row is already dimmed; don't dim the checkbox a second time.
        className="aria-disabled:opacity-100"
      />
      <label
        htmlFor={checkboxId}
        className={cn(
          "flex min-w-0 grow flex-col",
          hasAdminPermissions ? "cursor-pointer" : "cursor-not-allowed",
        )}
      >
        <span className="truncate text-sm text-content-primary">
          {project.name}
        </span>
        {project.slug !== project.name && (
          <span className="truncate text-xs text-content-secondary">
            {project.slug}
          </span>
        )}
      </label>
      <PendingRoleChangeBadge
        isOriginalAdmin={originalProjectRoles.includes(project.id)}
        isNewAdmin={newProjectRoles.includes(project.id)}
      />
    </div>
  );
  if (hasAdminPermissions) {
    return row;
  }
  return (
    <Tooltip
      asChild
      side="top"
      tip={`You do not have permission to manage roles for ${project.name}`}
    >
      {row}
    </Tooltip>
  );
}
