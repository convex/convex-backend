import type {
  experimental_generateVideo,
  JSONValue,
  ProviderMetadata,
} from "ai";

type VideoModel = Extract<
  Parameters<typeof experimental_generateVideo>[0]["model"],
  { specificationVersion: "v4" }
>;
type VideoOptions = Parameters<NonNullable<VideoModel["doGenerate"]>>[0];
type VideoResult = Awaited<ReturnType<NonNullable<VideoModel["doGenerate"]>>>;
type VideoFile = NonNullable<VideoOptions["image"]>;
type StatusOptions = {
  operation: JSONValue;
  headers?: Record<string, string | undefined>;
  abortSignal?: AbortSignal;
};
type StartResult = Omit<VideoResult, "videos"> & { operation: JSONValue };
type StatusResult = {
  providerMetadata?: ProviderMetadata;
  response: VideoResult["response"];
} & ({ status: "pending" | "completed" } | { status: "error"; error: string });
type GatewayVideoModel = Omit<
  VideoModel,
  "doStatus" | "handleWebhookOption"
> & {
  doStart(
    options: VideoOptions & { webhookUrl?: string },
  ): Promise<StartResult>;
  getStatus(options: StatusOptions): Promise<StatusResult>;
  download(options: StatusOptions): Promise<VideoResult>;
};

function statusResult(
  body: Record<string, unknown>,
  response: VideoResult["response"],
  providerMetadata?: ProviderMetadata,
): StatusResult {
  if (body.status === "pending" || body.status === "completed") {
    return { status: body.status, providerMetadata, response };
  }
  if (body.status === "error") {
    return {
      status: "error",
      error:
        typeof body.error === "string" ? body.error : "Video generation failed",
      providerMetadata,
      response,
    };
  }
  throw new Error("Invalid video status from the Convex AI gateway");
}

function fileUrl(file: VideoFile): string {
  if (file.type === "url") return file.url;
  const base64 =
    typeof file.data === "string"
      ? file.data
      : btoa(
          Array.from(file.data, (byte) => String.fromCharCode(byte)).join(""),
        );
  return `data:${file.mediaType};base64,${base64}`;
}

function prepare(options: VideoOptions, modelId: string) {
  if (options.n !== 1)
    throw new Error("The Convex video gateway accepts one video per call");
  const warnings: VideoResult["warnings"] = [];
  if (options.fps !== undefined)
    warnings.push({ type: "unsupported", feature: "fps" });
  const extra = options.providerOptions.convexGateway ?? {};
  const allowedOptions = [
    "resolution",
    "generate_audio",
    "frame_images",
    "input_references",
  ];
  for (const key of Object.keys(extra)) {
    if (!allowedOptions.includes(key))
      throw new Error(`Unsupported Convex video provider option: ${key}`);
  }
  return {
    warnings,
    body: {
      ...extra,
      resolution:
        options.resolution === undefined ? extra.resolution : undefined,
      model: modelId,
      prompt: options.prompt ?? "",
      aspect_ratio: options.aspectRatio,
      size: options.resolution,
      duration: options.duration,
      seed: options.seed,
      ...(options.generateAudio !== undefined && {
        generate_audio: options.generateAudio,
      }),
      ...(options.inputReferences && {
        input_references: options.inputReferences.map((file) => {
          const kind = file.mediaType?.startsWith("video/")
            ? "video"
            : file.mediaType?.startsWith("audio/")
              ? "audio"
              : "image";
          return {
            type: `${kind}_url`,
            [`${kind}_url`]: { url: fileUrl(file) },
          };
        }),
      }),
      ...(options.frameImages
        ? {
            frame_images: options.frameImages.map((frame) => ({
              type: "image_url",
              image_url: { url: fileUrl(frame.image) },
              frame_type: frame.frameType,
            })),
          }
        : options.image && {
            frame_images: [
              {
                type: "image_url",
                image_url: { url: fileUrl(options.image) },
                frame_type: "first_frame",
              },
            ],
          }),
    },
  };
}

function videos(body: Record<string, unknown>): VideoResult["videos"] {
  const data = body.data;
  if (
    !Array.isArray(data) ||
    data.length !== 1 ||
    typeof data[0]?.b64_json !== "string" ||
    !data[0].b64_json ||
    typeof data[0]?.media_type !== "string" ||
    !data[0].media_type.startsWith("video/")
  ) {
    throw new Error("Invalid video response from the Convex AI gateway");
  }
  return [
    { type: "base64", data: data[0].b64_json, mediaType: data[0].media_type },
  ];
}

export function createVideoModel(
  modelId: string,
  baseURL: string,
  fetch: typeof globalThis.fetch,
  usageMetadata: (usage: unknown) => ProviderMetadata | undefined,
): GatewayVideoModel {
  async function request(
    path: string,
    body: unknown,
    options: {
      headers?: Record<string, string | undefined>;
      abortSignal?: AbortSignal;
    },
  ) {
    const headers = new Headers();
    for (const [key, value] of Object.entries(options.headers ?? {})) {
      if (value !== undefined) headers.set(key, value);
    }
    headers.set("Content-Type", "application/json");
    const timestamp = new Date();
    const response = await fetch(`${baseURL}/${path}`, {
      method: "POST",
      headers,
      signal: options.abortSignal,
      body: JSON.stringify(body),
    });
    const result: unknown = await response.json();
    if (!result || typeof result !== "object" || Array.isArray(result))
      throw new Error("Invalid video response from the Convex AI gateway");
    const parsed = result as Record<string, unknown>;
    // Async job failures are returned as status results for the caller to handle.
    if (!response.ok || (parsed.error && parsed.status !== "error")) {
      const error = parsed.error as { message?: string } | undefined;
      throw new Error(
        error?.message ?? `Video generation failed (${response.status})`,
      );
    }
    return {
      body: parsed,
      response: {
        timestamp,
        modelId,
        headers: Object.fromEntries(response.headers),
      },
    };
  }
  function operationRequest(path: string, options: StatusOptions) {
    if (typeof options.operation !== "string" || !options.operation) {
      throw new Error("Invalid Convex video operation");
    }
    return request(`videos/${path}`, { operation: options.operation }, options);
  }
  return {
    specificationVersion: "v4",
    provider: "convexGateway",
    modelId,
    maxVideosPerCall: 1,
    async doGenerate(options) {
      const prepared = prepare(options, modelId);
      const { body, response } = await request(
        "videos/generations",
        prepared.body,
        options,
      );
      return {
        videos: videos(body),
        warnings: prepared.warnings,
        providerMetadata: usageMetadata(body.usage),
        response,
      };
    },
    async doStart(options) {
      const prepared = prepare(options, modelId);
      const { body, response } = await request(
        "videos",
        { ...prepared.body, webhook_url: options.webhookUrl },
        options,
      );
      if (
        typeof body.operation !== "string" ||
        !body.operation ||
        typeof body.id !== "string" ||
        (options.webhookUrl && typeof body.webhook_secret !== "string")
      )
        throw new Error("Invalid video operation from the Convex AI gateway");
      return {
        operation: body.operation,
        warnings: prepared.warnings,
        providerMetadata: {
          convexGateway: {
            inferenceId: body.id,
            ...(typeof body.webhook_secret === "string" && {
              webhookSecret: body.webhook_secret,
            }),
          },
        },
        response,
      };
    },
    async getStatus(options) {
      const { body, response } = await operationRequest("status", options);
      return statusResult(body, response, usageMetadata(body.usage));
    },
    async download(options) {
      const { body, response } = await operationRequest("download", options);
      return {
        videos: videos(body),
        warnings: [],
        providerMetadata: usageMetadata(body.usage),
        response,
      };
    },
  };
}
