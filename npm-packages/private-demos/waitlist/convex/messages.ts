import { v } from "convex/values";
import { query, mutation, internalMutation } from "./_generated/server";
import * as Waitlist from "./waitlist";

export const list = query({
  args: {},
  handler: async (ctx) => {
    return await ctx.db.query("messages").collect();
  },
});

export const send = mutation({
  args: { body: v.string(), user: v.string() },
  returns: v.null(),
  handler: async (ctx, args) => {
    const participants = await ctx.db
      .query("conversationParticipant")
      .withIndex("by_active", (q) => q.eq("active", true))
      .collect();
    if (participants.find((p) => p.user === args.user) === undefined) {
      throw new Error(
        "Cannot send message if not participating in conversation",
      );
    }
    const message = { body: args.body, author: `User ${args.user}` };
    await ctx.db.insert("messages", message);
  },
});

const MAX_USERS = 5;
const MAX_CONVERSATION_DURATION_SECONDS = 30;

export const updateConversation = internalMutation({
  args: {},
  returns: v.null(),
  handler: async (ctx, _args) => {
    const activeConversationParticipants = await ctx.db
      .query("conversationParticipant")
      .withIndex("by_active", (q) => q.eq("active", true))
      .collect();
    const conversationParticipantsUpForExpiration =
      activeConversationParticipants.filter(
        (p) =>
          p._creationTime <=
          Date.now() - MAX_CONVERSATION_DURATION_SECONDS * 1000,
      );
    const maxRoomLeftInConversation =
      MAX_USERS -
      (activeConversationParticipants.length -
        conversationParticipantsUpForExpiration.length);
    const headOfWaitlist = await Waitlist.getHead(
      ctx,
      maxRoomLeftInConversation,
    );
    const conversationParticipantsToExpire =
      conversationParticipantsUpForExpiration.slice(0, headOfWaitlist.length);
    for (const participant of conversationParticipantsToExpire) {
      await ctx.db.patch("conversationParticipant", participant._id, {
        active: false,
      });
    }
    for (const waitlistMember of headOfWaitlist) {
      await ctx.db.insert("conversationParticipant", {
        user: waitlistMember.user,
        active: true,
      });
      await Waitlist.removeFromQueue(ctx, { user: waitlistMember.user });
    }

    console.log(`Moved ${headOfWaitlist.length} user(s) to conversation.`);
  },
});

export type Status =
  | {
      status: "InConversation";
      expirationTime: number;
    }
  | {
      status: "RemovedFromConversation";
    }
  | Waitlist.Status;

export const getStatus = query({
  args: { user: v.string() },
  returns: v.union(
    v.object({
      status: v.literal("InConversation"),
      expirationTime: v.number(),
    }),
    v.object({
      status: v.literal("RemovedFromConversation"),
    }),
    Waitlist.waitlistStatusValidator,
  ),
  handler: async (ctx, args): Promise<Status> => {
    const participant = await ctx.db
      .query("conversationParticipant")
      .withIndex("by_user", (q) => q.eq("user", args.user))
      .first();
    if (participant === null) {
      return Waitlist.getStatus(ctx, args);
    }
    if (participant.active) {
      return {
        status: "InConversation" as const,
        expirationTime:
          participant._creationTime + MAX_CONVERSATION_DURATION_SECONDS,
      };
    } else {
      return {
        status: "RemovedFromConversation" as const,
      };
    }
  },
});
