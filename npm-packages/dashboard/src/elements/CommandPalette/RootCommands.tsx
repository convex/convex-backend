import { Command, useCommandState } from "cmdk";
import { useRouter } from "next/router";
import {
  CaretSortIcon,
  ExitIcon,
  Half2Icon,
  PersonIcon,
  QuestionMarkCircledIcon,
  TrashIcon,
} from "@radix-ui/react-icons";
import { SparklesIcon } from "@heroicons/react/24/outline";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";
import { useCurrentTeam } from "api/teams";
import { useCurrentProject } from "api/projects";
import { useSupportFormOpen } from "elements/SupportWidget";
import { openAskAI } from "elements/AskAI";
import { logout } from "lib/logout";
import {
  deploymentNavigation,
  deploymentSectionNavigation,
  findCurrentPage,
  NavigationDestination,
  NavigationTarget,
  profileSectionNavigation,
  projectNavigation,
  projectSectionNavigation,
  REMOTE_VALUE_PREFIX,
  teamNavigation,
  teamSectionNavigation,
} from "./navigation";
import { ActionItem, NavigationItem, ProjectItem } from "./items";
import { ComponentSwitchCommands } from "./ComponentCommands";
import { DeploymentSearchCommands } from "./DeploymentSearchCommands";
import type { DocumentSearchResultItem } from "./DocumentSearchResult";
import { DeploymentSearchGroup, ProjectSearchGroup } from "./searchGroups";
import { NoResultsMessage } from "./NoResultsMessage";
import { PalettePage } from "./pages";
import { usePaletteAnalytics } from "./analytics";

const PROFILE_TARGET: NavigationTarget = {
  label: "Profile Settings",
  href: "/profile",
  Icon: PersonIcon,
};

export function RootCommands({
  search,
  onNavigate,
  onOpenDetail,
  pushPage,
  onClose,
}: {
  search: string;
  onNavigate: (to: NavigationDestination) => void;
  onOpenDetail: (detail: DocumentSearchResultItem) => void;
  pushPage: (page: PalettePage) => void;
  onClose: () => void;
}) {
  const router = useRouter();
  const team = useCurrentTeam();
  const project = useCurrentProject();
  const { commandPaletteDeleteProjects } = useLaunchDarkly();
  const [, setSupportFormOpen] = useSupportFormOpen();
  const { trackSelected } = usePaletteAnalytics();

  const deploymentName =
    typeof router.query.deploymentName === "string"
      ? router.query.deploymentName
      : undefined;

  const deploymentUriPrefix =
    team && project && deploymentName
      ? `/t/${team.slug}/${project.slug}/${deploymentName}`
      : undefined;
  const deploymentNav =
    deploymentUriPrefix && project
      ? deploymentNavigation(deploymentUriPrefix)
      : undefined;
  // Sections within pages are only surfaced while searching, to keep the
  // browsable (empty-search) list scannable.
  const showSections = search.trim().length > 0;
  const projectNav =
    team && project
      ? projectNavigation(team.slug, project.slug, project.name)
      : undefined;
  const teamNav = team ? teamNavigation(team.slug, team.name) : undefined;

  const currentPage = findCurrentPage(
    [
      ...(deploymentNav
        ? [...deploymentNav.pages, ...deploymentNav.settings]
        : []),
      ...(projectNav ?? []),
      ...(teamNav ?? []),
      PROFILE_TARGET,
    ],
    router.asPath,
  );

  return (
    <>
      {currentPage && (
        <Command.Group heading="Current Page">
          <NavigationItem
            target={currentPage}
            onNavigate={onNavigate}
            hint="Current Page"
          />
        </Command.Group>
      )}
      {deploymentNav && project && (
        <Command.Group heading="Deployment">
          {[
            ...deploymentNav.pages,
            ...deploymentNav.settings,
            ...(showSections && deploymentUriPrefix
              ? deploymentSectionNavigation(deploymentUriPrefix)
              : []),
          ]
            // The current page is already pinned at the top.
            .filter((target) => target !== currentPage)
            .map((target) => (
              <NavigationItem
                key={target.label}
                target={target}
                onNavigate={onNavigate}
              />
            ))}
        </Command.Group>
      )}
      <ComponentSwitchCommands
        search={search}
        onOpenComponentsPage={() => pushPage({ type: "components" })}
        onClose={onClose}
      />
      {deploymentNav && (
        <DeploymentSearchCommands
          search={search}
          // Every data-plane result counts as one "data-search" selection kind.
          // Tables and functions navigate directly; documents, files, and
          // scheduled jobs open the detail panel — so track both paths.
          onNavigate={(to) => {
            trackSelected("data-search");
            onNavigate(to);
          }}
          onOpenDetail={(item) => {
            trackSelected("data-search");
            onOpenDetail(item);
          }}
        />
      )}
      {team && project && projectNav && (
        <Command.Group heading="Project">
          {/* Switch commands sit above the page-navigation items. */}
          <ActionItem
            value="page:switch-deployment"
            onSelect={() => pushPage({ type: "deployments", project })}
            Icon={CaretSortIcon}
            label="Switch Deployment…"
            drillIn
          />
          <SwitchProjectItem pushPage={pushPage} />
          {[
            ...projectNav,
            ...(showSections
              ? projectSectionNavigation(team.slug, project.slug)
              : []),
          ]
            .filter((target) => target !== currentPage)
            .map((target) => (
              <NavigationItem
                key={target.label}
                target={target}
                onNavigate={onNavigate}
              />
            ))}
        </Command.Group>
      )}
      {team && teamNav && (
        <Command.Group heading="Team">
          {/* Switch commands sit above the page-navigation items. */}
          <ActionItem
            value="page:teams"
            onSelect={() => pushPage({ type: "teams" })}
            Icon={CaretSortIcon}
            label="Switch Team…"
            drillIn
          />
          {!project && (
            <>
              <SwitchProjectItem pushPage={pushPage} label="Go to Project…" />
              <ActionItem
                value="page:team-deployments"
                onSelect={() => pushPage({ type: "teamDeployments" })}
                Icon={CaretSortIcon}
                label="Go to Deployment…"
                drillIn
              />
            </>
          )}
          {[
            ...teamNav,
            ...(showSections ? teamSectionNavigation(team.slug) : []),
          ]
            .filter((target) => target !== currentPage)
            .map((target) => (
              <NavigationItem
                key={target.label}
                target={target}
                onNavigate={onNavigate}
              />
            ))}
          {commandPaletteDeleteProjects && (
            <ActionItem
              value="page:delete-projects"
              onSelect={() => pushPage({ type: "deleteProjects" })}
              Icon={TrashIcon}
              label="Delete Projects…"
              description="DANGER: No confirmation will be offered after selecting projects to delete"
              destructive
              drillIn
            />
          )}
        </Command.Group>
      )}
      {team && (
        <ProjectSearchGroup
          team={team}
          search={search}
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
      )}
      {team && search.trim() && (
        <DeploymentSearchGroup
          team={team}
          // Inside a project, only search that project's deployments.
          project={project}
          search={search.trim()}
          onNavigate={onNavigate}
          pushPage={pushPage}
        />
      )}
      <Command.Group heading="Account">
        {currentPage !== PROFILE_TARGET && (
          <NavigationItem target={PROFILE_TARGET} onNavigate={onNavigate} />
        )}
        {showSections &&
          profileSectionNavigation().map((target) => (
            <NavigationItem
              key={target.label}
              target={target}
              onNavigate={onNavigate}
            />
          ))}
        <ActionItem
          value="page:theme"
          onSelect={() => pushPage({ type: "theme" })}
          Icon={Half2Icon}
          label="Change Dashboard Theme…"
          drillIn
        />
        <ActionItem
          value="action:log-out"
          onSelect={() => void logout()}
          Icon={ExitIcon}
          label="Log Out"
        />
      </Command.Group>
      <Command.Group heading="Help">
        <ActionItem
          value="action:ask-ai"
          onSelect={() => {
            onClose();
            openAskAI();
          }}
          Icon={SparklesIcon}
          label="Ask AI"
        />
        <ActionItem
          value="action:contact-support"
          onSelect={() => {
            onClose();
            setSupportFormOpen(true);
          }}
          Icon={QuestionMarkCircledIcon}
          label="Contact Support"
        />
      </Command.Group>
    </>
  );
}

function SwitchProjectItem({
  pushPage,
  // "Switch" only makes sense when a project is currently selected.
  label = "Switch Project…",
}: {
  pushPage: (page: PalettePage) => void;
  label?: string;
}) {
  return (
    <ActionItem
      value="page:projects"
      onSelect={() => pushPage({ type: "projects" })}
      Icon={CaretSortIcon}
      label={label}
      drillIn
    />
  );
}

export function AskAIQueryItem({ onClose }: { onClose: () => void }) {
  const search = useCommandState((state) => state.search).trim();
  const visibleCount = useCommandState((state) => state.filtered.count);
  const { trackSelected } = usePaletteAnalytics();
  if (!search) {
    return null;
  }
  return (
    <>
      <Command.Item
        value={`${REMOTE_VALUE_PREFIX}ask-ai-query`}
        className="animate-fadeInFromLoading"
        onSelect={() => {
          trackSelected("ask-ai-query");
          onClose();
          openAskAI(search);
        }}
      >
        <SparklesIcon className="text-content-secondary" />
        <span className="min-w-0 truncate">Ask AI “{search}”</span>
      </Command.Item>
      {visibleCount <= 1 && (
        <div className="flex flex-col items-center justify-center gap-1 py-6 text-sm whitespace-pre-wrap text-content-tertiary">
          <NoResultsMessage onClose={onClose} />
        </div>
      )}
    </>
  );
}
