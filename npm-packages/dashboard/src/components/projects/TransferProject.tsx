import { useProfile } from "api/profile";
import { useCurrentProject, useTransferProject } from "api/projects";
import { useTeams, useCurrentTeam, useTeamMembers } from "api/teams";
import { useHasCustomRolePermission } from "api/roles";
import { useTeamOrbSubscription } from "api/billing";
import { useSSOLoginRequired } from "api/api";
import { ssoLoginPath } from "lib/ssoLogin";
import { useSupportFormOpen } from "elements/SupportWidget";
import { projectResource } from "lib/permissions";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { Sheet } from "@ui/Sheet";
import { Combobox } from "@ui/Combobox";
import { Button } from "@ui/Button";
import { Callout } from "@ui/Callout";
import { Link } from "@ui/Link";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { useRouter } from "next/router";
import { EnvelopeClosedIcon, LockClosedIcon } from "@radix-ui/react-icons";
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

  const originSubscription = useTeamOrbSubscription(originTeam?.id);
  const destinationSubscription = useTeamOrbSubscription(
    destinationTeamId ?? undefined,
  );
  const bothTeamsOnFreePlan =
    destinationTeamId !== null &&
    [originSubscription, destinationSubscription].every(
      ({ subscription, hasError }) => !hasError && subscription === null,
    );
  const subscriptionsLoading =
    destinationTeamId !== null &&
    [originSubscription, destinationSubscription].some(
      ({ subscription, hasError }) => !hasError && subscription === undefined,
    );

  // When the destination team requires SSO login and this session isn't
  // SSO-authenticated for it, its members and plan never load.
  const [{ teamIds: ssoLockedTeamIds }] = useSSOLoginRequired();
  const destinationRequiresSSOLogin =
    destinationTeamId !== null &&
    ssoLockedTeamIds.includes(destinationTeamId.toString());
  // A session is SSO-authenticated for at most one team, so no dashboard
  // session can act on both teams when each one requires SSO login.
  // `ssoLoginId` is set only on teams that require it.
  const bothTeamsRequireSSOLogin =
    !!originTeam?.ssoLoginId && !!destinationTeam?.ssoLoginId;
  const destinationSSOLoginId =
    destinationRequiresSSOLogin && !bothTeamsRequireSSOLogin
      ? destinationTeam?.ssoLoginId
      : undefined;
  const [, setSupportFormOpen] = useSupportFormOpen();

  const loading =
    destinationTeamId &&
    !destinationRequiresSSOLogin &&
    !bothTeamsRequireSSOLogin
      ? !originTeamMembers || !destinationTeamMembers || subscriptionsLoading
      : false;
  const canTransfer =
    canTransferFromOrigin &&
    !bothTeamsRequireSSOLogin &&
    !destinationRequiresSSOLogin &&
    isAdminOfNewTeam &&
    !bothTeamsOnFreePlan;

  const [showConfirmation, setShowConfirmation] = useState(false);
  const validationError =
    !destinationTeamId || canTransfer
      ? undefined
      : !canTransferFromOrigin
        ? `You do not have permission to transfer this project from ${originTeam?.name}.`
        : bothTeamsRequireSSOLogin
          ? `${originTeam?.name} and ${destinationTeam?.name} both require Single Sign-On login, so this project can't currently be transferred from the dashboard. Contact support to transfer it.`
          : destinationRequiresSSOLogin
            ? `${destinationTeam?.name} requires you to log in with Single Sign-On to transfer projects to it.`
            : !isAdminOfNewTeam
              ? `You must be an admin of ${destinationTeam?.name} to transfer this project to ${destinationTeam?.name}.`
              : undefined;
  const router = useRouter();

  return (
    <Sheet>
      <h3 className="mb-4">Transfer Project</h3>
      <p className="mb-5 max-w-prose text-sm text-content-primary">
        Transfer this project to another team. At least one of the two teams
        must be on a paid plan.
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
          {canTransferFromOrigin &&
            destinationTeam &&
            destinationSSOLoginId && (
              <Button
                className="w-fit"
                variant="neutral"
                size="sm"
                icon={<LockClosedIcon />}
                onClick={() => {
                  // A full navigation, because `/api/auth/login` is an API
                  // route that redirects to AuthKit.
                  window.location.href = ssoLoginPath(
                    destinationSSOLoginId,
                    router.asPath,
                  );
                }}
              >
                Log in with SSO for {destinationTeam.name}
              </Button>
            )}
          {canTransferFromOrigin &&
            bothTeamsRequireSSOLogin &&
            project &&
            originTeam &&
            destinationTeam && (
              <Button
                className="w-fit"
                variant="neutral"
                size="sm"
                icon={<EnvelopeClosedIcon />}
                onClick={() =>
                  setSupportFormOpen({
                    defaultSubject: `Transfer project ${project.slug} to team ${destinationTeam.slug}`,
                    defaultMessage: `Please transfer the project ${project.slug} from team ${originTeam.slug} to team ${destinationTeam.slug}. Both teams require Single Sign-On login.`,
                  })
                }
              >
                Contact support
              </Button>
            )}
          {!loading &&
            !validationError &&
            bothTeamsOnFreePlan &&
            originTeam &&
            destinationTeam && (
              <Callout variant="upsell" className="max-w-prose">
                <div>
                  {originTeam.name} and {destinationTeam.name} are both on the
                  Free plan. Upgrade either team to a paid plan, such as
                  Starter, to transfer this project:{" "}
                  <Link href={`/t/${originTeam.slug}/settings/billing`}>
                    upgrade {originTeam.name}
                  </Link>{" "}
                  or{" "}
                  <Link href={`/t/${destinationTeam.slug}/settings/billing`}>
                    upgrade {destinationTeam.name}
                  </Link>
                  .
                </div>
              </Callout>
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
