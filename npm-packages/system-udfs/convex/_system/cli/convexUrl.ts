import { queryPrivateSystem } from "../secretSystemTables";

declare const process: { env: { CONVEX_CLOUD_URL: string } };

export const cloudUrl = queryPrivateSystem("ViewData")({
  args: {},
  handler: async function () {
    return process.env.CONVEX_CLOUD_URL;
  },
});
