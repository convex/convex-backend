import { Sheet } from "@ui/Sheet";
import { Button } from "@ui/Button";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { TeamResponse } from "generatedApi";
import { Tooltip } from "@ui/Tooltip";
import { Link } from "@ui/Link";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { PaginationControls } from "elements/PaginationControls";
import {
  formatDeploymentCount,
  LEGACY_ADMIN_KEYS_SECTION_ID,
  RECENT_LEGACY_ADMIN_KEY_USE_MS,
  useDisableTeamLegacyAdminKeys,
  useTeamLegacyAdminKeyUsage,
} from "api/legacyAdminKeys";
import { useIsCurrentMemberTeamAdmin } from "api/roles";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";
import { formatDistanceToNow } from "date-fns";
import { useRouter } from "next/router";
import { useEffect, useRef, useState } from "react";

export function LegacyAdminKeys({ team }: { team: TeamResponse }) {
  const { showLegacyKeyUsage: show } = useLaunchDarkly();
  const isTeamAdmin = useIsCurrentMemberTeamAdmin();
  const data = useTeamLegacyAdminKeyUsage(show ? team.id : undefined);
  const disableLegacyAdminKeys = useDisableTeamLegacyAdminKeys(team.id);
  const [showDisableDialog, setShowDisableDialog] = useState(false);

  const sheetRef = useRef<HTMLDivElement>(null);
  const { asPath } = useRouter();
  const isLoaded = show && data !== undefined;
  useEffect(() => {
    if (isLoaded && asPath.endsWith(`#${LEGACY_ADMIN_KEYS_SECTION_ID}`)) {
      sheetRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
      sheetRef.current?.focus({ preventScroll: true });
    }
  }, [isLoaded, asPath]);

  if (!show || data === undefined) {
    return null;
  }

  const disabledTime = data.disabledTime ?? null;

  const latestUse = data.deployments[0]?.lastUsedTime;
  const recentlyUsed = data.deployments.filter(
    (d) => Date.now() - d.lastUsedTime < RECENT_LEGACY_ADMIN_KEY_USE_MS,
  );

  return (
    <Sheet
      id={LEGACY_ADMIN_KEYS_SECTION_ID}
      ref={sheetRef}
      tabIndex={-1}
      className="scroll-mt-4 focus:ring-2 focus:ring-util-accent focus:outline-none"
    >
      <h3 className="mb-1">Legacy Admin Keys</h3>
      <p className="mb-4 max-w-prose text-content-secondary">
        Legacy admin keys are a type of key that is no longer issued by Convex
        deployments.
        {disabledTime === null &&
          " You may revoke access to all legacy keys used by this team."}
      </p>
      {disabledTime !== null ? (
        <p>
          Legacy admin keys were disabled for this team{" "}
          <RelativeTime time={disabledTime} />.
        </p>
      ) : (
        <>
          <p className="mb-4">
            {latestUse === undefined ? (
              "No recent legacy admin key use has been recorded for this team."
            ) : (
              <>
                {formatDeploymentCount(data.deployments.length)} in this team{" "}
                {data.deployments.length === 1 ? "has" : "have"} used a legacy
                admin key, most recently <RelativeTime time={latestUse} />.
              </>
            )}
          </p>
          {data.deployments.length > 0 && (
            <LegacyAdminKeyDeploymentList deployments={data.deployments} />
          )}
          <Button
            variant="danger"
            onClick={() => setShowDisableDialog(true)}
            disabled={!isTeamAdmin}
            tip={
              isTeamAdmin
                ? undefined
                : "Only team admins can disable legacy admin keys."
            }
          >
            Disable Legacy Admin Keys
          </Button>
          {showDisableDialog && (
            <ConfirmationDialog
              onClose={() => setShowDisableDialog(false)}
              onConfirm={() => disableLegacyAdminKeys()}
              validationText={"DISABLE LEGACY ADMIN KEYS"}
              confirmText="Disable"
              dialogTitle="Disable Legacy Admin Keys"
              dialogBody={
                recentlyUsed.length > 0 ? (
                  <>
                    {formatDeploymentCount(recentlyUsed.length)}{" "}
                    {recentlyUsed.length === 1 ? "has" : "have"} used a legacy
                    admin key recently. Anything still using one, such as a CI
                    pipeline or a server, will stop working within a minute.
                    This cannot be undone.
                  </>
                ) : (
                  "Every deployment in this team will reject legacy admin keys within a minute. This cannot be undone."
                )
              }
            />
          )}
        </>
      )}
    </Sheet>
  );
}

const DEPLOYMENTS_PAGE_SIZE = 10;

function LegacyAdminKeyDeploymentList({
  deployments,
}: {
  deployments: { deploymentName: string; lastUsedTime: number }[];
}) {
  const [page, setPage] = useState(1);
  const totalPages = Math.ceil(deployments.length / DEPLOYMENTS_PAGE_SIZE);
  const pageDeployments = deployments.slice(
    (page - 1) * DEPLOYMENTS_PAGE_SIZE,
    page * DEPLOYMENTS_PAGE_SIZE,
  );
  return (
    <div className="mb-4 flex max-w-prose flex-col gap-2">
      <div className="flex flex-col divide-y rounded-md border">
        {pageDeployments.map(({ deploymentName, lastUsedTime }) => (
          <div
            key={deploymentName}
            className="flex items-center justify-between gap-4 px-3 py-2"
          >
            <Link href={`/d/${deploymentName}/settings`} className="font-mono">
              {deploymentName}
            </Link>
            <TimestampDistance
              prefix="Last used"
              date={new Date(lastUsedTime)}
            />
          </div>
        ))}
      </div>
      {totalPages > 1 && (
        <PaginationControls
          currentPage={page}
          totalPages={totalPages}
          onPageChange={setPage}
        />
      )}
    </div>
  );
}

function RelativeTime({ time }: { time: number }) {
  return (
    <Tooltip
      asChild
      tip={new Date(time).toLocaleString(undefined, { timeZoneName: "short" })}
    >
      <span className="whitespace-nowrap underline decoration-dotted underline-offset-2">
        {formatDistanceToNow(time, { addSuffix: true }).replace("about ", "")}
      </span>
    </Tooltip>
  );
}
