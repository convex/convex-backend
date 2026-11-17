import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  // @snippet start table
  jobs: defineTable({
    input: v.string(),
    onComplete: v.function({
      fnType: "mutation",
      args: { jobId: v.string(), result: v.string() },
      returns: v.null(),
    }),
  }),
  // @snippet end table
  // @snippet start hooks
  hooks: defineTable({
    handle: v.function(),
  }),
  // @snippet end hooks
});
