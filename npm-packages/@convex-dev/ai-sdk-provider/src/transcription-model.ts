import {
  APICallError,
  type experimental_transcribe as transcribe,
  type ProviderMetadata,
} from "ai";

type TranscriptionModel = Extract<
  Parameters<typeof transcribe>[0]["model"],
  { specificationVersion: "v4" }
>;
type TranscriptionOptions = Parameters<TranscriptionModel["doGenerate"]>[0];
type TranscriptionResult = Awaited<
  ReturnType<TranscriptionModel["doGenerate"]>
>;

// OpenRouter names formats by extension, not media type.
const audioFormats: Record<string, string> = {
  "audio/mpeg": "mp3",
  "audio/mp3": "mp3",
  "audio/mpga": "mp3",
  "audio/x-mp3": "mp3",
  "audio/wav": "wav",
  "audio/wave": "wav",
  "audio/x-wav": "wav",
  "audio/flac": "flac",
  "audio/x-flac": "flac",
  "audio/mp4": "m4a",
  "audio/m4a": "m4a",
  "audio/x-m4a": "m4a",
  "audio/ogg": "ogg",
  "audio/webm": "webm",
  "audio/aac": "aac",
};

function base64(audio: Uint8Array | string): string {
  if (typeof audio === "string") return audio;
  // Chunked so large recordings stay under the engine's argument limit.
  let binary = "";
  for (let i = 0; i < audio.length; i += 0x8000) {
    binary += String.fromCharCode(...audio.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}

/**
 * The gateway accepts JSON only, so this model sends base64 audio instead of
 * the multipart upload `@ai-sdk/openai` uses.
 */
export function createTranscriptionModel(
  modelId: string,
  baseURL: string,
  fetch: typeof globalThis.fetch,
  usageMetadata: (usage: unknown) => ProviderMetadata | undefined,
): TranscriptionModel {
  return {
    specificationVersion: "v4",
    provider: "convexGateway",
    modelId,
    async doGenerate(
      options: TranscriptionOptions,
    ): Promise<TranscriptionResult> {
      const format =
        audioFormats[options.mediaType.split(";")[0].trim().toLowerCase()];
      if (!format) {
        throw new Error(
          `Unsupported audio type for the Convex AI gateway: ${options.mediaType}`,
        );
      }
      const extra = options.providerOptions?.convexGateway ?? {};
      const headers = new Headers();
      for (const [key, value] of Object.entries(options.headers ?? {})) {
        if (value !== undefined) headers.set(key, value);
      }
      headers.set("Content-Type", "application/json");
      const timestamp = new Date();
      const url = `${baseURL}/audio/transcriptions`;
      const requestBody = {
        ...extra,
        model: modelId,
        input_audio: { data: base64(options.audio), format },
      };
      const response = await fetch(url, {
        method: "POST",
        headers,
        signal: options.abortSignal,
        body: JSON.stringify(requestBody),
      });
      // A proxy error page is not JSON; the status message covers it.
      const body = (await response.json().catch(() => ({}))) as {
        text?: unknown;
        language?: unknown;
        duration?: unknown;
        segments?: Array<{ text?: unknown; start?: unknown; end?: unknown }>;
        usage?: { seconds?: unknown };
        error?: { message?: string };
      };
      if (!response.ok || typeof body.text !== "string") {
        throw new APICallError({
          message:
            body.error?.message ?? `Transcription failed (${response.status})`,
          url,
          requestBodyValues: requestBody,
          statusCode: response.status,
          responseHeaders: Object.fromEntries(response.headers),
          responseBody: JSON.stringify(body),
        });
      }
      const duration = body.duration ?? body.usage?.seconds;
      return {
        text: body.text,
        // Present with `response_format: "verbose_json"`.
        segments: (Array.isArray(body.segments) ? body.segments : []).flatMap(
          ({ text, start, end }) =>
            typeof text === "string" &&
            typeof start === "number" &&
            typeof end === "number"
              ? [{ text, startSecond: start, endSecond: end }]
              : [],
        ),
        language: typeof body.language === "string" ? body.language : undefined,
        durationInSeconds: typeof duration === "number" ? duration : undefined,
        warnings: [],
        providerMetadata: usageMetadata(body.usage),
        response: {
          timestamp,
          modelId,
          headers: Object.fromEntries(response.headers),
          body,
        },
      };
    },
  };
}
