import { Command } from "cmdk";
import { PlusIcon } from "@radix-ui/react-icons";
import { useCurrentTeam } from "api/teams";
import { useInfiniteProjects } from "api/projects";
import { useInfiniteDeployments } from "api/deployments";
import { useHasCustomRolePermission } from "api/roles";
import { useCreateProjectModalRequest } from "hooks/useCreateProjectModal";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import type { ProjectDetails, TeamResponse } from "generatedApi";
import { NavigationDestination } from "./navigation";
import {
  ActionItem,
  DeploymentItem,
  LoadingSignal,
  PinnedActions,
  ProjectItem,
} from "./items";
import { InfiniteScrollSentinel } from "./InfiniteScrollSentinel";
import { PalettePage } from "./pages";
import { usePaletteAnalytics } from "./analytics";

// The drilled-into "Switch Project" page: the full, searchable project list.
export function SwitchProjectCommands({
  search,
  onNavigate,
  pushPage,
  onClose,
}: {
  search: string;
  onNavigate: (to: NavigationDestination) => void;
  pushPage: (page: PalettePage) => void;
  onClose: () => void;
}) {
  const team = useCurrentTeam();
  const { trackSelected } = usePaletteAnalytics();
  const [, requestCreateProject] = useCreateProjectModalRequest();
  const canCreateCustom = useHasCustomRolePermission(
    team?.id,
    "project:create",
    { segments: [{ kind: "project", id: 0, slug: "" }] },
    true,
  );

  const isVercelManaged = team?.managedBy === "vercel";
  const canCreateProject =
    !!team && !isVercelManaged && canCreateCustom !== false;
  const createProjectTip = isVercelManaged
    ? "This team is managed by Vercel. You can create new projects through the Vercel dashboard."
    : canCreateCustom === false
      ? permissionDeniedTip(
          "You do not have permission to create projects in this team.",
          "project:create",
        )
      : undefined;

  return (
    <>
      {team ? (
        <ProjectSearchGroup
          team={team}
          search={search}
          full
          renderItem={(candidate) => (
            <ProjectItem
              key={candidate.id}
              project={candidate}
              teamSlug={team.slug}
              onNavigate={onNavigate}
              onDrill={() => pushPage({ type: "project", project: candidate })}
            />
          )}
        />
      ) : (
        <LoadingSignal />
      )}
      <PinnedActions>
        <ActionItem
          value="action:create-project"
          onSelect={() => {
            if (!team) {
              return;
            }
            trackSelected("create-project");
            onClose();
            requestCreateProject({ team });
          }}
          Icon={PlusIcon}
          label="Create Project…"
          disabled={!canCreateProject}
          tip={createProjectTip}
        />
      </PinnedActions>
    </>
  );
}

export function ProjectSearchGroup({
  team,
  search,
  full = false,
  pinnedProject,
  renderItem,
}: {
  team: TeamResponse;
  search: string;
  // Show the whole (paginated) list rather than a root-page teaser.
  full?: boolean;
  // A project to list first, ahead of the fetched page. Only while the list is
  // unfiltered: once the user searches, results stand on their own relevance.
  pinnedProject?: ProjectDetails;
  // How each result renders: a row that navigates into the project in the
  // palette proper, a pickable row in a picker menu.
  renderItem: (project: ProjectDetails) => React.ReactNode;
}) {
  const {
    projects,
    isLoading,
    isLoadingMore,
    hasMore,
    loadMore,
    debouncedQuery,
  } = useInfiniteProjects(team.id, search, false);
  const trimmed = search.trim();
  const stale = isLoading || debouncedQuery.trim() !== trimmed;

  // With no search, show a short list so the root page stays scannable;
  // server-side search takes over as soon as the user types.
  const page = full || trimmed ? projects : projects?.slice(0, 5);
  const pinned = trimmed ? undefined : pinnedProject;
  // cmdk keys rows by value, so the pinned project has to leave the list it's
  // hoisted out of.
  const shown = pinned
    ? [pinned, ...(page ?? []).filter((p) => p.id !== pinned.id)]
    : page;

  if (stale) {
    return (
      <Command.Group heading={`${team.name || team.slug} · Projects`}>
        <LoadingSignal />
      </Command.Group>
    );
  }

  return (
    <Command.Group heading={`${team.name || team.slug} · Projects`}>
      {shown?.map(renderItem)}
      {(full || trimmed) && (
        <InfiniteScrollSentinel
          hasMore={hasMore}
          isLoadingMore={!!isLoadingMore}
          loadMore={loadMore}
        />
      )}
    </Command.Group>
  );
}

// The drilled-into "Go to Deployment" page: every cloud deployment in the
// current team, across projects, searchable server-side.
export function TeamDeploymentsCommands({
  search,
  onNavigate,
  pushPage,
}: {
  search: string;
  onNavigate: (to: NavigationDestination) => void;
  pushPage: (page: PalettePage) => void;
}) {
  const team = useCurrentTeam();

  if (!team) {
    return <LoadingSignal />;
  }

  return (
    <DeploymentSearchGroup
      team={team}
      project={undefined}
      search={search}
      onNavigate={onNavigate}
      pushPage={pushPage}
    />
  );
}

export function DeploymentSearchGroup({
  team,
  project,
  search,
  onNavigate,
  pushPage,
}: {
  team: TeamResponse;
  // When set, the search is scoped to this project's deployments.
  project: ProjectDetails | undefined;
  search: string;
  onNavigate: (to: NavigationDestination) => void;
  pushPage: (page: PalettePage) => void;
}) {
  const q = search.trim();
  const {
    deployments,
    isLoading,
    isLoadingMore,
    hasMore,
    loadMore,
    debouncedQuery,
  } = useInfiniteDeployments(team.id, q, {
    projectId: project?.id,
    // These rows bypass cmdk's filter (paletteFilter always keeps remote
    // items), so clear to a loading row rather than keeping the prior query's
    // deployments visible while a new query loads.
    keepPreviousData: false,
  });
  const cloudDeployments = deployments.filter((d) => d.kind === "cloud");
  const stale = isLoading || debouncedQuery.trim() !== q;

  // This group loads alongside the project one, which already shows placeholder
  // rows under its own heading. Report the load but stay out of the list so the
  // wait reads as one block rather than two stacked headings.
  if (stale) {
    return <LoadingSignal rows={0} />;
  }

  return (
    <Command.Group
      heading={`${(project ? project.name || project.slug : undefined) ?? team.name ?? team.slug} · Deployments`}
    >
      {cloudDeployments.map((deployment) => (
        <DeploymentItem
          key={deployment.name}
          deployment={deployment}
          teamSlug={team.slug}
          projectSlug={project?.slug}
          // If we're not filtered to a project, render the project name in the item.
          showProject={project === undefined}
          remote
          onNavigate={onNavigate}
          onDrill={() =>
            pushPage({
              type: "deployment",
              deployment,
              projectSlug: project?.slug,
            })
          }
        />
      ))}
      <InfiniteScrollSentinel
        hasMore={hasMore}
        isLoadingMore={!!isLoadingMore}
        loadMore={loadMore}
      />
    </Command.Group>
  );
}
