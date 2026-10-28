import { actionGeneric } from "convex/server";
import { v } from "convex/values";

type ChatCompletion = (prompt: string) => Promise<string>;

export function defineChatCompletionAction(chatCompletion: ChatCompletion) {
  return actionGeneric({
    args: {
      prompt: v.string(),
    },
    returns: v.string(),
    handler: async (_ctx, { prompt }) => {
      return await chatCompletion(prompt);
    },
  });
}
