import {
  ExternalLinkIcon,
  GridIcon,
  ListBulletIcon,
  PlusIcon,
} from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { TextInput } from "@ui/TextInput";
import { useGlobalLocalStorage } from "@common/lib/useGlobalLocalStorage";
import { ProjectCard } from "components/projects/ProjectCard";
import { ProjectTagFilter } from "components/projects/ProjectTagFilter";
import {
  DeploymentList,
  DeploymentToolbar,
  useDeploymentsWithFilters,
} from "components/deployments/DeploymentList";
import { usePaginatedProjects } from "api/projects";
import {
  useProjectsPageSize,
  PROJECT_PAGE_SIZES,
} from "hooks/useProjectsPageSize";
import { useCurrentTeam } from "api/teams";
import { ProjectDetails, TeamResponse } from "generatedApi";
import { useCreateProjectModalRequest } from "hooks/useCreateProjectModal";
import { useProjectTagFilter } from "hooks/useProjectTagFilter";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";
import {
  useCursorPagination,
  useSnapBackOnEmptyPage,
} from "hooks/useCursorPagination";
import { useHasCustomRolePermission } from "api/roles";
import { PROPOSED_PROJECT_RESOURCE } from "lib/permissions";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { TEAM_RESOURCE } from "lib/permissions";
import { withAuthenticatedPage } from "lib/withAuthenticatedPage";
import Head from "next/head";
import { useState, useEffect } from "react";
import { useDebounce } from "react-use";
import { cn } from "@ui/cn";
import { SegmentedControl } from "@ui/SegmentedControl";
import { EmptySection } from "@common/elements/EmptySection";
import { OpenInVercel } from "components/OpenInVercel";
import { LoadingLogo } from "@ui/Loading";
import { PaginationControls } from "elements/PaginationControls";
import { useRouter } from "next/router";

export { getServerSideProps } from "lib/ssr";

export function TeamIndexPage() {
  const team = useCurrentTeam();
  const router = useRouter();

  const viewFromQuery = (router.query.view as string | undefined) ?? "projects";
  const currentView = viewFromQuery;
  const isDeploymentsView = currentView === "deployments";
  const projectFilter = router.query.projectId
    ? Number(router.query.projectId)
    : undefined;

  const handleViewChange = (view: string) => {
    const query: Record<string, string> = {};
    if (view !== "projects") {
      query.view = view;
    }
    void router.replace({ pathname: `/t/${team?.slug}`, query }, undefined, {
      shallow: true,
    });
  };

  return (
    <>
      <Head>{team && <title>{team.name} | Convex Dashboard</title>}</Head>
      <div className="h-full grow bg-background-primary p-4">
        <div className="m-auto max-w-3xl transition-all lg:max-w-5xl xl:max-w-7xl">
          <div className="flex w-full flex-col gap-2">
            {team && (
              <div className="w-full">
                <TeamContent
                  team={team}
                  isDeploymentsView={isDeploymentsView}
                  currentView={currentView}
                  onViewChange={handleViewChange}
                  projectFilter={projectFilter}
                />
              </div>
            )}
          </div>
        </div>
      </div>
    </>
  );
}

export default withAuthenticatedPage(TeamIndexPage);

const VIEW_OPTIONS = [
  { label: "Projects", value: "projects" },
  { label: "Deployments", value: "deployments" },
] as const;

function TeamContent({
  team,
  isDeploymentsView,
  currentView,
  onViewChange,
  projectFilter,
}: {
  team: TeamResponse;
  isDeploymentsView: boolean;
  currentView: string;
  onViewChange: (view: string) => void;
  projectFilter?: number;
}) {
  const [projectQuery, setProjectQuery] = useState("");
  const [debouncedProjectQuery, setDebouncedProjectQuery] = useState("");
  const [showAsList, setShowAsList] = useGlobalLocalStorage(
    "showProjectsAsList",
    false,
  );

  useDebounce(
    () => {
      setDebouncedProjectQuery(projectQuery);
    },
    300,
    [projectQuery],
  );

  const { tagEditUi } = useLaunchDarkly();
  const canListTags = useHasCustomRolePermission(
    team.id,
    "team:projectTags:view",
    TEAM_RESOURCE,
    true,
  );
  const [tagsFromUrl, setSelectedTags] = useProjectTagFilter();
  // A `?tag=` in the URL filters the list only once the filter is shown and
  // the member is known to be allowed to list tags, so neither a hidden
  // filter nor a still-loading permission check applies one.
  const selectedTags = tagEditUi && canListTags === true ? tagsFromUrl : [];

  const clearFilters = () => {
    setProjectQuery("");
    setDebouncedProjectQuery("");
    setSelectedTags([]);
  };

  return (
    <>
      <div className="mb-4 flex w-full animate-fadeInFromLoading flex-col gap-3">
        <div className="flex items-center gap-4">
          <SegmentedControl
            options={[...VIEW_OPTIONS]}
            value={currentView}
            onChange={onViewChange}
          />
          {!isDeploymentsView && <ProjectActions team={team} />}
        </div>
        {!isDeploymentsView && (
          <div className="mt-1 flex items-center gap-2">
            <div className="min-w-52 shrink-0">
              <TextInput
                placeholder="Search projects"
                value={projectQuery}
                onChange={(e) => setProjectQuery(e.target.value)}
                type="search"
                id="Search projects"
                isSearchLoading={debouncedProjectQuery !== projectQuery}
              />
            </div>
            {tagEditUi && (
              <ProjectTagFilter
                teamId={team.id}
                canListTags={canListTags}
                selectedTag={selectedTags[0]}
                onChangeTag={(tag) => setSelectedTags(tag ? [tag] : [])}
              />
            )}
            <div className="hidden gap-1 rounded-md border bg-background-secondary p-1 lg:flex">
              <Button
                icon={<GridIcon />}
                variant="neutral"
                inline
                size="xs"
                className={cn(!showAsList && "bg-background-tertiary")}
                onClick={() => setShowAsList(false)}
              />
              <Button
                icon={<ListBulletIcon />}
                variant="neutral"
                inline
                size="xs"
                className={cn(showAsList && "bg-background-tertiary")}
                onClick={() => setShowAsList(true)}
              />
            </div>
          </div>
        )}
      </div>
      {isDeploymentsView ? (
        <DeploymentsView team={team} projectFilter={projectFilter} />
      ) : (
        <ProjectGrid
          team={team}
          debouncedProjectQuery={debouncedProjectQuery}
          selectedTags={selectedTags}
          onClearFilters={clearFilters}
          showAsList={showAsList}
        />
      )}
    </>
  );
}

function DeploymentsView({
  team,
  projectFilter,
}: {
  team: TeamResponse;
  projectFilter?: number;
}) {
  const filters = useDeploymentsWithFilters(team.id, projectFilter);
  return (
    <div className="flex flex-col gap-4">
      <DeploymentToolbar projectFilter={projectFilter} filters={filters} />
      <DeploymentList team={team} filters={filters} />
    </div>
  );
}

function ProjectActions({ team }: { team: TeamResponse }) {
  const [, requestCreateProject] = useCreateProjectModalRequest();
  // Built-in admin/developer members can always create projects; custom-role
  // members need an explicit `project:create` grant.
  const canCreateCustom = useHasCustomRolePermission(
    team.id,
    "project:create",
    PROPOSED_PROJECT_RESOURCE,
    true,
  );
  const canCreate = canCreateCustom !== false;
  return (
    <div className="ml-auto flex items-center gap-2">
      {!team.managedBy && (
        <Button
          onClick={() => requestCreateProject({ team })}
          variant="neutral"
          size="sm"
          icon={<PlusIcon />}
          disabled={!canCreate}
          tip={
            !canCreate
              ? permissionDeniedTip(
                  "You do not have permission to create projects in this team.",
                  "project:create",
                )
              : undefined
          }
        >
          Create Project
        </Button>
      )}
      <OpenInVercel team={team} />
      <Button
        href="https://docs.convex.dev/tutorial"
        size="sm"
        target="_blank"
        icon={<ExternalLinkIcon />}
      >
        Start Tutorial
      </Button>
    </div>
  );
}

function ProjectGrid({
  team,
  debouncedProjectQuery,
  selectedTags,
  onClearFilters,
  showAsList,
}: {
  team: TeamResponse;
  debouncedProjectQuery: string;
  selectedTags: string[];
  onClearFilters: () => void;
  showAsList: boolean;
}) {
  const { pageSize, setPageSize } = useProjectsPageSize();

  const debouncedQuery = debouncedProjectQuery;
  const {
    currentCursor,
    currentPage: currentPageNumber,
    canGoPrevious,
    onNextPage,
    onPreviousPage,
    resetPagination,
  } = useCursorPagination();

  // Fetch paginated projects with debounced query
  const paginatedData = usePaginatedProjects(
    team?.id,
    {
      cursor: currentCursor,
      q: debouncedQuery.trim() || undefined,
      tags: selectedTags,
    },
    30000,
  );
  const hasFilters = !!debouncedQuery.trim() || selectedTags.length > 0;

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

  // A cursor only means something under the filters that produced it.
  const tagsKey = selectedTags.join(",");
  useEffect(() => {
    resetPagination();
  }, [debouncedQuery, tagsKey, resetPagination]);

  return (
    <div className="flex flex-col items-center">
      {projects.length === 0 && isLoading && (
        <div className="my-24 flex flex-col items-center gap-2">
          <LoadingLogo />
        </div>
      )}
      {projects.length === 0 && !isLoading && hasFilters && (
        <div className="my-24 flex animate-fadeInFromLoading flex-col items-center gap-2 text-content-secondary">
          There are no projects matching your filters.
          <Button variant="neutral" size="sm" onClick={onClearFilters}>
            Clear filters
          </Button>
        </div>
      )}
      {projects.length === 0 && !isLoading && !hasFilters && (
        <EmptySection
          header="Welcome to Convex!"
          sheet={false}
          body={
            <>
              <p className="text-sm">
                This team doesn't have any projects yet.{" "}
              </p>
              <p className="text-sm">Get started by following the tutorial.</p>
            </>
          }
          action={
            <Button
              href="https://docs.convex.dev/tutorial"
              target="_blank"
              icon={<ExternalLinkIcon />}
              className="mt-2"
            >
              Start Tutorial
            </Button>
          }
        />
      )}
      {showAsList ? (
        projects.length > 0 && (
          <div className="w-full overflow-hidden rounded-xl bg-background-secondary ring-1 ring-border-transparent">
            {projects.slice(0, pageSize).map((p: ProjectDetails, i: number) => (
              <div
                key={p.id}
                className={cn(
                  "first:rounded-t-xl last:rounded-b-xl",
                  i > 0 && "border-t",
                )}
              >
                <ProjectCard
                  project={p}
                  listItem
                  searchQuery={debouncedQuery}
                />
              </div>
            ))}
          </div>
        )
      ) : (
        <div className="grid w-full grow grid-cols-1 gap-4 lg:grid-cols-2 xl:grid-cols-3">
          {projects.slice(0, pageSize).map((p: ProjectDetails) => (
            <ProjectCard key={p.id} project={p} searchQuery={debouncedQuery} />
          ))}
        </div>
      )}

      {/* Bottom pagination controls */}
      {projects.length > 0 && (
        <div className="my-4 flex w-full justify-end">
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
        </div>
      )}
    </div>
  );
}
