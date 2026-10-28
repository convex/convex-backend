import {
  useDeleteAppAccessTokenByName,
  useTeamAppAccessTokens,
} from "api/accessTokens";
import { AppAccessTokenResponse, TeamResponse } from "generatedApi";
import { AuthorizedApplications } from "components/AuthorizedApplications";
import { OauthApps } from "components/teamSettings/OauthApps";
import { HelpTooltip } from "@ui/HelpTooltip";
import React, { Fragment } from "react";
import {
  TabGroup as HeadlessTabGroup,
  TabPanel as HeadlessTabPanel,
  TabPanels as HeadlessTabPanels,
} from "@headlessui/react";
import { Tab } from "@ui/Tab";
import { useRouter } from "next/router";
import { useHasCustomRolePermission } from "api/roles";
import { useProfile } from "api/profile";
import { teamTokenResource } from "lib/permissions";
import { NoPermissionMessage } from "elements/NoPermissionMessage";

export function ApplicationsLayout({ team }: { team: TeamResponse }) {
  const router = useRouter();
  // Determine selected tab based on route
  const isOauthApps = router.pathname.endsWith("/oauth-apps");
  const selectedIndex = isOauthApps ? 1 : 0;

  // `/teams/{team_id}/app_access_tokens` server-checks
  // `team:token:view` on `team:*:token:creator=<self>` (only the
  // requester's own app authorizations are returned). Without the
  // grant the request would hang and the tab would spin forever, so
  // skip the query and show a no-permission message instead.
  const profile = useProfile();
  const canViewOwnTeamTokens = useHasCustomRolePermission(
    team.id,
    "team:token:view",
    teamTokenResource(profile?.id ?? null),
    true,
  );
  const teamAccessTokens = useTeamAppAccessTokens(
    canViewOwnTeamTokens === false ? undefined : team.id,
  );
  const deleteTeamAccessToken = useDeleteAppAccessTokenByName({
    teamId: team.id,
  });

  const explainer = (
    <>
      <p className="text-sm text-content-primary">
        These 3rd-party applications have been authorized to access this team on
        your behalf.
      </p>
      <div className="my-2 text-sm text-content-primary">
        <span className="font-semibold">
          What can authorized applications do?
        </span>
        <ul className="mt-1 list-disc pl-4">
          <li>Create new projects</li>
          <li>Create new deployments</li>
          <li>
            <span className="flex items-center gap-1">
              Manage all projects on the team
              <HelpTooltip>
                This includes actions like deleting projects, managing custom
                domains, managing project environment variable defaults, and
                managing cloud backups and restores.
              </HelpTooltip>
            </span>
          </li>
          <li>
            <span className="flex items-center gap-1">
              Read and write data in all projects
              <HelpTooltip>
                Write access to Production deployments will depend on your
                team-level and project-level roles.
              </HelpTooltip>
            </span>
          </li>
        </ul>
      </div>
      <p className="mt-1 mb-2 text-sm text-content-primary">
        You cannot see applications that other members of your team have
        authorized.
      </p>
    </>
  );

  return (
    <div className="flex min-w-fit flex-col">
      <HeadlessTabGroup
        as={Fragment}
        selectedIndex={selectedIndex}
        onChange={(index) => {
          const base = `/t/${team.slug}/settings/applications`;
          if (index === 0) {
            void router.push(base);
          } else {
            void router.push(`${base}/oauth-apps`);
          }
        }}
      >
        <div className="sticky top-0 z-10 bg-background-primary">
          <h2 className="mb-4">Applications</h2>
          <div className="mb-4 flex gap-2">
            <Tab>Authorized Applications</Tab>
            <Tab>Your OAuth Applications</Tab>
          </div>
        </div>
        <HeadlessTabPanels>
          <HeadlessTabPanel
            className="focus-visible:outline-none"
            tabIndex={-1}
          >
            {canViewOwnTeamTokens === false ? (
              <NoPermissionMessage
                message="You do not have permission to view authorized applications."
                missingPermission="team:token:view"
              />
            ) : (
              <AuthorizedApplications
                accessTokens={teamAccessTokens}
                explainer={explainer}
                onRevoke={async (token: AppAccessTokenResponse) => {
                  await deleteTeamAccessToken({ name: token.name });
                }}
              />
            )}
          </HeadlessTabPanel>
          <HeadlessTabPanel
            className="focus-visible:outline-none"
            tabIndex={-1}
          >
            <OauthApps teamId={team.id} />
          </HeadlessTabPanel>
        </HeadlessTabPanels>
      </HeadlessTabGroup>
    </div>
  );
}
