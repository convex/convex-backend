import {
  TrashIcon,
  GearIcon,
  LayersIcon,
  PlusIcon,
  Pencil2Icon,
} from "@radix-ui/react-icons";
import {
  CommandLineIcon,
  SignalIcon,
  WrenchIcon,
} from "@heroicons/react/24/outline";
import { CardMenu, CardProps } from "elements/Card";
import { Tooltip } from "@ui/Tooltip";
import { Loading } from "@ui/Loading";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { useDeploymentUris } from "hooks/useDeploymentUris";
import { cn } from "@ui/cn";
import Link from "next/link";
import { ReactNode, useEffect, useState } from "react";
import { ProjectDetails } from "generatedApi";
import { DeploymentType } from "@convex-dev/platform/managementApi";
import { deploymentTypeLabel } from "@common/lib/deploymentTypeColorClasses";
import {
  LastViewedDeployment,
  useLastViewedNonDefaultDeployment,
} from "hooks/useLastViewed";
import { useDeploymentByName } from "api/deployments";
import {
  useHasCustomRolePermission,
  useHasProjectAdminPermissions,
  useMyCustomRoles,
} from "api/roles";
import { useCurrentTeam } from "api/teams";
import { useProfile } from "api/profile";
import { defaultDevDeploymentReference } from "lib/deploymentReference";
import { projectResource } from "lib/permissions";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { HighlightMatch } from "elements/HighlightMatch";
import { DeleteProjectModal } from "./modals/DeleteProjectModal";
import { ProjectTags } from "./ProjectTags";

const TOOLTIP_DELAY_MS = 500;

export function ProjectCard({
  project,
  listItem,
  searchQuery,
}: {
  project: ProjectDetails;
  listItem?: boolean;
  searchQuery?: string;
}) {
  const { id, slug, name } = project;
  const team = useCurrentTeam();

  const [deleteModal, setDeleteModal] = useState(false);

  const {
    prodHref,
    devHref,
    isProdDefault,
    prodDeploymentName,
    devDeploymentName,
    prodDeploymentReference,
    devDeploymentReference,
    hasDefaultProdDeployment,
    hasDefaultDevDeployment,
    generateHref,
    isLoading: isLoadingDeployments,
  } = useDeploymentUris(id, slug);

  const [lastViewed, setLastViewed] = useLastViewedNonDefaultDeployment(id);
  const suggested =
    lastViewed &&
    lastViewed.name !== prodDeploymentName &&
    lastViewed.name !== devDeploymentName
      ? lastViewed
      : undefined;
  // The pill shows the stored reference right away but only links to the
  // deployment once Big Brain confirms the member can still open it.
  const { deployment: fetchedDeployment, notFound } = useDeploymentByName(
    suggested && !suggested.deleted ? suggested.name : undefined,
  );

  const suggestedDeployment =
    fetchedDeployment?.name === suggested?.name ? fetchedDeployment : undefined;
  useEffect(() => {
    if (notFound && lastViewed && !lastViewed.deleted) {
      setLastViewed({ ...lastViewed, deleted: true });
    }
  }, [notFound, lastViewed, setLastViewed]);
  const recentDeployment: RecentDeployment | undefined = suggested && {
    name: suggested.name,
    reference:
      suggestedDeployment?.kind === "cloud"
        ? suggestedDeployment.reference
        : suggested.reference,
    deploymentType: suggested.deploymentType,
    status:
      suggested.deleted || notFound
        ? "deleted"
        : suggestedDeployment
          ? "available"
          : "loading",
  };
  const recentDeploymentHref =
    recentDeployment?.status === "available"
      ? generateHref(recentDeployment.name)
      : undefined;

  const profile = useProfile();

  const hasAdminPermissions = useHasProjectAdminPermissions(project.id);

  // For custom-role members the backend nulls out
  // `prodDeploymentName`/`devDeploymentName` for deployments they can't view,
  // so a missing default deployment may exist and be hidden. They only see
  // pills for the default deployments they can view. Project admins can view
  // every deployment in the project, so a missing one really is missing and
  // they get provision links like built-in members.
  const myRoles = useMyCustomRoles(team?.id);
  const isCustomRoleMember = myRoles?.role === "custom";
  const showProvisionLinks = !isCustomRoleMember || hasAdminPermissions;
  const hasVisibleDefaultDeployment =
    hasDefaultProdDeployment || hasDefaultDevDeployment;
  // Custom-role members who can't view either default deployment fall back to
  // the deployments list scoped to this project, so they can still discover
  // any deployments their role allows (e.g. preview).
  const fallsBackToDeploymentsList =
    !showProvisionLinks && !hasVisibleDefaultDeployment;

  const canDeleteCustom = useHasCustomRolePermission(
    team?.id,
    "project:delete",
    projectResource(project),
    false,
  );
  const canDeleteProject = hasAdminPermissions || canDeleteCustom === true;

  const deploymentsListHref = `/t/${team?.slug}?view=deployments&projectId=${project.id}`;

  const dropdownItems: NonNullable<CardProps["dropdownItems"]> = [
    {
      Icon: LayersIcon,
      text: "View Deployments",
      href: deploymentsListHref,
    },
    {
      Icon: GearIcon,
      text: "Settings",
      href: `/t/${team?.slug}/${project.slug}/settings`,
    },
    {
      Icon: TrashIcon,
      destructive: true,
      text: "Delete project",
      action: () => setDeleteModal(true),
      disabled: !canDeleteProject,
      tip: !canDeleteProject
        ? permissionDeniedTip(
            "You do not have permission to delete this project.",
            "project:delete",
          )
        : undefined,
    },
  ];

  const myDeploymentsHref =
    profile &&
    `${deploymentsListHref}&creator=${profile.id}&sort=createTime:desc`;

  const cardTarget: CardTarget =
    recentDeploymentHref !== undefined
      ? "recent"
      : fallsBackToDeploymentsList
        ? "deployments"
        : isProdDefault
          ? "production"
          : "development";
  const cardHref = {
    production: prodHref,
    development: devHref,
    recent: recentDeploymentHref,
    deployments: deploymentsListHref,
  }[cardTarget];

  const linkLabel = name?.length ? name : "Untitled Project";
  const href = deleteModal ? undefined : cardHref;

  const identity = (
    <div className="min-w-0 flex-1 basis-0">
      <div
        className={cn(
          "truncate text-sm",
          name?.length
            ? "font-semibold text-content-primary"
            : "font-medium text-content-secondary",
        )}
      >
        {name?.length ? (
          <HighlightMatch text={name} query={searchQuery} />
        ) : (
          "Untitled Project"
        )}
      </div>
      <div className="truncate text-xs text-content-secondary">
        <HighlightMatch text={slug} query={searchQuery} />
      </div>
    </div>
  );

  const deployments =
    isLoadingDeployments || profile === undefined ? (
      <Loading className="h-6 w-56 rounded-md" fullHeight={false} />
    ) : (
      <DeploymentLinks
        defaultTarget={cardTarget}
        devHref={devHref}
        prodHref={prodHref}
        prodDeploymentName={prodDeploymentName}
        devDeploymentName={devDeploymentName}
        prodDeploymentReference={prodDeploymentReference}
        devDeploymentReference={devDeploymentReference}
        defaultDevReference={defaultDevDeploymentReference(profile)}
        recentDeployment={recentDeployment}
        recentDeploymentHref={recentDeploymentHref}
        myDeploymentsHref={myDeploymentsHref}
        showProvisionLinks={showProvisionLinks}
        deploymentsListHref={
          fallsBackToDeploymentsList ? deploymentsListHref : undefined
        }
      />
    );

  const created = (
    <div className="relative z-10 min-w-0">
      <TimestampDistance
        date={new Date(project.createTime)}
        className="truncate text-content-tertiary"
        delayDuration={TOOLTIP_DELAY_MS}
        prefix="Created"
      />
    </div>
  );

  const overlayLink = href && (
    <Link
      href={href}
      className="absolute inset-0 z-0 rounded-[inherit] outline-none focus-visible:z-10 focus-visible:ring-2 focus-visible:ring-border-selected focus-visible:ring-inset"
    >
      <span className="sr-only">{linkLabel}</span>
    </Link>
  );

  const modal = team && deleteModal && (
    <DeleteProjectModal
      team={team}
      project={project}
      onClose={() => setDeleteModal(false)}
    />
  );

  if (listItem) {
    // Fixed-width columns keep deployments and timestamps aligned from one
    // project to the next. As the list narrows, the timestamp, then tags, then
    // deployments drop out so the project name keeps room to read.
    return (
      <div className="group relative flex animate-fadeInFromLoading items-center gap-4 rounded-[inherit] bg-background-secondary py-2.5 pr-3 pl-4 hover:bg-background-tertiary">
        {overlayLink}
        {identity}
        <div className="mr-12 hidden max-w-64 min-w-0 flex-1 basis-0 md:flex">
          <ProjectTags
            tags={project.tags}
            chipClassName="group-hover:bg-background-secondary"
          />
        </div>
        <div className="hidden w-72 shrink-0 sm:flex">{deployments}</div>
        <div className="hidden w-36 shrink-0 justify-end lg:flex">
          {created}
        </div>
        <CardMenu items={dropdownItems} />
        {modal}
      </div>
    );
  }

  return (
    <div className="group relative flex animate-fadeInFromLoading flex-col rounded-xl border transition-colors hover:border-border-selected">
      {overlayLink}
      <div className="flex flex-1 flex-col gap-3 rounded-t-[inherit] bg-background-secondary px-4 py-3.5">
        <div className="flex items-start gap-2">
          {identity}
          <div className="flex h-6 shrink-0 items-center">{created}</div>
          <div className="-mr-1">
            <CardMenu items={dropdownItems} />
          </div>
        </div>
        <ProjectTags tags={project.tags} />
      </div>
      {/* Translucent like the data page's selected-document panel, so the page
          background shows through and sets the footer apart from the body. */}
      <div className="flex min-h-11 items-center gap-2 rounded-b-[inherit] border-t bg-background-secondary/70 px-4 py-2.5">
        {deployments}
      </div>
      {modal}
    </div>
  );
}

function DeploymentLinks({
  devHref,
  prodHref,
  defaultTarget,
  prodDeploymentName,
  devDeploymentName,
  prodDeploymentReference,
  devDeploymentReference,
  defaultDevReference,
  recentDeployment,
  recentDeploymentHref,
  myDeploymentsHref,
  showProvisionLinks,
  deploymentsListHref,
}: {
  defaultTarget: CardTarget;
  devHref: string;
  prodHref: string;
  prodDeploymentName: string | null;
  devDeploymentName: string | null;
  prodDeploymentReference: string | null;
  devDeploymentReference: string | null;
  defaultDevReference: string;
  recentDeployment: RecentDeployment | undefined;
  recentDeploymentHref: string | undefined;
  myDeploymentsHref: string | undefined;
  showProvisionLinks: boolean;
  deploymentsListHref?: string;
}) {
  return (
    <div className="flex min-w-0 gap-1.5">
      {deploymentsListHref !== undefined && (
        <DeploymentPill
          href={deploymentsListHref}
          label="All Deployments"
          isDefault={defaultTarget === "deployments"}
          icon={<LayersIcon />}
        />
      )}
      {(showProvisionLinks || prodDeploymentName !== null) && (
        <DeploymentPill
          href={prodHref}
          label={prodDeploymentReference ?? "production"}
          isDefault={defaultTarget === "production"}
          exists={prodDeploymentName !== null}
          icon={<DeploymentTypeIcon deploymentType="prod" />}
          tip={
            prodDeploymentName === null
              ? "Add a new production deployment"
              : `Production · ${prodDeploymentName}`
          }
        />
      )}
      {(showProvisionLinks || devDeploymentName !== null) && (
        <DeploymentPill
          href={devHref}
          label={
            devDeploymentReference ??
            (devDeploymentName?.startsWith("local-")
              ? "local"
              : defaultDevReference)
          }
          isDefault={defaultTarget === "development"}
          exists={devDeploymentName !== null}
          icon={<DeploymentTypeIcon deploymentType="dev" />}
          tip={
            devDeploymentName === null
              ? "Add a new personal dev deployment"
              : `Development · ${devDeploymentName}`
          }
        />
      )}
      {recentDeployment && (
        <DeploymentPill
          href={recentDeploymentHref}
          label={recentDeployment.reference}
          isDefault={defaultTarget === "recent"}
          icon={
            <DeploymentTypeIcon
              deploymentType={recentDeployment.deploymentType}
            />
          }
          tip={
            recentDeployment.status === "deleted"
              ? "This deployment was deleted, or you no longer have access to it"
              : `${deploymentTypeLabel(recentDeployment.deploymentType)} · ${recentDeployment.name}`
          }
        />
      )}
      <DeploymentPill
        href={myDeploymentsHref}
        label="Your deployments"
        iconOnly={deploymentsListHref === undefined}
        icon={<LayersIcon />}
        tip="Your deployments"
      />
    </div>
  );
}

type RecentDeployment = LastViewedDeployment & {
  status: "loading" | "available" | "deleted";
};

type CardTarget = "production" | "development" | "recent" | "deployments";

// The same icon per type as the deployment switcher in the header.
function DeploymentTypeIcon({
  deploymentType,
}: {
  deploymentType: DeploymentType;
}) {
  switch (deploymentType) {
    case "prod":
      return (
        <SignalIcon
          strokeWidth={2}
          className="text-purple-500 dark:text-purple-200"
        />
      );
    case "dev":
      return (
        <CommandLineIcon
          strokeWidth={2}
          className="text-green-700 dark:text-green-500"
        />
      );
    case "preview":
      return <Pencil2Icon className="text-orange-600 dark:text-orange-400" />;
    case "custom":
      return <WrenchIcon strokeWidth={2} className="text-content-secondary" />;
    default: {
      deploymentType satisfies never;
      return null;
    }
  }
}

function DeploymentPill({
  href,
  label,
  icon,
  isDefault = false,
  exists = true,
  iconOnly = false,
  tip,
}: {
  href?: string;
  label: string;
  // Sized by the pill; replaced by a "+" when the deployment doesn't exist.
  icon: ReactNode;
  isDefault?: boolean;
  exists?: boolean;
  iconOnly?: boolean;
  tip?: ReactNode;
}) {
  const className = cn(
    "relative z-10 inline-flex h-6 min-w-0 items-center gap-1.5 rounded-md border bg-background-secondary px-2 text-xs font-medium whitespace-nowrap text-content-primary transition-colors hover:border-border-selected focus-visible:ring-2 focus-visible:ring-border-selected focus-visible:outline-none",
    !exists && "border-dashed text-content-secondary",
    iconOnly
      ? "w-6 shrink-0 justify-center px-0 text-content-secondary hover:text-content-primary"
      : "max-w-max flex-1 basis-0",
    isDefault &&
      "group-hover:border-border-selected group-has-[[data-deployment-pill]:hover]:border-border-transparent hover:border-border-selected!",
    href === undefined &&
      "cursor-not-allowed text-content-secondary hover:border-border-transparent",
  );
  const content = (
    <>
      <span
        aria-hidden
        className={cn(
          "flex size-3.5 shrink-0 items-center justify-center [&>svg]:size-full",
          !iconOnly && "-ml-0.5",
        )}
      >
        {exists ? icon : <PlusIcon />}
      </span>
      {/* Labels are mostly lowercase references, and Inter's x-height sits
          below the line box's center, so nudge them up to center on the icon. */}
      {!iconOnly && (
        <span className="max-w-40 translate-y-[-0.5px] truncate">{label}</span>
      )}
    </>
  );
  const link =
    href === undefined ? (
      <span
        role="link"
        aria-disabled
        // Focusable so keyboard users can reach the tooltip saying why.
        tabIndex={0}
        data-deployment-pill
        aria-label={iconOnly ? label : undefined}
        className={className}
      >
        {content}
      </span>
    ) : (
      <Link
        href={href}
        data-deployment-pill
        aria-label={iconOnly ? label : undefined}
        className={className}
      >
        {content}
      </Link>
    );

  return tip ? (
    <Tooltip tip={tip} delayDuration={TOOLTIP_DELAY_MS} asChild>
      {link}
    </Tooltip>
  ) : (
    link
  );
}
