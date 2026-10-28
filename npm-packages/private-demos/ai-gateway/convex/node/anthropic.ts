"use node";

import { convexGateway } from "@convex-dev/ai-sdk-provider";
import { generateText } from "ai";
import { actionGeneric } from "convex/server";
import { v } from "convex/values";
import { defineListModelsAction } from "../integrations/listModelsAction";
import { listModels as listModelsWithAnthropic } from "../integrations/anthropic";

export const listModels = defineListModelsAction(listModelsWithAnthropic);
export const messages = actionGeneric({
  args: {
    prompt: v.string(),
  },
  returns: v.string(),
  handler: async (_ctx, { prompt }) => {
    const { text } = await generateText({
      model: convexGateway.messages("anthropic/claude-haiku-4.5"),
      prompt,
    });
    if (!text) {
      throw new Error("The AI response did not contain text.");
    }
    return text;
  },
});
