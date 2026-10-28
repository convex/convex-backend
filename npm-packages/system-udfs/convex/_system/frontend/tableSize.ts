import sum from "lodash/sum";
import { queryGeneric } from "../secretSystemTables";
import { v } from "convex/values";
import { DatabaseReader } from "../../_generated/server";

export default queryGeneric("ViewData")({
  args: {
    tableName: v.string(),
    componentId: v.optional(v.union(v.string(), v.null())),
  },
  handler: async function tableSize({ db }, { tableName }): Promise<number> {
    if (!tableName) {
      return 0;
    }
    // internal queries don't show up now
    return await db.query(tableName).count();
  },
});

export const sizeOfAllTables = queryGeneric("ViewData")({
  args: { componentId: v.optional(v.union(v.string(), v.null())) },
  handler: async function allTableSizes({ db }): Promise<number> {
    // Getting private system table here is OK because there are no args to this
    // system UDF.
    const tables = await ((db as any).privateSystem as DatabaseReader)
      .query("_tables")
      // eslint-disable-next-line @convex-dev/no-filter-in-query -- very small system table
      .filter((q) => q.eq(q.field("state"), "active"))
      .collect();
    const tablesWithoutSystemTables = tables
      .map((table) => table.name)
      .filter((tableName) => !tableName.startsWith("_"));

    const tableCounts = Promise.all(
      tablesWithoutSystemTables.map(async (tableName) => {
        return await db.query(tableName as any).count();
      }),
    );

    return sum(await tableCounts);
  },
});
