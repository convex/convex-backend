import { v } from "convex/values";
import { queryPrivateSystem } from "../secretSystemTables";

// This query returns a new result every time
// the given table's document change in any way.
export default queryPrivateSystem("ViewEnvironmentVariables")({
  args: {},
  handler: async ({ db }) => {
    return await db
      .query("_environment_variables")
      .withIndex("by_name")
      .order("asc")
      .collect();
  },
});

export const get = queryPrivateSystem("ViewEnvironmentVariables")({
  args: {
    name: v.string(),
  },
  handler: async (_, { name }) => {
    // `npx convex env get <name>` looks up whatever name the caller asks
    // for, so there is no fixed set to declare in a `convex.config.ts`.
    // eslint-disable-next-line @convex-dev/no-process-env
    const value = process.env[name];
    if (value !== undefined) {
      return { name, value };
    } else {
      return null;
    }
  },
});
