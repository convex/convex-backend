import { JSX } from "react";
import {
  DeploymentApiProviderProps,
  DeploymentApiProvider,
} from "@common/lib/deploymentContext";
import { useAccessToken } from "hooks/useServerSideData";

export function MaybeDeploymentApiProvider({
  children,
  deploymentOverride,
}: DeploymentApiProviderProps): JSX.Element {
  const [accessToken] = useAccessToken();
  return accessToken ? (
    <DeploymentApiProvider deploymentOverride={deploymentOverride}>
      {children}
    </DeploymentApiProvider>
  ) : (
    // Render children without the deployment API provider
    // so the page can render and load server-side props.
    <>{children}</>
  );
}
