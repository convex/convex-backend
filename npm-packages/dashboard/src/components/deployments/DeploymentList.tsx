import { Button } from "@ui/Button";
import { TextInput } from "@ui/TextInput";
import { Combobox, MAX_DISPLAYED_OPTIONS } from "@ui/Combobox";
import { Spinner } from "@ui/Spinner";
import { usePaginatedProjects } from "api/projects";
import { usePaginatedDeployments } from "api/deployments";
import {
  useDeploymentsPageSize,
  DEPLOYMENT_PAGE_SIZES,
} from "hooks/useDeploymentsPageSize";
import { useTeamMembers } from "api/teams";
import { useProfile } from "api/profile";
import { TeamResponse } from "generatedApi";
import { useState, useEffect, useMemo } from "react";
import { useDebounce } from "react-use";
import { cn } from "@ui/cn";
import { LoadingLogo } from "@ui/Loading";
import { PaginationControls } from "elements/PaginationControls";
import {
  useCursorPagination,
  useSnapBackOnEmptyPage,
} from "hooks/useCursorPagination";
import { useRouter } from "next/router";
import { Link } from "@ui/Link";
import sortBy from "lodash/sortBy";
import { DeploymentRow } from "./DeploymentRow";

function PrefixedOption({
  prefix,
  defaultValue = "all",
  label,
  value,
  inButton,
}: {
  prefix: string;
  defaultValue?: string;
  label: string;
  value: string;
  inButton: boolean;
}) {
  if (inButton && value !== defaultValue) {
    return (
      <span>
        <span className="font-semibold">{prefix}</span> {label}
      </span>
    );
  }
  return <span>{label}</span>;
}

const SORT_OPTIONS = [
  { label: "Most Recently Deployed", value: "lastDeployTime:desc" },
  { label: "Least Recently Deployed", value: "lastDeployTime:asc" },
  { label: "Most Recently Created", value: "createTime:desc" },
  { label: "Least Recently Created", value: "createTime:asc" },
  { label: "Reference (A\u2013Z)", value: "reference:asc" },
  { label: "Reference (Z\u2013A)", value: "reference:desc" },
] as const;

const TYPE_FILTER_OPTIONS = [
  { label: "All types", value: "all" },
  { label: "Production", value: "prod" },
  { label: "Development", value: "dev" },
  { label: "Preview", value: "preview" },
  { label: "Custom", value: "custom" },
] as const;

export function useDeploymentsWithFilters(
  teamId: number,
  projectFilter?: number,
) {
  const router = useRouter();
  const { pageSize, setPageSize } = useDeploymentsPageSize();
  const teamMembers = useTeamMembers(teamId);
  const profile = useProfile();

  // Read filter state from query params
  const sort = (router.query.sort as string) || "lastDeployTime:desc";
  const typeFilter = (router.query.type as string) || "all";
  const creatorFilter = (router.query.creator as string) || "all";

  const updateQuery = (updates: Record<string, string | undefined>) => {
    const query = { ...router.query };
    for (const [key, value] of Object.entries(updates)) {
      if (value === undefined) {
        delete query[key];
      } else {
        query[key] = value;
      }
    }
    void router.push({ query }, undefined, { shallow: true });
  };

  const setSort = (value: string) =>
    updateQuery({
      sort: value === "lastDeployTime:desc" ? undefined : value,
    });
  const setTypeFilter = (value: string) =>
    updateQuery({ type: value === "all" ? undefined : value });
  const setCreatorFilter = (value: string) =>
    updateQuery({ creator: value === "all" ? undefined : value });

  const clearFilters = () => {
    setSearchQuery("");
    setDebouncedQuery("");
    updateQuery({
      sort: undefined,
      type: undefined,
      creator: undefined,
      projectId: undefined,
      q: undefined,
    });
  };

  const [searchQuery, setSearchQuery] = useState(
    (router.query.q as string) || "",
  );
  const [debouncedQuery, setDebouncedQuery] = useState(searchQuery);
  const {
    currentCursor,
    currentPage: currentPageNumber,
    canGoPrevious,
    onNextPage,
    onPreviousPage,
    resetPagination,
  } = useCursorPagination();

  useDebounce(
    () => {
      setDebouncedQuery(searchQuery);
      updateQuery({ q: searchQuery.trim() || undefined });
    },
    300,
    [searchQuery],
  );

  // Fetch projects for filter combobox (search-driven)
  const [projectSearchFilter, setProjectSearchFilter] = useState("");
  const [debouncedProjectFilter, setDebouncedProjectFilter] = useState("");

  useDebounce(
    () => {
      setDebouncedProjectFilter(projectSearchFilter);
    },
    300,
    [projectSearchFilter],
  );

  const paginatedProjects = usePaginatedProjects(teamId, {
    q: debouncedProjectFilter,
    limitOverride: MAX_DISPLAYED_OPTIONS,
  });

  const searchedProjects = paginatedProjects
    ? paginatedProjects.items
    : undefined;

  // Fetch paginated deployments
  const paginatedData = usePaginatedDeployments(
    teamId,
    {
      cursor: currentCursor,
      sortBy: sort.split(":")[0],
      sortOrder: sort.split(":")[1],
      deploymentType: typeFilter === "all" ? undefined : typeFilter,
      q: debouncedQuery.trim() || undefined,
      projectId: projectFilter,
      creator: creatorFilter === "all" ? undefined : Number(creatorFilter),
    },
    30000,
  );

  const deployments = paginatedData?.items ?? [];
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
    resetPagination();
  };

  // Reset cursor when filters/search change
  useEffect(() => {
    resetPagination();
  }, [
    debouncedQuery,
    sort,
    typeFilter,
    creatorFilter,
    projectFilter,
    resetPagination,
  ]);

  const memberOptions = useMemo(
    () => [
      { label: "All members", value: "all" },
      ...sortBy(
        (teamMembers ?? []).map((m) => ({
          label: m.name || m.email,
          value: m.id.toString(),
        })),
        [
          (option) =>
            profile && option.value === profile.id.toString() ? 0 : 1,
          (option) => option.label.toLowerCase(),
        ],
      ),
    ],
    [teamMembers, profile],
  );

  const projectOptionsMap = useMemo(() => {
    const map = new Map<number, { name: string; slug: string }>();
    for (const p of searchedProjects ?? []) {
      map.set(p.id, { name: p.name || p.slug, slug: p.slug });
    }
    return map;
  }, [searchedProjects]);

  const projectOptions = useMemo(
    () => [
      { label: "All projects", value: "all" },
      ...sortBy(
        Array.from(projectOptionsMap.entries()).map(([id, p]) => ({
          label: p.name !== p.slug ? `${p.name} (${p.slug})` : p.name,
          value: id.toString(),
        })),
        [(option) => option.label.toLowerCase()],
      ),
    ],
    [projectOptionsMap],
  );

  const ProjectOption = useMemo(
    () =>
      function ProjectOptionInner({
        value,
        inButton,
      }: {
        label: string;
        value: string;
        inButton: boolean;
      }) {
        if (value === "all") {
          return <span>All projects</span>;
        }
        const p = projectOptionsMap.get(Number(value));
        if (!p) return null;
        if (inButton) {
          return (
            <span>
              <span className="font-semibold">Project:</span> {p.name}
            </span>
          );
        }
        return (
          <span>
            {p.name}
            {p.name !== p.slug && (
              <span className="ml-1 text-content-secondary">({p.slug})</span>
            )}
          </span>
        );
      },
    [projectOptionsMap],
  );

  const setProjectFilter = (value: string) =>
    updateQuery({ projectId: value === "all" ? undefined : value });

  return {
    searchQuery,
    setSearchQuery,
    debouncedQuery,
    sort,
    setSort,
    typeFilter,
    setTypeFilter,
    creatorFilter,
    setCreatorFilter,
    clearFilters,
    deployments,
    hasMore,
    isLoading,
    currentPageNumber,
    pageSize,
    canGoPrevious,
    nextCursor,
    onNextPage,
    onPreviousPage,
    handlePageSizeChange,
    memberOptions,
    projectOptions,
    ProjectOption,
    teamMembers,
    projectFilter,
    setProjectFilter,
    setProjectSearchFilter,
    isLoadingProjects:
      !!paginatedProjects?.isLoading &&
      debouncedProjectFilter === projectSearchFilter,
  };
}

export type DeploymentsWithFilters = ReturnType<
  typeof useDeploymentsWithFilters
>;

export function DeploymentToolbar({
  projectFilter,
  filters,
}: {
  projectFilter?: number;
  filters: DeploymentsWithFilters;
}) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <div className="min-w-52 shrink-0">
        <TextInput
          placeholder="Search deployments"
          value={filters.searchQuery}
          onChange={(e) => filters.setSearchQuery(e.target.value)}
          type="search"
          id="Search deployments"
          isSearchLoading={
            filters.isLoading && filters.debouncedQuery === filters.searchQuery
          }
        />
      </div>
      <Combobox
        label="Project"
        labelHidden
        options={filters.projectOptions}
        Option={filters.ProjectOption}
        selectedOption={
          projectFilter !== undefined ? projectFilter.toString() : "all"
        }
        setSelectedOption={(v) => filters.setProjectFilter(v ?? "all")}
        onFilterChange={filters.setProjectSearchFilter}
        isLoadingOptions={filters.isLoadingProjects}
        searchPlaceholder="Search projects..."
        buttonClasses="w-fit"
        innerButtonClasses={
          projectFilter !== undefined
            ? "bg-yellow-100/50 dark:bg-yellow-600/20 hover:bg-yellow-100 dark:hover:bg-yellow-600/50"
            : ""
        }
        optionsWidth="fit"
      />
      <Combobox
        label="Type"
        labelHidden
        Option={(props) => <PrefixedOption prefix="Type:" {...props} />}
        options={[...TYPE_FILTER_OPTIONS]}
        selectedOption={filters.typeFilter}
        setSelectedOption={(v) => filters.setTypeFilter(v === null ? "all" : v)}
        disableSearch
        buttonClasses="w-fit"
        innerButtonClasses={
          filters.typeFilter !== "all"
            ? "bg-yellow-100/50 dark:bg-yellow-600/20 hover:bg-yellow-100 dark:hover:bg-yellow-600/50"
            : ""
        }
      />
      <Combobox
        label="Creator"
        labelHidden
        Option={(props) => <PrefixedOption prefix="Creator:" {...props} />}
        options={filters.memberOptions}
        selectedOption={filters.creatorFilter}
        setSelectedOption={(v) =>
          filters.setCreatorFilter(v === null ? "all" : v)
        }
        buttonClasses="w-fit"
        innerButtonClasses={
          filters.creatorFilter !== "all"
            ? "bg-yellow-100/50 dark:bg-yellow-600/20 hover:bg-yellow-100 dark:hover:bg-yellow-600/50"
            : ""
        }
        optionsWidth="fit"
      />
      <Combobox
        label="Sort"
        labelHidden
        Option={(props) => (
          <PrefixedOption prefix="Sort:" defaultValue="" {...props} />
        )}
        options={[...SORT_OPTIONS]}
        selectedOption={filters.sort}
        setSelectedOption={(v) => {
          if (v) filters.setSort(v);
        }}
        disableSearch
        buttonClasses="w-fit"
        innerButtonClasses={
          filters.sort !== "lastDeployTime:desc"
            ? "bg-yellow-100/50 dark:bg-yellow-600/20 hover:bg-yellow-100 dark:hover:bg-yellow-600/50"
            : ""
        }
      />
      {filters.isLoading && (
        <div>
          <Spinner className="size-4 text-content-secondary" />
        </div>
      )}
    </div>
  );
}

export function DeploymentList({
  team,
  filters,
}: {
  team: TeamResponse;
  filters: DeploymentsWithFilters;
}) {
  const {
    deployments,
    isLoading,
    debouncedQuery,
    pageSize,
    teamMembers,
    hasMore,
    currentPageNumber,
    canGoPrevious,
    nextCursor,
    onNextPage,
    onPreviousPage,
    handlePageSizeChange,
  } = filters;

  const hasActiveFilters =
    debouncedQuery.trim() !== "" ||
    filters.sort !== "lastDeployTime:desc" ||
    filters.typeFilter !== "all" ||
    filters.creatorFilter !== "all" ||
    filters.projectFilter !== undefined;

  return (
    <>
      {deployments.length === 0 && isLoading && (
        <div className="my-24 flex flex-col items-center gap-2">
          <LoadingLogo />
        </div>
      )}
      {deployments.length === 0 && !isLoading && hasActiveFilters && (
        <div className="my-24 flex animate-fadeInFromLoading flex-col items-center gap-2 text-content-secondary">
          <span>No deployments found matching your filters.</span>
          <Button
            variant="neutral"
            size="sm"
            onClick={() => filters.clearFilters()}
          >
            Clear filters
          </Button>
        </div>
      )}
      {deployments.length === 0 && !isLoading && !hasActiveFilters && (
        <div className="my-24 flex animate-fadeInFromLoading flex-col items-center gap-2 text-content-secondary">
          No deployments found.
        </div>
      )}

      {deployments.length > 0 && (
        <div className="w-full">
          <div className="w-full overflow-hidden rounded-xl bg-background-secondary ring-1 ring-border-transparent">
            {deployments.slice(0, pageSize).map((d, i) => (
              <div
                key={`${d.kind}-${d.name}`}
                className={cn(
                  "first:rounded-t-xl last:rounded-b-xl",
                  i > 0 && "border-t",
                )}
              >
                <DeploymentRow
                  deployment={d}
                  teamSlug={team.slug}
                  teamMembers={teamMembers}
                  showProject
                  listItem
                  searchQuery={debouncedQuery}
                />
              </div>
            ))}
          </div>
        </div>
      )}

      {deployments.length > 0 && (
        <div className="mb-4 flex w-full items-start justify-between gap-8">
          <p className="shrink text-xs text-pretty text-content-secondary">
            Looking for a local deployment?{" "}
            <Link href={`/t/${team.slug}`}>Visit your project directly</Link>.
          </p>
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
            pageSizeOptions={DEPLOYMENT_PAGE_SIZES}
          />
        </div>
      )}
    </>
  );
}
