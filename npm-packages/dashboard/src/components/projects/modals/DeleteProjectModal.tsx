import { ExclamationTriangleIcon } from "@radix-ui/react-icons";
import { Callout } from "@ui/Callout";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { Checkbox } from "@ui/Checkbox";
import { LoadingTransition } from "@ui/Loading";
import { WaitForDeploymentApi } from "@common/lib/deploymentContext";
import { useDeployments } from "api/deployments";
import { useTeamMembers } from "api/teams";
import { useDeleteProject } from "api/projects";
import { ProjectDetails, TeamResponse } from "generatedApi";
import udfs from "@common/udfs";
import { permissionDenial, useSystemQuery } from "@common/lib/useSystemQuery";
import { NoPermissionMessage } from "elements/NoPermissionMessage";
import { useState } from "react";
import { DeploymentInfoProvider } from "providers/DeploymentInfoProvider";
import { MaybeDeploymentApiProvider } from "providers/MaybeDeploymentApiProvider";
import { usePostHog } from "hooks/usePostHog";

export function DeleteProjectModal({
  onClose,
  onDelete,
  team,
  project,
}: {
  onClose: () => void;
  onDelete?: () => void;
  team: TeamResponse;
  project: ProjectDetails;
}) {
  const { capture } = usePostHog();
  const deleteProject = useDeleteProject(
    team.id,
    project.id,
    project.name || "Untitled Project",
  );

  const handleDelete = async () => {
    onDelete?.();
    await deleteProject();
    capture("deleted_projects");
    onClose();
  };
  const { deployments } = useDeployments(project.id);
  const defaultProdDeployment = deployments?.find(
    (d) => d.kind === "cloud" && d.deploymentType === "prod" && d.isDefault,
  );

  return deployments && defaultProdDeployment ? (
    <DeploymentInfoProvider
      deploymentOverride={defaultProdDeployment.name}
      deploymentUrlOverride={
        defaultProdDeployment.kind === "cloud"
          ? defaultProdDeployment.deploymentUrl
          : undefined
      }
    >
      <MaybeDeploymentApiProvider
        deploymentOverride={defaultProdDeployment.name}
      >
        <WaitForDeploymentApi sizeClass="hidden">
          <DeleteProjectModalContentWithDefaultProd
            team={team}
            project={project}
            onClose={onClose}
            handleDelete={handleDelete}
          />
        </WaitForDeploymentApi>
      </MaybeDeploymentApiProvider>
    </DeploymentInfoProvider>
  ) : (
    <DeleteProjectDialog
      onClose={onClose}
      onConfirm={handleDelete}
      validationText={`Delete ${project.name} and data in all deployments, including Production`}
    >
      <DeleteProjectModalContent team={team} />
    </DeleteProjectDialog>
  );
}

function DeleteProjectModalContentWithDefaultProd({
  team,
  project,
  onClose,
  handleDelete,
}: {
  team: TeamResponse;
  project: ProjectDetails;
  onClose: () => void;
  handleDelete: () => Promise<void>;
}) {
  const files = useSystemQuery(udfs.fileStorageV2.numFiles, {
    componentId: null,
  });
  const documents = useSystemQuery(udfs.tableSize.sizeOfAllTables, {
    componentId: null,
  });

  const doneLoading =
    files.status !== "pending" && documents.status !== "pending";
  // Counts are unavailable to a member without `deployment:data:view`, so ask
  // for the acknowledgement rather than assuming the project is empty.
  const showAdditionalConfirmation =
    files.status !== "success" ||
    documents.status !== "success" ||
    files.data > 0 ||
    documents.data > 0;
  // A dropped connection or a backend error leaves the counts unavailable
  // too, so only blame permissions when that's what actually happened.
  const countsDenied =
    (files.status === "error" && permissionDenial(files.error) !== null) ||
    (documents.status === "error" &&
      permissionDenial(documents.error) !== null);

  const [acceptedConsequences, setAcceptedConsequences] = useState(false);

  const shouldDisable = !acceptedConsequences && showAdditionalConfirmation;

  return (
    <DeleteProjectDialog
      onClose={onClose}
      onConfirm={handleDelete}
      validationText={
        shouldDisable
          ? undefined
          : `Delete ${project.name} and data in all deployments, including Production`
      }
      disableConfirm={shouldDisable}
    >
      <LoadingTransition
        loadingProps={{ fullHeight: false, className: "h-64" }}
      >
        {doneLoading && (
          <DeleteProjectModalContent
            team={team}
            additionalBody={
              showAdditionalConfirmation && (
                <Callout className="flex flex-col gap-2">
                  {files.status === "success" &&
                  documents.status === "success" ? (
                    <div className="flex items-start gap-2">
                      <ExclamationTriangleIcon className="mt-1" />
                      <div className="flex flex-col gap-1">
                        <span>
                          This project contains data in{" "}
                          <span className="font-semibold">Production</span>:
                        </span>
                        <ul className="ml-4 flex list-disc flex-col gap-1">
                          <li>
                            <span className="font-semibold">
                              {documents.data.toLocaleString()} Documents
                            </span>{" "}
                            stored across all tables.
                          </li>
                          <li>
                            <span className="font-semibold">
                              {files.data.toLocaleString()} Files
                            </span>{" "}
                            stored.
                          </li>
                        </ul>
                      </div>
                    </div>
                  ) : countsDenied ? (
                    <NoPermissionMessage
                      message="You do not have permission to read this project's Production data, so its contents can't be listed here."
                      missingPermission="deployment:data:view"
                    />
                  ) : (
                    <Callout variant="error">
                      This project's Production data could not be loaded, so its
                      contents can't be listed here.
                    </Callout>
                  )}
                  <label className="flex gap-2 text-sm">
                    <Checkbox
                      className="mt-0.5"
                      checked={acceptedConsequences}
                      onChange={(e) =>
                        setAcceptedConsequences(e.currentTarget.checked)
                      }
                    />{" "}
                    By checking this box, I acknowledge the consequences of
                    deleting this project.
                  </label>
                </Callout>
              )
            }
          />
        )}
      </LoadingTransition>
    </DeleteProjectDialog>
  );
}

function DeleteProjectModalContent({
  team,
  additionalBody,
}: {
  team: TeamResponse;
  additionalBody?: React.ReactNode;
}) {
  const members = useTeamMembers(team?.id);
  const numOtherTeamMembers = members ? members.length - 1 : 0;

  let warningMessage: React.ReactNode | string =
    "Delete this project and all associated data.";
  if (members && numOtherTeamMembers >= 1) {
    warningMessage = (
      <span>
        Delete this project for you and{" "}
        <span className="font-semibold">
          {numOtherTeamMembers} other team member
          {numOtherTeamMembers > 1 ? "s" : ""}
        </span>
        .
      </span>
    );
  }

  return (
    <div className="flex w-full flex-col gap-2">
      {warningMessage}
      <span className="font-semibold">
        Deleted projects cannot be recovered.
      </span>
      {additionalBody}
    </div>
  );
}

function DeleteProjectDialog({
  children,
  ...props
}: {
  onClose: () => void;
  onConfirm: () => Promise<void>;
  validationText?: string;
  disableConfirm?: boolean;
  children: React.ReactNode;
}) {
  return (
    <ConfirmationDialog
      {...props}
      confirmText="Delete Project"
      variant="danger"
      dialogTitle="Delete Project"
      dialogBody={children}
    />
  );
}
