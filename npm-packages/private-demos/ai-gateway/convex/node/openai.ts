"use node";

import { convexGateway } from "@convex-dev/ai-sdk-provider";
import { generateText } from "ai";
import { actionGeneric } from "convex/server";
import { v } from "convex/values";
import { defineChatCompletionAction } from "../integrations/chatCompletionAction";
import { defineListModelsAction } from "../integrations/listModelsAction";
import {
  chatCompletion as chatCompletionWithOpenAi,
  listModels as listModelsWithOpenAi,
} from "../integrations/openai";

export const listModels = defineListModelsAction(listModelsWithOpenAi);
export const chatCompletion = defineChatCompletionAction(
  chatCompletionWithOpenAi,
);
export const responses = actionGeneric({
  args: {
    prompt: v.string(),
  },
  returns: v.string(),
  handler: async (_ctx, { prompt }) => {
    const { text } = await generateText({
      model: convexGateway.responses("openai/gpt-5"),
      prompt,
    });
    if (!text) {
      throw new Error("The AI response did not contain text.");
    }
    return text;
  },
});
