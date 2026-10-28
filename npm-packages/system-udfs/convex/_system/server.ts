// Argument-validated versions of wrappers for use in system UDFs necessary
// because system UDFs are not analyzed.

import { GenericValidator, convexToJson } from "convex/values";
// This is where the alternatives are defined
import {
  // eslint-disable-next-line no-restricted-imports
  query as baseQuery,
  // eslint-disable-next-line no-restricted-imports
  mutation as baseMutation,
  // eslint-disable-next-line no-restricted-imports
  action as baseAction,
  // eslint-disable-next-line no-restricted-imports
  internalQuery as baseInternalQuery,
  // eslint-disable-next-line no-restricted-imports
  internalMutation as baseInternalMutation,
  // eslint-disable-next-line no-restricted-imports
  internalAction as baseInternalAction,
} from "../_generated/server";
import {
  // eslint-disable-next-line no-restricted-imports
  queryGeneric as baseQueryGeneric,
  // eslint-disable-next-line no-restricted-imports
  mutationGeneric as baseMutationGeneric,
  // eslint-disable-next-line no-restricted-imports
  actionGeneric as baseActionGeneric,
  // eslint-disable-next-line no-restricted-imports
  internalQueryGeneric as baseInternalQueryGeneric,
  // eslint-disable-next-line no-restricted-imports
  internalMutationGeneric as baseInternalMutationGeneric,
  // eslint-disable-next-line no-restricted-imports
  internalActionGeneric as baseInternalActionGeneric,
  currentSystemUdfInComponent,
} from "convex/server";

import { DefaultFunctionArgs } from "convex/server";
import { performOp, performAsyncSyscall } from "udf-syscall-ffi";

declare const Convex: {
  syscall: (op: string, jsonArgs: string) => string;
};

export type NoPermissionRequired = { noPermissionRequired: true };
// Allows permissions a system op without a specific operation check. Still requires an admin identity for system functions.
export const noPermissionRequired: NoPermissionRequired = {
  noPermissionRequired: true,
};

export type DeploymentOp =
  | "Deploy"
  | "ViewEnvironmentVariables"
  | "WriteEnvironmentVariables"
  | "PauseDeployment"
  | "UnpauseDeployment"
  | "ViewLogs"
  | "ViewMetrics"
  | "ViewIntegrations"
  | "WriteIntegrations"
  | "ViewData"
  | "WriteData"
  | "ViewBackups"
  | "CreateBackups"
  | "DownloadBackups"
  | "DeleteBackups"
  | "ImportBackups"
  | "ActAsUser"
  | "RunInternalQueries"
  | "RunInternalMutations"
  | "RunInternalActions"
  | "RunTestQuery"
  | "ViewAuditLog"
  | "ViewUsage"
  | "ViewUsageLimits"
  | "WriteUsageLimits"
  | "UseAiGateway";

export function requireOperation(
  operation: DeploymentOp | NoPermissionRequired,
): void {
  if (typeof operation === "object" && operation.noPermissionRequired) {
    return;
  }
  Convex.syscall("1.0/requireOperation", JSON.stringify({ operation }));
}

type FunctionDefinition = {
  args: Record<string, GenericValidator>;
  returns?: GenericValidator;
  handler: (ctx: any, args: DefaultFunctionArgs) => any;
};

type WrappedFunctionDefinition = {
  args: Record<string, GenericValidator>;
  returns?: GenericValidator;
  handler: (ctx: any, args: DefaultFunctionArgs) => any;
  exportArgs(): string;
  exportReturns(): string;
};

type Wrapper = (def: FunctionDefinition) => WrappedFunctionDefinition;

function withArgsValidated<T>(
  wrapper: T,
): (operation: DeploymentOp | NoPermissionRequired) => T {
  return (operation: DeploymentOp | NoPermissionRequired) => {
    if (!operation) {
      throw new Error("operation required for system udf");
    }
    return ((functionDefinition: FunctionDefinition) => {
      if (!("args" in functionDefinition)) {
        throw new Error("args validator required for system udf");
      }
      const wrap: Wrapper = wrapper as Wrapper;
      const func = wrap({
        args: functionDefinition.args,
        returns: functionDefinition.returns,
        handler: () => {},
      });
      const argsValidatorJson = func.exportArgs();
      const returnsValidatorJson = func.exportReturns();
      return wrap({
        args: functionDefinition.args,
        returns: functionDefinition.returns,
        handler: async (ctx: any, args: any) => {
          requireOperation(operation);
          const validateArgsResult = await performOp(
            "validateArgs",
            argsValidatorJson,
            JSON.stringify(convexToJson(args)),
          );
          if (!validateArgsResult.valid) {
            throw new Error(validateArgsResult.message);
          }
          const functionResult = await functionDefinition.handler(ctx, args);
          const validateReturnsResult = await performOp(
            "validateReturns",
            returnsValidatorJson,
            JSON.stringify(
              convexToJson(
                functionResult === undefined ? null : functionResult,
              ),
            ),
          );
          if (!validateReturnsResult.valid) {
            throw new Error(validateReturnsResult.message);
          }
          return functionResult;
        },
      });
    }) as T;
  };
}

export const queryGeneric = withArgsValidated(baseQueryGeneric);
const mutationGenericWithoutComponent = withArgsValidated(baseMutationGeneric);
export const actionGeneric = withArgsValidated(baseActionGeneric);
export const internalQueryGeneric = withArgsValidated(baseInternalQueryGeneric);
export const internalMutationGeneric = withArgsValidated(
  baseInternalMutationGeneric,
);
export const internalActionGeneric = withArgsValidated(
  baseInternalActionGeneric,
);

export const mutationGeneric = (
  operation: DeploymentOp,
): typeof baseMutationGeneric => {
  if (!operation) {
    throw new Error("operation required for system udf");
  }
  return ((functionDefinition: FunctionDefinition) => {
    return mutationGenericWithoutComponent(operation)({
      args: functionDefinition.args,
      returns: functionDefinition.returns,
      handler: async (ctx: any, args: any) => {
        if (
          "componentId" in args &&
          args.componentId !== null &&
          args.componentId !== undefined
        ) {
          const ref = currentSystemUdfInComponent(args.componentId);
          return await ctx.runMutation(ref, { ...args, componentId: null });
        }
        return functionDefinition.handler(ctx, args);
      },
    });
  }) as typeof baseMutationGeneric;
};

export async function writeAuditLog(
  action: string,
  metadata: Record<string, unknown>,
) {
  await performAsyncSyscall("1.0/writeDeploymentAuditLog", {
    action,
    metadata,
  });
}

// Specific to this schema.
export const query = withArgsValidated(baseQuery);
export const mutation = withArgsValidated(baseMutation);
export const action = withArgsValidated(baseAction);
export const internalQuery = withArgsValidated(baseInternalQuery);
export const internalMutation = withArgsValidated(baseInternalMutation);
export const internalAction = withArgsValidated(baseInternalAction);
