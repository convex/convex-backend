import {
  useTeams,
  useTeamMembers,
  useDeleteTeam,
  useUpdateTeam,
} from "api/teams";
import { usePaginatedProjects } from "api/projects";
import { useTeamOrbSubscription } from "api/billing";
import {
  useHasCustomRolePermission,
  useIsCurrentMemberTeamAdmin,
} from "api/roles";
import { TeamResponse } from "generatedApi";
import { Sheet } from "@ui/Sheet";
import { Button } from "@ui/Button";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { TEAM_RESOURCE } from "lib/permissions";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { useCallback, useState } from "react";
import startCase from "lodash/startCase";
import { OpenInVercel } from "components/OpenInVercel";
import { TeamNameForm } from "./TeamNameForm";
import { TeamSlugForm } from "./TeamSlugForm";
import { TeamIdSheet } from "./TeamIdSheet";
import { DefaultRegionForm } from "./DefaultRegionForm";
import { TEAM_SETTINGS_SECTIONS } from "lib/sectionAnchors";

export function TeamSettings({ team }: { team: TeamResponse }) {
  const updateTeam = useUpdateTeam(team.id);
  const isTeamAdmin = useIsCurrentMemberTeamAdmin();
  // `team:update`/`team:delete` are admin-only by default; custom roles need
  // explicit grants.
  const canUpdateCustom = useHasCustomRolePermission(
    team.id,
    "team:update",
    TEAM_RESOURCE,
    false,
  );
  const canDeleteCustom = useHasCustomRolePermission(
    team.id,
    "team:delete",
    TEAM_RESOURCE,
    false,
  );
  const canUpdate = isTeamAdmin || canUpdateCustom === true;
  const canDelete = isTeamAdmin || canDeleteCustom === true;
  const { teams } = useTeams();
  const teamMembers = useTeamMembers(team.id);
  const firstPageOfProjects = usePaginatedProjects(team.id, {});
  const hasProjects =
    firstPageOfProjects && firstPageOfProjects.items.length > 0;

  const [showDeleteTeamModal, setShowDeleteTeamModal] = useState(false);
  const { subscription } = useTeamOrbSubscription(team.id);

  const deleteTeam = useDeleteTeam(team.id);
  const deleteTeamAndRedirect = useCallback(async () => {
    await deleteTeam();
    // Completely reload the page to avoid race conditions
    window.location.href = "/";
  }, [deleteTeam]);

  return (
    <>
      <h2>Team Settings</h2>
      <TeamNameForm
        team={team}
        onUpdateTeam={updateTeam}
        canUpdate={canUpdate}
      />
      <TeamSlugForm
        team={team}
        onUpdateTeam={updateTeam}
        canUpdate={canUpdate}
      />
      <TeamIdSheet team={team} />
      <DefaultRegionForm
        team={team}
        onUpdateTeam={updateTeam}
        canUpdate={canUpdate}
      />
      <Sheet id={TEAM_SETTINGS_SECTIONS.deleteTeam.id}>
        <h3 className="mb-4">Delete Team</h3>
        <p className="mb-4">
          Permanently deletes this team.{" "}
          {team.managedBy !== "vercel" && (
            <>
              To delete your team, you must first remove all team members and
              delete all projects associated with the team.
            </>
          )}
        </p>
        {subscription && (
          <p className="mb-4">
            Deleting your team will automatically cancel your{" "}
            <span className="font-semibold">{subscription.plan.name}</span>{" "}
            subscription.
          </p>
        )}
        {team.managedBy === "vercel" && (
          <div className="flex items-center justify-between gap-4">
            <div>
              This team is managed by {startCase(team.managedBy)}. You may
              delete this Convex team by deleting your Convex integration in{" "}
              {startCase(team.managedBy)}.
            </div>
            <OpenInVercel team={team} />
          </div>
        )}
        {team.managedBy !== "vercel" && (
          <Button
            variant="danger"
            onClick={() => setShowDeleteTeamModal(true)}
            disabled={
              !canDelete ||
              !teams ||
              teams.length === 1 ||
              !teamMembers ||
              teamMembers.length > 1 ||
              !firstPageOfProjects ||
              hasProjects
            }
            tip={
              !canDelete
                ? permissionDeniedTip(
                    "You do not have permission to delete this team.",
                    "team:delete",
                  )
                : teams && teams.length === 1
                  ? "You cannot delete your last team."
                  : teamMembers && teamMembers.length > 1
                    ? "You must remove all other team members before deleting the team."
                    : hasProjects
                      ? "You must delete all projects before deleting the team."
                      : undefined
            }
          >
            Delete Team
          </Button>
        )}
        {showDeleteTeamModal && (
          <ConfirmationDialog
            onClose={() => setShowDeleteTeamModal(false)}
            onConfirm={deleteTeamAndRedirect}
            validationText={team.slug}
            confirmText="Delete"
            dialogTitle="Delete Team"
            dialogBody="Are you sure you want to delete this team? This action cannot be undone."
          />
        )}
      </Sheet>
    </>
  );
}
