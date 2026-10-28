import { Command } from "cmdk";
import { useMemo } from "react";
import { useCurrentTeam } from "api/teams";
import { useProfile } from "api/profile";
import { useDeployments } from "api/deployments";
import type { PlatformDeploymentResponse, ProjectDetails } from "generatedApi";
import {
  DeploymentPickerItem,
  LoadingSignal,
  ProjectPickerItem,
} from "./items";
import { ProjectSearchGroup } from "./searchGroups";
import { compareSwitcherDeployments } from "./deploymentOrder";
import type { DeploymentPicker } from "./picker";

// One project's deployments to pick from. A picker menu opens here, with the
// project list (PickProjectCommands) behind it to step back up to.
export function PickDeploymentCommands({
  project,
  picker,
  onSelect,
}: {
  project: ProjectDetails;
  picker: DeploymentPicker;
  onSelect: (deployment: PlatformDeploymentResponse) => void;
}) {
  const member = useProfile();
  const { deployments, isLoading } = useDeployments(project.id);
  // Pickers choose a deployment for another deployment to act on (restoring a
  // backup from one into another), which only cloud deployments take part in.
  const cloudDeployments = useMemo(
    () =>
      (deployments ?? [])
        .filter((deployment) => deployment.kind === "cloud")
        .sort(compareSwitcherDeployments(member?.id)),
    [deployments, member?.id],
  );

  return (
    <Command.Group heading="Deployments">
      {isLoading && !deployments ? (
        <LoadingSignal />
      ) : cloudDeployments.length === 0 ? (
        // A disabled item rather than a plain node: cmdk decides whether to show
        // its own "No results" empty state from the number of items in the list,
        // so a bare element would leave the list looking empty and stack that
        // message on top of this one.
        <Command.Item disabled>
          This project has no cloud deployments.
        </Command.Item>
      ) : (
        cloudDeployments.map((deployment) => (
          <DeploymentPickerItem
            key={deployment.name}
            deployment={deployment}
            picker={picker}
            onSelect={() => onSelect(deployment)}
          />
        ))
      )}
    </Command.Group>
  );
}

// The picker's root: which project to pick a deployment from. Choosing one
// drills into its deployments rather than navigating to it.
export function PickProjectCommands({
  search,
  pinnedProject,
  onSelectProject,
}: {
  search: string;
  // The project the picker currently points at, listed first.
  pinnedProject: ProjectDetails | undefined;
  onSelectProject: (project: ProjectDetails) => void;
}) {
  const team = useCurrentTeam();

  if (!team) {
    return <LoadingSignal />;
  }

  return (
    <ProjectSearchGroup
      team={team}
      search={search}
      full
      pinnedProject={pinnedProject}
      renderItem={(project) => (
        <ProjectPickerItem
          key={project.id}
          project={project}
          selected={project.id === pinnedProject?.id}
          onSelect={() => onSelectProject(project)}
        />
      )}
    />
  );
}
