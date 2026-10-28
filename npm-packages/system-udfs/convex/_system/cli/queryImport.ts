import { v } from "convex/values";
import { queryPrivateSystem } from "../secretSystemTables";
import { Doc } from "../../_generated/dataModel";

export default queryPrivateSystem("ViewBackups")({
  args: { importId: v.id("_snapshot_imports") },
  handler: async ({ db }, args) => {
    return await db.get(args.importId);
  },
});

export const list = queryPrivateSystem("ViewBackups")({
  args: {},
  handler: async function ({ db }): Promise<Doc<"_snapshot_imports">[]> {
    return await db.query("_snapshot_imports").order("desc").take(20);
  },
});
