import { useContext, useMemo, useState } from "react";
import type {
  ActiveDataSync,
  ActiveDataSyncSnapshotting,
} from "@convex-dev/platform/deploymentApi";
import {
  CheckCircledIcon,
  ChevronRightIcon,
  ExternalLinkIcon,
} from "@radix-ui/react-icons";
import { formatDistanceToNow } from "date-fns";
import { Button } from "@ui/Button";
import { Modal } from "@ui/Modal";
import { Tooltip } from "@ui/Tooltip";
import { ProgressBar, ProgressBarWithPercent } from "@ui/ProgressBar";
import { Spinner } from "@ui/Spinner";
import { cn } from "@ui/cn";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import {
  DeploymentInfoContext,
  PermissionsContext,
} from "@common/lib/deploymentContext";
import { useActiveDataSyncs } from "@common/features/settings/lib/api";
import { formatNumber, formatNumberCompact } from "@common/lib/format";
import { HealthLabel } from "./HealthIndicator";

// Sync ids minted for the Fivetran connector carry this prefix (see
// `DataSyncClient::sync_id_prefix` in the backend), which is what narrows the
// deployment-wide active sync listing down to this integration.
const FIVETRAN_SYNC_ID_PREFIX = "fivetran-";

// Counts are optional after the snapshot because an S3 export stores only a
// timestamp by then.
export type SyncStatus =
  | ActiveDataSyncSnapshotting
  | {
      type: "stale" | "upToDate";
      syncedTs: number;
      totalTables?: number;
      numDocumentsSynced?: number;
    };

// The backend drops a sync 3 days after its most recent page. Past the halfway
// mark, a sync is idle enough that its presence in the list needs explaining.
const IDLE_SYNC_MS = 1.5 * 24 * 60 * 60 * 1000;

/**
 * The right-hand side of the Fivetran card: its active syncs once they load,
 * and setup instructions until then.
 *
 * Lives in its own component so the listing -- and the permission lookup that
 * gates it, which fans out into several team and role queries -- runs for the
 * one card that reports syncs rather than for every card on the page.
 */
export function FivetranSyncStatus({ setupHref }: { setupHref: string }) {
  const { showFivetranSyncProgress } = useContext(DeploymentInfoContext);
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canViewData = useIsOperationAllowed("ViewData");
  const allSyncs = useActiveDataSyncs(showFivetranSyncProgress && canViewData);
  const syncs = useMemo(
    () =>
      allSyncs?.filter((sync) =>
        sync.syncId.startsWith(FIVETRAN_SYNC_ID_PREFIX),
      ),
    [allSyncs],
  );

  if (syncs && syncs.length > 0) {
    // Syncs are already running, so setup instructions are the less useful
    // thing to offer here.
    return <ActiveFivetranSyncsButton syncs={syncs} />;
  }
  return (
    <Button
      href={setupHref}
      target="_blank"
      className="flex items-center gap-2"
      inline
      variant="neutral"
    >
      <div>Get Started</div>
      <ExternalLinkIcon />
    </Button>
  );
}

/**
 * Rolls the active syncs up into the status word and detail line every other
 * integration card shows, and opens the per-sync progress on click.
 */
function ActiveFivetranSyncsButton({ syncs }: { syncs: ActiveDataSync[] }) {
  const [isModalOpen, setIsModalOpen] = useState(false);
  // Several syncs have no single status between them, so the card counts them
  // and leaves the per-sync detail to the modal.
  const summary = syncs.length === 1 ? summarize(syncs[0].status) : undefined;
  return (
    <>
      <Button
        variant="unstyled"
        aria-label="Show Fivetran sync details"
        className="flex items-center gap-3 rounded-sm px-2 py-1 hover:bg-background-tertiary"
        onClick={() => setIsModalOpen(true)}
      >
        <div className="flex flex-col items-end">
          {summary ? (
            <>
              <HealthLabel type={summary.type}>{summary.label}</HealthLabel>
              <p className="text-xs text-content-secondary">{summary.detail}</p>
            </>
          ) : (
            <p className="text-xs">{`${syncs.length} active syncs`}</p>
          )}
        </div>
        <ChevronRightIcon className="text-content-secondary" />
      </Button>
      {isModalOpen && (
        <Modal
          onClose={() => setIsModalOpen(false)}
          title="Fivetran syncs"
          size="md"
        >
          <FivetranSyncProgress syncs={syncs} />
        </Modal>
      )}
    </>
  );
}

export function FivetranSyncProgress({ syncs }: { syncs: ActiveDataSync[] }) {
  return (
    <div className="flex flex-col gap-4 py-3">
      {syncs.map((sync) => (
        <SyncProgress
          key={sync.syncId}
          status={sync.status}
          isIdle={Date.now() - sync.lastUpdated > IDLE_SYNC_MS}
        />
      ))}
    </div>
  );
}

export function SyncProgress({
  status,
  isIdle = false,
}: {
  status: SyncStatus;
  isIdle?: boolean;
}) {
  // Traversing tables for the first time and streaming later changes are both
  // "the data isn't current yet", so they read as one state.
  const isSyncing = status.type !== "upToDate";
  return (
    <div className="flex flex-col gap-1">
      <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-1">
        <div className="flex items-center gap-1.5">
          {isSyncing ? (
            // `Spinner` defaults to `ml-auto` for use as a trailing element.
            <Spinner className="ml-0" />
          ) : (
            <CheckCircledIcon className="size-4 shrink-0 text-content-success" />
          )}
          <Tooltip
            tip={
              isIdle
                ? "A sync stays listed for 3 days after its most recent page."
                : undefined
            }
          >
            <p
              className={cn(
                "text-xs font-medium",
                isIdle && "underline decoration-dotted",
              )}
            >
              {isSyncing ? "Syncing" : "Up to date"}
            </p>
          </Tooltip>
        </div>
        {/* An initial snapshot has no consistent timestamp to report yet --
            its progress bar is the status. A sync past that point does, but
            while it is catching up that timestamp trails the data. */}
        {status.type !== "snapshotting" && (
          <TimestampDistance
            prefix={status.type === "stale" ? "Synced up until" : "Last synced"}
            date={new Date(status.syncedTs / 1e6)}
          />
        )}
      </div>
      {status.type === "snapshotting" && (
        <SnapshotProgressBar status={status} />
      )}
      {status.type === "snapshotting" ? (
        <p className="text-xs text-content-secondary">
          {snapshotDetail(status)}
        </p>
      ) : (
        status.totalTables !== undefined &&
        status.numDocumentsSynced !== undefined && (
          <p className="text-xs text-content-secondary">
            {`${formatNumber(status.totalTables)} ${status.totalTables === 1 ? "table" : "tables"} · ${formatNumber(status.numDocumentsSynced)} documents synced`}
          </p>
        )
      )}
    </div>
  );
}

function SnapshotProgressBar({
  status,
}: {
  status: ActiveDataSyncSnapshotting;
}) {
  // The totals are 0 until the deployment's table summaries finish
  // bootstrapping, leaving nothing to measure the snapshot against.
  if (status.totalDocuments === 0) {
    return (
      <ProgressBar fraction={undefined} ariaLabel="Initial snapshot progress" />
    );
  }
  return (
    <ProgressBarWithPercent
      // `numDocumentsSynced` counts tombstones and re-synced revisions, so it
      // can overshoot the total: cap below 100% rather than claim the snapshot
      // is finished.
      fraction={Math.min(
        0.99,
        status.numDocumentsSynced / status.totalDocuments,
      )}
      variant="stripes"
      ariaLabel="Initial snapshot progress"
    />
  );
}

function snapshotDetail(status: ActiveDataSyncSnapshotting): string {
  const table = status.currentComponent
    ? `${status.currentComponent}/${status.currentTable}`
    : status.currentTable;
  // Tables whose traversal has finished are counted in `numTablesSynced`, so
  // the table being read is the next one.
  const tableIndex = Math.min(status.numTablesSynced + 1, status.totalTables);
  const documents =
    status.totalDocuments === 0
      ? `${formatNumber(status.numDocumentsSynced)} documents synced`
      : `${formatNumberCompact(status.numDocumentsSynced)} of ${formatNumberCompact(status.totalDocuments)} documents`;
  return `${table} · table ${formatNumber(tableIndex)} of ${formatNumber(status.totalTables)} · ${documents}`;
}

/**
 * A lone sync gets the status word and detail line every other integration
 * card shows. Only a sync that has caught up counts as "Active": until then
 * the data a consumer can read is behind the deployment.
 */
export function summarize(status: SyncStatus): {
  type: "active" | "pending";
  label: string;
  detail: string;
} {
  if (status.type === "snapshotting") {
    return {
      type: "pending",
      label: "Syncing",
      detail: snapshotProgressLabel(status),
    };
  }
  // `syncedTs` is nanoseconds since the epoch.
  const syncedAt = distanceToNow(status.syncedTs / 1e6);
  return status.type === "stale"
    ? {
        type: "pending",
        label: "Syncing",
        detail: `Synced up until ${syncedAt}`,
      }
    : { type: "active", label: "Active", detail: `Last synced ${syncedAt}` };
}

function distanceToNow(ms: number): string {
  return formatDistanceToNow(new Date(ms), { addSuffix: true }).replace(
    "about ",
    "",
  );
}

function snapshotProgressLabel(status: ActiveDataSyncSnapshotting): string {
  if (status.totalDocuments === 0) {
    return "initial snapshot";
  }
  const percent = Math.round(
    Math.min(0.99, status.numDocumentsSynced / status.totalDocuments) * 100,
  );
  return `${percent}% of initial snapshot`;
}
