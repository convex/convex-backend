import {
  CheckCircledIcon,
  CheckIcon,
  ChevronDownIcon,
  ChevronUpIcon,
  CrossCircledIcon,
} from "@radix-ui/react-icons";
import { Spinner } from "@ui/Spinner";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { Tooltip } from "@ui/Tooltip";
import { Button } from "@ui/Button";
import { Sheet } from "@ui/Sheet";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { TeamMemberLink } from "elements/TeamMemberLink";
import { useQuery } from "convex/react";
import udfs from "@common/udfs";
import { Doc, Id } from "system-udfs/convex/_generated/dataModel";
import { formatDistanceStrict } from "date-fns";
import { Link } from "@ui/Link";
import { useCurrentTeam, useTeamMembers } from "api/teams";
import { snapshotImportFormat } from "system-udfs/convex/tableDefs/snapshotImport";
import { Infer } from "convex/values";
import { useCancelImport, useConfirmImport } from "hooks/deploymentApi";
import {
  Disclosure,
  DisclosureButton,
  DisclosurePanel,
} from "@headlessui/react";
import { useState } from "react";
import { PuzzlePieceIcon } from "@common/elements/icons";

function ConfirmImportButton({
  snapshotImport,
}: {
  snapshotImport: Doc<"_snapshot_imports">;
}) {
  const [showDialog, setShowDialog] = useState(false);
  const confirmImport = useConfirmImport();
  return (
    <>
      {showDialog && (
        <ConfirmationDialog
          confirmText="Confirm"
          dialogTitle="Confirm import"
          variant="primary"
          dialogBody={
            <div>
              <div>Are you sure you want to perform the following import?</div>
              <ImportSummary
                snapshotImportCheckpoints={snapshotImport.checkpoints}
              />
            </div>
          }
          onConfirm={() => confirmImport(snapshotImport._id)}
          onClose={() => setShowDialog(false)}
        />
      )}
      <Button onClick={() => setShowDialog(true)}>Confirm</Button>
    </>
  );
}

function CancelImportButton({
  importId,
}: {
  importId: Id<"_snapshot_imports">;
}) {
  const [showDialog, setShowDialog] = useState(false);
  const cancelImport = useCancelImport();
  return (
    <>
      {showDialog && (
        <ConfirmationDialog
          confirmText="Confirm"
          dialogTitle="Cancel import"
          dialogBody="Are you sure you want to cancel this import?"
          onConfirm={() => cancelImport(importId)}
          onClose={() => setShowDialog(false)}
        />
      )}
      <Button variant="danger" onClick={() => setShowDialog(true)}>
        Cancel
      </Button>
    </>
  );
}

function ImportStateBody({
  snapshotImport,
}: {
  snapshotImport: Doc<"_snapshot_imports">;
}) {
  switch (snapshotImport.state.state) {
    case "uploaded":
      return (
        <div className="flex items-center gap-2">
          <Spinner className="ml-0" /> Uploading snapshot
        </div>
      );
    case "waiting_for_confirmation":
      return (
        <div>
          {(snapshotImport.checkpoints === null ||
            snapshotImport.checkpoints === undefined) && (
            <div className="font-mono whitespace-pre-wrap">
              {snapshotImport.state.message_to_confirm}
            </div>
          )}
          <div className="flex gap-2">
            <ConfirmImportButton snapshotImport={snapshotImport} />
            <CancelImportButton importId={snapshotImport._id} />
          </div>
        </div>
      );
    case "in_progress":
      return (
        <div>
          <CancelImportButton importId={snapshotImport._id} />
          <div className="flex flex-col">
            {snapshotImport.state.checkpoint_messages.map((message, index) => (
              <div key={index} className="flex items-center gap-2">
                <CheckIcon /> {message}
              </div>
            ))}
            <div className="flex items-center gap-2">
              <Spinner className="ml-0" />{" "}
              {snapshotImport.state.progress_message}
            </div>
          </div>
        </div>
      );
    case "completed": {
      const completedDate = new Date(
        Number(snapshotImport.state.timestamp / BigInt(1_000_000)),
      );
      return (
        <div>
          <Tooltip tip={completedDate.toLocaleString()}>
            <div className="flex items-center gap-1 border p-1 text-sm text-content-primary">
              <CheckCircledIcon className="min-w-4 text-util-success" />
              {`Completed ${formatDistanceStrict(completedDate, new Date(), {
                addSuffix: true,
              })}`}
            </div>
          </Tooltip>
        </div>
      );
    }
    case "failed":
      return (
        <div className="flex w-fit items-center gap-1 rounded-sm border p-1 text-sm">
          <CrossCircledIcon className="min-w-4 text-content-errorSecondary" />
          {snapshotImport.state.error_message}
        </div>
      );

    default:
      throw new Error(
        `unexpected snapshot import state ${snapshotImport.state}`,
      );
  }
}

function ImportStatePill({
  snapshotImportState,
}: {
  snapshotImportState: Doc<"_snapshot_imports">["state"]["state"];
}) {
  switch (snapshotImportState) {
    case "uploaded":
    case "waiting_for_confirmation":
      return (
        <span className="size-fit rounded-sm bg-blue-100 p-1 text-center text-xs text-blue-900 dark:bg-blue-900 dark:text-blue-100">
          pending confirmation
        </span>
      );
    case "in_progress":
      return (
        <span className="size-fit rounded-sm bg-blue-100 p-1 text-center text-xs text-blue-900 dark:bg-blue-900 dark:text-blue-100">
          in progress
        </span>
      );

    case "completed":
      return (
        <span className="h-fit w-14 rounded-sm bg-background-success p-1 text-center text-xs text-content-success">
          success
        </span>
      );
    case "failed":
      return (
        <span className="h-fit w-14 rounded-sm bg-background-error p-1 text-center text-xs text-content-error">
          failure
        </span>
      );
    default:
      throw new Error(
        `unexpected snapshot import state ${snapshotImportState}`,
      );
  }
}

function snapshotImportFormatToText(
  format: Infer<typeof snapshotImportFormat>,
) {
  switch (format.format) {
    case "csv":
      return "CSV";
    case "jsonl":
      return "JSONL";
    case "json_array":
      return "JSON";
    case "zip":
      return "ZIP";
    default: {
      format satisfies never;
      return "";
    }
  }
}

export function ImportSummary({
  snapshotImportCheckpoints,
}: {
  snapshotImportCheckpoints:
    | Doc<"_snapshot_imports">["checkpoints"]
    | null
    | undefined;
}) {
  if (
    snapshotImportCheckpoints === null ||
    snapshotImportCheckpoints === undefined
  ) {
    return null;
  }
  const checkpointsByComponent = snapshotImportCheckpoints.reduce(
    (acc, checkpoint) => {
      const componentPath = checkpoint.component_path ?? undefined;
      if (!acc.has(componentPath)) {
        acc.set(componentPath, []);
      }
      acc.get(componentPath)?.push(checkpoint);
      return acc;
    },
    new Map<
      string | undefined,
      NonNullable<Doc<"_snapshot_imports">["checkpoints"]>
    >(),
  );
  return (
    <div className="flex flex-col gap-4">
      {Array.from(checkpointsByComponent.entries()).map(
        ([componentPath, checkpoints]) => (
          <div key={componentPath}>
            {componentPath ? (
              <h5 className="mb-1 flex w-full items-center space-x-1">
                <PuzzlePieceIcon className="text-content-secondary" />
                <span>{componentPath}</span>
              </h5>
            ) : null}
            <table className="mr-auto border-collapse border text-left text-xs">
              <thead className="border">
                <tr>
                  <th className="border px-2 py-0.5 font-semibold">Table</th>
                  <th className="border px-2 py-0.5 font-semibold">Created</th>
                  <th className="border px-2 py-0.5 font-semibold">Deleted</th>
                </tr>
              </thead>
              <tbody>
                {checkpoints.map((checkpoint) => (
                  <tr key={checkpoint.display_table_name}>
                    <td className="border px-2 py-0.5 font-mono">
                      <span>{checkpoint.display_table_name}</span>
                    </td>
                    <td className="border px-2 py-0.5 tabular-nums">
                      {Number(
                        checkpoint.total_num_rows_to_write,
                      ).toLocaleString()}{" "}
                      {checkpoint.display_table_name === "_storage"
                        ? `file${Number(checkpoint.total_num_rows_to_write) === 1 ? "" : "s"}`
                        : `document${Number(checkpoint.total_num_rows_to_write) === 1 ? "" : "s"}`}
                    </td>
                    <td className="border px-2 py-0.5 tabular-nums">
                      {Number(
                        checkpoint.existing_rows_to_delete,
                      ).toLocaleString()}{" "}
                      of{" "}
                      {Number(
                        checkpoint.existing_rows_in_table,
                      ).toLocaleString()}{" "}
                      {checkpoint.display_table_name === "_storage"
                        ? "files"
                        : "documents"}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ),
      )}
    </div>
  );
}

export function ImportState({
  snapshotImport,
}: {
  snapshotImport: Doc<"_snapshot_imports"> & { memberName: string };
}) {
  return (
    <Disclosure>
      {({ open }) => (
        <>
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex gap-2">
              <ImportStatePill
                snapshotImportState={snapshotImport.state.state}
              />
              <div className="flex flex-wrap items-center gap-1">
                <TeamMemberLink
                  memberId={
                    snapshotImport.member_id === null ||
                    snapshotImport.member_id === undefined
                      ? snapshotImport.member_id
                      : Number(snapshotImport.member_id)
                  }
                  name={snapshotImport.memberName}
                />

                {`imported a snapshot from a ${snapshotImportFormatToText(snapshotImport.format)} file.`}
              </div>
            </div>
            <div className="flex items-center gap-2">
              <TimestampDistance
                prefix="Started "
                date={new Date(snapshotImport._creationTime)}
              />
              <Tooltip side="left" tip="View entry metadata" asChild>
                <DisclosureButton
                  as={Button}
                  inline
                  variant="neutral"
                  size="xs"
                  aria-label={open ? "Hide details" : "Show details"}
                >
                  {open ? <ChevronUpIcon /> : <ChevronDownIcon />}
                </DisclosureButton>
              </Tooltip>
            </div>
          </div>
          <DisclosurePanel>
            <div className="mt-2 flex flex-col gap-2">
              <ImportStateBody snapshotImport={snapshotImport} />
              <ImportSummary
                snapshotImportCheckpoints={snapshotImport.checkpoints}
              />
            </div>
          </DisclosurePanel>
        </>
      )}
    </Disclosure>
  );
}

const useSnapshotImports = () => {
  const existingImports: Doc<"_snapshot_imports">[] | undefined =
    useQuery(udfs.snapshotImport.list) ?? [];
  const currentTeam = useCurrentTeam();
  const teamMembers = useTeamMembers(currentTeam?.id) ?? [];
  return existingImports
    ?.filter((s) => s.requestor.type === "snapshotImport")
    .map((s) => {
      const member = teamMembers.find((m) => BigInt(m.id) === s.member_id);
      const memberName = member?.name || member?.email || "Unknown member";
      return {
        ...s,
        memberName,
      };
    });
};

export function SnapshotImport() {
  const existingImports = useSnapshotImports();

  return (
    <Sheet>
      <div className="flex flex-col gap-4">
        <div className="flex flex-col gap-4">
          <div>
            <h3 className="mb-2">Snapshot Import and Cloud Restore</h3>
            <p className="text-content-primary">
              Import tables into your database from a snapshot.{" "}
              <Link
                target="_blank"
                href="https://docs.convex.dev/database/import-export/import"
              >
                Learn more
              </Link>
            </p>
          </div>
        </div>
        {existingImports && existingImports.length !== 0 && (
          <div className="flex flex-col gap-4">
            <h4>Recent Imports</h4>
            {existingImports.map(
              (
                existingImport: Doc<"_snapshot_imports"> & {
                  memberName: string;
                },
              ) => (
                <div className="text-content-primary" key={existingImport._id}>
                  <ImportState snapshotImport={existingImport} />
                </div>
              ),
            )}
          </div>
        )}
      </div>
    </Sheet>
  );
}
