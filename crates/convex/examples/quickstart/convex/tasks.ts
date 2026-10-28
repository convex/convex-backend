import { query } from "./_generated/server";

export const get = query({
  handler: async ({ db }) => {
    return await db.query("tasks").collect();
  },
});
