import { EnvironmentVariable } from "./common";
import { queryPrivateSystem } from "../secretSystemTables";
export default queryPrivateSystem("ViewEnvironmentVariables")({
  args: {},
  handler: async ({ db }): Promise<EnvironmentVariable[]> => {
    // TODO: return `null` as the value of a secret, once the deployed
    // dashboard shows secrets without their value.
    return await db
      .query("_environment_variables")
      .withIndex("by_name")
      .order("asc")
      .collect();
  },
});
