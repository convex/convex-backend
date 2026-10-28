import { Doc } from "../../_generated/dataModel";
import { queryPrivateSystem } from "../secretSystemTables";
import { v } from "convex/values";

export const getArgs = queryPrivateSystem("ViewData")({
  args: {
    componentId: v.optional(v.union(v.string(), v.null())),
    argsId: v.id("_scheduled_job_args"),
  },
  handler: async function (
    { db },
    { argsId },
  ): Promise<Doc<"_scheduled_job_args"> | null> {
    return await db.get(argsId);
  },
});
