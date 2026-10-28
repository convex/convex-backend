import { convexToJson, jsonToConvex, Value } from "../../values/index.js";
import { version } from "../../index.js";
import { performAsyncSyscall } from "./syscall.js";
import { parseArgs } from "../../common/index.js";
import {
  FunctionReference,
  FunctionReference_future,
} from "../../server/api.js";
import { getFunctionAddress } from "../components/paths.js";
import { validateArg } from "./validate.js";

function syscallArgs(
  requestId: string,
  functionReference: any,
  args?: Record<string, Value>,
) {
  const address = getFunctionAddress(functionReference);
  return {
    ...address,
    args: convexToJson(parseArgs(args)),
    version,
    requestId,
  };
}

export function setupActionCalls(requestId: string) {
  return {
    runQuery: async (
      query:
        | FunctionReference<"query", "public" | "internal">
        | FunctionReference_future<"query", "public" | "internal">,
      args?: Record<string, Value>,
    ): Promise<any> => {
      const result = await performAsyncSyscall(
        "1.0/actions/query",
        syscallArgs(requestId, query, args),
      );
      return jsonToConvex(result);
    },
    runMutation: async (
      mutation:
        | FunctionReference<"mutation", "public" | "internal">
        | FunctionReference_future<"mutation", "public" | "internal">,
      args?: Record<string, Value>,
    ): Promise<any> => {
      const result = await performAsyncSyscall(
        "1.0/actions/mutation",
        syscallArgs(requestId, mutation, args),
      );
      return jsonToConvex(result);
    },
    runAction: async (
      action:
        | FunctionReference<"action", "public" | "internal">
        | FunctionReference_future<"action", "public" | "internal">,
      args?: Record<string, Value>,
    ): Promise<any> => {
      const result = await performAsyncSyscall(
        "1.0/actions/action",
        syscallArgs(requestId, action, args),
      );
      return jsonToConvex(result);
    },
  };
}

/**
 * A Convex-managed service. The backend decides which names it accepts; this
 * type lists the ones the current release knows about.
 */
export type ServiceName = "ai-gateway";

/**
 * Get a short-lived credential for calling a Convex-managed service.
 *
 * This function can only be called while an action is running. The credential
 * is scoped to the current deployment and should be sent as a bearer token.
 * Credentials expire a few minutes after they're issued. The action runtime
 * caches and refreshes them as needed, so call this function whenever making
 * a service request rather than reusing one credential for a long action.
 *
 * @param service - The service the credential may access.
 * @returns A JWT to send as `Authorization: Bearer <token>`. Keep it inside
 * the action: don't return it to clients or store it in environment
 * variables.
 */
export async function getServiceToken(service: ServiceName): Promise<string> {
  validateArg(service, 1, "getServiceToken", "service");
  return await performAsyncSyscall("1.0/createServiceToken", {
    service,
    version,
  });
}

/**
 * Get the base URL of a Convex-managed service.
 *
 * This function can only be called while an action is running. Pair it with
 * {@link getServiceToken} to reach the service.
 *
 * @param service - The service to address.
 * @returns The service's origin, without a trailing slash.
 */
export async function getServiceUrl(service: ServiceName): Promise<string> {
  validateArg(service, 1, "getServiceUrl", "service");
  return await performAsyncSyscall("1.0/getServiceUrl", {
    service,
    version,
  });
}
