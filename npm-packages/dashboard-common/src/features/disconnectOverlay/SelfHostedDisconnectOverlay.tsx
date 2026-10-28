import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { DisconnectedOverlay } from "./DisconnectOverlay";
import { useContext } from "react";

export function SelfHostedDisconnectOverlay() {
  const deploymentInfo = useContext(DeploymentInfoContext);
  const deploymentUrl = deploymentInfo.ok ? deploymentInfo.deploymentUrl : "";
  return (
    <DisconnectedOverlay>
      <p className="mb-2">
        Check that your Convex server is running and accessible at{" "}
        <code className="text-sm">{deploymentUrl}</code>.
      </p>
      <p>If you continue to have issues, try restarting your Convex server.</p>
    </DisconnectedOverlay>
  );
}
