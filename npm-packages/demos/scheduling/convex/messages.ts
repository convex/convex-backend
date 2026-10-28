import { query, mutation, internalMutation } from "./_generated/server";
import { internal } from "./_generated/api";
import { v } from "convex/values";

export const list = query({
  args: {},
  handler: async (ctx) => {
    return await ctx.db.query("messages").collect();
  },
});

export const send = mutation({
  args: {
    body: v.string(),
    author: v.string(),
  },
  handler: async (ctx, { body, author }) => {
    const message = { body, author };
    await ctx.db.insert("messages", message);
  },
});

// @snippet start self-destructing-message
function formatMessage(body: string, secondsLeft: number) {
  return `${body} (This message will self-destruct in ${secondsLeft} seconds)`;
}

export const sendExpiringMessage = mutation({
  args: { body: v.string(), author: v.string() },
  handler: async (ctx, { body, author }) => {
    const id = await ctx.db.insert("messages", {
      body: formatMessage(body, 5),
      author,
    });
    await ctx.scheduler.runAfter(
      1000,
      internal.messages.updateExpiringMessage,
      {
        messageId: id,
        body,
        secondsLeft: 4,
      },
    );
  },
});

export const updateExpiringMessage = internalMutation({
  args: {
    messageId: v.id("messages"),
    body: v.string(),
    secondsLeft: v.number(),
  },
  handler: async (ctx, { messageId, body, secondsLeft }) => {
    if (secondsLeft > 0) {
      await ctx.db.patch("messages", messageId, {
        body: formatMessage(body, secondsLeft),
      });
      await ctx.scheduler.runAfter(
        1000,
        internal.messages.updateExpiringMessage,
        {
          messageId,
          body,
          secondsLeft: secondsLeft - 1,
        },
      );
    } else {
      await ctx.db.delete("messages", messageId);
    }
  },
});
// @snippet end self-destructing-message
