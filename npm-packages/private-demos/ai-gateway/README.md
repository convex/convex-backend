# AI gateway

This internal demo exercises `getServiceToken("ai-gateway")` against the local
AI gateway from regular and Node actions, plus the published-style
`@convex-dev/ai-sdk-provider` path that targets `ai-gateway.convex.dev`.

Start Funrun, Conductor, Usher, and the AI gateway as described in
[`ops/services/ai-gateway/release.md`](../../../ops/services/ai-gateway/release.md).
Then deploy and run the examples:

```shell
just convex-usher dev --once
just convex-usher run fetch:listModels '{}'
just convex-usher run openai:listModels '{}'
just convex-usher run node/fetch:listModels '{}'
just convex-usher run node/openai:listModels '{}'
just convex-usher run node/anthropic:listModels '{}'
just convex-usher run openai:chatCompletion \
  '{"prompt":"Reply with one short sentence."}'
just convex-usher run node/openai:chatCompletion \
  '{"prompt":"Reply with one short sentence."}'
just convex-usher run node/openai:responses \
  '{"prompt":"Reply with one short sentence."}'
just convex-usher run node/anthropic:messages \
  '{"prompt":"Reply with one short sentence."}'
```

Against a reachable `ai-gateway.convex.dev` (or hosts override), run the AI SDK
provider example:

```shell
just convex-usher run node/aiSdkProvider:chatCompletion \
  '{"prompt":"Reply with one short sentence."}'
```

- The OpenAI SDK accepts the Convex token through its asynchronous `apiKey`
  provider and sends it as a bearer token.

- `node/anthropic:messages` uses `@convex-dev/ai-sdk-provider`'s
  `convexGateway.messages(...)` model to exercise the gateway's native Anthropic
  Messages endpoint. The Anthropic SDK remains in the demo for listing models.

- `node/openai:responses` uses the provider's `convexGateway.responses(...)`
  model to exercise the gateway's native OpenAI Responses endpoint. The OpenAI
  SDK examples continue to cover model listing and Chat Completions.

## Jev Decisions (alpha)

Classify a support ticket as `urgent`, `normal`, or `low` with Jev. The
`node/aiSdkProvider:triageTicket` action passes the ticket and priority criteria
to AI SDK's `evaluate()` with `convexGateway.evaluationModel()` and returns the
selected priority.

Build the workspace provider and deploy `/alpha/decisions` to the gateway before
running this example. The provider handles authentication with
`getServiceToken("ai-gateway")`. Each request incurs AI usage charges.

```shell
just convex-usher run node/aiSdkProvider:triageTicket \
  '{"ticket":"All users are getting 500 errors when signing in. There is no workaround."}'
```
