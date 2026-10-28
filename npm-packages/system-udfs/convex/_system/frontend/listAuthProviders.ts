import { Doc } from "../../_generated/dataModel";
import { queryPrivateSystem } from "../secretSystemTables";

export default queryPrivateSystem("ViewData")({
  args: {},
  handler: async ({ db }): Promise<Doc<"_auth">[]> => {
    return await db.query("_auth").order("asc").collect();
  },
});
