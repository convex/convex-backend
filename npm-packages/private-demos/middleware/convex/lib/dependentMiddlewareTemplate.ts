import { ObjectType, v } from "convex/values";
import { DatabaseReader } from "../_generated/server";
import { generateQueryWithMiddleware } from "./middlewareUtils";
import { Doc } from "../_generated/dataModel";

// ----------------------------------------------------------------------
// Two middlewares like in `middlewareTemplate.ts`

type MyRequiredContextA = { db: DatabaseReader };
type TransformedContextA = { session: Doc<"sessions"> | null };

const myValidatorA = { sessionId: v.id("sessions") };

const myTransformA = async (
  ctx: MyRequiredContextA,
  args: ObjectType<typeof myValidatorA>,
): Promise<TransformedContextA> => {
  const session = await ctx.db.get("sessions", args.sessionId);
  return { session };
};

// Middleware B depends on middleware A
type MyRequiredContextB = {
  db: DatabaseReader;
  session: Doc<"sessions"> | null;
};
type TransformedContextB = { experiments: Record<string, boolean> };

const myValidatorB = {};

const myTransformB = async (
  ctx: MyRequiredContextB,
  _args: ObjectType<typeof myValidatorB>,
): Promise<TransformedContextB> => {
  const _sessionId = ctx.session?._id;
  const experiments = {}; // look up experiments in db according to session ID
  return { experiments };
};

// Manually merge them

// Don't require session since it's provided for middleware B by middleware A
type RequiredContextBoth = { db: DatabaseReader };
type TransformedContextBoth = TransformedContextA & TransformedContextB;
const myValidatorBoth = { ...myValidatorA, ...myValidatorB };

const myTransformBoth = async (
  ctx: RequiredContextBoth,
  args: ObjectType<typeof myValidatorBoth>,
): Promise<TransformedContextBoth> => {
  const ctxA = {
    ...ctx,
    ...(await myTransformA(ctx, args)),
  };
  return {
    ...ctxA,
    ...(await myTransformB(ctxA, args)),
  };
};

// ----------------------------------------------------------------------

// ----------------------------------------------------------------------
// No need to modify these aside from renaming

// Helper function to allow applying this transform to multiple types of `Context`
// (e.g. QueryCtx, MutaitonCtx)
const myTransformGeneric = async <Ctx extends Record<string, any>>(
  ctx: Ctx & RequiredContextBoth,
  args: ObjectType<typeof myValidatorBoth>,
): Promise<
  Omit<Ctx, keyof TransformedContextBoth> & TransformedContextBoth
> => {
  return { ...ctx, ...(await myTransformBoth(ctx, args)) };
};

export const queryWithMyTransform = generateQueryWithMiddleware(
  myValidatorBoth,
  myTransformGeneric,
);

// ----------------------------------------------------------------------
// Examples

const _q = queryWithMyTransform({
  args: { a: v.string() },
  handler: async ({ db, experiments, session }, { a }) => {
    // Do stuff!
    console.log(experiments, db, a, session);
  },
});
