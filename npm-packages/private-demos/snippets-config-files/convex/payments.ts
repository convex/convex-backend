import { v } from "convex/values";
import { internalAction } from "./_generated/server";

export const sendPaymentEmail = internalAction({
  args: { email: v.string() },
  handler: async (_ctx, { email }) => {
    console.log(`Sending payment reminder to ${email}`);
  },
});
