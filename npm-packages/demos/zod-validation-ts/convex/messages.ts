import { mutationWithZod, queryWithZod } from "./lib/zod";
import { z } from "zod";
import { Doc } from "./_generated/dataModel";
import { withSystemFields } from "convex-helpers/server/zod4";

export const send = mutationWithZod({
  args: { body: z.string(), author: z.string() },
  handler: async (ctx, { body, author }) => {
    await ctx.db.insert("messages", { body, author });
  },
});

export const list = queryWithZod({
  args: {}, // We don't have any args to validate
  handler: async (ctx): Promise<Doc<"messages">[]> => {
    return await ctx.db.query("messages").collect();
  },
  // Output validation is optional
  returns: z.array(
    z.object(
      withSystemFields("messages", { body: z.string(), author: z.string() }),
    ),
  ),
});
