import { Card } from "elements/Card";
import { cn } from "@ui/cn";
import { Tooltip } from "@ui/Tooltip";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { TeamMemberLink } from "elements/TeamMemberLink";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { MemberResponse } from "generatedApi";
import { deploymentTypeColorClasses } from "@common/lib/deploymentTypeColorClasses";
import {
  CommandLineIcon,
  SignalIcon,
  WrenchIcon,
} from "@heroicons/react/24/outline";
import { Pencil2Icon } from "@radix-ui/react-icons";
import { useProjectById } from "api/projects";
import { HighlightMatch } from "elements/HighlightMatch";

// Deployments created after this date have deploy time tracking,
// so missing lastDeployTime means "never deployed" rather than unknown.
const DEPLOY_TRACKING_CUTOFF = new Date("2026-03-01T00:00:00Z").getTime();

function deploymentTypeLabel(deployment: PlatformDeploymentResponse): string {
  switch (deployment.deploymentType) {
    case "prod":
      return "Production";
    case "dev":
      return "Development";
    case "preview":
      return "Preview";
    case "custom":
      return "Custom";
    default:
      return "";
  }
}

export function DeploymentRow({
  deployment,
  teamSlug,
  teamMembers,
  showProject = true,
  listItem,
  searchQuery,
}: {
  deployment: PlatformDeploymentResponse;
  teamSlug: string;
  teamMembers?: MemberResponse[];
  showProject?: boolean;
  listItem?: boolean;
  searchQuery?: string;
}) {
  const { project } = useProjectById(deployment.projectId);
  const creator = teamMembers?.find((tm) => tm.id === deployment.creator);
  const creatorName = creator?.name || creator?.email || "Unknown";

  const projectSlug = project?.slug ?? "";
  const projectName = project?.name?.length ? project.name : projectSlug;
  const href =
    deployment.kind === "cloud"
      ? `/t/${teamSlug}/${projectSlug}/${deployment.name}`
      : undefined;

  return (
    <Card
      cardClassName="group animate-fadeInFromLoading"
      contentClassName="!py-2"
      href={href}
      linkLabel={projectName}
      listItem={listItem}
    >
      <div className="flex items-center gap-3 overflow-hidden">
        {/* Project name section */}
        {showProject && (
          <div className="w-56 shrink-0">
            <div className="truncate text-sm font-semibold text-content-primary">
              {projectName}
            </div>
            <div className="truncate text-xs text-content-secondary">
              {projectSlug}
            </div>
          </div>
        )}

        {/* Deployment type icon */}
        <Tooltip tip={deploymentTypeLabel(deployment)}>
          <div
            className={cn(
              "relative z-10 inline-flex shrink-0 items-center justify-center self-center rounded-full p-1 ring-3 ring-background-secondary",
              deploymentTypeColorClasses(deployment.deploymentType),
            )}
          >
            <DeploymentIcon deployment={deployment} />
          </div>
        </Tooltip>

        {/* Reference + deployment name */}
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <div className="flex items-center gap-2">
            {deployment.kind === "cloud" && deployment.reference && (
              <span className="truncate text-sm font-semibold text-content-primary">
                <HighlightMatch
                  text={deployment.reference}
                  query={searchQuery}
                />
              </span>
            )}
            {deployment.kind === "local" && (
              <span className="truncate text-sm font-semibold text-content-primary">
                Port {deployment.port}
              </span>
            )}
          </div>

          {/* Second line: deployment name */}
          {deployment.kind === "cloud" && (
            <span className="truncate text-xs text-content-secondary">
              <HighlightMatch text={deployment.name} query={searchQuery} />
            </span>
          )}
        </div>

        {/* Right-aligned: timestamps + creator */}
        <div className="ml-auto flex min-w-max shrink-0 flex-col items-end gap-0.5 text-xs whitespace-nowrap text-content-secondary">
          {deployment.kind === "cloud" && deployment.lastDeployTime ? (
            <TimestampDistance
              date={new Date(deployment.lastDeployTime)}
              prefix="Deployed"
            />
          ) : deployment.kind === "cloud" ? (
            deployment.createTime >= DEPLOY_TRACKING_CUTOFF ? (
              <span>Never deployed</span>
            ) : null
          ) : null}
          <div className="flex items-center gap-1">
            <TimestampDistance
              date={new Date(deployment.createTime)}
              prefix="Created"
            />
            {deployment.creator && (
              <span className="relative z-10 flex items-center gap-1">
                <span>by</span>
                <TeamMemberLink
                  memberId={deployment.creator}
                  name={creatorName}
                  isMember={!!creator}
                />
              </span>
            )}
          </div>
        </div>
      </div>
    </Card>
  );
}

function DeploymentIcon({
  deployment,
}: {
  deployment: PlatformDeploymentResponse;
}) {
  if (deployment.deploymentType === "dev") {
    return <CommandLineIcon className="size-3.5" />;
  }
  if (deployment.deploymentType === "prod") {
    return <SignalIcon className="size-3.5" />;
  }
  if (deployment.deploymentType === "preview") {
    return <Pencil2Icon className="size-3.5" />;
  }
  if (deployment.deploymentType === "custom") {
    return <WrenchIcon className="size-3.5" />;
  }
  return null;
}
