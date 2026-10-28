import { defineChatCompletionAction } from "./integrations/chatCompletionAction";
import { defineListModelsAction } from "./integrations/listModelsAction";
import {
  chatCompletion as chatCompletionWithOpenAi,
  listModels as listModelsWithOpenAi,
} from "./integrations/openai";

export const listModels = defineListModelsAction(listModelsWithOpenAi);
export const chatCompletion = defineChatCompletionAction(
  chatCompletionWithOpenAi,
);
