import { useWorkOS } from "hooks/useWorkOS";
import { ChevronLeftIcon } from "@radix-ui/react-icons";
import { BreadcrumbLink } from "components/header/BreadcrumbLink/BreadcrumbLink";
import { Header } from "components/header/Header/Header";
import { NavBar } from "components/header/NavBar/NavBar";
import { useTeams } from "api/teams";
import { useProjectBySlug } from "api/projects";
import {
  useRememberLastViewedProject,
  useRememberLastViewedTeam,
} from "hooks/useLastViewed";
import Link from "next/link";
import { useRouter } from "next/router";
import { useEffect } from "react";
import { useAccessToken } from "hooks/useServerSideData";
import { ProjectSelector } from "components/header/ProjectSelector/ProjectSelector";

import {
  PROVISION_PROD_PAGE_NAME,
  PROVISION_DEV_PAGE_NAME,
} from "@common/lib/deploymentContext";
import { UsageBanner, useCurrentUsageBanner } from "./UsageBanner";
import {
  FailedPaymentBanner,
  useShowFailedPaymentBanner,
} from "./FailedPaymentBanner";
import {
  UpdateBillingAddressBanner,
  useShowUpdateBillingAddressBanner,
} from "./UpdateBillingAddressBanner";

export function DashboardHeader() {
  const [accessToken] = useAccessToken();

  return accessToken ? <DashboardHeaderWhenLoggedIn /> : <div />;
}

const NO_TEAM_ROUTES = [
  "/invite/[inviteId]",
  "/auth",
  "/accept",
  "/suspended",
  "/verify",
  "/join-vercel-team",
];

const NO_HEADER_ROUTES = ["/oauth/authorize/project", "/oauth/authorize/team"];

function DashboardHeaderWhenLoggedIn() {
  const { user } = useWorkOS();
  const router = useRouter();

  const projectSlug = router?.query.project as string;
  const { teams, selectedTeamSlug } = useTeams();
  const team = teams?.find((t) => t.slug === selectedTeamSlug);
  const selectedProject = useProjectBySlug(team?.id, projectSlug);

  useEffect(() => {
    if (projectSlug && selectedProject === null) {
      void router.push("/404");
    }
  }, [selectedProject, projectSlug, router]);

  useRememberLastViewedTeam(selectedTeamSlug);
  useRememberLastViewedProject(projectSlug);

  const projectSelector = (
    <ProjectSelector
      teams={teams}
      selectedProject={projectSlug ? (selectedProject ?? undefined) : undefined}
      selectedTeamSlug={selectedTeamSlug}
    />
  );
  const inNoTeamRoute = NO_TEAM_ROUTES.some((r) => r === router.pathname);
  const inNoHeaderRoute = NO_HEADER_ROUTES.some((r) => r === router.pathname);
  const getHeaderContent = () => {
    if (inNoTeamRoute) {
      return null;
    }
    if (router.route === "/profile") {
      return (
        <div className="flex h-full gap-2">
          <Link href="/" className="group flex h-full items-center">
            <div className="flex items-center gap-1 rounded-sm px-1 py-1.5 text-xs text-content-secondary group-hover:bg-background-tertiary">
              <ChevronLeftIcon />
              Back
            </div>
          </Link>
          <BreadcrumbLink
            href="/profile"
            className="flex items-center truncate"
          >
            Profile Settings
          </BreadcrumbLink>
        </div>
      );
    }

    if (
      router.route.endsWith("/[project]/settings") ||
      router.route.endsWith("/[project]") ||
      router.route.includes("/[project]/[deploymentName]") ||
      router.route.includes(`/[project]/${PROVISION_PROD_PAGE_NAME}`) ||
      router.route.includes(`/[project]/${PROVISION_DEV_PAGE_NAME}`)
    ) {
      return selectedProject && projectSelector;
    }

    return (
      <div className="flex h-full gap-6">
        {projectSelector}
        <NavBar
          items={[
            { label: "Home", href: `/t/${selectedTeamSlug}` },
            {
              label: "Team Settings",
              href: `/t/${selectedTeamSlug}/settings`,
            },
          ]}
          activeLabel={
            router.asPath.startsWith(`/t/${selectedTeamSlug}/settings`)
              ? "Team Settings"
              : "Home"
          }
        />
      </div>
    );
  };

  // Make sure that only one banner shows up at a time because of the way the
  // layout is set up right now
  const usageBannerVariant = useCurrentUsageBanner(team?.id ?? null);
  const showFailedPaymentBanner = useShowFailedPaymentBanner();
  const showUpdateBillingAddressBanner = useShowUpdateBillingAddressBanner();

  if (inNoHeaderRoute) {
    return null;
  }

  return (
    <div className="sticky top-0 z-40">
      <Header user={user}>{getHeaderContent()}</Header>

      {showFailedPaymentBanner && <FailedPaymentBanner />}
      {!showFailedPaymentBanner && showUpdateBillingAddressBanner && (
        <UpdateBillingAddressBanner />
      )}
      {!showFailedPaymentBanner &&
        !showUpdateBillingAddressBanner &&
        usageBannerVariant !== null && (
          <UsageBanner team={team!} variant={usageBannerVariant} />
        )}
    </div>
  );
}
