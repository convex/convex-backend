import { Command } from "cmdk";
import React, { useMemo } from "react";
import { GearIcon, PlusIcon } from "@radix-ui/react-icons";
import { cn } from "@ui/cn";
import {
  deploymentTypeColorClasses,
  deploymentTypeLabel,
} from "@common/lib/deploymentTypeColorClasses";
import {
  PROVISION_DEV_PAGE_NAME,
  PROVISION_PROD_PAGE_NAME,
} from "@common/lib/deploymentContext";
import { useCurrentTeam } from "api/teams";
import { useProfile } from "api/profile";
import { useDeployments } from "api/deployments";
import type {
  PlatformDeploymentResponse,
  ProjectDetails,
  TeamResponse,
} from "generatedApi";
import { projectNavigation, projectSectionNavigation } from "./navigation";
import {
  DeploymentItem,
  DeploymentTypeIcon,
  LoadingSignal,
  NavigationItem,
} from "./items";
import { compareSwitcherDeployments } from "./deploymentOrder";
import { usePaletteAnalytics } from "./analytics";

// Commands for a drilled-into project: its pages plus all of its (cloud)
// deployments, which drill further into deployment pages.
export function ProjectCommands({
  project,
  onNavigate,
  onSelectDeployment,
}: {
  project: ProjectDetails;
  onNavigate: (href: string) => void;
  onSelectDeployment: (deployment: PlatformDeploymentResponse) => void;
}) {
  const team = useCurrentTeam();

  if (!team) {
    return <LoadingSignal />;
  }

  return (
    <>
      <Command.Group heading="Project">
        {[
          ...projectNavigation(team.slug, project.slug, project.name),
          ...projectSectionNavigation(team.slug, project.slug),
        ].map((target) => (
          <NavigationItem
            key={target.label}
            target={target}
            onNavigate={onNavigate}
          />
        ))}
      </Command.Group>
      <DeploymentsGroup
        team={team}
        project={project}
        onNavigate={onNavigate}
        onSelectDeployment={onSelectDeployment}
      />
    </>
  );
}

// The drilled-into "Switch Deployment" page: the project's deployments to
// switch between, preceded by a Project Settings shortcut only in the
// contextual menu the header's deployment switcher opens into.
export function SwitchDeploymentCommands({
  project,
  onNavigate,
  onSelectDeployment,
  contextual,
}: {
  project: ProjectDetails;
  onNavigate: (href: string) => void;
  onSelectDeployment: (deployment: PlatformDeploymentResponse) => void;
  // Only the anchored deployment-switcher menu shows the Project Settings
  // shortcut; the main palette's Switch Deployment page omits it.
  contextual: boolean;
}) {
  const team = useCurrentTeam();

  if (!team) {
    return <LoadingSignal />;
  }

  return (
    <>
      {contextual && (
        <Command.Group heading="Project">
          <NavigationItem
            target={{
              label: "Project Settings",
              href: `/t/${team.slug}/${project.slug}/settings`,
              Icon: GearIcon,
            }}
            onNavigate={onNavigate}
          />
        </Command.Group>
      )}
      <DeploymentsGroup
        team={team}
        project={project}
        onNavigate={onNavigate}
        onSelectDeployment={onSelectDeployment}
      />
    </>
  );
}

function DeploymentsGroup({
  team,
  project,
  onNavigate,
  onSelectDeployment,
}: {
  team: TeamResponse;
  project: ProjectDetails;
  onNavigate: (href: string) => void;
  onSelectDeployment: (deployment: PlatformDeploymentResponse) => void;
}) {
  const member = useProfile();
  const { deployments, isLoading } = useDeployments(project.id);
  const shownDeployments = useMemo(
    () =>
      (deployments ?? [])
        .filter((d) => (d.kind === "local" ? d.isActive : true))
        .sort(compareSwitcherDeployments(member?.id)),
    [deployments, member?.id],
  );
  const prodDeployments = shownDeployments.filter(
    (d) => d.deploymentType === "prod",
  );
  const nonProdDeployments = shownDeployments.filter(
    (d) => d.deploymentType !== "prod",
  );
  const hasPersonalDev = shownDeployments.some(
    (d) => d.deploymentType === "dev" && d.creator === member?.id,
  );

  return (
    <Command.Group heading="Deployments">
      {isLoading && !deployments ? (
        <LoadingSignal />
      ) : (
        <>
          {prodDeployments.length === 0 && (
            <CreateDeploymentItem
              deploymentType="prod"
              team={team}
              project={project}
              onNavigate={onNavigate}
            />
          )}
          {prodDeployments.map((deployment) => (
            <DeploymentItem
              key={deployment.name}
              deployment={deployment}
              teamSlug={team.slug}
              projectSlug={project.slug}
              onNavigate={onNavigate}
              onDrill={() => onSelectDeployment(deployment)}
            />
          ))}
          {/* Each create row sits in the slot its deployment type would occupy,
              so production stays above development whichever one is missing. */}
          {!hasPersonalDev && (
            <CreateDeploymentItem
              deploymentType="dev"
              team={team}
              project={project}
              onNavigate={onNavigate}
            />
          )}
          {nonProdDeployments.map((deployment) => (
            <DeploymentItem
              key={deployment.name}
              deployment={deployment}
              teamSlug={team.slug}
              projectSlug={project.slug}
              onNavigate={onNavigate}
              onDrill={() => onSelectDeployment(deployment)}
            />
          ))}
        </>
      )}
    </Command.Group>
  );
}

function CreateDeploymentItem({
  deploymentType,
  team,
  project,
  onNavigate,
}: {
  deploymentType: "prod" | "dev";
  team: TeamResponse;
  project: ProjectDetails;
  onNavigate: (href: string) => void;
}) {
  const { trackSelected } = usePaletteAnalytics();
  const typeLabel = deploymentTypeLabel(deploymentType);
  const provisionPage =
    deploymentType === "prod"
      ? PROVISION_PROD_PAGE_NAME
      : PROVISION_DEV_PAGE_NAME;
  return (
    <Command.Item
      value={`action:create-${deploymentType}-deployment`}
      keywords={[typeLabel, deploymentType, "create deployment"]}
      className="rounded-md outline-1 -outline-offset-1 outline-border-selected/60 outline-dashed"
      onSelect={() => {
        trackSelected(`create-${deploymentType}-deployment`);
        onNavigate(`/t/${team.slug}/${project.slug}/${provisionPage}`);
      }}
    >
      <div
        className={cn(
          "inline-flex shrink-0 items-center justify-center rounded-full p-1 opacity-50",
          deploymentTypeColorClasses(deploymentType),
        )}
      >
        <DeploymentTypeIcon deploymentType={deploymentType} />
      </div>
      <span className="flex min-w-0 flex-col">
        <span className="truncate text-content-secondary">{typeLabel}</span>
        <span className="truncate text-xs text-content-tertiary">
          Create a {typeLabel.toLowerCase()} deployment
        </span>
      </span>
      <PlusIcon className="ml-auto size-4 shrink-0 text-content-tertiary" />
    </Command.Item>
  );
}
