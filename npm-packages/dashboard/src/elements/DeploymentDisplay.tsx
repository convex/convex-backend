import {
  CommandLineIcon,
  SignalIcon,
  WrenchIcon,
} from "@heroicons/react/24/outline";
import { CaretSortIcon, GearIcon, Pencil2Icon } from "@radix-ui/react-icons";
import {
  useCurrentDeployment,
  useDeploymentByName,
  useDeployments,
} from "api/deployments";
import { useCurrentTeam, useTeamEntitlements, useTeamMembers } from "api/teams";
import { useProfile } from "api/profile";
import { useRememberLastViewedDeploymentForProject } from "hooks/useLastViewed";
import { cn } from "@ui/cn";
import { useRouter } from "next/router";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { ProjectDetails } from "generatedApi";
import { Button } from "@ui/Button";
import { useCurrentProject } from "api/projects";
import { useContext, useRef, useState, useEffect } from "react";
import {
  useCommandPaletteAnchor,
  useCommandPaletteOpen,
  useOpenCommandPalette,
} from "elements/CommandPalette";
import { usePaletteAnalytics } from "elements/CommandPalette/analytics";
import {
  PROVISION_DEV_PAGE_NAME,
  PROVISION_PROD_PAGE_NAME,
} from "@common/lib/deploymentContext";
import { deploymentTypeColorClasses } from "@common/lib/deploymentTypeColorClasses";
import { useListVanityDomains } from "api/vanityDomains";
import { useQuery } from "convex/react";
import udfs from "@common/udfs";
import { DeploymentProvider } from "components/projectSettings/CustomDomains";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { useContainerWidth } from "../hooks/useContainerWidth";

function DeploymentDomainInfo({
  deployment,
  whoseName,
}: {
  deployment: PlatformDeploymentResponse;
  whoseName: string | null;
}) {
  const team = useCurrentTeam();
  const deploymentInfo = useContext(DeploymentInfoContext);
  const canViewData =
    deploymentInfo?.useIsOperationAllowed("ViewData") ?? false;
  const hasEntitlement = !!useTeamEntitlements(team?.id)?.customDomainsEnabled;
  const domains = useListVanityDomains(
    hasEntitlement ? deployment?.name : undefined,
  );
  const vanityCloudDomains = domains?.filter(
    (d) => d.requestDestination === "convexCloud",
  );
  // Skip the system UDF when the member can't view data; otherwise the
  // query throws and the error escapes React's error boundary on the
  // next WebSocket notify (uncatchable in userland).
  const canonicalCloudUrl = useQuery(
    udfs.convexCloudUrl.default,
    canViewData ? undefined : "skip",
  );
  const vanityDomain =
    vanityCloudDomains?.find((d) => d.domain === canonicalCloudUrl) ||
    vanityCloudDomains?.[0];
  return (
    <DeploymentLabel
      deployment={deployment}
      whoseName={whoseName}
      vanityUrl={
        vanityDomain?.verificationTime ? vanityDomain.domain : undefined
      }
    />
  );
}

function DeploymentLabelWrapper({
  deployment,
  whoseName,
  deploymentName,
}: {
  deployment: PlatformDeploymentResponse;
  whoseName: string | null;
  deploymentName: string;
}) {
  return (
    <DeploymentProvider
      deploymentName={deploymentName}
      deploymentUrl={
        deployment.kind === "cloud" ? deployment.deploymentUrl : undefined
      }
    >
      <DeploymentDomainInfo deployment={deployment} whoseName={whoseName} />
    </DeploymentProvider>
  );
}

export function DeploymentDisplay({ project }: { project: ProjectDetails }) {
  const router = useRouter();

  const deploymentName =
    typeof router.query.deploymentName === "string"
      ? router.query.deploymentName
      : undefined;
  // The project's whole deployment list is what the switcher menu needs; this
  // label only needs the deployment the URL names, so fetch that one directly
  // rather than leaving the header empty until the list lands. The by-name
  // endpoint only serves cloud deployments, so local ones wait for the list.
  const { deployments } = useDeployments(project.id);
  const fetchByName =
    deployments === undefined && !deploymentName?.startsWith("local-");
  const deploymentFromName = useDeploymentByName(
    fetchByName ? deploymentName : undefined,
  );
  const fallbackDeployment =
    deploymentFromName?.projectId === project.id &&
    deploymentFromName?.name === deploymentName
      ? deploymentFromName
      : undefined;
  const deployment = useCurrentDeployment() ?? fallbackDeployment;
  const member = useProfile();

  useRememberLastViewedDeploymentForProject(project.slug, deployment?.name);

  const teamMembers = useTeamMembers(project.teamId);

  const isProdSelected = deployment?.deploymentType === "prod";
  const isPreview = deployment?.deploymentType === "preview";
  const isDoneLoading =
    isProdSelected || // prod deploys are straightforward
    isPreview || // preview deployments don't require additional info
    (deployment !== undefined &&
      member !== undefined &&
      teamMembers !== undefined); // info required to render dev deploy

  const whose = teamMembers?.find((tm) => tm.id === deployment?.creator);
  const profile = useProfile();
  const whoseName =
    whose?.email === profile?.email
      ? null
      : whose?.name || whose?.email || "Teammate";

  const isProjectSettings = router.route.endsWith("/[project]/settings");
  const isProvisionProd = router.route.endsWith(
    `/[project]/${PROVISION_PROD_PAGE_NAME}`,
  );
  const isProvisionDev = router.route.endsWith(
    `/[project]/${PROVISION_DEV_PAGE_NAME}`,
  );
  const isProvisionPage = isProvisionProd || isProvisionDev;
  const team = useCurrentTeam();
  const currentProject = useCurrentProject();

  return isProjectSettings ? (
    team && currentProject ? (
      <DeploymentLabelProjectSettings currentProject={currentProject} />
    ) : null
  ) : isProvisionPage ? (
    team && currentProject ? (
      <DeploymentLabelProvisionDeployment
        currentProject={currentProject}
        isProvisionProd={isProvisionProd}
      />
    ) : null
  ) : !isDoneLoading ? null : (
    <DeploymentLabelWrapper
      deployment={deployment}
      whoseName={whoseName}
      deploymentName={deployment.name}
    />
  );
}

export function DeploymentLabel({
  whoseName,
  deployment,
  vanityUrl,
}: {
  deployment: PlatformDeploymentResponse;
  whoseName: string | null;
  vanityUrl?: string;
}) {
  const team = useCurrentTeam();
  const project = useCurrentProject();
  const { openMenu, buttonRef, selected } =
    useDeploymentSwitcherTrigger(project);

  const [containerRef, containerWidth] = useContainerWidth<HTMLDivElement>();

  // Hysteresis buffer for domain display
  const DOMAIN_SHOW_THRESHOLD = 350;
  const DOMAIN_HIDE_THRESHOLD = 320;
  const [showDomain, setShowDomain] = useState(
    containerWidth > DOMAIN_SHOW_THRESHOLD,
  );

  useEffect(() => {
    if (showDomain && containerWidth < DOMAIN_HIDE_THRESHOLD) {
      setShowDomain(false);
    }
    if (!showDomain && containerWidth > DOMAIN_SHOW_THRESHOLD) {
      setShowDomain(true);
    }
  }, [containerWidth, showDomain]);

  // Estimate minimum widths (adjust as needed)
  const minTypeWidth = 100;
  const minDomainWidth = 180;
  const minNameWidth = 120;
  const padding = 48; // for icons, gaps, caret, etc.

  const showName =
    containerWidth > minTypeWidth + minDomainWidth + minNameWidth + padding;

  if (!team || !project) {
    return null;
  }
  return (
    <div
      ref={containerRef}
      className={cn(
        "my-2 flex min-w-24 grow overflow-visible p-px",
        "overflow-auto",
      )}
    >
      <Button
        variant="unstyled"
        id="select-deployment"
        data-testid="select-deployment"
        className={cn(
          "flex h-9.25 items-center gap-2 truncate rounded-full border text-sm font-medium transition-opacity hover:opacity-80",
          selected && "opacity-80",
          "focus-visible:ring-1 focus-visible:ring-border-selected focus-visible:outline-hidden",
          deploymentTypeColorClasses(deployment.deploymentType),
        )}
        type="button"
        ref={buttonRef}
        aria-haspopup="menu"
        aria-expanded={selected}
        tabIndex={0}
        onClick={openMenu}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") openMenu(e as any);
        }}
      >
        <div className="flex size-full animate-fadeInFromLoading cursor-pointer items-center gap-1 px-4">
          {deployment.deploymentType === "dev" ? (
            <CommandLineIcon className="size-4 min-w-4" />
          ) : deployment.deploymentType === "prod" ? (
            <SignalIcon className="size-4 min-w-4" />
          ) : deployment.deploymentType === "preview" ? (
            <Pencil2Icon className="size-4 min-w-4" />
          ) : deployment.deploymentType === "custom" ? (
            <WrenchIcon className="size-4 min-w-4" />
          ) : null}
          <span className="max-w-24 truncate sm:contents">
            {getDeploymentLabel({
              deployment,
              whoseName,
            })}
          </span>
          {showDomain && deployment.kind === "cloud" && vanityUrl && (
            <>
              <span
                className="animate-fadeInFromLoading px-0.5 font-normal"
                role="separator"
              >
                •
              </span>
              <span
                className="block max-w-60 animate-fadeInFromLoading truncate font-mono font-normal"
                title={vanityUrl}
              >
                {vanityUrl}
              </span>
            </>
          )}
          {showName && deployment.kind === "cloud" && (
            <>
              <span
                className="animate-fadeInFromLoading px-0.5 font-normal"
                role="separator"
              >
                •
              </span>
              <span className="animate-fadeInFromLoading font-mono font-normal">
                {deployment.name}
              </span>
            </>
          )}
          {showName && deployment.kind === "local" && (
            <>
              <span
                className="animate-fadeInFromLoading px-0.5 font-normal"
                role="separator"
              >
                •
              </span>
              <span className="animate-fadeInFromLoading font-mono text-sm font-normal">
                Port {deployment.port}
              </span>
            </>
          )}
          <CaretSortIcon
            className={cn(
              "ml-auto size-5 shrink-0",
              deploymentTypeColorClasses(deployment.deploymentType),
              "bg-transparent",
            )}
          />
        </div>
      </Button>
    </div>
  );
}

export function DeploymentLabelProjectSettings({
  currentProject,
}: {
  currentProject: ProjectDetails;
}) {
  const { openMenu, buttonRef, selected } =
    useDeploymentSwitcherTrigger(currentProject);

  return (
    <div
      key="projectSettings"
      className="my-2 mr-px flex grow items-stretch overflow-visible p-px"
    >
      <Button
        variant="unstyled"
        className={cn(
          "flex h-9.25 items-center gap-2 rounded-full px-3",
          "border bg-background-secondary text-content-primary",
          "truncate text-sm font-medium transition-opacity hover:bg-background-tertiary",
          selected && "border-border-selected bg-background-tertiary",
        )}
        ref={buttonRef}
        tabIndex={0}
        role="button"
        aria-haspopup="menu"
        aria-expanded={selected}
        onClick={openMenu}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") openMenu(e as any);
        }}
      >
        <GearIcon className="size-4 min-w-4" />
        <span className="max-w-24 truncate sm:contents">Project settings</span>
        <CaretSortIcon className="ml-auto size-5 bg-transparent" />
      </Button>
    </div>
  );
}

export function DeploymentLabelProvisionDeployment({
  currentProject,
  isProvisionProd,
}: {
  currentProject: ProjectDetails;
  isProvisionProd: boolean;
}) {
  const { openMenu, buttonRef, selected } =
    useDeploymentSwitcherTrigger(currentProject);

  return (
    <div
      key="provisionDeployment"
      className="my-2 mr-px flex grow items-stretch overflow-visible p-px"
    >
      <Button
        variant="unstyled"
        className={cn(
          "flex h-9.25 items-center gap-2 rounded-full px-3",
          "border border-dashed",
          "truncate text-sm font-medium transition-opacity hover:opacity-80",
          selected && "opacity-80",
          // eslint-disable-next-line better-tailwindcss/no-unknown-classes -- eslint-plugin-better-tailwindcss v4 incorrectly identifies "prod"/"dev" as class names
          deploymentTypeColorClasses(isProvisionProd ? "prod" : "dev"),
          "[--bg-opacity:50%]",
        )}
        ref={buttonRef}
        tabIndex={0}
        role="button"
        aria-haspopup="menu"
        aria-expanded={selected}
        onClick={openMenu}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") openMenu(e as any);
        }}
      >
        {isProvisionProd ? (
          <SignalIcon className="size-4 min-w-4" />
        ) : (
          <CommandLineIcon className="size-4 min-w-4" />
        )}
        <span className="max-w-32 truncate sm:contents">
          {isProvisionProd ? "Production" : "Development (Cloud)"}
        </span>
        <CaretSortIcon className="ml-auto size-5 bg-transparent" />
      </Button>
    </div>
  );
}

export function getDeploymentLabel({
  deployment,
  whoseName,
}: {
  deployment: PlatformDeploymentResponse;
  whoseName: string | null; // null = mine
}): string {
  if (
    (deployment.deploymentType === "custom" ||
      ("isDefault" in deployment &&
        !deployment.isDefault &&
        deployment.deploymentType !== "preview")) &&
    "reference" in deployment
  ) {
    return deployment.reference;
  }

  switch (deployment.deploymentType) {
    case "prod":
      return "Production";
    case "preview":
      return `Preview: ${deployment.previewIdentifier || "Unknown"}`;
    case "dev": {
      if (deployment.kind === "local") {
        return deployment.deviceName;
      }
      return whoseName === null ? "Development (Cloud)" : `${whoseName}’s Dev`;
    }
    case "custom":
      return "Custom";
    default: {
      deployment.deploymentType satisfies never;
      return "";
    }
  }
}

function useDeploymentSwitcherTrigger(
  project: ProjectDetails | null | undefined,
) {
  const buttonRef = useRef<HTMLButtonElement>(null);
  const openCommandPalette = useOpenCommandPalette();
  const [isPaletteOpen] = useCommandPaletteOpen();
  const [paletteAnchor] = useCommandPaletteAnchor();
  const { trackOpened } = usePaletteAnalytics();

  return {
    buttonRef,
    openMenu: (e: React.MouseEvent) => {
      e.preventDefault();
      const button = buttonRef.current;
      if (!button || !project) {
        return;
      }
      trackOpened("deployment-selector");
      const rect = button.getBoundingClientRect();
      openCommandPalette({
        pages: [{ type: "deployments", project }],
        anchor: {
          left: rect.left,
          top: rect.bottom + 8,
          source: "deployment-switcher",
        },
      });
    },
    selected: isPaletteOpen && paletteAnchor?.source === "deployment-switcher",
  };
}
