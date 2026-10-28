import React from "react";
import type { Value } from "convex/values";
import type { DocumentByName, SystemDataModel } from "convex/server";
import { DownloadIcon, ExternalLinkIcon } from "@radix-ui/react-icons";
import { getVisibleTableName } from "@common/lib/utils";
import { formatBytes } from "@common/lib/format";
import { stringifyValue } from "@common/lib/stringifyValue";
import { ReadonlyCode } from "@common/elements/ReadonlyCode";
import { DetailPanel } from "@common/elements/DetailPanel";
import { Button } from "@ui/Button";
import type { FileMetadata } from "system-udfs/convex/_system/frontend/fileStorageV2";
import type { NavigationDestination } from "./navigation";

export type ScheduledFunctionDoc = DocumentByName<
  SystemDataModel,
  "_scheduled_functions"
>;

export const SCHEDULED_STATE_LABEL: Record<
  ScheduledFunctionDoc["state"]["kind"],
  { label: string; className: string }
> = {
  pending: { label: "Pending", className: "text-content-warning" },
  inProgress: { label: "Running", className: "text-content-primary" },
  success: { label: "Success", className: "text-content-success" },
  failed: { label: "Failed", className: "text-content-error" },
  canceled: { label: "Canceled", className: "text-content-tertiary" },
};

export type DocumentSearchResultItem =
  | {
      type: "document";
      tableName: string;
      id: string;
      value: Value;
      target: NavigationDestination;
    }
  | { type: "file"; file: FileMetadata; target: NavigationDestination }
  | {
      type: "scheduled";
      job: ScheduledFunctionDoc;
      target: NavigationDestination;
    };

export function DocumentSearchResult({
  detail,
  onNavigate,
  onClose,
}: {
  detail: DocumentSearchResultItem;
  onNavigate: (to: NavigationDestination) => void;
  onClose: () => void;
}) {
  const { header, action, body } = detailContent(detail);
  return (
    <DetailPanel
      onClose={onClose}
      header={
        <span className="flex flex-wrap items-center gap-x-3 gap-y-1">
          <span>{header}</span>
          <span className="flex gap-2">
            {detail.type === "file" && (
              <Button
                variant="neutral"
                size="sm"
                icon={<DownloadIcon />}
                href={detail.file.url}
                download
                target="_blank"
              >
                Download
              </Button>
            )}
            <Button
              variant="neutral"
              size="sm"
              icon={<ExternalLinkIcon />}
              onClick={() => onNavigate(detail.target)}
            >
              {action}
            </Button>
          </span>
        </span>
      }
      content={<div className="h-full">{body}</div>}
    />
  );
}

function detailContent(detail: DocumentSearchResultItem): {
  header: string;
  action: string;
  body: React.ReactNode;
} {
  switch (detail.type) {
    case "document":
      return {
        header: `Document in ${getVisibleTableName(detail.tableName)}`,
        action: "View in Data",
        body: (
          <div className="h-full overflow-hidden rounded-sm border p-4">
            <ReadonlyCode
              path={`commandPalette-document-${detail.id}`}
              code={stringifyValue(detail.value, true)}
              disableLineNumbers
            />
          </div>
        ),
      };
    case "file":
      return {
        header: "File",
        action: "View in Files",
        body: <FileDetailBody file={detail.file} />,
      };
    case "scheduled":
      return {
        header: "Scheduled function",
        action: "View in Scheduled Jobs",
        body: <ScheduledDetailBody job={detail.job} />,
      };
    default: {
      detail satisfies never;
      return { header: "", action: "", body: null };
    }
  }
}

function DetailField({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex gap-3 text-sm">
      <span className="w-28 shrink-0 text-content-tertiary">{label}</span>
      <span className="min-w-0 wrap-break-word text-content-primary">
        {children}
      </span>
    </div>
  );
}

function FileDetailBody({ file }: { file: FileMetadata }) {
  return (
    <div className="flex flex-col gap-3">
      <DetailField label="Storage ID">
        <span className="font-mono">{file._id}</span>
      </DetailField>
      <DetailField label="Size">{formatBytes(Number(file.size))}</DetailField>
      <DetailField label="Content type">
        {file.contentType || "Unknown"}
      </DetailField>
      <DetailField label="Uploaded">
        {new Date(file._creationTime).toLocaleString()}
      </DetailField>
    </div>
  );
}

function ScheduledDetailBody({ job }: { job: ScheduledFunctionDoc }) {
  const status = SCHEDULED_STATE_LABEL[job.state.kind];
  return (
    <div className="flex flex-col gap-3">
      <DetailField label="Function">
        <span className="font-mono">{job.name}</span>
      </DetailField>
      <DetailField label="Status">
        <span className={status.className}>{status.label}</span>
        {job.state.kind === "failed" && (
          <span className="text-content-error"> · {job.state.error}</span>
        )}
      </DetailField>
      <DetailField label="Scheduled">
        {new Date(job.scheduledTime).toLocaleString()}
      </DetailField>
      {job.completedTime !== undefined && (
        <DetailField label="Completed">
          {new Date(job.completedTime).toLocaleString()}
        </DetailField>
      )}
      <DetailField label="Arguments">
        <span className="font-mono break-all">{stringifyValue(job.args)}</span>
      </DetailField>
    </div>
  );
}
