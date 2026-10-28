import { v } from "convex/values";
import { query } from "./_generated/server";
import { mutationWithSession } from "./lib/sessions";

export const list = query({
  args: {},
  handler: async (ctx) => {
    const messages = await ctx.db.query("messages").collect();
    return Promise.all(
      messages.map(async (message) => {
        const { author, ...messageBody } = message;
        const name = (await ctx.db.get("users", author))!.name;
        return { author: name, ...messageBody };
      }),
    );
  },
});

export const send = mutationWithSession({
  args: { body: v.string() },
  handler: async (ctx, { body }) => {
    let userId = ctx.user?._id;
    if (!userId) {
      const { sessionId } = ctx;
      userId = await ctx.db.insert("users", { name: "Anonymous", sessionId });
    }
    await ctx.db.insert("messages", { body, author: userId });
  },
});
