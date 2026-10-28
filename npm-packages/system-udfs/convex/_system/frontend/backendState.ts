import { Infer } from "convex/values";
import { queryPrivateSystem } from "../secretSystemTables";
import { noPermissionRequired } from "../server";

import { backendState as backendStateValidator } from "../../schema";

type BackendState = Infer<typeof backendStateValidator>;

export const backendState = queryPrivateSystem(noPermissionRequired)({
  args: {},
  handler: async function ({ db }): Promise<BackendState> {
    const row = (await db.query("_backend_state").first())!;
    return { system: row.system, usage_limit: row.usage_limit, user: row.user };
  },
});
