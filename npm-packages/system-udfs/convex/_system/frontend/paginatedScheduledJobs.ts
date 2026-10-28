import { Doc } from "../../_generated/dataModel";
import { PaginationResult, paginationOptsValidator } from "convex/server";
import { queryPrivateSystem } from "../secretSystemTables";
import { v } from "convex/values";
import { maximumBytesRead, maximumRowsRead } from "../paginationLimits";

export default queryPrivateSystem("ViewData")({
  args: {
    componentId: v.optional(v.union(v.string(), v.null())),
    paginationOpts: paginationOptsValidator,
    udfPath: v.optional(v.string()),
  },
  handler: async function (
    { db },
    { paginationOpts, udfPath },
  ): Promise<PaginationResult<Doc<"_scheduled_jobs">>> {
    if (udfPath === undefined) {
      return await db
        .query("_scheduled_jobs")
        .withIndex("by_next_ts", (q) => q.gt("nextTs", null))
        .order("asc")
        .paginate({
          ...paginationOpts,
          maximumBytesRead,
          maximumRowsRead,
        });
    } else {
      return await db
        .query("_scheduled_jobs")
        .withIndex("by_udf_path_and_next_event_ts", (q) =>
          q.eq("udfPath", udfPath).gt("nextTs", null),
        )
        .order("asc")
        .paginate({
          ...paginationOpts,
          maximumBytesRead,
          maximumRowsRead,
        });
    }
  },
});
