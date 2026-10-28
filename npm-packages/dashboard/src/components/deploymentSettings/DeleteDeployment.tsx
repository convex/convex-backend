import React, { useContext, useState } from "react";
import { ExclamationTriangleIcon } from "@radix-ui/react-icons";
import { Link } from "@ui/Link";
import { useRouter } from "next/router";
import { Sheet } from "@ui/Sheet";
import { Button } from "@ui/Button";
import { Callout } from "@ui/Callout";
import { Checkbox } from "@ui/Checkbox";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { LoadingTransition } from "@ui/Loading";
import udfs from "@common/udfs";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { permissionDenial, useSystemQuery } from "@common/lib/useSystemQuery";
import { NoPermissionMessage } from "elements/NoPermissionMessage";
import { useDeleteDeployment } from "api/deployments";
import {
  useHasCustomRolePermission,
  useHasProjectAdminPermissions,
} from "api/roles";
import { useCurrentTeam } from "api/teams";
import { useCurrentProject } from "api/projects";
import { deploymentResource } from "lib/permissions";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { DEPLOYMENT_SETTINGS_SECTIONS } from "lib/sectionAnchors";

export function DeleteDeployment() {
  const { useCurrentDeployment, deploymentsURI } = useContext(
    DeploymentInfoContext,
  );
  const deployment = useCurrentDeployment();
  const deploymentType = deployment?.deploymentType ?? "prod";
  const isLocal = deployment?.kind === "local";
  const [showDeleteModal, setShowDeleteModal] = useState(false);

  const hasAdminPermissions = useHasProjectAdminPermissions(
    deployment?.projectId,
  );
  const team = useCurrentTeam();
  const project = useCurrentProject();
  const resource =
    project && deployment && deployment.kind === "cloud"
      ? deploymentResource(project, {
          id: deployment.id,
          deploymentType: deployment.deploymentType,
          creator: deployment.creator ?? null,
        })
      : undefined;
  // Built-in developers can delete non-prod deployments; admins can
  // delete anything; custom-role members need an explicit
  // `deployment:delete` grant regardless of deployment type (custom
  // roles deny by default).
  const isProd = deploymentType === "prod";
  const canDeleteCustom = useHasCustomRolePermission(
    team?.id,
    "deployment:delete",
    resource,
    !isProd,
  );

  const canDelete = hasAdminPermissions || canDeleteCustom === true;

  if (!deployment) {
    return null;
  }

  // Local deployments are automatically cleaned up
  if (isLocal) {
    return (
      <Sheet id={DEPLOYMENT_SETTINGS_SECTIONS.deleteDeployment.id}>
        <div className="flex w-full flex-col gap-4">
          <h3>Delete Deployment</h3>
          <div className="flex max-w-prose flex-col gap-2">
            <p className="text-sm text-content-primary">
              Local deployments are automatically deleted when they become
              inactive.
            </p>
          </div>
        </div>
      </Sheet>
    );
  }

  return (
    <Sheet id={DEPLOYMENT_SETTINGS_SECTIONS.deleteDeployment.id}>
      <div className="flex w-full flex-col gap-4 lg:grid lg:grid-cols-[1fr_auto]">
        <h3>Delete Deployment</h3>
        <div className="flex items-start lg:row-span-2">
          <Button
            variant="danger"
            onClick={() => setShowDeleteModal(true)}
            disabled={!canDelete}
            tip={
              !canDelete
                ? permissionDeniedTip(
                    "You do not have permission to delete this deployment.",
                    "deployment:delete",
                  )
                : ""
            }
          >
            Delete Deployment
          </Button>
        </div>
        <div className="flex max-w-prose flex-col gap-2">
          <p className="text-sm text-content-primary">
            {deploymentType === "prod" && (
              <span className="font-semibold">
                This is a Production deployment.{" "}
              </span>
            )}
            All data and files in this deployment will be permanently deleted. A
            confirmation dialog will appear before proceeding.
          </p>
          <p className="text-sm text-content-primary">
            <span className="font-semibold">
              Consider creating and downloading a backup before deleting.
            </span>{" "}
            <Link href={`${deploymentsURI}/settings/backups`}>
              Go to Backups
            </Link>
          </p>
        </div>
      </div>

      {showDeleteModal && (
        <DeleteDeploymentModal
          deployment={deployment}
          onClose={() => setShowDeleteModal(false)}
        />
      )}
    </Sheet>
  );
}

function DeleteDeploymentModal({
  deployment,
  onClose,
}: {
  deployment: {
    name: string;
    deploymentType: string;
    projectId: number;
    reference?: string;
  };
  onClose: () => void;
}) {
  const router = useRouter();
  const files = useSystemQuery(udfs.fileStorageV2.numFiles, {
    componentId: null,
  });
  const documents = useSystemQuery(udfs.tableSize.sizeOfAllTables, {
    componentId: null,
  });

  const doneLoading =
    files.status !== "pending" && documents.status !== "pending";
  // Counts are unavailable to a member without `deployment:data:view`, so ask
  // for the acknowledgement rather than assuming the deployment is empty.
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
  const isProd = deployment.deploymentType === "prod";

  const [acceptedConsequences, setAcceptedConsequences] = useState(false);

  const shouldDisable = !acceptedConsequences && showAdditionalConfirmation;

  const teamSlug = router.query.team as string;
  const projectSlug = router.query.project as string;
  const projectSettingsURI = `/t/${teamSlug}/${projectSlug}/settings`;
  const deleteDeployment = useDeleteDeployment(
    deployment.projectId,
    deployment.name,
    projectSettingsURI,
  );

  const handleDelete = async () => {
    await deleteDeployment();
  };

  const deploymentTypeLabel =
    deployment.deploymentType.charAt(0).toUpperCase() +
    deployment.deploymentType.slice(1);

  const validationText = `Delete ${deployment.reference ?? deploymentTypeLabel} and all data`;

  return (
    <ConfirmationDialog
      onClose={onClose}
      onConfirm={handleDelete}
      confirmText="Delete Deployment"
      variant="danger"
      dialogTitle="Delete Deployment"
      validationText={shouldDisable ? undefined : validationText}
      disableConfirm={shouldDisable}
      dialogBody={
        <LoadingTransition
          loadingProps={{ fullHeight: false, className: "h-64" }}
        >
          {doneLoading && (
            <div className="flex w-full flex-col gap-2">
              <p>
                Delete this{" "}
                {isProd && <span className="font-semibold">Production</span>}{" "}
                deployment and all associated data.
              </p>
              <span className="font-semibold">
                Deleted deployments cannot be recovered.
              </span>

              {showAdditionalConfirmation && (
                <Callout className="flex flex-col gap-2">
                  {files.status === "success" &&
                  documents.status === "success" ? (
                    <div className="flex items-start gap-2">
                      <ExclamationTriangleIcon className="mt-1" />
                      <div className="flex flex-col gap-1">
                        <span>
                          This deployment contains data
                          {isProd && (
                            <>
                              {" "}
                              in{" "}
                              <span className="font-semibold">Production</span>
                            </>
                          )}
                          :
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
                      message="You do not have permission to read this deployment's data, so its contents can't be listed here."
                      missingPermission="deployment:data:view"
                    />
                  ) : (
                    <Callout variant="error">
                      This deployment's data could not be loaded, so its
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
                    deleting this deployment.
                  </label>
                </Callout>
              )}
              <p className="text-sm text-content-secondary">
                After deleting this deployment, you will be redirected to the
                Project Settings page.
              </p>
            </div>
          )}
        </LoadingTransition>
      }
    />
  );
}
