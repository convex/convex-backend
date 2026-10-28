import { useContext } from "react";
import { Link } from "@ui/Link";
import { useAdminKey, useDeploymentUrl } from "@common/lib/deploymentApi";
import { useNents } from "@common/lib/useNents";
import { toast } from "@common/lib/utils";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { displayName } from "@common/lib/functions/generateFileTree";

export function useCancelAllJobs(): (udfPath?: string) => Promise<void> {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  const { selectedNent } = useNents();
  const { deploymentsURI, reportHttpError } = useContext(DeploymentInfoContext);

  return async (udfPath?: string) => {
    const body = JSON.stringify({
      udfPath,
      componentPath: selectedNent?.path ?? undefined,
      componentId: selectedNent?.id ?? undefined,
    });
    const res = await fetch(`${deploymentUrl}/api/cancel_all_jobs`, {
      method: "POST",
      headers: {
        Authorization: `Convex ${adminKey}`,
        "Content-Type": "application/json",
      },
      body,
    });
    if (res.status !== 200) {
      const err = await res.json();
      reportHttpError("POST", res.url, err);
      if (err.code === "OptimisticConcurrencyControlFailure") {
        toast(
          "error",
          <span>
            There are too many functions being scheduled in this deployment.{" "}
            <Link href={`${deploymentsURI}/settings/pause-deployment`}>
              Pause your deployment
            </Link>{" "}
            to cancel all functions.
          </span>,
          "CancelJobsOCC",
        );
      } else {
        toast("error", err.message);
      }
      throw err;
    } else {
      toast(
        "success",
        udfPath
          ? `Canceled all scheduled runs for ${displayName(udfPath, selectedNent?.path ?? null)}.`
          : "Canceled all scheduled runs.",
      );
    }
  };
}

export function useCancelJob(): (
  id: string,
  componentId: string | null,
) => Promise<void> {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  const { reportHttpError } = useContext(DeploymentInfoContext);

  return async (id: string, componentId: string | null) => {
    const body = JSON.stringify({ id, componentId });
    const res = await fetch(`${deploymentUrl}/api/cancel_job`, {
      method: "POST",
      headers: {
        Authorization: `Convex ${adminKey}`,
        "Content-Type": "application/json",
      },
      body,
    });
    if (res.status !== 200) {
      const err = await res.json();
      reportHttpError("POST", res.url, err);
      toast("error", err.message);
    } else {
      toast("success", "Scheduled run canceled.");
    }
  };
}

export function useDeleteScheduledJobsTable(): () => Promise<void> {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  const { selectedNent } = useNents();
  const { reportHttpError } = useContext(DeploymentInfoContext);

  return async () => {
    const body = JSON.stringify({
      componentId: selectedNent?.id ?? undefined,
    });
    const res = await fetch(
      `${deploymentUrl}/api/delete_scheduled_functions_table`,
      {
        method: "POST",
        headers: {
          Authorization: `Convex ${adminKey}`,
          "Content-Type": "application/json",
        },
        body,
      },
    );
    if (res.status !== 200) {
      const err = await res.json();
      reportHttpError("POST", res.url, err);
      toast("error", err.message);
    } else {
      toast("success", "Scheduled functions table deleted.");
    }
  };
}
