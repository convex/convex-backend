/**
 * Allows you to persist state server-side, associated with a sessionId stored
 * on the client (in localStorage, e.g.). You wrap your mutation / query with
 * withSession or withOptionalSession and it passes in "session" in the "ctx"
 * (first parameter) argument to your function. withOptionalSession allows
 * the sessionId to be null or invalid, and passes in `session: null` if so.
 */
import { ObjectType, v } from "convex/values";
import { DatabaseReader } from "../_generated/server";
import {
  generateMiddlewareContextOnly,
  generateMutationWithMiddleware,
  generateQueryWithMiddleware,
} from "./middlewareUtils";
import { Doc } from "../_generated/dataModel";

// ----------------------------------------------------------------------
// withSession

type SessionRequiredContext = { db: DatabaseReader };
type ContextWithSession = { session: Doc<"sessions"> };
const sessionValidator = { sessionId: v.id("sessions") };

const addSession = async (
  ctx: SessionRequiredContext,
  args: ObjectType<typeof sessionValidator>,
): Promise<ContextWithSession> => {
  const session = args.sessionId
    ? await ctx.db.get("sessions", args.sessionId)
    : null;
  if (!session) {
    throw new Error(
      "Session must be initialized first. " +
        "Are you wrapping your code with <SessionProvider>? " +
        "Are you requiring a session from a query that executes immediately?",
    );
  }
  return { session };
};
// ----------------------------------------------------------------------

// ----------------------------------------------------------------------
// No need to modify these aside from renaming

// Helper function to allow applying this transform to multiple types of `Context`
// (e.g. QueryCtx, MutaitonCtx)
const addSessionGeneric = async <Ctx extends Record<string, any>>(
  ctx: Ctx & SessionRequiredContext,
  args: ObjectType<typeof sessionValidator>,
): Promise<Omit<Ctx, keyof ContextWithSession> & ContextWithSession> => {
  return { ...ctx, ...(await addSession(ctx, args)) };
};

export const withSession = generateMiddlewareContextOnly(
  sessionValidator,
  addSession,
);

export const queryWithSession = generateQueryWithMiddleware(
  sessionValidator,
  addSessionGeneric,
);

export const mutationWithSession = generateMutationWithMiddleware(
  sessionValidator,
  addSessionGeneric,
);

// ----------------------------------------------------------------------
// withOptionalSession

type OptionalSessionRequiredContext = { db: DatabaseReader };
type OptionalSessionTransformedContext = { session: Doc<"sessions"> | null };
const optionalSessionValidator = {
  sessionId: v.union(v.null(), v.id("sessions")),
};

const addOptionalSession = async (
  ctx: OptionalSessionRequiredContext,
  args: ObjectType<typeof optionalSessionValidator>,
): Promise<OptionalSessionTransformedContext> => {
  const session = args.sessionId
    ? await ctx.db.get("sessions", args.sessionId)
    : null;
  return { session };
};
// ----------------------------------------------------------------------

// ----------------------------------------------------------------------
// No need to modify these aside from renaming

// Helper function to allow applying this transform to multiple types of `Context`
// (e.g. QueryCtx, MutaitonCtx)
const addOptionalSessionGeneric = async <Ctx extends Record<string, any>>(
  ctx: Ctx & OptionalSessionRequiredContext,
  args: ObjectType<typeof optionalSessionValidator>,
): Promise<
  Omit<Ctx, keyof OptionalSessionTransformedContext> &
    OptionalSessionTransformedContext
> => {
  return { ...ctx, ...(await addOptionalSession(ctx, args)) };
};

export const withOptionalSession = generateMiddlewareContextOnly(
  optionalSessionValidator,
  addOptionalSession,
);

export const queryWithOptionalSession = generateQueryWithMiddleware(
  optionalSessionValidator,
  addOptionalSessionGeneric,
);

export const mutationWithOptionalSession = generateMutationWithMiddleware(
  optionalSessionValidator,
  addOptionalSessionGeneric,
);
