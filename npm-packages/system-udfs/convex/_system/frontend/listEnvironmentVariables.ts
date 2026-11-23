import { EnvironmentVariable } from "./common";
import { queryPrivateSystem } from "../secretSystemTables";
export default queryPrivateSystem("ViewEnvironmentVariables")({
  args: {},
  handler: async ({ db }): Promise<EnvironmentVariable[]> => {
    const docs = await db
      .query("_environment_variables")
      .withIndex("by_name")
      .order("asc")
      .collect();
    return docs.map((doc) => (doc.secret ? { ...doc, value: null } : doc));
  },
});
