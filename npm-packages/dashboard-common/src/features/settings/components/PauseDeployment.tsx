import React, { useContext, useEffect, useState } from "react";
import { useQuery } from "convex/react";
import { Sheet } from "@ui/Sheet";
import { Loading } from "@ui/Loading";
import { Button } from "@ui/Button";
import { Callout } from "@ui/Callout";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import udfs from "@common/udfs";
import {
  DeploymentInfoContext,
  PermissionsContext,
} from "@common/lib/deploymentContext";
import { PermissionDeniedTip } from "@common/elements/NoPermissionMessage";
import { deploymentIdentityLabel } from "@common/lib/deploymentTypeColorClasses";
import { usePauseDeployment, useUnpauseDeployment } from "../lib/api";

// TODO insert link to docs here
const RESUME_EXPLANATION: string[] = [
  "New function calls can be made.",
  "Any functions scheduled before the deployment was paused will run.",
  "Cron jobs will resume according to their schedule.",
];

const PAUSE_EXPLANATION: string[] = [
  "New function calls will return an error.",
  "Scheduled jobs will queue and run when the deployment is resumed.",
  "Cron jobs will be skipped.",
];

export function PauseDeployment({
  onPausedDeployment,
}: {
  onPausedDeployment?: () => void;
}) {
  const deploymentState = useQuery(udfs.deploymentState.deploymentState);
  const { useCurrentDeployment } = useContext(DeploymentInfoContext);
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const deployment = useCurrentDeployment();
  const deploymentType = deployment?.deploymentType ?? "prod";
  const deploymentIdentity = deployment
    ? deploymentIdentityLabel(deployment)
    : null;
  const [paused, setPaused] = useState(false);
  const [showConfirmation, setShowConfirmation] = useState(false);

  const canPauseOp = useIsOperationAllowed("PauseDeployment");
  const canUnpauseOp = useIsOperationAllowed("UnpauseDeployment");
  const canPauseOrResume = paused ? canUnpauseOp : canPauseOp;

  const pauseDeployment = usePauseDeployment();
  const unpauseDeployment = useUnpauseDeployment();
  useEffect(() => {
    if (deploymentState) {
      setPaused(deploymentState.state === "paused");
    }
  }, [deploymentState]);
  async function toggle() {
    if (paused) {
      await unpauseDeployment();
    } else {
      await pauseDeployment();
      onPausedDeployment?.();
    }
  }
  function changeVerb(isPaused: boolean) {
    return isPaused ? "Resume" : "Pause";
  }

  const { useCurrentTeam, useTeamUsageState } = useContext(
    DeploymentInfoContext,
  );
  // Prevent direct access to this page if the team is disabled/paused
  const team = useCurrentTeam();
  const teamUsageState = useTeamUsageState(team?.id ?? null);
  if (teamUsageState === "Paused" || teamUsageState === "Disabled") {
    return (
      <Sheet>
        <h3 className="mb-4">Pause Deployment</h3>
        <Callout variant="error">
          Your projects are currently disabled. See the banner above for next
          steps.
        </Callout>
      </Sheet>
    );
  }

  return (
    <div>
      {deploymentState === undefined ? (
        <Loading />
      ) : (
        <Sheet className="flex w-full flex-col gap-4 lg:grid lg:grid-cols-[1fr_auto]">
          {showConfirmation && (
            <ConfirmationDialog
              onClose={() => setShowConfirmation(false)}
              onConfirm={() => Promise.resolve(toggle())}
              confirmText={
                changeVerb(paused) +
                (deploymentType === "prod" ? " Production" : "")
              }
              dialogTitle={`${changeVerb(paused)} Deployment`}
              dialogBody={
                <>
                  Are you sure you want to {changeVerb(paused).toLowerCase()}{" "}
                  {deploymentIdentity === null ? (
                    "this deployment?"
                  ) : (
                    <>
                      <span className="font-semibold">
                        {deploymentIdentity}
                      </span>
                      ?
                    </>
                  )}
                </>
              }
              variant={paused ? undefined : "danger"}
            />
          )}
          <h3>Pause Deployment</h3>
          <div className="flex items-start lg:row-span-2">
            <Button
              className="lg:order-2"
              variant={paused ? "primary" : "danger"}
              onClick={() => setShowConfirmation(true)}
              disabled={!canPauseOrResume}
              tip={
                !canPauseOrResume ? (
                  <PermissionDeniedTip
                    message={`You do not have permission to ${changeVerb(paused).toLowerCase()} this deployment.`}
                    action={paused ? "deployment:unpause" : "deployment:pause"}
                  />
                ) : (
                  ""
                )
              }
            >
              {paused ? "Resume Deployment" : "Pause Deployment"}
            </Button>
          </div>
          <p>
            This deployment is currently <b>{paused ? "paused" : "running"}</b>.
          </p>

          <div className="lg:order-1">
            When a deployment is {`${changeVerb(paused).toLowerCase()}d`}:
            <ul className="list-outside list-disc pl-4">
              {(paused ? RESUME_EXPLANATION : PAUSE_EXPLANATION).map((line) => (
                <li key={line}>{line}</li>
              ))}
            </ul>
          </div>
        </Sheet>
      )}
    </div>
  );
}
