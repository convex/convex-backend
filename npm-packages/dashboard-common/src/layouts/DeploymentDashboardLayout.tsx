import {
  FileIcon,
  TableIcon,
  CodeIcon,
  StopwatchIcon,
  CounterClockwiseClockIcon,
  TextAlignBottomIcon,
  GearIcon,
  InfoCircledIcon,
  CubeIcon,
} from "@radix-ui/react-icons";
import { useQuery } from "convex/react";
import { Link } from "@ui/Link";
import { JSX, useContext, useState, useEffect } from "react";
import udfs from "@common/udfs";
import classNames from "classnames";
import {
  PermissionsProvider,
  PermissionsContext,
  DeploymentInfoContext,
} from "@common/lib/deploymentContext";
import { useGlobalLocalStorage } from "@common/lib/useGlobalLocalStorage";
import { useCollapseSidebarState } from "@common/lib/useCollapseSidebarState";
import { PulseIcon } from "@common/elements/icons";
import {
  Sidebar,
  SidebarGroup,
  useCurrentPage,
} from "@common/elements/Sidebar";
import { FunctionRunnerWrapper } from "@common/features/functionRunner/components/FunctionRunnerWrapper";
import { FunctionsProvider } from "@common/lib/functions/FunctionsProvider";
import { useIsGlobalRunnerShown } from "@common/features/functionRunner/lib/functionRunner";
import { useIsCloudDeploymentInSelfHostedDashboard } from "@common/lib/useIsCloudDeploymentInSelfHostedDashboard";
import { Tooltip } from "@ui/Tooltip";
import Image from "next/image";
import { ClosePanelButton } from "@ui/ClosePanelButton";
import { UsageLimitDisabledBanner } from "@common/features/settings/components/UsageLimitDisabledBanner";

type LayoutProps = {
  children: JSX.Element;
  auditLogsEnabled?: boolean;
  visiblePages?: string[];
  onRanCustomQuery?: () => void;
  onCopiedQueryResult?: () => void;
};

export function DeploymentDashboardLayout({
  children,
  auditLogsEnabled = true,
  visiblePages,
  onRanCustomQuery,
  onCopiedQueryResult,
}: LayoutProps) {
  const [collapsed, setCollapsed] = useCollapseSidebarState();
  const [isGlobalRunnerVertical, setIsGlobalRunnerVertical] =
    useGlobalLocalStorage("functionRunnerOrientation", false);
  const [isRunnerExpanded, setIsRunnerExpanded] = useState(false);
  const isGlobalRunnerShown = useIsGlobalRunnerShown();
  const { deploymentsURI: uriPrefix } = useContext(DeploymentInfoContext);
  const { canViewDataCached } = useContext(PermissionsContext);
  const { isCloudDeploymentInSelfHostedDashboard, deploymentName } =
    useIsCloudDeploymentInSelfHostedDashboard();

  const allExploreDeploymentPages = [
    {
      key: "health",
      label: "Health",
      Icon: PulseIcon,
      href: `${uriPrefix}/`,
    },
    {
      key: "data",
      label: "Data",
      Icon: TableIcon,
      href: `${uriPrefix}/data`,
    },
    {
      key: "schema",
      label: "Schema",
      Icon: CubeIcon,
      href: `${uriPrefix}/schema`,
    },
    {
      key: `functions`,
      label: "Functions",
      Icon: CodeIcon,
      href: `${uriPrefix}/functions`,
    },
    {
      key: "files",
      label: "Files",
      Icon: FileIcon,
      href: `${uriPrefix}/files`,
    },
    {
      key: "schedules",
      label: "Schedules",
      Icon: StopwatchIcon,
      href: `${uriPrefix}/schedules/functions`,
    },
    {
      key: "logs",
      label: "Logs",
      Icon: (props: any) => (
        <TextAlignBottomIcon
          {...props}
          style={{ marginBottom: "2px", marginTop: "-2px" }}
        />
      ),
      href: `${uriPrefix}/logs`,
    },
  ];

  // Filter tabs based on visiblePages if provided
  const exploreDeploymentPages = visiblePages
    ? allExploreDeploymentPages.filter((page) =>
        visiblePages.includes(page.key),
      )
    : allExploreDeploymentPages;

  const allConfigureItems = [
    {
      key: "history",
      label: "History",
      Icon: CounterClockwiseClockIcon,
      href: isCloudDeploymentInSelfHostedDashboard
        ? `https://dashboard.convex.dev/d/${deploymentName}/history`
        : `${uriPrefix}/history`,
      target: isCloudDeploymentInSelfHostedDashboard ? "_blank" : undefined,
      muted: !auditLogsEnabled,
      tooltip: auditLogsEnabled
        ? undefined
        : "Deployment history is only available on the Pro plan.",
    },
    {
      key: "settings",
      label: "Settings",
      Icon: GearIcon,
      href: `${uriPrefix}/settings`,
    },
  ];

  // Filter configure items based on visiblePages if provided
  const configureItems = visiblePages
    ? allConfigureItems.filter((item) => visiblePages.includes(item.key))
    : allConfigureItems;

  const sidebarItems: SidebarGroup[] = [
    {
      key: "explore",
      items: exploreDeploymentPages,
    },
    {
      key: "configure",
      items: configureItems,
    },
  ].filter((group) => group.items.length > 0);

  return (
    <PermissionsProvider>
      <FunctionsProvider>
        <div className="flex h-full grow flex-col overflow-y-hidden">
          {(visiblePages === undefined ||
            visiblePages.includes("settings")) && <DeploymentBanners />}
          <MobileBanner />
          <div className="flex h-full flex-col overflow-y-auto sm:flex-row">
            {sidebarItems.length > 0 && (
              <Sidebar
                collapsed={!!collapsed}
                setCollapsed={setCollapsed}
                items={sidebarItems}
                header={
                  process.env.NEXT_PUBLIC_HIDE_HEADER ? (
                    <EmbeddedConvexLogo collapsed={!!collapsed} />
                  ) : undefined
                }
              />
            )}
            <div
              className={classNames(
                "flex w-full grow overflow-x-hidden",
                !isGlobalRunnerVertical && "flex-col",
              )}
            >
              {/* If the function runner is fully expanded, hide the content */}
              <div
                className={
                  isRunnerExpanded && isGlobalRunnerShown
                    ? "size-0"
                    : "scrollbar size-full overflow-x-auto"
                }
              >
                {children}
              </div>
              {canViewDataCached && (
                <FunctionRunnerWrapper
                  setIsVertical={setIsGlobalRunnerVertical}
                  isVertical={!!isGlobalRunnerVertical}
                  isExpanded={isRunnerExpanded}
                  setIsExpanded={setIsRunnerExpanded}
                  onRanCustomQuery={onRanCustomQuery}
                  onCopiedQueryResult={onCopiedQueryResult}
                />
              )}
            </div>
          </div>
        </div>
      </FunctionsProvider>
    </PermissionsProvider>
  );
}

function DeploymentBanners() {
  const { canViewDataCached } = useContext(PermissionsContext);
  const backendState = useQuery(
    udfs.backendState.backendState,
    canViewDataCached ? {} : "skip",
  );

  const { useCurrentTeam, useCurrentUsageBanner, deploymentsURI } = useContext(
    DeploymentInfoContext,
  );
  const team = useCurrentTeam();
  const teamUsageBanner = useCurrentUsageBanner(team?.id ?? null);

  const isPaused =
    backendState?.system === "none" &&
    backendState?.user === "paused" &&
    teamUsageBanner !== "Paused";
  const isUsageLimitDisabled = backendState?.usage_limit === "disabled";

  return (
    <>
      {isPaused && (
        <div className="border-y bg-background-error py-2 text-center text-content-error">
          This deployment is paused. Resume your deployment on the{" "}
          <Link passHref href={`${deploymentsURI}/settings/pause-deployment`}>
            settings
          </Link>{" "}
          page.
        </div>
      )}
      {isUsageLimitDisabled && (
        <UsageLimitDisabledBanner
          usageLimitsUri={`${deploymentsURI}/settings/usage-limits`}
        />
      )}
    </>
  );
}

function MobileBanner() {
  const isMobile = useMobileDetection();
  const [isDismissed, setIsDismissed] = useGlobalLocalStorage(
    "mobileBannerDismissed",
    false,
  );
  const { isSelfHosted } = useContext(DeploymentInfoContext);

  if (!isMobile || isDismissed || isSelfHosted) {
    return null;
  }

  return (
    <div className="flex items-center justify-between gap-2 border-b bg-util-accent/10 px-4 py-2 text-sm dark:bg-util-accent/30">
      <div className="flex items-center gap-2">
        <InfoCircledIcon className="size-4 shrink-0" />
        <p>
          The Convex dashboard is designed for desktop. We recommend using a
          desktop browser for the best experience.
        </p>
      </div>
      <ClosePanelButton onClose={() => setIsDismissed(true)} />
    </div>
  );
}

function EmbeddedConvexLogo({ collapsed }: { collapsed: boolean }) {
  const currentPage = useCurrentPage();
  const { deploymentName } = useIsCloudDeploymentInSelfHostedDashboard();

  const href = deploymentName
    ? `https://dashboard.convex.dev/d/${deploymentName}/${currentPage ?? ""}`
    : "https://dashboard.convex.dev";

  return (
    <>
      {/* Vertical layout on small screens */}
      <div className="mr-2 sm:hidden">
        <Tooltip tip="Convex" side="bottom" asChild>
          <a
            className="flex h-full items-center"
            href={href}
            target="_blank"
            rel="noreferrer"
          >
            <Image
              src="/convex-logo-only.svg"
              width="24"
              height="24"
              alt="Convex logo"
            />
          </a>
        </Tooltip>
      </div>

      {/* Horizontal layout on larger screens, with some text when not collapsed */}
      <div className="hidden sm:block">
        <Tooltip tip={collapsed && "Convex"} side="bottom" asChild>
          <a
            href={href}
            target="_blank"
            rel="noreferrer"
            className={
              collapsed
                ? "flex w-full justify-center"
                : "flex items-center gap-2 px-1.5 py-0.5"
            }
          >
            <Image
              src="/convex-logo-only.svg"
              width={collapsed ? 24 : 18}
              height={collapsed ? 24 : 18}
              alt="Convex logo"
            />
            {!collapsed && <div className="text-sm font-medium">Convex</div>}
          </a>
        </Tooltip>
      </div>
    </>
  );
}

function useMobileDetection(): boolean {
  const [isMobile, setIsMobile] = useState(false);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    const mediaQuery = window.matchMedia("(max-width: 450px)");

    setIsMobile(mediaQuery.matches);

    const handler = (e: MediaQueryListEvent) => setIsMobile(e.matches);
    mediaQuery.addEventListener("change", handler);

    return () => mediaQuery.removeEventListener("change", handler);
  }, []);

  return isMobile;
}
