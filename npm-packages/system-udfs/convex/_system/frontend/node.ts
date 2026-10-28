import { queryPrivateSystem } from "../secretSystemTables";

export const version = queryPrivateSystem("ViewData")({
  args: {},
  handler: async function ({ db }) {
    const modules = await db.query("_modules").take(1000);
    const nodeModules = modules.filter((m) => m.environment === "node");

    // We keep lambdas around even if they are not currently in use. So, we need to
    // return early if there are no node actions currently deployed.
    if (nodeModules.length === 0 && modules.length < 1000) {
      return null;
    }

    const version = await db
      .query("_aws_lambda_versions")
      .order("desc")
      // eslint-disable-next-line @convex-dev/no-filter-in-query -- very small table
      .filter((q) => q.eq(q.field("typeConfig.type"), "static"))
      .first();

    if (!version) {
      return null;
    }

    return version.lambdaConfig.runtime;
  },
});
