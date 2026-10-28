import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { Link } from "@ui/Link";
import { useContext } from "react";

export function FunctionRunnerDisabledWhilePaused() {
  const { deploymentsURI } = useContext(DeploymentInfoContext);
  return (
    <>
      The function runner is not available while the deployment is paused. To
      resume your deployment, go to{" "}
      <Link href={`${deploymentsURI}/settings/pause-deployment`}>settings</Link>
      .
    </>
  );
}
