import { TableNames } from "../../_generated/dataModel";
import { queryGeneric } from "../server";
import { v } from "convex/values";

// This query returns a new result every time
// the given table's document change in any way.
export default queryGeneric("ViewData")({
  args: { tableName: v.string() },
  handler: async ({ db }, { tableName }) => {
    await db.query(tableName as TableNames).count();
    return Math.random();
  },
});
