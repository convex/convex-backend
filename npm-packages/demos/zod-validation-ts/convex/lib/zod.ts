import { action, mutation, query } from "../_generated/server";
import {
  zCustomAction,
  zCustomMutation,
  zCustomQuery,
} from "convex-helpers/server/zod4";
// You can futher customize the query builder, see convex-helpers/server/customFunctions.
import { NoOp } from "convex-helpers/server/customFunctions";

// A custom version of a query builder that uses zod to validate the input and output types.
export const queryWithZod = zCustomQuery(query, NoOp);

// A custom version of a mutation builder that uses zod to validate the input and output types.
export const mutationWithZod = zCustomMutation(mutation, NoOp);

// A custom version of an action builder that uses zod to validate the input and output types.
export const actionWithZod = zCustomAction(action, NoOp);
