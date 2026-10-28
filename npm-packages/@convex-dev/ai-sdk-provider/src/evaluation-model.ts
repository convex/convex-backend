import {
  APICallError,
  type experimental_evaluate,
  type ProviderMetadata,
} from "ai";
import { z } from "zod";

type EvaluationModel = Exclude<
  Parameters<typeof experimental_evaluate>[0]["model"],
  string
>;

const probabilities = z.record(z.string(), z.number()).optional();
const responseSchema = z.object({
  id: z.string(),
  model: z.string(),
  answers: z.record(
    z.string(),
    z.discriminatedUnion("type", [
      z.object({
        type: z.literal("choice"),
        choice: z.string(),
        probabilities,
      }),
      z.object({ type: z.literal("score"), score: z.number(), probabilities }),
      z.object({ type: z.literal("noul"), noul: z.number() }),
    ]),
  ),
  usage: z.object({
    input_tokens: z.number().int().nonnegative(),
    output_tokens: z.number().int().nonnegative(),
    cost: z.number().nonnegative().optional(),
  }),
});

export function createEvaluationModel(
  modelId: string,
  baseURL: string,
  fetch: typeof globalThis.fetch,
  usageMetadata: (usage: unknown) => ProviderMetadata | undefined,
): EvaluationModel {
  return {
    specificationVersion: "v4",
    provider: "convexGateway",
    modelId,
    supportedQuestionTypes: ["choice", "score", "boolean"],
    async doEvaluate({
      state,
      questions,
      abortSignal,
      headers,
      providerOptions,
    }) {
      const url = `${baseURL}/decisions`;
      const body = {
        model: modelId,
        state,
        questions: Object.fromEntries(
          Object.entries(questions).map(([id, question]) => [
            id,
            question.type === "boolean"
              ? { ...question, type: "noul" }
              : question,
          ]),
        ),
      };
      const requestHeaders = new Headers();
      for (const [key, value] of Object.entries(headers ?? {})) {
        if (value !== undefined) requestHeaders.set(key, value);
      }
      requestHeaders.set("Content-Type", "application/json");
      const response = await fetch(url, {
        method: "POST",
        headers: requestHeaders,
        signal: abortSignal,
        body: JSON.stringify(body),
      });
      const responseBody = await response.text();
      if (!response.ok) {
        throw new APICallError({
          message: `Evaluation failed (${response.status}): ${responseBody}`,
          url,
          requestBodyValues: body,
          statusCode: response.status,
          responseHeaders: Object.fromEntries(response.headers),
          responseBody,
        });
      }
      const rawBody: unknown = JSON.parse(responseBody);
      const result = responseSchema.parse(rawBody);
      return {
        answers: Object.fromEntries(
          Object.entries(result.answers).map(([id, answer]) => [
            id,
            answer.type === "noul"
              ? { type: "boolean" as const, probability: answer.noul }
              : answer,
          ]),
        ),
        usage: {
          inputTokens: result.usage.input_tokens,
          outputTokens: result.usage.output_tokens,
        },
        // Jev rounds scores and probabilities independently to two decimal places.
        rounding: { probabilityDecimals: 2, scoreDecimals: 2 },
        providerMetadata: usageMetadata(result.usage),
        warnings: Object.keys(providerOptions ?? {}).map((provider) => ({
          type: "unsupported" as const,
          feature: `providerOptions.${provider}`,
        })),
        response: {
          id: result.id,
          modelId: result.model,
          headers: Object.fromEntries(response.headers),
          body: rawBody,
        },
      };
    },
  };
}
