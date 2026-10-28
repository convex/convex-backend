import { Command, useCommandState } from "cmdk";
import React, { useContext } from "react";
import { useRouter } from "next/router";
import { CaretRightIcon, Pencil2Icon, StackIcon } from "@radix-ui/react-icons";
import {
  CommandLineIcon,
  SignalIcon,
  WrenchIcon,
} from "@heroicons/react/24/outline";
import { Button } from "@ui/Button";
import { KEYCAP_CLASSES, KeyboardShortcut } from "@ui/KeyboardShortcut";
import { Loading } from "@ui/Loading";
import { Tooltip } from "@ui/Tooltip";
import { cn } from "@ui/cn";
import type { DeploymentType } from "@convex-dev/platform/managementApi";
import {
  deploymentTypeColorClasses,
  deploymentTypeLabel,
} from "@common/lib/deploymentTypeColorClasses";
import { useProjectById } from "api/projects";
import { useCurrentTeam } from "api/teams";
import { useMyCustomRoles } from "api/roles";
import { useDeploymentUris } from "hooks/useDeploymentUris";
import { useLastViewedDeploymentForProject } from "hooks/useLastViewed";
import type { PlatformDeploymentResponse, ProjectDetails } from "generatedApi";
import type { NavigationTarget } from "./navigation";
import { REMOTE_VALUE_PREFIX } from "./navigation";
import { useCopyAction } from "./copy";
import type { DeploymentPicker } from "./picker";
import { usePaletteAnalytics } from "./analytics";

// Items whose default action is direct navigation drill into their nested
// view instead when this flag is set. The dialog sets it from Shift+Enter and
// ArrowRight just before cmdk fires the selection, and clears it right after.
export const DrillModifierContext = React.createContext<{ current: boolean }>({
  current: false,
});

export function useConsumeDrillModifier() {
  const flag = useContext(DrillModifierContext);
  return () => {
    const active = flag.current;
    flag.current = false;
    return active;
  };
}

// Indices of `text` covered by an occurrence of any whitespace-separated
// token of `query`, case-insensitively. Mirrors the palette filter's
// substring-token matching.
function matchedIndices(query: string, text: string): Set<number> {
  const indices = new Set<number>();
  const lowerText = text.toLowerCase();
  for (const token of query.trim().toLowerCase().split(/\s+/)) {
    if (!token) {
      continue;
    }
    let idx = lowerText.indexOf(token);
    while (idx !== -1) {
      for (let i = idx; i < idx + token.length; i++) {
        indices.add(i);
      }
      idx = lowerText.indexOf(token, idx + 1);
    }
  }
  return indices;
}

// Renders text with the characters matching the current palette search
// emphasized, in the same style as the schema page's search.
export function HighlightedText({ text }: { text: string }) {
  const search = useCommandState((state) => state.search);
  if (!search.trim()) {
    return <span>{text}</span>;
  }
  const indices = matchedIndices(search, text);
  if (indices.size === 0) {
    return <span>{text}</span>;
  }
  const parts: React.ReactNode[] = [];
  let buffer = "";
  let bufferHighlighted = false;
  const flush = () => {
    if (!buffer) {
      return;
    }
    parts.push(
      bufferHighlighted ? (
        <span key={parts.length} className="font-semibold text-content-accent">
          {buffer}
        </span>
      ) : (
        <span key={parts.length}>{buffer}</span>
      ),
    );
    buffer = "";
  };
  for (let i = 0; i < text.length; i++) {
    const highlighted = indices.has(i);
    if (highlighted !== bufferHighlighted) {
      flush();
      bufferHighlighted = highlighted;
    }
    buffer += text[i];
  }
  flush();
  // Wrap the runs in a single element: many items lay their children out with
  // flex + gap, which would otherwise insert space between the runs.
  return <span>{parts}</span>;
}

export function NavigationItem({
  target,
  onNavigate,
  hint,
}: {
  target: NavigationTarget;
  onNavigate: (href: string) => void;
  // Right-aligned annotation, e.g. "Current Page".
  hint?: string;
}) {
  const { label, href, Icon, parent, keywords } = target;
  const searchKeywords = keywords ?? [label];
  const { trackSelected } = usePaletteAnalytics();
  return (
    <Command.Item
      value={`nav:${href}:${label}`}
      keywords={parent ? [...searchKeywords, parent] : searchKeywords}
      onSelect={() => {
        trackSelected(`navigate:${label}`);
        onNavigate(href);
      }}
    >
      <Icon className="text-content-secondary" />
      {/* Two lines: the page/section itself, then where it lives. */}
      <span className="flex min-w-0 flex-col">
        <span className="truncate">
          <HighlightedText text={label} />
        </span>
        {parent && (
          <span className="truncate text-xs text-content-tertiary">
            <HighlightedText text={parent} />
          </span>
        )}
      </span>
      {hint && (
        <span className="ml-auto shrink-0 text-xs text-content-tertiary">
          {hint}
        </span>
      )}
    </Command.Item>
  );
}

// A non-navigation command (drill-in page, theme change, tool, …).
export function ActionItem({
  value,
  onSelect,
  Icon,
  label,
  description,
  destructive = false,
  drillIn = false,
  disabled = false,
  tip,
}: {
  value: string;
  onSelect: () => void;
  Icon: React.FC<{ className?: string }>;
  label: string;
  // Optional second line, e.g. a warning about what the action does.
  description?: string;
  // Render in the error color, for destructive actions.
  destructive?: boolean;
  drillIn?: boolean;
  disabled?: boolean;
  tip?: React.ReactNode;
}) {
  const { trackSelected } = usePaletteAnalytics();
  const body = (
    <>
      <Icon
        className={cn(
          "size-4.5 shrink-0",
          destructive ? "text-content-error" : "text-content-secondary",
        )}
      />
      <span className="flex min-w-0 flex-col">
        <span className={cn("truncate", destructive && "text-content-error")}>
          <HighlightedText text={label} />
        </span>
        {description && (
          <span
            className={cn(
              "truncate text-xs",
              destructive ? "text-content-error/80" : "text-content-tertiary",
            )}
          >
            {description}
          </span>
        )}
      </span>
      {drillIn && <DrillInHint />}
    </>
  );
  return (
    <Command.Item
      value={value}
      keywords={[label]}
      disabled={disabled}
      className={cn("select-none", disabled && "pointer-events-none")}
      onSelect={() => {
        trackSelected(value);
        onSelect();
      }}
    >
      {disabled && tip ? (
        // A disabled item is pointer-events-none so cmdk ignores it; re-enable
        // events on just the tooltip trigger so hovering the row still explains
        // why the action is unavailable.
        <Tooltip
          tip={tip}
          side="top"
          asChild
          className="pointer-events-auto flex w-full items-center gap-2"
        >
          <span>{body}</span>
        </Tooltip>
      ) : (
        body
      )}
    </Command.Item>
  );
}

// Reports that palette content is loading.
export const PaletteLoadingContext = React.createContext<
  (() => () => void) | null
>(null);

export function LoadingSignal({ rows = 5 }: { rows?: number }) {
  const beginLoading = React.useContext(PaletteLoadingContext);
  React.useEffect(() => beginLoading?.(), [beginLoading]);
  const id = React.useId();
  return (
    <>
      {Array.from({ length: rows }, (_, index) => (
        <Command.Item
          key={index}
          value={`${REMOTE_VALUE_PREFIX}loading:${id}:${index}`}
          disabled
          data-placeholder=""
          aria-label="Loading result"
        >
          <Loading
            fullHeight={false}
            className="size-4.5 shrink-0 rounded-full"
          />
          <span className="flex min-w-0 grow flex-col gap-2.5">
            <Loading fullHeight={false} className="h-3.5 w-2/3" />
            <Loading fullHeight={false} className="h-3 w-1/3" />
          </span>
        </Command.Item>
      ))}
    </>
  );
}

// Lets the active drill-in page publish a short status line into the palette
// footer (e.g. how many items are currently selected).
export const PaletteStatusContext = React.createContext<
  ((status: React.ReactNode) => void) | null
>(null);

export const PaletteConfirmContext = React.createContext<
  ((action: (() => void) | null) => void) | null
>(null);

export function PinnedActions({ children }: { children: React.ReactNode }) {
  return (
    <Command.Group
      data-pinned=""
      className={cn(
        "sticky bottom-0 z-20 -mx-1 border-t p-1",
        "bg-background-primary",
      )}
    >
      {children}
    </Command.Group>
  );
}

export function CurrentBadge({ label = "Current" }: { label?: string }) {
  return <span className="rounded-sm border px-1.5 py-0.5">{label}</span>;
}

function ItemPrimary({ children }: { children: React.ReactNode }) {
  return <span data-item-primary="">{children}</span>;
}

function DrillButton({ onDrill }: { onDrill: () => void }) {
  return (
    <Button
      variant="unstyled"
      aria-label="Browse"
      tip={
        <span className="flex items-center gap-1">
          Browse
          <KeyboardShortcut value={["Right"]} className={KEYCAP_CLASSES} />
        </span>
      }
      data-secondary-action=""
      className={cn(
        "mr-1 ml-1.5 shrink-0 rounded-lg p-1.5 text-content-tertiary",
        "hover:bg-background-secondary hover:text-content-primary",
        "dark:hover:bg-background-tertiary",
      )}
      onClick={(e) => {
        e.preventDefault();
        e.stopPropagation();
        onDrill();
      }}
    >
      <CaretRightIcon className="size-4" />
    </Button>
  );
}

export function DrillInHint({ kind }: { kind?: React.ReactNode }) {
  return (
    <span className="ml-auto flex shrink-0 items-center gap-1 text-xs text-content-tertiary">
      {kind}
      <CaretRightIcon className="size-4" />
    </span>
  );
}

// A project result. Selecting it goes straight to the project; the drill
// modifier (Shift+Enter / ArrowRight / clicking the caret) opens its pages
// and deployments instead.
export function ProjectItem({
  project,
  teamSlug,
  onNavigate,
  onDrill,
}: {
  project: ProjectDetails;
  teamSlug: string;
  onNavigate: (href: string) => void;
  onDrill: () => void;
}) {
  const consumeDrillModifier = useConsumeDrillModifier();
  const { trackSelected } = usePaletteAnalytics();
  const router = useRouter();
  const isCurrent = router.query.project === project.slug;
  const team = useCurrentTeam();
  const isCustomRoleMember = useMyCustomRoles(team?.id)?.role === "custom";
  // Prefer the project's last-viewed deployment (and current subpage) like the
  // old header menu; `/t/team/project` would instead redirect to the default
  // dev/prod deployment, dropping that context and bouncing you off the
  // deployment you're already on. Custom-role members go to the deployments
  // list instead, since they may not be able to view the default deployment.
  const { generateHref, defaultHref } = useDeploymentUris(
    project.id,
    project.slug,
    teamSlug,
  );
  const [lastViewedDeployment] = useLastViewedDeploymentForProject(
    project.slug,
  );
  const projectHref = isCustomRoleMember
    ? `/t/${teamSlug}?view=deployments&projectId=${project.id}`
    : lastViewedDeployment
      ? generateHref(lastViewedDeployment)
      : defaultHref;
  const value = `${REMOTE_VALUE_PREFIX}project:${project.id}`;
  useCopyAction(value, { label: "slug", getText: () => project.slug });
  return (
    <Command.Item
      value={value}
      className="animate-fadeInFromLoading"
      onSelect={() => {
        trackSelected("switch-project");
        if (consumeDrillModifier()) {
          return onDrill();
        }
        return onNavigate(projectHref);
      }}
    >
      <ItemPrimary>
        <ProjectRowBody project={project} />
        {isCurrent && (
          <span className="ml-auto shrink-0 text-xs text-content-tertiary">
            <CurrentBadge />
          </span>
        )}
      </ItemPrimary>
      <DrillButton onDrill={onDrill} />
    </Command.Item>
  );
}

// A deployment result. Selecting it goes straight to the deployment's Health
// page; the drill modifier opens its page list instead (also the fallback
// while the project slug needed for the direct link is still loading).
export function DeploymentItem({
  deployment,
  teamSlug,
  projectSlug: knownProjectSlug,
  onNavigate,
  onDrill,
  remote = false,
  showProject = false,
}: {
  deployment: PlatformDeploymentResponse;
  teamSlug: string;
  projectSlug?: string;
  onNavigate: (href: string) => void;
  onDrill: () => void;
  // Whether this item comes from server-side search (bypasses the client
  // filter) rather than an already-loaded local list.
  remote?: boolean;
  showProject?: boolean;
}) {
  const consumeDrillModifier = useConsumeDrillModifier();
  const router = useRouter();
  const { project } = useProjectById(deployment.projectId);
  const projectSlug = knownProjectSlug ?? project?.slug;
  const typeLabel = deploymentTypeLabel(deployment.deploymentType);
  const { primary } = deploymentRowText(deployment);
  const { trackSelected } = usePaletteAnalytics();
  // When switching straight to another deployment, keep whatever subpage the
  // user is on (Data, Logs, a settings tab, …) instead of resetting to Health.
  // Only meaningful while already viewing a deployment.
  const currentView =
    typeof router.query.deploymentName === "string"
      ? router.asPath.split(/[?#]/)[0].split("/").slice(5).join("/")
      : "";
  const isCurrent = router.query.deploymentName === deployment.name;
  const value = `${remote ? REMOTE_VALUE_PREFIX : ""}deployment:${deployment.name}`;
  useCopyAction(
    value,
    deployment.kind === "cloud"
      ? { label: "deployment reference", getText: () => deployment.reference }
      : { label: "deployment name", getText: () => deployment.name },
  );
  return (
    <Command.Item
      value={value}
      className="animate-fadeInFromLoading"
      keywords={remote ? undefined : [primary, deployment.name, typeLabel]}
      onSelect={() => {
        trackSelected("switch-deployment");
        if (consumeDrillModifier() || !projectSlug) {
          return onDrill();
        }
        const base = `/t/${teamSlug}/${projectSlug}/${deployment.name}`;
        return onNavigate(currentView ? `${base}/${currentView}` : base);
      }}
    >
      <ItemPrimary>
        <DeploymentRowBody
          deployment={deployment}
          projectName={showProject ? project?.name || project?.slug : undefined}
        />
        {isCurrent && (
          <span className="ml-auto shrink-0 text-xs text-content-tertiary">
            <CurrentBadge />
          </span>
        )}
      </ItemPrimary>
      <DrillButton onDrill={onDrill} />
    </Command.Item>
  );
}

// A deployment row in picker mode: choosing it hands the deployment to the
// picker rather than navigating to it.
export function DeploymentPickerItem({
  deployment,
  picker,
  onSelect,
}: {
  deployment: PlatformDeploymentResponse;
  picker: DeploymentPicker;
  onSelect: () => void;
}) {
  const { trackSelected } = usePaletteAnalytics();
  const { primary, secondary } = deploymentRowText(deployment);
  const typeLabel = deploymentTypeLabel(deployment.deploymentType);
  const unavailableReason = picker.unavailableReason?.(deployment);
  const isSelected = picker.selectedDeploymentName === deployment.name;
  const value = `pick-deployment:${deployment.name}`;
  useCopyAction(
    value,
    deployment.kind === "cloud"
      ? { label: "deployment reference", getText: () => deployment.reference }
      : null,
  );
  const body = (
    <>
      <DeploymentRowBody deployment={deployment} />
      {isSelected && (
        <span className="ml-auto shrink-0 text-xs text-content-tertiary">
          <CurrentBadge label="Selected" />
        </span>
      )}
    </>
  );
  return (
    <Command.Item
      // Not the `deployment:` value the switcher rows use: that marks a row as
      // browsable, and a picked deployment has no nested view to browse into.
      value={value}
      className={cn(
        "animate-fadeInFromLoading",
        unavailableReason && "pointer-events-none",
      )}
      keywords={[primary, secondary, deployment.name, typeLabel]}
      disabled={unavailableReason !== undefined}
      onSelect={() => {
        trackSelected("pick-deployment");
        onSelect();
      }}
    >
      {unavailableReason ? (
        // A disabled item is pointer-events-none so cmdk ignores it; re-enable
        // events on just the tooltip trigger so hovering the row still explains
        // why it can't be picked.
        <Tooltip
          tip={unavailableReason}
          side="top"
          asChild
          className="pointer-events-auto flex w-full items-center gap-2"
        >
          <span>{body}</span>
        </Tooltip>
      ) : (
        body
      )}
    </Command.Item>
  );
}

// A project row in picker mode: choosing it drills into that project's
// deployments rather than navigating anywhere.
export function ProjectPickerItem({
  project,
  selected,
  onSelect,
}: {
  project: ProjectDetails;
  // The project the picker currently points at.
  selected: boolean;
  onSelect: () => void;
}) {
  const { trackSelected } = usePaletteAnalytics();
  const value = `${REMOTE_VALUE_PREFIX}project:${project.id}`;
  useCopyAction(value, { label: "slug", getText: () => project.slug });
  return (
    <Command.Item
      value={value}
      className="animate-fadeInFromLoading"
      onSelect={() => {
        trackSelected("pick-project");
        onSelect();
      }}
    >
      <ProjectRowBody project={project} />
      <DrillInHint
        kind={selected ? <CurrentBadge label="Selected" /> : undefined}
      />
    </Command.Item>
  );
}

// The stacked-projects icon, then the project's name over its slug.
function ProjectRowBody({ project }: { project: ProjectDetails }) {
  return (
    <>
      <StackIcon className="text-content-secondary" />
      <span className="flex min-w-0 flex-col">
        <span className="truncate">
          <HighlightedText text={project.name || project.slug} />
        </span>
        <span className="truncate text-xs text-content-tertiary">
          <HighlightedText text={project.slug} />
        </span>
      </span>
    </>
  );
}

// Local deployments have no cloud reference; show the device and port they're
// running on instead, matching the header's old deployment menu.
function deploymentRowText(deployment: PlatformDeploymentResponse) {
  return {
    primary:
      deployment.kind === "cloud"
        ? deployment.reference
        : deployment.deviceName,
    secondary:
      deployment.kind === "local" ? `Port ${deployment.port}` : deployment.name,
  };
}

// The type badge, then the deployment's reference (or device) over its name
// (or port).
function DeploymentRowBody({
  deployment,
  projectName,
}: {
  deployment: PlatformDeploymentResponse;
  projectName?: string;
}) {
  const { primary, secondary } = deploymentRowText(deployment);
  return (
    <>
      <div
        className={cn(
          "inline-flex shrink-0 items-center justify-center rounded-full p-1",
          deploymentTypeColorClasses(deployment.deploymentType),
        )}
      >
        <DeploymentTypeIcon deploymentType={deployment.deploymentType} />
      </div>
      <span className="flex min-w-0 flex-col">
        <span className="truncate">
          <HighlightedText text={primary} />
        </span>
        <span className="truncate text-xs text-content-tertiary">
          <HighlightedText text={secondary} />
          {projectName && (
            <span className="text-content-tertiary"> · {projectName}</span>
          )}
        </span>
      </span>
    </>
  );
}

export function DeploymentTypeIcon({
  deploymentType,
}: {
  deploymentType: DeploymentType;
}) {
  switch (deploymentType) {
    case "prod":
      return <SignalIcon className="size-3.5" />;
    case "dev":
      return <CommandLineIcon className="size-3.5" />;
    case "preview":
      return <Pencil2Icon className="size-3.5" />;
    case "custom":
      return <WrenchIcon className="size-3.5" />;
    default: {
      deploymentType satisfies never;
      return null;
    }
  }
}
