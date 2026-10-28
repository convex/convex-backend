import { Infer, v } from "convex/values";
import { MutationCtx, QueryCtx, mutation } from "./_generated/server";

export const waitlistStatusValidator = v.union(
  v.object({
    status: v.literal("OnWaitlist"),
    position: v.number(),
    headPosition: v.number(),
    tailPosition: v.number(),
  }),
  v.object({
    status: v.literal("NotOnWaitlist"),
  }),
);
export type Status = Infer<typeof waitlistStatusValidator>;

export const getStatus = async (
  ctx: QueryCtx,
  args: { user: string },
): Promise<Status> => {
  const userInWaitlist = await ctx.db
    .query("waitlist")
    .withIndex("by_user", (q) => q.eq("user", args.user))
    .unique();
  if (userInWaitlist === null) {
    return {
      status: "NotOnWaitlist",
    };
  }
  const tail = await ctx.db
    .query("waitlist")
    .withIndex("by_position")
    .order("desc")
    .first();

  const head = await ctx.db
    .query("waitlist")
    .withIndex("by_position")
    .order("asc")
    .first();

  return {
    status: "OnWaitlist",
    position: userInWaitlist.position,
    headPosition: head!.position,
    tailPosition: tail!.position,
  };
};

export const removeFromQueue = async (
  ctx: MutationCtx,
  args: { user: string },
) => {
  const userInQueue = await ctx.db
    .query("waitlist")
    .withIndex("by_user", (q) => q.eq("user", args.user))
    .unique();
  if (userInQueue === null) {
    return;
  }
  await ctx.db.delete("waitlist", userInQueue._id);
};

export const getHead = async (ctx: QueryCtx, size: number) => {
  const headOfWaitlist = await ctx.db
    .query("waitlist")
    .withIndex("by_position")
    .order("asc")
    .take(size);
  return headOfWaitlist;
};

export const join = mutation({
  args: {
    user: v.string(),
  },
  returns: v.null(),
  handler: async (ctx, args) => {
    const userInWaitlist = await ctx.db
      .query("waitlist")
      .withIndex("by_user", (q) => q.eq("user", args.user))
      .unique();
    if (userInWaitlist !== null) {
      return;
    }
    const tail = await ctx.db
      .query("waitlist")
      .withIndex("by_position")
      .order("desc")
      .first();
    const newPosition = (tail?.position ?? 0) + 1;
    await ctx.db.insert("waitlist", {
      user: args.user,
      position: newPosition,
    });
  },
});

export const leave = mutation({
  args: {
    user: v.string(),
  },
  handler: async (ctx, args) => {
    return removeFromQueue(ctx, args);
  },
});
