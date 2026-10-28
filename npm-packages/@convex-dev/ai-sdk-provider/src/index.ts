import { createAnthropic } from "@ai-sdk/anthropic";
import { createOpenAI } from "@ai-sdk/openai";
import {
  createOpenAICompatible,
  type MetadataExtractor,
} from "@ai-sdk/openai-compatible";
import {
  defaultSettingsMiddleware,
  wrapEmbeddingModel,
  wrapLanguageModel,
} from "ai";
import { getServiceToken } from "convex/server";
import { createEvaluationModel } from "./evaluation-model.js";
import { createTranscriptionModel } from "./transcription-model.js";
import { createVideoModel } from "./video-model.js";

type Provider = ReturnType<typeof createOpenAICompatible>;
type ChatModel = ReturnType<Provider>;
type EmbeddingModel = ReturnType<typeof wrapEmbeddingModel>;
type ProviderMetadata = NonNullable<
  Awaited<ReturnType<MetadataExtractor["extractMetadata"]>>
>;
type ImageModel = ReturnType<Provider["imageModel"]>;
type LanguageModel = Parameters<typeof wrapLanguageModel>[0]["model"];
type GatewayLanguageModel = ReturnType<typeof wrapLanguageModel>;

export interface GatewayModelOptions {
  /** Require an inference endpoint with a zero-data-retention policy. */
  zdr?: boolean;
}

const maxEmbeddingsPerCall = 512;
// Official providers require a credential before gatewayFetch replaces it with a deployment JWT.
const placeholderCredential = "convex-gateway";

/**
 * A deployment can set `CONVEX_INTERNAL_AI_GATEWAY_HOST` to reach a different
 * gateway, which is how internal apps use staging.
 */
const productionGatewayHost = "https://ai-gateway.convex.dev";

function gatewayBaseURL(version: "v1" | "alpha" = "v1"): string {
  return `${process.env.CONVEX_INTERNAL_AI_GATEWAY_HOST || productionGatewayHost}/${version}`;
}

async function gatewayFetch(
  input: RequestInfo | URL,
  init?: RequestInit,
): Promise<Response> {
  const token = await getServiceToken("ai-gateway");
  const headers = new Headers(init?.headers);
  // Deployment JWT is the only accepted credential for the hosted gateway.
  headers.set("Authorization", `Bearer ${token}`);
  return globalThis.fetch(input, { ...init, headers });
}

// The SDK's standard usage mapping omits the gateway's dollar costs.
function convexGatewayUsageMetadata(
  usage: unknown,
): ProviderMetadata | undefined {
  if (!usage || typeof usage !== "object") return undefined;
  const { cost, cost_details } = usage as ProviderMetadata[string];
  const metadata: ProviderMetadata[string] = {};
  if (typeof cost === "number") metadata.cost = cost;
  if (cost_details && typeof cost_details === "object") {
    metadata.costDetails = cost_details;
  }
  return Object.keys(metadata).length > 0
    ? { convexGateway: metadata }
    : undefined;
}

const costMetadataExtractor: MetadataExtractor = {
  extractMetadata: async ({ parsedBody }) =>
    convexGatewayUsageMetadata(
      (parsedBody as { usage?: unknown } | undefined)?.usage,
    ),
  createStreamExtractor: () => {
    // Streamed responses deliver usage (incl. cost) on the final chunk.
    let usage: unknown;
    return {
      processChunk(parsedChunk: unknown) {
        const chunk = parsedChunk as { usage?: unknown } | undefined;
        if (chunk?.usage) usage = chunk.usage;
      },
      buildMetadata: () => convexGatewayUsageMetadata(usage),
    };
  },
};

function createGatewayProvider(fetch = gatewayFetch): Provider {
  return createOpenAICompatible({
    name: "convexGateway",
    baseURL: gatewayBaseURL(),
    fetch,
    metadataExtractor: costMetadataExtractor,
    supportsStructuredOutputs: true,
    supportedUrls: () => ({ "image/*": [/^https?:\/\/.*$/] }),
  });
}

function sdkModelId(
  gatewayModelId: string,
  provider: "anthropic" | "openai",
): string {
  const prefix = `${provider}/`;
  const modelId = gatewayModelId.startsWith(prefix)
    ? gatewayModelId.slice(prefix.length)
    : gatewayModelId;
  // OpenRouter uses dots in Anthropic versions; the Anthropic SDK's capability lookup uses hyphens.
  return provider === "anthropic" ? modelId.replaceAll(".", "-") : modelId;
}

function gatewayModelFetch(
  gatewayModelId: string,
  input: RequestInfo | URL,
  init?: RequestInit,
  options?: GatewayModelOptions,
): Promise<Response> {
  const body = JSON.parse(init?.body as string);
  body.model = gatewayModelId;
  if (options?.zdr !== undefined) {
    body.provider = { ...body.provider, zdr: options.zdr };
  }
  return gatewayFetch(input, { ...init, body: JSON.stringify(body) });
}

function gatewayLanguageModel(
  modelId: string,
  model: LanguageModel,
  middleware?: Parameters<typeof wrapLanguageModel>[0]["middleware"],
): GatewayLanguageModel {
  return wrapLanguageModel({
    model,
    modelId,
    // The gateway does not expose the providers' batch APIs.
    middleware: middleware ?? { specificationVersion: "v4" },
  });
}

/**
 * The recommended model interface for text generation through the Convex AI gateway.
 * Use `messages` or `responses` only for endpoint-specific features.
 *
 * `getServiceToken` caches and refreshes credentials within the current action,
 * so calling this more than once in the same action is fine.
 */
export function convexGateway(
  modelId: string,
  options?: GatewayModelOptions,
): ChatModel {
  return createGatewayProvider((input, init) =>
    gatewayModelFetch(modelId, input, init, options),
  )(modelId);
}

convexGateway.messages = function (
  modelId: string,
  options?: GatewayModelOptions,
): GatewayLanguageModel {
  const provider = createAnthropic({
    name: "convexGateway.messages",
    baseURL: gatewayBaseURL(),
    authToken: placeholderCredential,
    fetch: (input, init) => gatewayModelFetch(modelId, input, init, options),
  });
  return gatewayLanguageModel(
    modelId,
    provider.messages(sdkModelId(modelId, "anthropic")),
  );
};

convexGateway.responses = function (
  modelId: string,
  options?: GatewayModelOptions,
): GatewayLanguageModel {
  const provider = createOpenAI({
    name: "convexGateway.responses",
    baseURL: gatewayBaseURL(),
    apiKey: placeholderCredential,
    fetch: (input, init) => gatewayModelFetch(modelId, input, init, options),
  });
  return gatewayLanguageModel(
    modelId,
    provider.responses(sdkModelId(modelId, "openai")),
    defaultSettingsMiddleware({
      settings: { providerOptions: { openai: { store: false } } },
    }),
  );
};

/** Evaluate typed questions through the alpha Decisions API. */
convexGateway.evaluationModel = function (
  modelId: string,
  options?: GatewayModelOptions,
) {
  return createEvaluationModel(
    modelId,
    gatewayBaseURL("alpha"),
    (input, init) => gatewayModelFetch(modelId, input, init, options),
    convexGatewayUsageMetadata,
  );
};

convexGateway.embeddingModel = function (
  modelId: string,
  options?: GatewayModelOptions,
): EmbeddingModel {
  return wrapEmbeddingModel({
    model: createGatewayProvider((input, init) =>
      gatewayModelFetch(modelId, input, init, options),
    ).embeddingModel(modelId),
    middleware: {
      specificationVersion: "v4",
      overrideMaxEmbeddingsPerCall: () => maxEmbeddingsPerCall,
    },
  });
};

/** Image model for the AI SDK's `generateImage`. */
convexGateway.imageModel = function (modelId: string): ImageModel {
  const { specificationVersion, provider, maxImagesPerCall } =
    createGatewayProvider().imageModel(modelId);
  return {
    specificationVersion,
    provider,
    modelId,
    maxImagesPerCall,
    async doGenerate(options) {
      if (options.files?.length || options.mask) {
        throw new Error(
          "Convex AI Gateway does not support image editing. Use a text prompt without input images or a mask.",
        );
      }
      // The SDK's image adapter discards `usage` and has no metadata hook, so
      // each call reads it from its own response through a per-call fetch.
      let usage: unknown;
      const model = createGatewayProvider(async (input, init) => {
        const response = await gatewayFetch(input, init);
        if (response.ok) {
          usage = (await response.clone().json()).usage;
        }
        return response;
      }).imageModel(modelId);
      const result = await model.doGenerate(options);
      const metadata = convexGatewayUsageMetadata(usage)?.convexGateway;
      return {
        ...result,
        ...(metadata && {
          // The SDK's image metadata type requires a per-image `images` array.
          providerMetadata: { convexGateway: { ...metadata, images: [] } },
        }),
      };
    },
  };
};

/** Speech-to-text for the AI SDK's `experimental_transcribe`. */
convexGateway.transcriptionModel = function (modelId: string) {
  return createTranscriptionModel(
    modelId,
    gatewayBaseURL(),
    gatewayFetch,
    convexGatewayUsageMetadata,
  );
};

/**
 * Text-to-speech for the AI SDK's `experimental_generateSpeech`. The gateway
 * reports speech cost after the call, so results carry no cost metadata.
 */
convexGateway.speechModel = function (modelId: string) {
  const provider = createOpenAI({
    name: "convexGateway.speech",
    baseURL: gatewayBaseURL(),
    apiKey: placeholderCredential,
    fetch: (input, init) => gatewayModelFetch(modelId, input, init),
  });
  return provider.speech(modelId);
};

/** Video generation for the AI SDK's `experimental_generateVideo`. */
convexGateway.videoModel = function (modelId: string) {
  return createVideoModel(
    modelId,
    gatewayBaseURL(),
    gatewayFetch,
    convexGatewayUsageMetadata,
  );
};

export { verifyVideoWebhook, type VideoWebhookEvent } from "./video-webhook.js";
