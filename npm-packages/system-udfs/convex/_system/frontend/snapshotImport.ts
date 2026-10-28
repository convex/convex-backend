import { queryPrivateSystem } from "../secretSystemTables";
import { Doc } from "../../_generated/dataModel";

export const list = queryPrivateSystem("ViewBackups")({
  args: {},
  handler: async function ({ db }): Promise<Doc<"_snapshot_imports">[]> {
    return await db.query("_snapshot_imports").order("desc").take(20);
  },
});
