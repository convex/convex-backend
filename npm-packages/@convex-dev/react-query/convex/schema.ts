import { defineSchema, defineTable } from "convex/server";
import { authTables } from "@convex-dev/auth/server";
import { v } from "convex/values";
import { typedV } from "convex-helpers/validators";

const schema = defineSchema({
  ...authTables,
  messages: defineTable({
    author: v.id("users"),
    body: v.string(),
  }).index("by_author", ["author"]),
});
export default schema;

export const vv = typedV(schema);
