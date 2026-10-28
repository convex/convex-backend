import { GenericValidator } from "convex/values";
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
} from "../../_generated/server";
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

import { DeploymentOp } from "../server";

type FunctionDefinition = {
  args: Record<string, GenericValidator>;
  returns?: GenericValidator;
  handler: (ctx: any, args: DefaultFunctionArgs) => any;
};

// Mock version: ignores `operation` and passes through to the base registrar.

function ignoringOperation<T extends (...args: any[]) => any>(
  wrapper: T,
): (operation: DeploymentOp) => T {
  return (_operation: DeploymentOp) => wrapper;
}

export const queryGeneric = ignoringOperation(baseQueryGeneric);
const mutationGenericWithoutComponent = ignoringOperation(baseMutationGeneric);
export const actionGeneric = ignoringOperation(baseActionGeneric);
export const internalQueryGeneric = ignoringOperation(baseInternalQueryGeneric);
export const internalMutationGeneric = ignoringOperation(
  baseInternalMutationGeneric,
);
export const internalActionGeneric = ignoringOperation(
  baseInternalActionGeneric,
);

export const mutationGeneric = (
  operation: DeploymentOp,
): typeof baseMutationGeneric => {
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

// Specific to this schema.
export const query = ignoringOperation(baseQuery);
export const mutation = ignoringOperation(baseMutation);
export const action = ignoringOperation(baseAction);
export const internalQuery = ignoringOperation(baseInternalQuery);
export const internalMutation = ignoringOperation(baseInternalMutation);
export const internalAction = ignoringOperation(baseInternalAction);
