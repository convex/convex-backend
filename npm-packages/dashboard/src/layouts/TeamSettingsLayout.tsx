import classNames from "classnames";
import { Loading } from "@ui/Loading";
import { PageContent } from "@common/elements/PageContent";
import { useCurrentTeam, useTeamEntitlements } from "api/teams";
import startCase from "lodash/startCase";
import Head from "next/head";
import React from "react";
import { TeamResponse } from "generatedApi";
import { SidebarLink } from "@common/elements/Sidebar";
import {
  TEAM_SETTINGS_PAGE_ICONS,
  TeamSettingsPage,
} from "layouts/teamSettingsPages";

export function TeamSettingsLayout({
  page: selectedPage,
  Component,
  title,
  fillHeight = false,
}: {
  page: TeamSettingsPage;
  Component: React.FunctionComponent<{ team: TeamResponse }>;
  title: string;
  /**
   * Hands the page a definite height rather than one that grows with its
   * content, for a page that scrolls regions of its own: without it their
   * `h-full` resolves against an auto height and they grow instead of
   * scrolling. Falls back to growing where the height above is itself
   * indefinite, as it is when the sidebar stacks on a narrow screen.
   */
  fillHeight?: boolean;
}) {
  const selectedTeam = useCurrentTeam();

  const entitlements = useTeamEntitlements(selectedTeam?.id);
  const auditLogsEnabled = entitlements?.auditLogRetentionDays !== 0;

  const pages: TeamSettingsPage[] = [
    "general",
    "members",
    "billing",
    "usage",
    "referrals",
    "access-tokens",
    "applications",
  ];

  return (
    <>
      <Head>
        {selectedTeam && (
          <title>
            {title} | {selectedTeam.name} | Convex Dashboard
          </title>
        )}
      </Head>
      <PageContent>
        <div
          className={classNames(
            "flex flex-col sm:flex-row h-full",
            "oveflow-hidden",
          )}
        >
          <aside
            className={classNames(
              "flex sm:flex-col gap-1",
              "min-w-40 sm:w-68",
              "h-fit sm:h-auto sm:min-h-fit",
              "px-3 py-2",
              "overflow-x-auto scrollbar-none",
              "bg-background-secondary",
              "sm:shadow-sm sm:border-r",
              "border-b sm:border-b-0",
            )}
          >
            {pages.map((page) => (
              <SidebarLink
                href={`/t/${selectedTeam?.slug}/settings/${
                  page === "general" ? "" : page
                }`}
                Icon={TEAM_SETTINGS_PAGE_ICONS[page]}
                isActive={page === selectedPage}
                key={page}
              >
                {startCase(page)}
              </SidebarLink>
            ))}
            <SidebarLink
              isActive={selectedPage === "audit-log"}
              href={`/t/${selectedTeam?.slug}/settings/audit-log`}
              Icon={TEAM_SETTINGS_PAGE_ICONS["audit-log"]}
              disabled={!auditLogsEnabled}
              proBadge={!auditLogsEnabled}
            >
              Audit Log
            </SidebarLink>
            <SidebarLink
              isActive={selectedPage === "custom-roles"}
              href={`/t/${selectedTeam?.slug}/settings/custom-roles`}
              Icon={TEAM_SETTINGS_PAGE_ICONS["custom-roles"]}
            >
              Custom Roles
            </SidebarLink>
            <SidebarLink
              isActive={selectedPage === "team-authentication"}
              href={`/t/${selectedTeam?.slug}/settings/team-authentication`}
              Icon={TEAM_SETTINGS_PAGE_ICONS["team-authentication"]}
            >
              Team Authentication
            </SidebarLink>
          </aside>
          <div className="scrollbar w-full overflow-y-auto">
            <div
              className={classNames(
                "flex max-w-7xl flex-col gap-6 p-6",
                fillHeight ? "h-full" : "min-h-full",
              )}
            >
              {selectedTeam ? (
                <Component team={selectedTeam} key={selectedTeam.id} />
              ) : (
                <Loading className="h-200" fullHeight={false} />
              )}
            </div>
          </div>
        </div>
      </PageContent>
    </>
  );
}
