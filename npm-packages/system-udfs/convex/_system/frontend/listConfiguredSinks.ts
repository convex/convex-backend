import { Integration } from "./common";
import { queryPrivateSystem } from "../secretSystemTables";
export default queryPrivateSystem("ViewIntegrations")({
  args: {},
  handler: async ({ db }): Promise<Integration[]> => {
    const integrations = await db.query("_log_sinks").collect();
    return Promise.all(
      integrations.map(async (integration) => {
        if (integration.config.type !== "s3Export") {
          return integration;
        }
        // The AWS secret access key is write-only: `ViewIntegrations` is a read
        // permission, so the stored secret must not leave the deployment.
        const { secretAccessKey: _secret, ...config } = integration.config;
        const state = await db
          .query("_s3_export_state")
          .withIndex("by_config_id", (q) => q.eq("configId", integration._id))
          .unique();
        return {
          ...integration,
          config,
          ...(state ? { exportProgress: state.checkpoint.progress } : {}),
        };
      }),
    );
  },
});
