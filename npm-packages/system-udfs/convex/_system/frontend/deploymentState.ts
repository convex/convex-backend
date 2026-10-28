import { Infer } from "convex/values";
import { queryPrivateSystem } from "../secretSystemTables";
import { noPermissionRequired } from "../server";

import { oldBackendState } from "../../tableDefs/deploymentAuditLogTable";
import { backendState } from "../../schema";

type OldBackendState = Infer<typeof oldBackendState>;
type BackendState = Infer<typeof backendState>;

export const deploymentState = queryPrivateSystem(noPermissionRequired)({
  args: {},
  handler: async function ({ db }): Promise<{ state: OldBackendState }> {
    const row = (await db.query("_backend_state").first())!;
    return { state: toOldBackendStateLossy(row) };
  },
});

function toOldBackendStateLossy(row: BackendState): OldBackendState {
  if (row.system === "disabled") return "disabled";
  if (row.system === "suspended") return "suspended";
  if (row.user === "paused") return "paused";
  return "running";
}
