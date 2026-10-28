import { query } from "./_generated/server";

export const get = query({
  args: {},
  handler: async ({ db }) => {
    return await db.query("tasks").collect();
  },
});
