import { PaginationResult, paginationOptsValidator } from "convex/server";
import { v } from "convex/values";
import { maximumBytesRead, maximumRowsRead } from "../paginationLimits";
import { queryGeneric } from "../secretSystemTables";

export default queryGeneric("ViewData")({
  args: {
    table: v.string(),
    order: v.union(v.literal("asc"), v.literal("desc")),
    paginationOpts: paginationOptsValidator,
  },
  handler: async (
    { db },
    { table, order, paginationOpts },
  ): Promise<PaginationResult<any>> => {
    return await (table.startsWith("_") ? db.system : db)
      .query(table as any)
      .order(order)
      .paginate({ ...paginationOpts, maximumRowsRead, maximumBytesRead });
  },
});
