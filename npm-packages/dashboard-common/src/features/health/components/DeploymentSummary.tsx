import { Sheet } from "@ui/Sheet";
import { cn } from "@ui/cn";
import { useQuery } from "convex/react";
import udfs from "@common/udfs";
import {
  LiveTimestampDistanceInner,
  TimestampDistance,
} from "@common/elements/TimestampDistance";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import {
  ClockIcon,
  CommandLineIcon,
  SignalIcon,
  WrenchIcon,
} from "@heroicons/react/24/outline";
import {
  GlobeIcon,
  Pencil2Icon,
  RocketIcon,
  ArchiveIcon,
  CodeIcon,
  DimensionsIcon,
  ExternalLinkIcon,
  QuestionMarkCircledIcon,
} from "@radix-ui/react-icons";
import { PackageVersionIcon } from "@common/elements/icons";
import { Link } from "@ui/Link";
import { useContext, useEffect, useState } from "react";
import semver from "semver";
import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";
import { Spinner } from "@ui/Spinner";
import {
  DeploymentInfoContext,
  PermissionsContext,
} from "@common/lib/deploymentContext";
import {
  deploymentTypeColorClasses,
  deploymentTypeLabel,
} from "@common/lib/deploymentTypeColorClasses";
import {
  IndexCreationStatus,
  SchemaValidationStatus,
  useDeployProgress,
} from "@common/features/health/components/DeployProgress";

function useLatestConvexVersion(currentVersion: string | undefined) {
  const [hasUpdate, setHasUpdate] = useState(false);
  const [latestVersion, setLatestVersion] = useState<string>();

  useEffect(() => {
    let isMounted = true;

    async function checkVersion() {
      try {
        const response = await fetch(
          "https://registry.npmjs.org/convex/latest",
        );
        if (!response.ok) return;

        const data = await response.json();
        if (!isMounted) return;

        setLatestVersion(data.version);

        if (currentVersion && data.version) {
          const currentSemver = semver.parse(currentVersion);
          const latestSemver = semver.parse(data.version);
          if (!currentSemver || !latestSemver) return;
          const isHigherMinor =
            latestSemver.major === currentSemver.major &&
            latestSemver.minor > currentSemver.minor;
          const isHigherMajor = latestSemver.major > currentSemver.major;
          const hasNewVersion = isHigherMajor || isHigherMinor;
          setHasUpdate(hasNewVersion);
        }
      } catch {
        // Swallow any errors and don't show update notice
      }
    }

    if (currentVersion) {
      void checkVersion();
    }

    return () => {
      isMounted = false;
    };
  }, [currentVersion]);

  return { hasUpdate, latestVersion };
}

function DeploymentIcon({
  deployment,
  className,
}: {
  deployment: PlatformDeploymentResponse;
  className?: string;
}) {
  if (deployment.deploymentType === "dev") {
    return (
      <CommandLineIcon
        className={className}
        aria-label="Development deployment"
      />
    );
  }
  if (deployment.deploymentType === "prod") {
    return (
      <SignalIcon className={className} aria-label="Production deployment" />
    );
  }
  if (deployment.deploymentType === "preview") {
    return (
      <Pencil2Icon className={className} aria-label="Preview deployment" />
    );
  }
  if (deployment.deploymentType === "custom") {
    return <WrenchIcon className={className} aria-label="Custom deployment" />;
  }
  return null;
}

export function DeploymentSummary({
  deployment,
  teamSlug,
  projectSlug,
  lastBackupTime,
  teamMembers,
  regions,
  showDeployProgress = false,
}: {
  deployment: PlatformDeploymentResponse;
  teamSlug: string;
  projectSlug: string;
  lastBackupTime?: number | null;
  teamMembers?: Array<{ id: number; name?: string | null; email: string }>;
  regions?: Array<{ name: string; displayName: string }>;
  showDeployProgress?: boolean;
}) {
  const { TeamMemberLink, healthDeployProgressEnabled } = useContext(
    DeploymentInfoContext,
  );
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canViewData = useIsOperationAllowed("ViewData");
  const canViewBackups = useIsOperationAllowed("ViewBackups");
  const deployProgress = useDeployProgress(
    showDeployProgress && healthDeployProgressEnabled,
  );

  const lastPushEvent = useQuery(
    udfs.deploymentEvents.lastPushEvent,
    canViewData ? {} : "skip",
  );

  // Resolve the team member who last deployed from the push event
  const deployer = teamMembers?.find(
    (tm) => lastPushEvent && tm.id === Number(lastPushEvent.member_id),
  );
  const deployerId = lastPushEvent
    ? Number(lastPushEvent.member_id) || undefined
    : undefined;
  const deployerName = deployer?.name || deployer?.email || undefined;

  const convexCloudUrl = useQuery(
    udfs.convexCloudUrl.default,
    canViewData ? {} : "skip",
  );
  const convexSiteUrl = useQuery(
    udfs.convexSiteUrl.default,
    canViewData ? {} : "skip",
  );
  const serverVersion = useQuery(
    udfs.getVersion.default,
    canViewData ? undefined : "skip",
  );
  const { hasUpdate, latestVersion } = useLatestConvexVersion(
    serverVersion || undefined,
  );

  const backupSettingsUrl = `/t/${teamSlug}/${projectSlug}/${deployment.name}/settings/backups`;

  // Get display name for region
  const regionDisplayName =
    deployment.kind === "cloud"
      ? regions?.find((r) => r.name === deployment.region)?.displayName ||
        deployment.region
      : undefined;

  // Check if we're still loading critical data. Queries we deliberately
  // skipped (no `deployment:data:view`) return `undefined`, so exclude
  // them from the loading gate.
  const isLoading =
    canViewData &&
    (lastPushEvent === undefined ||
      serverVersion === undefined ||
      (deployment.kind === "cloud" &&
        (convexCloudUrl === undefined || convexSiteUrl === undefined)));

  if (isLoading) {
    return (
      <Sheet className="flex w-fit flex-col bg-transparent" padding={false}>
        <div className="flex min-h-30 min-w-lg items-center justify-center rounded-lg bg-background-secondary p-2 py-3">
          <div>
            <Spinner className="size-8" />
          </div>
        </div>
      </Sheet>
    );
  }

  // The Cloud URL panel only renders when (1) the deployment is cloud
  // and (2) the member can view data (the URL queries are
  // ViewData-gated). When the panel is hidden, round all corners of
  // the main panel.
  const showUrlPanel = deployment.kind === "cloud" && canViewData;
  const mainPanelRounding = showUrlPanel
    ? "rounded-l-lg rounded-tr-lg rounded-bl-none lg:flex-1 lg:rounded-tr-none lg:rounded-bl-lg"
    : "rounded-lg";

  return (
    <Sheet className="flex w-fit flex-col bg-transparent" padding={false}>
      <div className="flex flex-col lg:flex-row">
        {/* Main deployment info */}
        <div
          className={cn(
            "flex flex-col gap-4 bg-background-secondary p-2 py-3 lg:pr-4",
            mainPanelRounding,
          )}
        >
          {/* Row 1: Type + Reference + Name (always together) */}
          <div className="flex flex-wrap items-center gap-2">
            <div
              className={cn(
                "inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium",
                deploymentTypeColorClasses(deployment.deploymentType),
              )}
            >
              <DeploymentIcon deployment={deployment} className="size-3.5" />
              <span>{deploymentTypeLabel(deployment.deploymentType)}</span>
            </div>
            {deployment.kind === "cloud" ? (
              <div className="flex flex-wrap gap-x-1.5 text-sm text-content-primary">
                <strong className="font-medium">{deployment.reference}</strong>{" "}
                <span className="font-mono text-content-secondary">
                  ({deployment.name})
                </span>
              </div>
            ) : (
              <span className="font-mono text-sm text-content-secondary">
                {deployment.name}
              </span>
            )}
          </div>

          {/* Row 2: Region/Port + Class + Version */}
          <div className="flex flex-wrap items-center gap-6">
            {/* Region + Class stay together when wrapping */}
            <div className="flex items-center gap-6">
              {/* Region (cloud) or Port (local) */}
              {deployment.kind === "cloud" && (
                <div className="flex items-center gap-2">
                  <Tooltip tip="Deployment region">
                    <GlobeIcon
                      className="size-4 shrink-0 text-content-secondary"
                      aria-label="Region"
                    />
                  </Tooltip>
                  <div className="text-sm text-content-primary">
                    {regionDisplayName}
                  </div>
                </div>
              )}
              {deployment.kind === "local" && (
                <div className="flex items-center gap-2">
                  <Tooltip tip="Local port">
                    <CodeIcon
                      className="size-4 shrink-0 text-content-secondary"
                      aria-label="Port"
                    />
                  </Tooltip>
                  <div className="text-sm text-content-primary">
                    Port {deployment.port}
                  </div>
                </div>
              )}

              {/* Deployment Class (cloud only) */}
              {deployment.kind === "cloud" && (
                <div className="flex items-center gap-2">
                  <Tooltip
                    tip={
                      <span className="flex items-center gap-1">
                        Deployment class
                        <Link
                          href="https://docs.convex.dev/production/state/limits"
                          target="_blank"
                          rel="noopener noreferrer"
                        >
                          <QuestionMarkCircledIcon className="size-3.5" />
                        </Link>
                      </span>
                    }
                  >
                    <DimensionsIcon
                      className="size-4 shrink-0 text-content-secondary"
                      aria-label="Deployment class"
                    />
                  </Tooltip>
                  <div className="text-sm text-content-primary">
                    {deployment.class.toUpperCase()}
                  </div>
                </div>
              )}
            </div>

            {/* Convex Version */}
            {lastPushEvent && (
              <div className="flex items-center gap-2">
                <Tooltip tip="Convex package version">
                  <PackageVersionIcon
                    className="text-content-secondary"
                    aria-label="Convex package version"
                  />
                </Tooltip>
                <div className="flex items-center">
                  <span className="text-sm text-content-primary">
                    Convex {serverVersion}
                  </span>
                  {hasUpdate && (
                    <Button
                      tip={`A ${
                        serverVersion && latestVersion
                          ? latestVersion.split(".")[0] !==
                            serverVersion.split(".")[0]
                            ? "major"
                            : latestVersion.split(".")[1] !==
                                serverVersion.split(".")[1]
                              ? "minor"
                              : "patch"
                          : ""
                      } update is available for Convex (${serverVersion} → ${latestVersion})`}
                      size="xs"
                      inline
                      aria-label="Convex NPM Package Upgrade Available"
                      href="https://github.com/convex/convex-js/blob/main/CHANGELOG.md#changelog"
                      target="_blank"
                      // eslint-disable-next-line no-restricted-syntax -- manual Link-Button hybrid implementation
                      className="h-5 text-content-link"
                    >
                      <div>({latestVersion} available)</div>
                    </Button>
                  )}
                </div>
              </div>
            )}
          </div>

          {/* Row 3: Last Deployed + Last Backup */}
          <div className="flex flex-wrap items-center gap-6 gap-y-4">
            {/* Last Deployed — hidden when the member can't view data
                since the underlying query is `deployment:data:view`
                gated and we'd otherwise misreport "Never deployed". */}
            {canViewData && (
              <div className="flex items-center gap-2">
                <Tooltip tip="Last deployment">
                  <RocketIcon
                    className="size-4 shrink-0 text-content-secondary"
                    aria-label="Last deployment"
                  />
                </Tooltip>
                {!lastPushEvent ? (
                  <span className="text-sm text-content-secondary">
                    Never deployed
                  </span>
                ) : (
                  <div className="flex flex-wrap items-center gap-1 text-sm text-content-primary">
                    <span>Last deployed</span>
                    <TimestampDistance
                      date={new Date(lastPushEvent._creationTime)}
                      className="text-sm text-content-primary"
                    />
                    {deployerId !== undefined && deployerName && (
                      <>
                        <span>by</span>
                        <TeamMemberLink
                          memberId={deployerId}
                          name={deployerName}
                        />
                      </>
                    )}
                  </div>
                )}
              </div>
            )}

            {/* Last Backup (only for cloud deployments). Loads
                independently from the rest of the summary — render a
                small placeholder while the backup list is in flight so
                the panel doesn't reflow when it resolves. */}
            {deployment.kind === "cloud" && canViewBackups && (
              <div className="flex items-center gap-2">
                <Tooltip tip="Last backup">
                  <ArchiveIcon
                    className="size-4 shrink-0 text-content-secondary"
                    aria-label="Last backup"
                  />
                </Tooltip>
                <div className="flex items-center gap-1">
                  {lastBackupTime === undefined ? (
                    <Spinner className="size-4" />
                  ) : lastBackupTime === null ? (
                    deployment.class.startsWith("d") ? (
                      <span className="text-sm text-content-primary">
                        Backup every 24 hours
                      </span>
                    ) : (
                      <span className="text-sm text-content-secondary">
                        No backup yet
                      </span>
                    )
                  ) : (
                    <>
                      <span className="text-sm text-content-primary">
                        Last backup created{" "}
                      </span>
                      <TimestampDistance
                        date={new Date(lastBackupTime)}
                        className="text-sm text-content-primary"
                      />
                    </>
                  )}
                  <Link
                    href={backupSettingsUrl}
                    aria-label="View backup settings"
                  >
                    <ExternalLinkIcon className="size-3.5" />
                  </Link>
                </div>
              </div>
            )}
          </div>

          {/* Row 4: Work the last deploy is still doing. Each line unfolds
              its progress inline on hover rather than in a popover. */}
          {deployProgress.schema && (
            <SchemaValidationStatus schema={deployProgress.schema} />
          )}
          {deployProgress.indexes.length > 0 && (
            <IndexCreationStatus indexes={deployProgress.indexes} />
          )}

          {/* Row 5: Expiry warning (ephemeral deployments) */}
          {deployment.kind === "cloud" && deployment.expiresAt && (
            <div className="flex items-center gap-2">
              <Tooltip tip="This deployment will be automatically deleted">
                <ClockIcon
                  className="size-4 shrink-0 text-content-warning"
                  aria-label="Expiry"
                />
              </Tooltip>
              <Tooltip
                tip={new Date(deployment.expiresAt).toLocaleString(undefined, {
                  timeZoneName: "short",
                })}
              >
                <span className="text-sm text-content-warning">
                  Will expire on{" "}
                  {new Date(deployment.expiresAt).toLocaleDateString(
                    undefined,
                    { month: "short", day: "numeric", year: "numeric" },
                  )}{" "}
                  (
                  <LiveTimestampDistanceInner
                    date={new Date(deployment.expiresAt)}
                  />
                  )
                </span>
              </Tooltip>
            </div>
          )}
        </div>

        {/* Deployment URLs (cloud only, hidden when the member lacks
            `deployment:data:view` since the URL queries are gated). */}
        {showUrlPanel && (
          <div className="flex flex-col justify-start gap-4 rounded-b-lg border-t bg-background-secondary/70 p-2 py-4 lg:rounded-r-lg lg:rounded-bl-none lg:border-t-0 lg:border-l lg:py-3 lg:pl-4">
            <div className="flex flex-col gap-1">
              <span className="text-xs font-medium text-content-secondary">
                Cloud URL
              </span>
              <Link
                href={convexCloudUrl!}
                target="_blank"
                rel="noopener noreferrer"
                className="font-mono text-xs break-all"
                noUnderline
              >
                {convexCloudUrl}
              </Link>
            </div>
            <div className="flex flex-col gap-1">
              <span className="text-xs font-medium text-content-secondary">
                HTTP Actions URL
              </span>
              <Link
                href={convexSiteUrl!}
                target="_blank"
                rel="noopener noreferrer"
                className="font-mono text-xs break-all"
                noUnderline
              >
                {convexSiteUrl}
              </Link>
            </div>
          </div>
        )}
      </div>
    </Sheet>
  );
}
