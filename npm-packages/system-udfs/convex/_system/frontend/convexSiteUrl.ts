import { queryPrivateSystem } from "../secretSystemTables";

declare const process: { env: { CONVEX_SITE_URL: string } };

export default queryPrivateSystem("ViewData")({
  args: {},
  handler: async function () {
    return process.env.CONVEX_SITE_URL;
  },
});
