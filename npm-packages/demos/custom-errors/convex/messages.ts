import { v, ConvexError } from "convex/values";
// @snippet start query
import { mutation, query } from "./_generated/server";

const MESSAGES_LIMIT = 20;
const MESSAGE_CHARS_LIMIT = 50;
export const list = query({
  args: {},
  handler: async (ctx) => {
    const messages = await ctx.db.query("messages").collect();
    if (messages.length > MESSAGES_LIMIT) {
      throw new ConvexError({
        message: "Too many messages!",
        length: messages.length,
        code: "MESSAGE_LIMIT",
      });
    }
    return Promise.all(
      messages.map(async (message) => ({
        ...message,
      })),
    );
  },
});
// @snippet end query

export const sendMessage = mutation({
  args: { body: v.string(), author: v.string() },
  handler: async (ctx, args) => {
    const { body, author } = args;
    if (body.length > MESSAGE_CHARS_LIMIT) {
      throw new ConvexError("Message is over 50 characters.");
    }
    await ctx.db.insert("messages", { body, author, format: "text" });
  },
});

export const clearMessages = mutation({
  args: {},
  handler: async (ctx) => {
    const messages = await ctx.db.query("messages").collect();
    return Promise.all(
      messages.map(async (message) => {
        const id = message._id;
        await ctx.db.delete("messages", id);
        return id;
      }),
    );
  },
});
