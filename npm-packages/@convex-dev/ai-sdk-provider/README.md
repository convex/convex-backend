# `@convex-dev/ai-sdk-provider`

Use the Convex AI gateway with the AI SDK from a Convex action.

```ts
"use node";

import { convexGateway } from "@convex-dev/ai-sdk-provider";
import { embedMany, generateText } from "ai";
import { action } from "./_generated/server";
import { v } from "convex/values";

export const chat = action({
  args: { prompt: v.string() },
  handler: async (_ctx, { prompt }) => {
    const { text } = await generateText({
      model: convexGateway("anthropic/claude-sonnet-4.5"),
      prompt,
    });
    return text;
  },
});

export const embed = action({
  args: { values: v.array(v.string()) },
  handler: async (_ctx, { values }) => {
    const { embeddings } = await embedMany({
      model: convexGateway.embeddingModel("openai/text-embedding-3-small"),
      values,
    });
    return embeddings;
  },
});
```

Embedding batches larger than the gateway's 512-input limit are split into
multiple requests by the AI SDK.

## Optional ZDR routing

Pass `{ zdr: true }` to use inference endpoints with a zero-data-retention
policy:

```ts
const { text } = await generateText({
  model: convexGateway("openai/gpt-4o-mini", { zdr: true }),
  prompt: "Summarize this document",
});
```

Also supported by `messages`, `responses`, `embeddingModel`, and
`evaluationModel`. The setting applies to that model instance and overrides
`provider.zdr` from other SDK options. Omit it to use the default policy;
`false` cannot disable a stricter gateway policy.

Not every model has a ZDR endpoint. See
[routing requirements and retention limits](https://docs.convex.dev/ai-gateway/api#provider-preferences).

## Choose a model interface

For text generation, start with `convexGateway(model)`. It works across model
providers and is the recommended default. Use an endpoint-specific model only
when you need features unique to Anthropic Messages or OpenAI Responses:

```ts
convexGateway.messages("anthropic/claude-sonnet-4.5");
convexGateway.responses("openai/gpt-5");
```

## Structured decisions

Use provider version 0.2.1 or later for correct validation of Jev's rounded
scores and probabilities.

[Jev](https://docs.typesafe.ai/introduction) evaluates `choice`, `score`, and
`boolean` questions about the context you provide in `state`. Call AI SDK's
experimental `evaluate` from an action:

```ts
import { experimental_evaluate as evaluate } from "ai";
import { convexGateway } from "@convex-dev/ai-sdk-provider";

const decision = await evaluate({
  model: convexGateway.evaluationModel("typesafe/jev-1.13"),
  state: { ticket: "Customer cannot sign in" },
  questions: {
    priority: {
      type: "choice",
      instructions: "Choose the response priority",
      criteria: {
        urgent: "Respond now",
        normal: "Respond today",
      },
    },
    needsReview: {
      type: "boolean",
      instructions: "Does a human need to review this?",
    },
  },
});

console.log(decision.answers.priority.choice);
console.log(decision.answers.needsReview.probability);
```

This requires AI SDK 7.0.105 or later. The evaluation interface and
`/alpha/decisions` endpoint are experimental. Authentication is handled
automatically. Pass `abortSignal` to `evaluate` to cancel a request. Dollar cost
is available in `decision.providerMetadata?.convexGateway?.cost`. The original
gateway response, including provider-specific fields such as `confidence`, is
available in `decision.response.body`.

`getServiceToken("ai-gateway")` supplies a short-lived deployment JWT. The
action runtime caches and refreshes the credential as needed, so `convexGateway`
calls it on every request.

Requires Convex 1.45 or later and AI SDK 7.0.105 or later. The provider works in
the default runtime and in Node.js actions, which need Node.js 22 or later.

## Generate images

Image generation is in alpha. The request and response format may change.

```ts
import { generateImage } from "ai";
import { convexGateway } from "@convex-dev/ai-sdk-provider";

const { images } = await generateImage({
  model: convexGateway.imageModel("openai/gpt-image-1"),
  prompt: "A mountain lake at sunrise",
});
```

Image request costs are available in each entry of `result.calls`, at
`providerMetadata?.convexGateway?.cost`, in US dollars. Each call may generate
multiple images; the cost is for the call, not each image.

## Transcribe and generate speech

Speech-to-text and text-to-speech are in alpha.

```ts
import {
  experimental_generateSpeech as generateSpeech,
  experimental_transcribe as transcribe,
} from "ai";
import { convexGateway } from "@convex-dev/ai-sdk-provider";

const { text, providerMetadata } = await transcribe({
  model: convexGateway.transcriptionModel("openai/whisper-large-v3-turbo"),
  audio: recordingBytes,
});
const cost = providerMetadata.convexGateway?.cost;

const { audio } = await generateSpeech({
  model: convexGateway.speechModel("hexgrad/kokoro-82m"),
  text: "Your order has shipped.",
  voice: "af_heart",
});
```

Speech results have no `cost` in `providerMetadata`, because the cost is known
only after the audio is sent. Convex still bills for speech. The charge appears
on the Convex usage page.

## Generate videos

Video generation is in alpha. APIs may change, and completion callbacks are best
effort. Save async operation handles to check status and retrieve results.

```ts
import { experimental_generateVideo as generateVideo } from "ai";
import { convexGateway } from "@convex-dev/ai-sdk-provider";

const { video, providerMetadata } = await generateVideo({
  model: convexGateway.videoModel("google/veo-3.1"),
  prompt: "A camera pan across a mountain lake",
  duration: 8,
  aspectRatio: "16:9",
  resolution: "1280x720",
});

const bytes = video.uint8Array;
const cost = providerMetadata?.convexGateway?.cost;
```

The call waits for generation and download, with a 64 MiB limit per video. It
fails if generation takes longer than about five minutes; use async videos for
longer jobs. Store the returned bytes in Convex file storage. The AI SDK splits
`n > 1` into separate requests. Cancelling a request does not cancel the
upstream job and can still incur a charge.

An image in the prompt becomes the first frame. Use `frameImages` for explicit
frames, `inputReferences` for image/audio/video references, and `generateAudio`
for audio. `fps` returns an unsupported-option warning.

`providerOptions.convexGateway` accepts `resolution` (such as `720p`),
`generate_audio`, `frame_images`, and `input_references`. Standard SDK options
take precedence. Supported values depend on the
[OpenRouter model](https://openrouter.ai/docs/guides/overview/multimodal/video-generation).

## Async videos

With AI SDK 7.0.105 or later, use `experimental_startVideo` to submit a job
without waiting for the video. See the
[async availability requirements](https://docs.convex.dev/ai-gateway/images-and-videos#async-videos)
before using these routes.

Create an application job first. Use its ID as `requestId` in the callback URL
so the receiver can find the job and its saved secret.

```ts
import { experimental_startVideo as startVideo } from "ai";
import { convexGateway } from "@convex-dev/ai-sdk-provider";

async function startJob(requestId: string) {
  const modelId = "google/veo-3.1";
  const started = await startVideo({
    model: convexGateway.videoModel(modelId),
    prompt: "A camera pan across a mountain lake",
    duration: 8,
    webhookUrl: `${process.env.CONVEX_SITE_URL}/video-complete?requestId=${requestId}`,
    maxRetries: 0,
  });

  return {
    modelId,
    operation: started.operation,
    inferenceId: started.providerMetadata?.convexGateway?.inferenceId,
    webhookSecret: started.providerMetadata?.convexGateway?.webhookSecret,
  };
}
```

Save the returned job in private application storage before the action returns.
Automatic submission retries are disabled because a lost response can still mean
a paid job was accepted. Callback URLs must use the deployment's own HTTPS
`<deployment>.convex.site` origin; redirects are rejected. If `CONVEX_SITE_URL`
uses a custom domain, use the deployment's default `convex.site` origin in the
example instead.

### Receive a callback

Load the saved job and verify the raw request body before updating application
state:

```ts
import { verifyVideoWebhook } from "@convex-dev/ai-sdk-provider";

const event = await verifyVideoWebhook({
  body: await request.text(),
  signature: request.headers.get("x-convex-video-signature"),
  secret: savedJob.webhookSecret,
});

if (event.id !== savedJob.inferenceId) {
  return new Response("Wrong job", { status: 400 });
}
```

Deduplicate `(event.id, event.status)` in the same mutation that saves the
event. For `completed`, schedule an action to download the video. Record
`failed`, `cancelled`, or `expired` as terminal failures. Return 204 after
saving the event; return a non-2xx response if the job's secret has not been
saved yet.

### Retrieve the video

In a later action, use the saved operation to check status and download:

```ts
const model = convexGateway.videoModel(savedJob.modelId);
const status = await model.getStatus({ operation: savedJob.operation });

if (status.status === "completed") {
  const result = await model.download({ operation: savedJob.operation });
  const video = result.videos[0];
  const cost = result.providerMetadata?.convexGateway?.cost;
}
```

`getStatus` returns `pending`, `completed`, or `error`. When the status is
`error`, the `error` field contains the error message. Store the downloaded
video in application storage, since each `download` call fetches it again.
Operations expire after seven days; upstream video retention may be shorter.

Omit `webhookUrl` to use status checks alone. If a completion callback is
missed, use the saved operation to check the job and retrieve its result.
Applications that need automatic recovery can periodically check unfinished
jobs.
