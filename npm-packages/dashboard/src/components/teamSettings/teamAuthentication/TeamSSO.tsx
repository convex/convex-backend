import { useRouter } from "next/router";
import { Link } from "@ui/Link";
import { cn } from "@ui/cn";
import type { TeamResponse } from "generatedApi";
import { useGetDirectorySync } from "api/directorySync";
import {
  useHasCustomRolePermission,
  useIsCurrentMemberTeamAdmin,
} from "api/roles";
import { useTeamEntitlements } from "api/teams";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";
import { DIRECTORY_SYNC_RESOURCE, MEMBER_RESOURCE } from "lib/permissions";
import { DirectorySyncSheet } from "./DirectorySyncSheet";
import { DomainsSheet } from "./DomainsSheet";
import {
  ReviewDirectoryChanges,
  reviewDirectoryChangesTitle,
} from "./ReviewDirectoryChanges";
import { SingleSignOnSheet } from "./SingleSignOnSheet";

export function TeamSSO({ team }: { team: TeamResponse }) {
  const { directorySync: directorySyncFlag } = useLaunchDarkly();
  const router = useRouter();

  const canViewDirectorySync = useHasCustomRolePermission(
    team.id,
    "directorySync:view",
    DIRECTORY_SYNC_RESOURCE,
    true,
  );
  const { data: directorySync } = useGetDirectorySync(team.id, {
    isPaused: !directorySyncFlag || canViewDirectorySync !== true,
  });
  // Reaching the subpage by URL answers to the permissions that gate the
  // controls opening it: the roster names members and the roles they hold, so
  // `member:view` throughout, and the right to enable on top of that while the
  // directory still manages nobody.
  const isTeamAdmin = useIsCurrentMemberTeamAdmin();
  const canEnableCustom = useHasCustomRolePermission(
    team.id,
    "directorySync:enable",
    DIRECTORY_SYNC_RESOURCE,
    false,
  );
  const canViewMembers = useHasCustomRolePermission(
    team.id,
    "member:view",
    MEMBER_RESOURCE,
    true,
  );
  const entitlements = useTeamEntitlements(team.id);
  const canEnable =
    (isTeamAdmin || canEnableCustom === true) &&
    (entitlements?.directorySyncEnabled ?? false);
  const managementEnabled = directorySync?.enabled ?? false;
  // Nothing to review until a directory is linked either, so a stale or
  // hand-written link lands on the settings it belongs to rather than on a
  // roster that would only report what the member may not read. The directory
  // query is paused without `directorySync:view`, so that gates this too.
  const showReview =
    router.query.review === "1" &&
    directorySync?.directory?.linked === true &&
    canViewMembers !== false &&
    (managementEnabled || canEnable);

  const settingsHref = {
    pathname: "/t/[team]/settings/team-authentication",
    query: { team: team.slug },
  } as const;

  return (
    // `-mb-6` hands the bottom of the page to the panes, which carry their
    // own bottom padding so scrolled content clears the edge.
    <div className="-mx-6 -mb-6 flex min-h-0 flex-1 flex-col">
      <div
        // Names the breadcrumb so the docs screenshots of the subpage can crop
        // to it alongside the subpage itself.
        data-testid="team-authentication-breadcrumb"
        className="sticky top-0 z-10 -mt-6 flex items-center gap-2 bg-background-primary p-6"
      >
        {showReview ? (
          <Link href={settingsHref}>
            <h2>Team Authentication</h2>
          </Link>
        ) : (
          <h2>Team Authentication</h2>
        )}
        {showReview && (
          <>
            <span className="text-content-secondary" role="separator">
              /
            </span>
            <h2>{reviewDirectoryChangesTitle(managementEnabled)}</h2>
          </>
        )}
      </div>
      <div className="relative flex min-h-0 flex-1 overflow-x-hidden">
        <div
          className={cn(
            "flex size-full min-h-0 gap-6 transition-transform duration-500 motion-reduce:transition-none",
            showReview ? "-translate-x-[calc(100%+1.5rem)]" : "translate-x-0",
          )}
        >
          <div
            className={cn(
              "scrollbar flex w-full shrink-0 flex-col gap-8 overflow-y-auto px-6 pb-6",
              showReview ? "pointer-events-none select-none" : "",
            )}
            // @ts-expect-error https://github.com/facebook/react/issues/17157
            inert={showReview ? "inert" : undefined}
          >
            <div className="flex max-w-2xl flex-col gap-8">
              <DomainsSheet team={team} />
              <SingleSignOnSheet team={team} />
              {directorySyncFlag && <DirectorySyncSheet team={team} />}
            </div>
          </div>
          <div
            className={cn(
              // The roster claims the height the pane has and scrolls its own
              // table, down to the point where the pane scrolls instead.
              "scrollbar flex w-full shrink-0 flex-col overflow-y-auto px-6 pb-6",
              !showReview ? "pointer-events-none select-none" : "",
            )}
            // @ts-expect-error https://github.com/facebook/react/issues/17157
            inert={!showReview ? "inert" : undefined}
          >
            {showReview && (
              <ReviewDirectoryChanges
                team={team}
                enabled={managementEnabled}
                onEnabled={() => {
                  void router.push(settingsHref, undefined, { shallow: true });
                }}
              />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
