import { v } from "convex/values";
import { withSecretValueHidden } from "../environmentVariables";
import { queryPrivateSystem } from "../secretSystemTables";

// This query returns a new result every time
// the given table's document change in any way.
export default queryPrivateSystem("ViewEnvironmentVariables")({
  args: {},
  handler: async ({ db }) => {
    const docs = await db
      .query("_environment_variables")
      .withIndex("by_name")
      .order("asc")
      .collect();
    // `secret` is null for an undecided variable. CLIs tell that apart from
    // a deployment that predates secrets, whose variables have no `secret`.
    return docs.map((doc) => ({
      ...withSecretValueHidden(doc),
      secret: doc.secret ?? null,
    }));
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
