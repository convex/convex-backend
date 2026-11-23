import { ConvexError, v } from "convex/values";
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
  handler: async ({ db }, { name }) => {
    const doc = await db
      .query("_environment_variables")
      .withIndex("by_name", (q) => q.eq("name", name))
      .unique();
    if (doc?.secret) {
      throw new ConvexError({
        code: "EnvironmentVariableIsSecret",
        message: `${name} is a secret: its value can be overwritten but not read.`,
      });
    }
    // `npx convex env get <name>` looks up whatever name the caller asks
    // for, so there is no fixed set to declare in a `convex.config.ts`.
    // Reading `process.env` also covers system variables like
    // `CONVEX_SITE_URL`, which have no document.
    // eslint-disable-next-line @convex-dev/no-process-env
    const value = process.env[name];
    if (value !== undefined) {
      return { name, value };
    } else {
      return null;
    }
  },
});
