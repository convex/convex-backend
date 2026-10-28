import { FunctionResult } from "convex/browser";
import { ConvexReactClient } from "convex/react";
import { UserIdentityAttributes } from "convex/server";
import { convexToJson, jsonToConvex } from "convex/values";
import { useReducer, useCallback, useContext } from "react";
import { createGlobalState, usePrevious } from "react-use";
import { Id } from "system-udfs/convex/_generated/dataModel";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { useDeploymentUrl, useAdminKey } from "@common/lib/deploymentApi";
import { useDeepEqualsEffect } from "@common/lib/useDeepEqualsEffect";

const useGlobalReactClientState = createGlobalState<ConvexReactClient>();

export function useGlobalReactClient(
  identity?: UserIdentityAttributes,
): [ConvexReactClient, number] {
  const [identityKey, updateIdentityKey] = useReducer((x) => x + 1, 0);
  const [client, setClient] = useGlobalReactClientState();
  const deploymentData = useContext(DeploymentInfoContext);

  const previousDeploymentUrl = usePrevious(
    deploymentData?.ok && deploymentData.deploymentUrl,
  );

  useDeepEqualsEffect(() => {
    if (!deploymentData || !deploymentData.ok) return;

    const { deploymentUrl, adminKey } = deploymentData;

    let c = client;
    if (!c || deploymentUrl !== previousDeploymentUrl) {
      if (c) {
        void c.close();
      }
      // We don't have a client yet or the deployment url has changed. Let's create a new one.
      c = new ConvexReactClient(deploymentUrl, {
        reportDebugInfoToConvex: true,
      });
      setClient(c);
    } else {
      // There's either a new client, or a change to the dependencies
      c.setAdminAuth(adminKey, identity);
      updateIdentityKey();
    }
  }, [identity, client, deploymentData, previousDeploymentUrl]);

  return [client, identityKey];
}

export function useRunTestFunction() {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  return useCallback(
    async (
      transpiled: string,
      componentId?: Id<"_components">,
    ): Promise<FunctionResult> => {
      const response = await fetch(`${deploymentUrl}/api/run_test_function`, {
        headers: { "content-type": "application/json" },
        method: "POST",
        body: JSON.stringify({
          bundle: {
            path: "testQuery.js",
            source: transpiled,
          },
          adminKey,
          componentId,
          args: convexToJson({}),
          format: "convex_encoded_json",
        }),
      });
      if (!response.ok) {
        if (response.status >= 400 && response.status <= 499) {
          const body = await response.json();
          return {
            success: false,
            errorMessage: body.message.toString(),
            logLines: [],
          };
        }
        return {
          success: false,
          errorMessage:
            "Encountered an error running this function. Please try again or contact support.",
          logLines: [],
        };
      }
      const body = await response.json();
      return body.status === "success"
        ? ({
            success: true,
            value: jsonToConvex(body.value),
            logLines: body.logLines || [],
          } as const)
        : ({
            success: false,
            errorMessage: body.errorMessage,
            errorData:
              body.errorData !== undefined && jsonToConvex(body.errorData),
            logLines: body.logLines || [],
          } as const);
    },
    [deploymentUrl, adminKey],
  );
}
