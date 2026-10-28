import { Doc } from "../../_generated/dataModel";
import { queryPrivateSystem } from "../secretSystemTables";
export default queryPrivateSystem("ViewEnvironmentVariables")({
  args: {},
  handler: async ({ db }): Promise<Doc<"_environment_variables">[]> => {
    return await db
      .query("_environment_variables")
      .withIndex("by_name")
      .order("asc")
      .collect();
  },
});
