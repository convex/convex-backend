import type { ComponentType } from "react";
import {
  PieChartIcon,
  ClockIcon,
  CodeIcon,
  CounterClockwiseClockIcon,
  CubeIcon,
  FileIcon,
  GearIcon,
  GlobeIcon,
  HomeIcon,
  LayersIcon,
  Link2Icon,
  PersonIcon,
  StopwatchIcon,
  TableIcon,
  TextAlignBottomIcon,
  TrashIcon,
} from "@radix-ui/react-icons";
import { ArrowsRightLeftIcon, KeyIcon } from "@heroicons/react/24/outline";
import { PulseIcon } from "@common/elements/icons";
import { DEPLOYMENT_SETTINGS_PAGE_ICONS } from "@common/layouts/deploymentSettingsPages";
import { TEAM_SETTINGS_PAGE_ICONS } from "layouts/teamSettingsPages";
import {
  DEPLOYMENT_SETTINGS_SECTIONS,
  PROFILE_SECTIONS,
  PROJECT_SETTINGS_SECTIONS,
  TEAM_SETTINGS_SECTIONS,
} from "lib/sectionAnchors";

// Where a command can navigate: a plain href, or a pathname + query object
// (the form next/router.push accepts) for destinations with encoded params.
export type NavigationDestination =
  | string
  | { pathname: string; query: Record<string, string> };

export type NavigationTarget = {
  label: string;
  href: string;
  Icon: ComponentType<{ className?: string }>;
  // Shown as the item's second line: the parent page (e.g. "Deployment
  // Settings" for a section) or parent resource (team/project/deployment).
  parent?: string;
  // Overrides the label as the text the search matches against. Used by
  // section items whose display label carries a "Go to …" decoration that
  // shouldn't be searchable.
  keywords?: string[];
};

// Data-plane pages, matching the deployment sidebar plus the settings
// subpages. `uriPrefix` is `/t/{team}/{project}/{deploymentName}`.
export function deploymentNavigation(uriPrefix: string): {
  pages: NavigationTarget[];
  settings: NavigationTarget[];
} {
  const pages: NavigationTarget[] = [
    { label: "Health", href: `${uriPrefix}/`, Icon: PulseIcon },
    { label: "Data", href: `${uriPrefix}/data`, Icon: TableIcon },
    { label: "Schema", href: `${uriPrefix}/schema`, Icon: CubeIcon },
    { label: "Functions", href: `${uriPrefix}/functions`, Icon: CodeIcon },
    { label: "Files", href: `${uriPrefix}/files`, Icon: FileIcon },
    {
      label: "Scheduled Functions",
      href: `${uriPrefix}/schedules/functions`,
      Icon: StopwatchIcon,
    },
    {
      label: "Cron Jobs",
      href: `${uriPrefix}/schedules/crons`,
      Icon: ClockIcon,
    },
    { label: "Logs", href: `${uriPrefix}/logs`, Icon: TextAlignBottomIcon },
    {
      label: "History",
      href: `${uriPrefix}/history`,
      Icon: CounterClockwiseClockIcon,
    },
  ];
  const settings: NavigationTarget[] = [
    {
      label: "Deployment Settings",
      href: `${uriPrefix}/settings`,
      Icon: DEPLOYMENT_SETTINGS_PAGE_ICONS.general,
    },
    {
      label: "Environment Variables",
      parent: "Deployment Settings",
      href: `${uriPrefix}/settings/environment-variables`,
      Icon: DEPLOYMENT_SETTINGS_PAGE_ICONS["environment-variables"],
    },
    {
      label: "Usage Limits",
      parent: "Deployment Settings",
      href: `${uriPrefix}/settings/usage-limits`,
      Icon: DEPLOYMENT_SETTINGS_PAGE_ICONS["usage-limits"],
    },
    {
      label: "Authentication",
      parent: "Deployment Settings",
      href: `${uriPrefix}/settings/authentication`,
      Icon: DEPLOYMENT_SETTINGS_PAGE_ICONS.authentication,
    },
    {
      label: "Custom Domains",
      parent: "Deployment Settings",
      href: `${uriPrefix}/settings/custom-domains`,
      Icon: DEPLOYMENT_SETTINGS_PAGE_ICONS["custom-domains"],
    },
    {
      label: "Components",
      parent: "Deployment Settings",
      href: `${uriPrefix}/settings/components`,
      Icon: DEPLOYMENT_SETTINGS_PAGE_ICONS.components,
    },
    {
      label: "Backup & Restore",
      parent: "Deployment Settings",
      href: `${uriPrefix}/settings/backups`,
      Icon: DEPLOYMENT_SETTINGS_PAGE_ICONS.backups,
    },
    {
      label: "Integrations",
      parent: "Deployment Settings",
      href: `${uriPrefix}/settings/integrations`,
      Icon: DEPLOYMENT_SETTINGS_PAGE_ICONS.integrations,
    },
  ];
  return { pages, settings };
}

// Section (sub-header) targets just scroll to a section of a settings page,
// but some section names read like actions on their own ("Delete Account",
// "Transfer Project"). Show a "Go to …" label so it's clear the command
// navigates there, but keep the bare section name as the search text so the
// "Go to" decoration isn't matched.
function sectionSearch(subheader: string): {
  label: string;
  keywords: string[];
} {
  return { label: `Go to "${subheader}"`, keywords: [subheader] };
}

// Sections within the deployment settings (general) page, deep-linked by
// anchor; the hierarchical label tells the user where the tool lives.
export function deploymentSectionNavigation(
  uriPrefix: string,
): NavigationTarget[] {
  const base = `${uriPrefix}/settings`;
  const Icon = DEPLOYMENT_SETTINGS_PAGE_ICONS.general;
  return Object.values(DEPLOYMENT_SETTINGS_SECTIONS).map(({ id, label }) => ({
    ...sectionSearch(label),
    href: `${base}#${id}`,
    Icon,
    parent: "Deployment Settings",
  }));
}

export function projectNavigation(
  teamSlug: string,
  projectSlug: string,
  // The project these pages belong to, for the items' second line.
  projectName?: string,
): NavigationTarget[] {
  return [
    {
      label: "Deployments",
      href: `/t/${teamSlug}/${projectSlug}`,
      Icon: LayersIcon,
    },
    {
      label: "Project Settings",
      href: `/t/${teamSlug}/${projectSlug}/settings`,
      Icon: GearIcon,
    },
    {
      label: "Project Usage",
      href: `/t/${teamSlug}/settings/usage?projectSlug=${projectSlug}`,
      Icon: PieChartIcon,
    },
  ].map((target) => ({ ...target, parent: projectName ?? projectSlug }));
}

// Sections within the project settings page, which has anchor IDs for each
// section, so these deep-link directly.
export function projectSectionNavigation(
  teamSlug: string,
  projectSlug: string,
): NavigationTarget[] {
  const base = `/t/${teamSlug}/${projectSlug}/settings`;
  const S = PROJECT_SETTINGS_SECTIONS;
  // Project Usage is intentionally omitted: the top-level "Project Usage"
  // command already links to the (more useful) team-usage view scoped to the
  // project.
  const sections: [(typeof S)[keyof typeof S], NavigationTarget["Icon"]][] = [
    [S.editProject, GearIcon],
    [S.projectAdmins, PersonIcon],
    [S.customDomains, GlobeIcon],
    [S.previewDeployKeys, KeyIcon],
    [S.authorizedApplications, Link2Icon],
    [
      S.environmentVariables,
      DEPLOYMENT_SETTINGS_PAGE_ICONS["environment-variables"],
    ],
    [S.transferProject, ArrowsRightLeftIcon],
    [S.deleteProject, TrashIcon],
  ];
  return sections.map(([{ id, label }, Icon]) => ({
    ...sectionSearch(label),
    href: `${base}#${id}`,
    Icon,
    parent: "Project Settings",
  }));
}

export function teamNavigation(
  teamSlug: string,
  // The team these pages belong to, for the items' second line.
  teamName?: string,
): NavigationTarget[] {
  const uriPrefix = `/t/${teamSlug}`;
  const team = teamName ?? teamSlug;
  return [
    { label: "Projects", href: uriPrefix, Icon: HomeIcon, parent: team },
    {
      label: "Team Settings",
      href: `${uriPrefix}/settings`,
      Icon: TEAM_SETTINGS_PAGE_ICONS.general,
      parent: team,
    },
    {
      label: "Members",
      parent: "Team Settings",
      href: `${uriPrefix}/settings/members`,
      Icon: TEAM_SETTINGS_PAGE_ICONS.members,
    },
    {
      label: "Access Tokens",
      parent: "Team Settings",
      href: `${uriPrefix}/settings/access-tokens`,
      Icon: TEAM_SETTINGS_PAGE_ICONS["access-tokens"],
    },
    {
      label: "Custom Roles",
      parent: "Team Settings",
      href: `${uriPrefix}/settings/custom-roles`,
      Icon: TEAM_SETTINGS_PAGE_ICONS["custom-roles"],
    },
    {
      label: "Usage",
      parent: "Team Settings",
      href: `${uriPrefix}/settings/usage`,
      Icon: TEAM_SETTINGS_PAGE_ICONS.usage,
    },
    {
      label: "Billing",
      parent: "Team Settings",
      href: `${uriPrefix}/settings/billing`,
      Icon: TEAM_SETTINGS_PAGE_ICONS.billing,
    },
    {
      label: "Audit Log",
      parent: "Team Settings",
      href: `${uriPrefix}/settings/audit-log`,
      Icon: TEAM_SETTINGS_PAGE_ICONS["audit-log"],
    },
    {
      label: "Referrals",
      parent: "Team Settings",
      href: `${uriPrefix}/settings/referrals`,
      Icon: TEAM_SETTINGS_PAGE_ICONS.referrals,
    },
    {
      label: "Team Authentication",
      parent: "Team Settings",
      href: `${uriPrefix}/settings/team-authentication`,
      Icon: TEAM_SETTINGS_PAGE_ICONS["team-authentication"],
    },
    {
      label: "OAuth Applications",
      parent: "Team Settings",
      href: `${uriPrefix}/settings/applications`,
      Icon: TEAM_SETTINGS_PAGE_ICONS.applications,
    },
  ];
}

// Sections within the team settings (general) and members pages.
export function teamSectionNavigation(teamSlug: string): NavigationTarget[] {
  const base = `/t/${teamSlug}/settings`;
  const Icon = TEAM_SETTINGS_PAGE_ICONS.general;
  const S = TEAM_SETTINGS_SECTIONS;
  const onGeneral = [
    S.teamName,
    S.teamSlug,
    S.teamId,
    S.defaultRegion,
    S.deleteTeam,
  ].map(({ id, label }) => ({
    ...sectionSearch(label),
    parent: "Team Settings",
    href: `${base}#${id}`,
    Icon,
  }));
  return [
    ...onGeneral,
    {
      ...sectionSearch(S.inviteMember.label),
      parent: "Team Settings",
      // Invite Member lives on the Members page, not the general settings page.
      href: `${base}/members#${S.inviteMember.id}`,
      Icon: TEAM_SETTINGS_PAGE_ICONS.members,
    },
  ];
}

export function teamSwitchDestination(
  teamSlug: string,
  currentPathname: string,
): string {
  const teamRoute = "/t/[team]";
  const suffix = currentPathname.startsWith(`${teamRoute}/settings`)
    ? currentPathname.slice(teamRoute.length)
    : "";
  // A suffix with its own dynamic segment has no value to substitute in.
  return suffix.includes("[") ? `/t/${teamSlug}` : `/t/${teamSlug}${suffix}`;
}

// Sections within the profile page.
export function profileSectionNavigation(): NavigationTarget[] {
  const Icon = PersonIcon;
  return Object.values(PROFILE_SECTIONS).map(({ id, label }) => ({
    ...sectionSearch(label),
    href: `/profile#${id}`,
    Icon,
    parent: "Profile Settings",
  }));
}

function normalizePath(path: string): string {
  return path.replace(/\/+$/, "") || "/";
}

// Whether `asPath` (the router's current URL) is the page `target` points to.
// Requires the pathname to match exactly and, when the target's href carries
// query params (e.g. Project Usage's projectSlug), those params to be present
// with the same values.
export function isCurrentPage(
  target: NavigationTarget,
  asPath: string,
): boolean {
  const [targetPath, targetQuery = ""] = target.href.split("?");
  const [currentPath, currentQuery = ""] = asPath.split("#")[0].split("?");
  if (normalizePath(targetPath) !== normalizePath(currentPath)) {
    return false;
  }
  const wanted = new URLSearchParams(targetQuery);
  const actual = new URLSearchParams(currentQuery);
  return [...wanted.entries()].every(([key, val]) => actual.get(key) === val);
}

export function findCurrentPage(
  targets: NavigationTarget[],
  asPath: string,
): NavigationTarget | undefined {
  return targets.find((target) => isCurrentPage(target, asPath));
}

// Items whose value carries this prefix come from server-side search, so the
// client-side filter must not second-guess them.
export const REMOTE_VALUE_PREFIX = "remote:";

// Every whitespace-separated token of the search must appear somewhere in the
// haystack, case-insensitively.
export function matchesSearch(search: string, haystack: string): boolean {
  const needle = search.trim().toLowerCase();
  if (!needle) {
    return true;
  }
  const target = haystack.toLowerCase();
  return needle.split(/\s+/).every((token) => target.includes(token));
}

// Substring-based filter for cmdk: every whitespace-separated token of the
// search must appear in the item's keywords (labels and item data — no
// synonyms). All matches score equally so results keep their DOM (group)
// order, which makes ranking predictable.
export function paletteFilter(
  value: string,
  search: string,
  keywords?: string[],
): number {
  if (value.startsWith(REMOTE_VALUE_PREFIX)) {
    return 1;
  }
  return matchesSearch(
    search,
    keywords && keywords.length > 0 ? keywords.join(" ") : value,
  )
    ? 1
    : 0;
}
