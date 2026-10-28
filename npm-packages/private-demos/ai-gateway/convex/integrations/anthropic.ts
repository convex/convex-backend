"use node";

import Anthropic from "@anthropic-ai/sdk";
import { getServiceToken } from "convex/server";
import {
  LOCAL_GATEWAY_URL,
  type ModelListSummary,
  summarizeModels,
} from "./modelList";

async function createAnthropicClient(gatewayUrl: string): Promise<Anthropic> {
  return new Anthropic({
    baseURL: gatewayUrl,
    apiKey: null,
    authToken: await getServiceToken("ai-gateway"),
  });
}

export async function listModels(
  gatewayUrl = LOCAL_GATEWAY_URL,
): Promise<ModelListSummary> {
  const anthropic = await createAnthropicClient(gatewayUrl);
  const models = await anthropic.models.list();
  return summarizeModels({
    object: "list",
    data: models.data,
  });
}
