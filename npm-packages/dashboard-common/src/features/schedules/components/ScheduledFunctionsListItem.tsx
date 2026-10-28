import { DotsVerticalIcon } from "@radix-ui/react-icons";
import { PermissionDeniedTip } from "@common/elements/NoPermissionMessage";
import { JSONValue, jsonToConvex } from "convex/values";
import Link from "next/link";
import { memo, useContext, useState } from "react";
import { ScheduledJob } from "system-udfs/convex/_system/frontend/common";
import { areEqual } from "react-window";
import { useCancelJob } from "@common/features/schedules/lib/api";
import { useNents } from "@common/lib/useNents";
import { PermissionsContext } from "@common/lib/deploymentContext";
import { useFunctionUrl } from "@common/lib/deploymentApi";
import { useCopy } from "@common/lib/useCopy";
import { DetailPanel } from "@common/elements/DetailPanel";
import { ReadonlyCode } from "@common/elements/ReadonlyCode";
import { Loading } from "@ui/Loading";
import { stringifyValue } from "@common/lib/stringifyValue";
import { prettier } from "@common/lib/format";
import { FunctionNameOption } from "@common/elements/FunctionNameOption";
import { functionIdentifierValue } from "@common/lib/functions/generateFileTree";
import { Menu, MenuItem } from "@ui/Menu";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { useQuery } from "convex/react";
import udfs from "@common/udfs";
import { Id } from "system-udfs/convex/_generated/dataModel";

type JobItemProps = {
  data: { jobs: ScheduledJob[] };
  index: number;
  style: React.CSSProperties;
};
export const JOB_ITEM_SIZE = 50;

export const ScheduledFunctionsListItem = memo(JobItem, areEqual);

function JobItem({ data, index, style }: JobItemProps) {
  const job = data.jobs[index];
  if (!job) {
    return (
      <div style={{ height: JOB_ITEM_SIZE, ...style }}>
        <Loading>
          <div className="h-4" />
        </Loading>
      </div>
    );
  }

  return <JobItemImpl job={job} style={style} />;
}

function JobItemImpl({
  job,
  style,
}: {
  job: ScheduledJob;
  style: React.CSSProperties;
}) {
  const {
    nextTs,
    udfArgs,
    state,
    _id,
    udfPath,
    component: componentPath,
    argsId,
  } = job;
  const { nents } = useNents();
  const componentId = componentPath
    ? (nents?.find((n) => n.path === componentPath)?.id ?? null)
    : null;
  const url = useFunctionUrl(udfPath, componentId);
  const [showDeleteModal, setShowDeleteModal] = useState(false);
  const [showArgs, setShowArgs] = useState(false);
  const { selectedNent } = useNents();
  const cancelJob = useCancelJob();
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canCancelJobs = useIsOperationAllowed("WriteData");

  if (nextTs === null) {
    throw new Error("Could not find timestamp to run scheduled job at");
  }
  const date = new Date(Number(nextTs / BigInt(1000000))).toLocaleString();
  if (_id === null) {
    throw new Error("Scheduled job id is null");
  }

  const currentlyRunning = state.type === "inProgress";
  const copyId = useCopy("Scheduled Function ID");

  return (
    <div style={style} className="border-b transition-all last:border-b-0">
      {showArgs &&
        (udfArgs ? (
          <ShowArgsPanelWithArgs udfArgs={udfArgs} setShowArgs={setShowArgs} />
        ) : argsId ? (
          <ShowArgsPanel
            argsId={argsId}
            setShowArgs={setShowArgs}
            componentId={selectedNent?.id ?? null}
          />
        ) : null)}
      <div className="flex items-center gap-4 p-2 text-sm">
        {/* eslint-disable-next-line react/forbid-elements */}
        <button
          type="button"
          className="w-20 truncate text-left text-xs"
          onClick={() => copyId(_id)}
        >
          {_id.substring(0, 10)}...
        </button>
        <span className="w-36 text-xs">{date}</span>
        <span className="w-20 text-xs text-content-secondary">
          {currentlyRunning ? "Running" : "Pending"}
        </span>
        <div className="w-48 text-xs hover:underline">
          <Link href={url}>
            <FunctionNameOption
              label={functionIdentifierValue(
                udfPath,
                componentPath,
                componentId ?? undefined,
              )}
              oneLine
            />
          </Link>
        </div>
        <div className="ml-auto">
          <Menu
            placement="bottom-end"
            buttonProps={{
              "aria-label": "Scheduled function settings",
              icon: <DotsVerticalIcon />,
              size: "xs",
              variant: "neutral",
            }}
          >
            <MenuItem action={() => setShowArgs(true)}>View Arguments</MenuItem>
            <MenuItem
              action={() => setShowDeleteModal(true)}
              disabled={currentlyRunning || !canCancelJobs}
              tip={
                !canCancelJobs && (
                  <PermissionDeniedTip
                    message="You do not have permission to cancel scheduled runs in this deployment."
                    action="deployment:data:write"
                  />
                )
              }
              variant="danger"
            >
              Cancel
            </MenuItem>
          </Menu>
        </div>
      </div>
      <div>
        {showDeleteModal && (
          <ConfirmationDialog
            onClose={() => setShowDeleteModal(false)}
            onConfirm={() => cancelJob(_id, selectedNent?.id ?? null)}
            confirmText="Confirm"
            dialogTitle="Cancel run"
            dialogBody={
              <div>You are canceling a run scheduled to occur at {date}.</div>
            }
          />
        )}
      </div>
    </div>
  );
}

function ShowArgsPanel({
  argsId,
  setShowArgs,
  componentId,
}: {
  argsId: Id<"_scheduled_job_args">;
  setShowArgs: React.Dispatch<React.SetStateAction<boolean>>;
  componentId: string | null;
}) {
  const args = useQuery(udfs.scheduler.getArgs, { argsId, componentId });
  const udfArgs = args?.args;
  return udfArgs ? (
    <ShowArgsPanelWithArgs udfArgs={udfArgs} setShowArgs={setShowArgs} />
  ) : (
    <DetailPanel
      onClose={() => setShowArgs(false)}
      header="Arguments for scheduled function"
      content={undefined}
    />
  );
}

function ShowArgsPanelWithArgs({
  udfArgs,
  setShowArgs,
}: {
  udfArgs: ArrayBuffer;
  setShowArgs: React.Dispatch<React.SetStateAction<boolean>>;
}) {
  const udfArgsParsed: JSONValue[] = JSON.parse(
    new TextDecoder().decode(udfArgs),
  );
  return (
    <DetailPanel
      onClose={() => setShowArgs(false)}
      header="Arguments for scheduled function"
      content={
        <div className="h-full rounded-sm p-4">
          <ReadonlyCode
            disableLineNumbers
            path="scheduling"
            code={`${prettier(`
                    [${udfArgsParsed
                      .map((arg) => stringifyValue(jsonToConvex(arg)))
                      .join(",")}]`).slice(0, -1)}
                    `}
          />
        </div>
      }
    />
  );
}
