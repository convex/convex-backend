import { componentTablesIncluded } from "./s3ExportSelection";
import { useContext, useState } from "react";
import { ChevronRightIcon, ExternalLinkIcon } from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { Link } from "@ui/Link";
import { Modal } from "@ui/Modal";
import { Spinner } from "@ui/Spinner";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { ConnectedDeploymentContext } from "@common/lib/deploymentContext";
import { AnalyticsIntegration } from "@common/lib/integrationHelpers";
import { SyncProgress, SyncStatus, summarize } from "./FivetranSyncProgress";
import { HealthIndicator, HealthLabel } from "./HealthIndicator";

type S3Export = NonNullable<
  Extract<AnalyticsIntegration, { kind: "s3Export" }>["existing"]
>;

export function useDeploymentName(): string {
  return useContext(ConnectedDeploymentContext).deployment.deploymentName;
}

// Mirrors `warehouse_uri` in crates_private/porter/src/managed_sync.rs. Porter
// names the export's directory after the instance, which is the deployment
// name.
export function exportRoot(prefix: string | undefined, deploymentName: string) {
  return [...(prefix ?? "").split("/").filter(Boolean), deploymentName].join(
    "/",
  );
}

// Mirrors `glue_database_name` in crates_private/porter/src/managed_sync.rs.
export function glueDatabaseName(deploymentName: string) {
  return deploymentName.toLowerCase().replace(/[^a-z0-9]/g, "_");
}

/**
 * The least-privilege IAM policy the export's access key needs. The account ID
 * is `*` so users don't have to look theirs up. The key still can't reach
 * another account's catalog unless that account grants it access.
 */
export function iamPolicy({
  bucket,
  region,
  prefix,
  deploymentName,
}: {
  bucket: string;
  region: string;
  prefix: string;
  deploymentName: string;
}): string {
  const root = exportRoot(prefix, deploymentName);
  const glue = `arn:aws:glue:${region || "<region>"}:*`;
  const database = glueDatabaseName(deploymentName);
  const bucketArn = `arn:aws:s3:::${bucket || "<bucket>"}`;
  return JSON.stringify(
    {
      Version: "2012-10-17",
      Statement: [
        {
          Effect: "Allow",
          Action: ["s3:ListBucket"],
          Resource: bucketArn,
          Condition: { StringLike: { "s3:prefix": [`${root}/*`] } },
        },
        {
          Effect: "Allow",
          // Iceberg uploads large files in parts and aborts the upload when a
          // write fails.
          Action: [
            "s3:GetObject",
            "s3:PutObject",
            "s3:DeleteObject",
            "s3:AbortMultipartUpload",
          ],
          Resource: `${bucketArn}/${root}/*`,
        },
        {
          Effect: "Allow",
          Action: [
            "glue:GetDatabase",
            "glue:CreateDatabase",
            "glue:GetTable",
            "glue:GetTables",
            "glue:CreateTable",
            "glue:UpdateTable",
            "glue:DeleteTable",
          ],
          Resource: [
            `${glue}:catalog`,
            `${glue}:database/${database}`,
            `${glue}:table/${database}/*`,
          ],
        },
      ],
    },
    null,
    2,
  );
}

/**
 * The right-hand side of the S3 export card: the sync's status, and its
 * destination on click.
 */
export function S3ExportStatus({ existing }: { existing: S3Export }) {
  const [isModalOpen, setIsModalOpen] = useState(false);
  const status = syncStatus(existing);
  const summary = status ? summarize(status) : undefined;
  return (
    <>
      <Button
        variant="unstyled"
        aria-label="Show S3 export details"
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
            <>
              {needsRoleArn(existing) ? (
                <HealthLabel type="pending">Finish AWS setup</HealthLabel>
              ) : isStarting(existing) ? (
                <HealthLabel type="pending">Starting first sync</HealthLabel>
              ) : (
                <HealthIndicator status={existing.status} />
              )}
              <TimestampDistance
                prefix="Created"
                date={new Date(existing._creationTime)}
              />
            </>
          )}
        </div>
        <ChevronRightIcon className="text-content-secondary" />
      </Button>
      {isModalOpen && (
        <Modal onClose={() => setIsModalOpen(false)} title="AWS S3" size="md">
          <S3ExportDetails existing={existing} />
        </Modal>
      )}
    </>
  );
}

function S3ExportDetails({ existing }: { existing: S3Export }) {
  const deploymentName = useDeploymentName();
  const { bucket, region, prefix } = existing.config;
  const dataLakePath = `${exportRoot(prefix, deploymentName)}/data-lake/`;
  const database = glueDatabaseName(deploymentName);
  const status = syncStatus(existing);
  const components = componentTablesIncluded(existing.config.selection);
  return (
    <div className="flex flex-col gap-4 py-3 text-xs">
      {status ? (
        <SyncProgress status={status} />
      ) : needsRoleArn(existing) ? (
        <p className="font-medium">
          Waiting for the role ARN. Edit the export to launch the CloudFormation
          stack and paste it.
        </p>
      ) : isStarting(existing) ? (
        <div className="flex items-center gap-1.5">
          {/* `Spinner` defaults to `ml-auto` for use as a trailing element. */}
          <Spinner className="ml-0" />
          <p className="font-medium">Starting first sync</p>
        </div>
      ) : (
        <div className="flex flex-col gap-1">
          <HealthIndicator status={existing.status} />
          {existing.status.type === "failed" && (
            <p className="text-content-errorSecondary">
              {existing.status.reason}
            </p>
          )}
        </div>
      )}
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2">
        <dt className="text-content-secondary">Component tables</dt>
        <dd>
          {components === "indeterminate"
            ? "Custom selection"
            : components
              ? "Included"
              : "Excluded"}
        </dd>
        <dt className="text-content-secondary">S3 location</dt>
        <dd className="min-w-0 break-all">
          <Link
            href={`https://s3.console.aws.amazon.com/s3/buckets/${bucket}?region=${region}&prefix=${encodeURIComponent(dataLakePath)}`}
            target="_blank"
            className="inline-flex items-center gap-1"
          >
            s3://{bucket}/{dataLakePath}
            <ExternalLinkIcon className="shrink-0" />
          </Link>
        </dd>
        <dt className="text-content-secondary">Glue database</dt>
        <dd className="min-w-0 break-all">
          <Link
            href={`https://${region}.console.aws.amazon.com/glue/home?region=${region}#/v2/data-catalog/databases/view/${database}`}
            target="_blank"
            className="inline-flex items-center gap-1"
          >
            {database}
            <ExternalLinkIcon className="shrink-0" />
          </Link>
        </dd>
      </dl>
    </div>
  );
}

// A role export the admin has not finished on the AWS side. A failure or a
// deletion in progress says more, so those statuses win.
export function needsRoleArn(existing: S3Export) {
  return (
    existing.config.roleArn === "" &&
    existing.status.type !== "failed" &&
    existing.status.type !== "deleting"
  );
}

// No page has landed yet, and nothing has gone wrong.
function isStarting(existing: S3Export) {
  return (
    !existing.exportProgress &&
    existing.status.type !== "failed" &&
    existing.status.type !== "deleting"
  );
}

/**
 * The export's progress in the shape the Fivetran status UI takes. Undefined
 * when the sink's own status says more: before the first page, or once the
 * export has failed or is being deleted.
 */
export function syncStatus(existing: S3Export): SyncStatus | undefined {
  const progress = existing.exportProgress;
  if (
    !progress ||
    existing.status.type === "failed" ||
    existing.status.type === "deleting"
  ) {
    return undefined;
  }
  if (progress.type !== "snapshotting") {
    // Both `ts` and `syncedTs` are nanoseconds since the epoch.
    return { type: progress.type, syncedTs: Number(progress.ts) };
  }
  // The stored progress uses int64s and nulls for unknown totals, where the
  // Fivetran UI expects numbers and 0.
  return {
    type: "snapshotting",
    numTablesSynced: Number(progress.numTablesSynced),
    totalTables: Number(progress.totalTables),
    currentComponent: progress.currentComponent,
    currentTable: progress.currentTable,
    numDocumentsInCurrentTable: Number(progress.numDocumentsInCurrentTable),
    totalDocumentsInCurrentTable: Number(
      progress.totalDocumentsInCurrentTable ?? 0,
    ),
    numDocumentsSynced: Number(progress.numDocumentsSynced),
    totalDocuments: Number(progress.totalDocuments ?? 0),
  };
}
