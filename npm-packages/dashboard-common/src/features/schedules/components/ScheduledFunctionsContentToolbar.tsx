import { TrashIcon } from "@radix-ui/react-icons";
import { PermissionDeniedTip } from "@common/elements/NoPermissionMessage";
import { useRouter } from "next/router";
import { useContext, useState } from "react";
import {
  useCancelAllJobs,
  useDeleteScheduledJobsTable,
} from "@common/features/schedules/lib/api";
import {
  DeploymentInfoContext,
  PermissionsContext,
} from "@common/lib/deploymentContext";
import {
  itemIdentifier,
  useCurrentOpenFunction,
  useModuleFunctions,
} from "@common/lib/functions/FunctionsProvider";
import { Combobox } from "@ui/Combobox";
import {
  displayName,
  functionIdentifierFromValue,
  functionIdentifierValue,
} from "@common/lib/functions/generateFileTree";
import { FunctionNameOption } from "@common/elements/FunctionNameOption";
import { Button } from "@ui/Button";
import { SchedulerStatus } from "@common/elements/SchedulerStatus";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { Tooltip } from "@ui/Tooltip";
import { HelpTooltip } from "@ui/HelpTooltip";
import { Checkbox } from "@ui/Checkbox";
import { Callout } from "@ui/Callout";
import { cn } from "@ui/cn";

function DeleteScheduledFunctionsTableMessage() {
  return (
    <>
      An emergency delete will permanently erase the state of all scheduled
      jobs. No scheduled jobs (including completed ones) will appear when you
      query the <code>_scheduled_functions</code> table.
    </>
  );
}

export function ScheduledFunctionsContentToolbar({
  reload,
}: {
  reload: () => Promise<void>;
}) {
  const currentOpenFunction = useCurrentOpenFunction();
  const moduleFunctions = useModuleFunctions();
  const router = useRouter();
  const [showDeleteModal, setShowDeleteModal] = useState(false);
  const [deleteTable, setDeleteTable] = useState(false);
  const cancelJobs = useCancelAllJobs();
  const deleteScheduledJobsTable = useDeleteScheduledJobsTable();

  const { useCurrentDeployment } = useContext(DeploymentInfoContext);
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const deployment = useCurrentDeployment();
  const canCancelJobs = useIsOperationAllowed("WriteData");

  return (
    <div className="flex w-full flex-wrap items-center gap-4">
      <div className="flex w-full flex-wrap gap-4">
        <Combobox
          label="Filter scheduled runs by function"
          className="w-88"
          buttonClasses="w-88"
          optionsWidth="full"
          searchPlaceholder="Search functions..."
          selectedOption={
            currentOpenFunction ? itemIdentifier(currentOpenFunction) : null
          }
          setSelectedOption={async (item) => {
            if (!item) {
              const { function: _, ...query } = router.query;
              await router.push({ query });
              return;
            }
            const { identifier, componentPath } =
              functionIdentifierFromValue(item);
            await router.push({
              query: {
                ...router.query,
                function: identifier,
                componentPath,
              },
            });
          }}
          options={[
            {
              label: functionIdentifierValue("All functions"),
              value: null,
            },
            ...moduleFunctions.map((value) => ({
              label: itemIdentifier(value),
              value: itemIdentifier(value),
            })),
          ]}
          Option={(props) => (
            <FunctionNameOption {...{ ...props, disableTruncation: true }} />
          )}
          processFilterOption={(option) => {
            const id = functionIdentifierFromValue(option);
            return id.componentPath
              ? `${id.componentPath}/${id.identifier}`
              : id.identifier;
          }}
        />
        <Button
          variant="danger"
          size="sm"
          onClick={() => setShowDeleteModal(true)}
          icon={<TrashIcon />}
          disabled={!canCancelJobs}
          tip={
            !canCancelJobs && (
              <PermissionDeniedTip
                message="You do not have permission to cancel scheduled runs in this deployment."
                action="deployment:data:write"
              />
            )
          }
        >
          Cancel All {currentOpenFunction && "(for the selected function)"}
        </Button>

        <div className="ml-auto">
          <SchedulerStatus small />
        </div>
      </div>
      {showDeleteModal && (
        <ConfirmationDialog
          onClose={() => {
            setShowDeleteModal(false);
            setDeleteTable(false);
          }}
          onConfirm={async () => {
            if (deleteTable) {
              await deleteScheduledJobsTable();
            } else {
              await cancelJobs(currentOpenFunction?.identifier);
            }
            void reload();
          }}
          confirmText="Confirm"
          dialogTitle="Cancel all runs"
          validationText={
            deleteTable
              ? "Delete the _scheduled_functions table"
              : deployment?.deploymentType === "prod"
                ? "Cancel all"
                : undefined
          }
          dialogBody={
            <div className="flex flex-col gap-2">
              <div>
                You are canceling all scheduled runs for{" "}
                {currentOpenFunction?.displayName ? (
                  <span className="font-mono font-semibold">
                    {displayName(
                      currentOpenFunction?.displayName,
                      currentOpenFunction?.componentPath ?? null,
                    )}
                  </span>
                ) : (
                  `all functions${currentOpenFunction?.componentPath ? " in the selected component." : ""}`
                )}
                .
              </div>
              <Tooltip
                tip={
                  currentOpenFunction &&
                  "Emergency deletes are only available when canceling scheduled runs for all functions."
                }
                className="w-fit"
              >
                <label
                  className={cn(
                    "flex cursor-pointer items-center gap-2",
                    currentOpenFunction &&
                      "w-fit cursor-not-allowed rounded-sm bg-background-tertiary p-2 text-content-secondary",
                  )}
                >
                  <Checkbox
                    checked={deleteTable}
                    disabled={!!currentOpenFunction}
                    onChange={() => setDeleteTable(!deleteTable)}
                    className="ml-1"
                  />
                  <span className="flex items-center gap-1 text-sm">
                    Emergency delete (faster)
                    {!deleteTable && (
                      <HelpTooltip>
                        <DeleteScheduledFunctionsTableMessage />
                      </HelpTooltip>
                    )}
                  </span>
                </label>
              </Tooltip>
              {deleteTable && (
                <Callout>
                  <span className="text-sm">
                    <DeleteScheduledFunctionsTableMessage />
                  </span>
                </Callout>
              )}
            </div>
          }
        />
      )}
    </div>
  );
}
