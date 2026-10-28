import { Auth } from "convex/server";
import { ObjectType, v } from "convex/values";
import { DatabaseReader } from "../_generated/server";
import { generateQueryWithMiddleware } from "./middlewareUtils";
import { Doc } from "../_generated/dataModel";

// ----------------------------------------------------------------------
// Two middlewares like in `middlewareTemplate.ts`

type MyRequiredContextA = { db: DatabaseReader; auth: Auth };
type TransformedContextA = { user: string };

const myValidatorA = { myArg: v.string() };

const myTransformA = async (
  ctx: MyRequiredContextA,
  args: ObjectType<typeof myValidatorA>,
): Promise<TransformedContextA> => {
  // Change this
  return { user: args.myArg };
};

type MyRequiredContextB = { db: DatabaseReader };
type TransformedContextB = { session: Doc<"sessions"> | null };

const myValidatorB = { sessionId: v.id("sessions") };

const myTransformB = async (
  ctx: MyRequiredContextB,
  args: ObjectType<typeof myValidatorB>,
): Promise<TransformedContextB> => {
  const session = await ctx.db.get("sessions", args.sessionId);
  return { session };
};

// Merge them!
type MyRequiredContextBoth = MyRequiredContextA & MyRequiredContextB;
type TransformedContextBoth = TransformedContextA & TransformedContextB;
const myValidatorBoth = { ...myValidatorA, ...myValidatorB };

const myTransformBoth = async (
  ctx: MyRequiredContextBoth,
  args: ObjectType<typeof myValidatorBoth>,
): Promise<TransformedContextBoth> => {
  return {
    ...(await myTransformA(ctx, args)),
    ...(await myTransformB(ctx, args)),
  };
};

// ----------------------------------------------------------------------

// ----------------------------------------------------------------------
// No need to modify these aside from renaming

// Helper function to allow applying this transform to multiple types of `Context`
// (e.g. QueryCtx, MutaitonCtx)
const myTransformGeneric = async <Ctx extends Record<string, any>>(
  ctx: Ctx & MyRequiredContextBoth,
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
  handler: async ({ db, user, session }, { a }) => {
    // Do stuff!
    console.log(user, db, a, session);
  },
});
