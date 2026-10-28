import { ConnectedDeployment } from "@common/lib/deploymentContext";
import { ConvexHttpClient } from "convex/browser";
import udfs from "@common/udfs";
import { DisconnectedOverlay } from "./DisconnectOverlay";
import { useCallback, useEffect, useState } from "react";
import { Callout } from "@ui/Callout";
import {
  CheckCircledIcon,
  CrossCircledIcon,
  InfoCircledIcon,
} from "@radix-ui/react-icons";
import { HelpTooltip } from "@ui/HelpTooltip";
import { Button } from "@ui/Button";
import { Spinner } from "@ui/Spinner";

export function CloudDisconnectOverlay({
  deployment,
  deploymentName,
  openSupportForm,
  statusWidget,
}: {
  deployment: ConnectedDeployment;
  deploymentName: string;
  openSupportForm?: (defaultSubject: string, defaultMessage: string) => void;
  statusWidget?: React.ReactNode;
}): React.ReactNode {
  const isReachable = useCanReachDeploymentOverHTTP(deployment);

  const handleContactSupport = useCallback(() => {
    const defaultMessage = `I'm unable to connect to my deployment "${deploymentName}".

Deployment URL: ${deployment.deploymentUrl}
HTTP reachable: ${isReachable === null ? "checking..." : isReachable ? "yes" : "no"}
Browser Version: ${navigator.userAgent}

Please help me troubleshoot this connection issue.`;

    const defaultSubject = `Unable to connect to ${deploymentName}`;

    if (openSupportForm) {
      openSupportForm(defaultSubject, defaultMessage);
    }
  }, [deploymentName, deployment.deploymentUrl, isReachable, openSupportForm]);

  return (
    <DisconnectedOverlay>
      <div className="space-y-4">
        <div>
          <h4 className="mb-2">Connection Status</h4>
          <div className="flex flex-col gap-2">
            <p className="flex items-center gap-1 text-sm">
              <div className="w-fit rounded-full bg-background-error p-1">
                <CrossCircledIcon
                  className="text-content-error"
                  aria-hidden="true"
                />
              </div>
              WebSocket connection failed
            </p>
            {isReachable === null ? (
              <p className="flex items-center gap-1 text-sm text-content-secondary">
                <div className="p-1">
                  <Spinner />
                </div>
                Checking HTTP connection...
              </p>
            ) : isReachable ? (
              <p className="flex items-center gap-1 text-sm">
                <div className="w-fit rounded-full bg-background-success p-1">
                  <CheckCircledIcon
                    className="text-content-success"
                    aria-hidden="true"
                  />
                </div>
                HTTP connection successful
              </p>
            ) : (
              <p className="flex items-center gap-1 text-sm">
                <div className="w-fit rounded-full bg-background-error p-1">
                  <CrossCircledIcon
                    className="text-content-error"
                    aria-hidden="true"
                  />
                </div>
                HTTP connection failed
              </p>
            )}
          </div>
        </div>

        <div>
          <h4 className="mb-2">Troubleshooting</h4>
          {isReachable ? (
            <>
              <Callout className="mb-3" variant="hint">
                <div className="flex flex-col gap-2">
                  <h5 className="flex items-center gap-1">
                    <InfoCircledIcon />
                    Your deployment is online
                  </h5>
                  <p>
                    This connection issue is likely due to a problem with your
                    browser or network connection.
                  </p>
                </div>
              </Callout>
              <p className="mb-2">
                Please try the following troubleshooting steps:
              </p>
            </>
          ) : (
            <p className="mb-2 text-sm">
              There may be a client-side network issue. Try:
            </p>
          )}
          <ul className="ml-2 list-inside list-disc space-y-1 text-sm">
            <li>
              Switching to a different network. (i.e. WiFi, ethernet, or
              cellular)
            </li>
            <li>
              <span className="inline-flex items-center gap-1">
                Reloading the browser page
                <HelpTooltip>
                  The Convex dashboard will automatically attempt to reconnect
                  to your deployment, but refreshing the page may help in some
                  cases.
                </HelpTooltip>
              </span>
            </li>
            <li>Disabling your VPN</li>
            <li>Disabling browser extensions</li>
          </ul>
        </div>

        {statusWidget && (
          <div>
            <h4 className="mb-2">Convex Status</h4>
            {statusWidget}
          </div>
        )}

        {isReachable === false && openSupportForm && (
          <div className="border-t pt-2">
            <p className="text-sm text-content-secondary">
              <Button inline onClick={handleContactSupport}>
                Tried all of the troubleshooting steps? Contact support
              </Button>
            </p>
          </div>
        )}
      </div>
    </DisconnectedOverlay>
  );
}

function useCanReachDeploymentOverHTTP(
  deployment: Pick<ConnectedDeployment, "deploymentUrl" | "adminKey">,
): boolean | null {
  const { deploymentUrl, adminKey } = deployment;
  const [isReachable, setIsReachable] = useState<boolean | null>(null);

  useEffect(() => {
    let canceled = false;

    const checkReachability = async () => {
      try {
        // Exercise the deployment's HTTP API with an authenticated query
        // rather than a bare `no-cors` request, which resolves opaquely even
        // when the backend can't actually serve traffic. `getVersion` is a
        // cheap, always-available system query, so a successful response
        // confirms HTTP is genuinely working end to end.
        const client = new ConvexHttpClient(deploymentUrl);
        client.setAdminAuth(adminKey);
        await client.query(udfs.getVersion.default, {});
        if (!canceled) {
          setIsReachable(true);
        }
      } catch {
        if (!canceled) {
          setIsReachable(false);
        }
      }
    };

    void checkReachability();

    return () => {
      canceled = true;
    };
  }, [deploymentUrl, adminKey]);

  return isReachable;
}
