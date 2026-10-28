import { v } from "convex/values";
import { internalMutation } from "./_generated/server";

export const bar = internalMutation({
  args: {
    requestMetadata: v.any(),
    documentId: v.any(),
  },
  handler: () => {},
});
