import { useProfile } from "api/profile";
import { useCurrentProject, useTransferProject } from "api/projects";
import { useTeams, useCurrentTeam, useTeamMembers } from "api/teams";
import { useHasCustomRolePermission } from "api/roles";
import { projectResource } from "lib/permissions";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { Sheet } from "@ui/Sheet";
import { Combobox } from "@ui/Combobox";
import { Button } from "@ui/Button";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { useRouter } from "next/router";
import { useState } from "react";

export function TransferProject() {
  const project = useCurrentProject();
  const { selectedTeamSlug, teams } = useTeams();
  const originTeam = useCurrentTeam();
  const [destinationTeamId, setDestinationTeamId] = useState<number | null>(
    null,
  );
  const transferProject = useTransferProject(
    project?.id,
    destinationTeamId ?? undefined,
    originTeam?.id,
  );
  const destinationTeam = teams?.find((t) => t.id === destinationTeamId);
  // Projects in a Vercel-managed team must be provisioned through the Vercel
  // Marketplace, so the server rejects transfers into one.
  const eligibleDestinationTeams = teams?.filter(
    (t) => t.slug !== selectedTeamSlug && t.managedBy !== "vercel",
  );
  const hasEligibleDestinationTeam =
    eligibleDestinationTeams !== undefined &&
    eligibleDestinationTeams.length > 0;
  const noDestinationTeamMessage =
    "You must be a member of another team that is not managed by Vercel to transfer a project.";

  const me = useProfile();
  const originTeamMembers = useTeamMembers(originTeam?.id);
  const destinationTeamMembers = useTeamMembers(destinationTeamId ?? undefined);
  const isAdminOfOldTeam =
    originTeamMembers?.find((member) => member.id === me?.id)?.role === "admin";
  const isAdminOfNewTeam =
    destinationTeamMembers?.find((member) => member.id === me?.id)?.role ===
    "admin";

  // Allow custom-role members with `project:transfer` on the origin team
  // (scoped to this project) to initiate transfers. The destination side
  // still requires team admin to keep the destination-selector UI simple;
  // the server enforces the destination permission at request time.
  const canTransferCustom = useHasCustomRolePermission(
    originTeam?.id,
    "project:transfer",
    project ? projectResource(project) : undefined,
    false,
  );
  const canTransferFromOrigin = isAdminOfOldTeam || canTransferCustom === true;

  const loading = destinationTeamId
    ? !originTeamMembers || !destinationTeamMembers
    : false;
  const canTransfer = canTransferFromOrigin && isAdminOfNewTeam;

  const [showConfirmation, setShowConfirmation] = useState(false);
  const validationError =
    !destinationTeamId || canTransfer
      ? undefined
      : !canTransferFromOrigin
        ? `You do not have permission to transfer this project from ${originTeam?.name}.`
        : `You must be an admin of ${destinationTeam?.name} to transfer this project to ${destinationTeam?.name}.`;
  const router = useRouter();

  return (
    <Sheet>
      <h3 className="mb-4">Transfer Project</h3>
      <p className="mb-5 max-w-prose text-sm text-content-primary">
        Transfer this project to another team.
      </p>
      {hasEligibleDestinationTeam && (
        <div className="mb-4 flex flex-col gap-1">
          <Combobox
            label={
              <div className="flex items-center gap-2">Destination Team</div>
            }
            labelHidden={false}
            placeholder="Select a team"
            buttonProps={{
              loading,
              tip:
                validationError ||
                (!canTransferFromOrigin
                  ? permissionDeniedTip(
                      "You do not have permission to transfer this project.",
                      "project:transfer",
                    )
                  : undefined),
            }}
            options={
              eligibleDestinationTeams?.map((team) => ({
                label: team.name,
                value: team.id,
              })) || []
            }
            selectedOption={destinationTeamId}
            setSelectedOption={setDestinationTeamId}
            disabled={!canTransferFromOrigin}
          />
          {!loading && validationError && (
            <p
              className="max-w-prose animate-fadeInFromLoading text-xs text-content-errorSecondary"
              role="alert"
            >
              {validationError}
            </p>
          )}
        </div>
      )}
      <Button
        variant="primary"
        disabled={
          loading ||
          !destinationTeamId ||
          !canTransfer ||
          !hasEligibleDestinationTeam
        }
        tip={
          !hasEligibleDestinationTeam
            ? noDestinationTeamMessage
            : !destinationTeamId
              ? "Select a team to transfer this project to."
              : undefined
        }
        onClick={() => setShowConfirmation(true)}
      >
        Transfer
      </Button>
      {project &&
        originTeam &&
        destinationTeam &&
        project &&
        showConfirmation && (
          <ConfirmationDialog
            confirmText="Transfer"
            validationText={`Transfer ${project.slug} from ${originTeam.slug} to ${destinationTeam.slug}`}
            dialogTitle={`Transfer Project to ${destinationTeam.name}?`}
            dialogBody={
              <div className="flex flex-col gap-2">
                Are you sure you want to transfer this project?
              </div>
            }
            onConfirm={async () => {
              await transferProject({
                destinationTeamId: destinationTeam.id,
              });
              setShowConfirmation(false);
              await router.replace(
                `/t/${destinationTeam.slug}/${project.slug}/settings`,
              );
            }}
            variant="primary"
            onClose={() => setShowConfirmation(false)}
          />
        )}
    </Sheet>
  );
}
