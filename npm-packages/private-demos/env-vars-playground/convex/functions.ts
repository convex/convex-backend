import { query, action, env } from "./_generated/server";

export const greeting = query({
  args: {},
  handler: async () => {
    return env.GREETING;
  },
});

export const greetingAction = action({
  args: {},
  handler: async () => {
    return env.GREETING;
  },
});
