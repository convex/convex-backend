import { getServiceToken } from "convex/server";
import {
  LOCAL_GATEWAY_URL,
  type ModelList,
  type ModelListSummary,
  summarizeModels,
} from "./modelList";

export async function listModels(
  gatewayUrl = LOCAL_GATEWAY_URL,
): Promise<ModelListSummary> {
  const token = await getServiceToken("ai-gateway");
  const response = await fetch(`${gatewayUrl}/v1/models`, {
    headers: {
      Authorization: `Bearer ${token}`,
    },
  });
  if (!response.ok) {
    throw new Error(`AI gateway returned ${response.status}`);
  }
  return summarizeModels((await response.json()) as ModelList);
}
