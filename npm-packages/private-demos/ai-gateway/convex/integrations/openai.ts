import { getServiceToken } from "convex/server";
import OpenAI from "openai";
import {
  LOCAL_GATEWAY_URL,
  type ModelListSummary,
  summarizeModels,
} from "./modelList";

function createOpenAIClient(gatewayUrl: string): OpenAI {
  return new OpenAI({
    baseURL: `${gatewayUrl}/v1`,
    apiKey: () => getServiceToken("ai-gateway"),
  });
}

export async function listModels(
  gatewayUrl = LOCAL_GATEWAY_URL,
): Promise<ModelListSummary> {
  return summarizeModels(await createOpenAIClient(gatewayUrl).models.list());
}

export async function chatCompletion(
  prompt: string,
  gatewayUrl = LOCAL_GATEWAY_URL,
): Promise<string> {
  const completion = await createOpenAIClient(
    gatewayUrl,
  ).chat.completions.create({
    model: "~anthropic/claude-sonnet-latest",
    messages: [{ role: "user", content: prompt }],
  });
  const text = completion.choices[0]?.message.content;
  if (!text) {
    throw new Error("The AI response did not contain text.");
  }
  return text;
}
