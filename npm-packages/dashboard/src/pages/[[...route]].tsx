import { useRouter } from "next/router";
import Link from "next/link";
import { useTeams, usePotentialVercelTeams } from "api/teams";
import { useEffect, useRef, useState } from "react";
import { Loading } from "@ui/Loading";
import { withAuthenticatedPage } from "lib/withAuthenticatedPage";
import { useProjectById } from "api/projects";
import { useSupportFormOpen } from "elements/SupportWidget";
import { toast } from "@common/lib/utils";

export { getServerSideProps } from "lib/ssr";

function vercelPathSubpath(vercelPath: string | undefined): string {
  if (vercelPath === "billing") return "/settings/billing";
  if (vercelPath === "usage") return "/settings/usage";
  return "";
}

function buildJoinHref(
  teamId: number,
  vercelPath: string | undefined,
  projectId: string | undefined,
): string {
  const params = new URLSearchParams({ teamId: teamId.toString() });
  if (vercelPath) params.set("vercelPath", vercelPath);
  if (projectId) params.set("projectId", projectId);
  return `/join-vercel-team?${params.toString()}`;
}

function RedirectToTeam() {
  const { selectedTeamSlug } = useTeams();
  const router = useRouter();

  useEffect(() => {
    // If we reached the catch-all route on a route starting with /t/,
    // the user tried to go to a page we don't have.
    if (router.asPath.startsWith("/t/")) {
      void router.replace("/404");
      return;
    }
    if (router.query.route?.length) {
      void router.replace(`/t/${router.asPath}`);
    } else if (selectedTeamSlug) {
      void router.replace(`/t/${selectedTeamSlug}`);
    }
  }, [selectedTeamSlug, router]);

  return <Loading />;
}

function VercelLoginRedirect({
  firstVercelTeamSlug,
  vercelPath,
  projectId,
}: {
  firstVercelTeamSlug: string | undefined;
  vercelPath: string | undefined;
  projectId: string | undefined;
}) {
  const router = useRouter();
  const { data: potentialTeams, error } = usePotentialVercelTeams();
  const [, setSupportFormOpen] = useSupportFormOpen();
  const didDispatch = useRef(false);
  const [resolveProjectId, setResolveProjectId] = useState<string | null>(null);

  useEffect(() => {
    if (didDispatch.current) {
      return;
    }
    if (potentialTeams === undefined && !error) {
      return;
    }
    const potentialTeam = potentialTeams?.[0];

    if (vercelPath === "support") {
      setSupportFormOpen(true);
    }

    // If the potential-teams lookup failed or returned nothing, fall back to
    // the old behavior: redirect to the first Vercel team (or home), or
    // resolve the projectId if one was provided.
    if (!potentialTeam) {
      didDispatch.current = true;
      if (projectId) {
        setResolveProjectId(projectId);
      } else {
        void router.replace(
          firstVercelTeamSlug
            ? `/t/${firstVercelTeamSlug}${vercelPathSubpath(vercelPath)}`
            : "/",
        );
      }
      return;
    }

    // Needs to join before we can honor any vercelPath/projectId: route
    // through the confirmation page and preserve them so we end up at the
    // right destination after joining.
    if (!firstVercelTeamSlug) {
      didDispatch.current = true;
      void router.replace(
        buildJoinHref(potentialTeam.teamId, vercelPath, projectId),
      );
      return;
    }

    didDispatch.current = true;
    toast(
      "info",
      <>
        You've been invited to join {potentialTeam.teamName} through the Vercel
        marketplace.{" "}
        <Link
          href={buildJoinHref(potentialTeam.teamId, vercelPath, projectId)}
          className="underline"
        >
          View invitation
        </Link>
      </>,
      `vercel-pending-team-${potentialTeam.teamId}`,
      false,
    );
    if (projectId) {
      setResolveProjectId(projectId);
    } else {
      void router.replace(
        `/t/${firstVercelTeamSlug}${vercelPathSubpath(vercelPath)}`,
      );
    }
  }, [
    firstVercelTeamSlug,
    potentialTeams,
    error,
    router,
    vercelPath,
    projectId,
    setSupportFormOpen,
  ]);

  if (resolveProjectId) {
    return <RedirectToProjectById id={resolveProjectId} />;
  }
  return <Loading />;
}

function Main() {
  const { query } = useRouter();
  const { teams } = useTeams();

  const firstVercelTeam = teams?.find((t) => t.managedBy === "vercel");
  const vercelPath =
    typeof query.vercelPath === "string" ? query.vercelPath : undefined;
  const projectId =
    typeof query.projectId === "string" ? query.projectId : undefined;

  // When any Vercel-flow param is present (including projectId from
  // `resource_id` in the auth callback), run the join-consent flow first so
  // users who aren't on the team yet land at /join-vercel-team instead of
  // getting stuck in RedirectToProjectById.
  if (query.vercelLogin || vercelPath || projectId) {
    return (
      <VercelLoginRedirect
        firstVercelTeamSlug={firstVercelTeam?.slug}
        vercelPath={vercelPath}
        projectId={projectId}
      />
    );
  }

  return <RedirectToTeam />;
}

function RedirectToProjectById({ id }: { id: string }) {
  const { project } = useProjectById(parseInt(id));
  const { teams } = useTeams();
  const projectTeam = teams?.find((team) => team.id === project?.teamId);
  const router = useRouter();
  useEffect(() => {
    if (project && projectTeam) {
      void router.replace(`/t/${projectTeam.slug}/${project.slug}`);
    }
  }, [project, projectTeam, router]);
  return <Loading />;
}

export default withAuthenticatedPage(Main);
