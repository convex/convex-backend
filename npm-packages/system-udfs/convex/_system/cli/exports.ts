import { Export } from "../frontend/common";
import { queryPrivateSystem } from "../secretSystemTables";

export const getLatest = queryPrivateSystem("ViewBackups")({
  args: {},
  handler: async ({ db }): Promise<Export | null> => {
    return await db
      .query("_exports")
      .withIndex("by_requestor", (q) => q.eq("requestor", "snapshotExport"))
      .order("desc")
      .first();
  },
});
