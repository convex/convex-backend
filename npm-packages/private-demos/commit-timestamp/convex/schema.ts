import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  work: defineTable({
    payload: v.string(),
    updatedAt: v.commitTs(),
  }).index("by_updatedAt", ["updatedAt"]),
});
