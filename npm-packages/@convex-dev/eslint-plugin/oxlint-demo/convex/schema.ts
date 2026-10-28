import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  messages: defineTable({
    author: v.string(),
    channel: v.string(),
    body: v.string(),
  })
    // oxlint-disable-next-line @convex-dev/no-duplicate-indexes
    .index("by_author", ["author"])
    .index("by_author_and_channel", ["author", "channel"]),
});
