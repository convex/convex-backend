import { Integration } from "./common";
import { queryPrivateSystem } from "../secretSystemTables";
export default queryPrivateSystem("ViewIntegrations")({
  args: {},
  handler: async ({ db }): Promise<Integration[]> => {
    const sinks = await db.query("_log_sinks").collect();
    return Promise.all(
      sinks.map(async (sink) => {
        if (sink.config.type !== "s3Export") {
          return sink;
        }
        // The AWS secret access key is write-only: `ViewIntegrations` is a read
        // permission, so the stored secret must not leave the deployment.
        const { secretAccessKey: _secret, ...config } = sink.config;
        const state = await db
          .query("_s3_export_state")
          .withIndex("by_config_id", (q) => q.eq("configId", sink._id))
          .unique();
        return {
          ...sink,
          config,
          ...(state ? { exportProgress: state.checkpoint.progress } : {}),
        };
      }),
    );
  },
});
