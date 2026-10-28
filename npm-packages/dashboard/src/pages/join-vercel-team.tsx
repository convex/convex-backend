import { useRouter } from "next/router";
import { useState } from "react";
import { LoginLayout } from "layouts/LoginLayout";
import { withAuthenticatedPage } from "lib/withAuthenticatedPage";
import { Sheet } from "@ui/Sheet";
import { Button } from "@ui/Button";
import { Callout } from "@ui/Callout";
import { LoadingLogo } from "@ui/Loading";
import { useJoinVercelTeam, usePotentialVercelTeams } from "api/teams";
import { useSupportFormOpen } from "elements/SupportWidget";
import VercelLogo from "logos/vercel.svg";

function destinationAfterJoin(
  slug: string,
  vercelPath: string | undefined,
  projectId: string | undefined,
): string {
  // When projectId is set, bounce through `/` so RedirectToProjectById
  // resolves the project slug now that we're on the team.
  if (projectId) return `/?projectId=${projectId}`;
  if (vercelPath === "billing") return `/t/${slug}/settings/billing`;
  if (vercelPath === "usage") return `/t/${slug}/settings/usage`;
  return `/t/${slug}`;
}

export { getServerSideProps } from "lib/ssr";

function JoinVercelTeam() {
  const router = useRouter();
  const teamIdParam = router.query.teamId;
  const teamId =
    typeof teamIdParam === "string" ? Number(teamIdParam) : undefined;

  const vercelPath =
    typeof router.query.vercelPath === "string"
      ? router.query.vercelPath
      : undefined;
  const projectId =
    typeof router.query.projectId === "string"
      ? router.query.projectId
      : undefined;

  const { data: potentialTeams, error } = usePotentialVercelTeams();
  const team =
    teamId !== undefined && !Number.isNaN(teamId)
      ? potentialTeams?.find((t) => t.teamId === teamId)
      : undefined;

  const joinTeam = useJoinVercelTeam(teamId ?? 0);
  const [, setSupportFormOpen] = useSupportFormOpen();
  const [isJoining, setIsJoining] = useState(false);

  if (potentialTeams === undefined && !error) {
    return (
      <LoginLayout>
        <LoadingLogo />
      </LoginLayout>
    );
  }

  if (!team) {
    return (
      <LoginLayout>
        <Sheet className="flex flex-col gap-6">
          <span role="alert" className="max-w-prose text-content-primary">
            This invitation is no longer valid, or you don't have permission to
            join this team.
          </span>
          <div>
            <Button href="/" variant="neutral">
              Go back
            </Button>
          </div>
        </Sheet>
      </LoginLayout>
    );
  }

  const displayName = team.teamName.replace(/ \(Vercel\)$/, "");

  return (
    <LoginLayout>
      <Sheet className="flex max-w-prose flex-col gap-4">
        <div className="flex items-center gap-3">
          <VercelLogo className="size-6 fill-content-primary" />
          <h2>Join {displayName}</h2>
        </div>
        <p className="text-content-primary">
          You've been invited to join{" "}
          <span className="font-semibold">{displayName}</span> through the
          Vercel marketplace. Accepting will add you as a member of this team.
        </p>
        {team.pricingNotice && (
          <Callout variant="upsell">
            <p>{team.pricingNotice}</p>
          </Callout>
        )}
        <form
          className="flex gap-2"
          onSubmit={async (e) => {
            e.preventDefault();
            setIsJoining(true);
            try {
              const result = await joinTeam();
              if (vercelPath === "support") {
                setSupportFormOpen(true);
              }
              window.location.href = destinationAfterJoin(
                result!.slug,
                vercelPath,
                projectId,
              );
            } catch {
              setIsJoining(false);
            }
          }}
        >
          <Button type="submit" loading={isJoining}>
            Join team
          </Button>
          <Button href="/" variant="neutral" disabled={isJoining}>
            Cancel
          </Button>
        </form>
      </Sheet>
    </LoginLayout>
  );
}

export default withAuthenticatedPage(JoinVercelTeam);
