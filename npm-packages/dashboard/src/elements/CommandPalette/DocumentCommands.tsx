import { Command } from "cmdk";
import React, { useContext } from "react";
import { useQuery } from "convex/react";
import type { Value } from "convex/values";
import { FileIcon, StopwatchIcon, TableIcon } from "@radix-ui/react-icons";
import udfs from "@common/udfs";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { documentHref, getVisibleTableName } from "@common/lib/utils";
import { formatBytes } from "@common/lib/format";
import { stringifyValue } from "@common/lib/stringifyValue";
import type { FileMetadata } from "system-udfs/convex/_system/frontend/fileStorageV2";
import { REMOTE_VALUE_PREFIX } from "./navigation";
import { type PaletteCopyAction, useCopyAction } from "./copy";
import {
  SCHEDULED_STATE_LABEL,
  type ScheduledFunctionDoc,
  type DocumentSearchResultItem,
} from "./DocumentSearchResult";

export type DocumentRef = {
  tableName: string;
  id: string;
  componentId: string | null;
};

// A document's value - if it's a file, returns the file metadata instead.
export function useDocumentValue(
  document: DocumentRef | null,
): Value | FileMetadata | undefined {
  const isFile = document?.tableName === "_file_storage";
  const file = useQuery(
    udfs.fileStorageV2.getFile,
    document && isFile
      ? {
          storageId: document.id,
          componentId: document.componentId ?? undefined,
        }
      : "skip",
  );
  const value = useQuery(
    udfs.getById.default,
    document && !isFile
      ? { id: document.id, componentId: document.componentId }
      : "skip",
  );
  return (isFile ? file : value) ?? undefined;
}

export function documentGroupHeading(tableName: string): React.ReactNode {
  switch (tableName) {
    case "_file_storage":
      return "File";
    case "_scheduled_jobs":
      return "Scheduled function";
    default:
      return (
        <span>
          Document in{" "}
          <span className="font-mono">{getVisibleTableName(tableName)}</span>
        </span>
      );
  }
}

export function ViewDocumentItem({
  document,
  value,
  onOpenDetail,
}: {
  document: DocumentRef;
  value: Value | FileMetadata;
  onOpenDetail: (detail: DocumentSearchResultItem) => void;
}) {
  const { deploymentsURI, captureMessage } = useContext(DeploymentInfoContext);
  switch (document.tableName) {
    case "_file_storage":
      return (
        <FileRow
          document={document}
          file={value as FileMetadata}
          onOpenDetail={onOpenDetail}
        />
      );
    case "_scheduled_jobs":
      return (
        <ScheduledRow
          document={document}
          job={value as unknown as ScheduledFunctionDoc}
          onOpenDetail={onOpenDetail}
        />
      );
    default:
      return (
        <ResultItem
          value={rowValue(document)}
          Icon={TableIcon}
          title={document.id}
          description={stringifyValue(value)}
          copy={{
            label: "document",
            getText: () => stringifyValue(value, true),
          }}
          onSelect={() =>
            onOpenDetail({
              type: "document",
              tableName: document.tableName,
              id: document.id,
              value,
              target: documentHref({
                deploymentsURI,
                tableName: document.tableName,
                id: document.id,
                componentId: document.componentId,
                captureMessage,
              }),
            })
          }
        />
      );
  }
}

function FileRow({
  document,
  file,
  onOpenDetail,
}: {
  document: DocumentRef;
  file: FileMetadata;
  onOpenDetail: (detail: DocumentSearchResultItem) => void;
}) {
  const { deploymentsURI } = useContext(DeploymentInfoContext);
  return (
    <ResultItem
      value={rowValue(document)}
      Icon={FileIcon}
      title={file._id}
      description={`${file.contentType || "Unknown type"} · ${formatBytes(Number(file.size))}`}
      copy={{ label: "storage ID", getText: () => file._id }}
      onSelect={() =>
        onOpenDetail({
          type: "file",
          file,
          target: {
            pathname: `${deploymentsURI}/files`,
            query: {
              id: file._id,
              ...(document.componentId
                ? { component: document.componentId }
                : {}),
            },
          },
        })
      }
    />
  );
}

function ScheduledRow({
  document,
  job,
  onOpenDetail,
}: {
  document: DocumentRef;
  job: ScheduledFunctionDoc;
  onOpenDetail: (detail: DocumentSearchResultItem) => void;
}) {
  const { deploymentsURI } = useContext(DeploymentInfoContext);
  const status = SCHEDULED_STATE_LABEL[job.state.kind];
  return (
    <ResultItem
      value={rowValue(document)}
      Icon={StopwatchIcon}
      title={job.name}
      description={
        <>
          <span className={status.className}>{status.label}</span>
          {job.state.kind === "failed" && ` · ${job.state.error}`}
          {" · "}
          {stringifyValue(job.args)}
        </>
      }
      copy={{
        label: "document",
        getText: () => stringifyValue(job as unknown as Value, true),
      }}
      onSelect={() =>
        onOpenDetail({
          type: "scheduled",
          job,
          target: {
            pathname: `${deploymentsURI}/schedules/functions`,
            query: {
              function: job.name,
              ...(document.componentId
                ? { component: document.componentId }
                : {}),
            },
          },
        })
      }
    />
  );
}

export function ResultItem({
  value,
  Icon,
  title,
  description,
  copy,
  onSelect,
}: {
  value: string;
  Icon: React.FC<{ className?: string }>;
  // Every row here is titled by an identifier — a document ID, a storage ID, a
  // function name — so the title is always set in mono.
  title: string;
  description: React.ReactNode;
  copy: PaletteCopyAction;
  onSelect: () => void;
}) {
  useCopyAction(value, copy);
  return (
    <Command.Item
      value={value}
      className="animate-fadeInFromLoading"
      onSelect={onSelect}
    >
      <Icon className="text-content-secondary" />
      <span className="flex min-w-0 flex-col">
        <span className="truncate font-mono">{title}</span>
        <span className="truncate text-xs text-content-tertiary">
          {description}
        </span>
      </span>
    </Command.Item>
  );
}

function rowValue(document: DocumentRef): string {
  return `${REMOTE_VALUE_PREFIX}document-open:${document.tableName}:${document.id}`;
}
