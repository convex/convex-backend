import { httpRouter } from "convex/server";
import { api } from "./_generated/api";
import { httpAction } from "./_generated/server";

const http = httpRouter();

// HTTP action calling nested mutation for request metadata
http.route({
  method: "POST",
  path: "/requestMetadataFromMutation",
  handler: httpAction(async (ctx, _request) => {
    const metadata = await ctx.runMutation(
      api.requestMetadata.fromMutation,
      {},
    );
    return new Response(JSON.stringify(metadata));
  }),
});

// HTTP action calling nested V8 action for request metadata
http.route({
  method: "POST",
  path: "/requestMetadataFromAction",
  handler: httpAction(async (ctx, _request) => {
    const metadata = await ctx.runAction(api.requestMetadata.fromAction, {});
    return new Response(JSON.stringify(metadata));
  }),
});

// HTTP action calling nested node action for request metadata
http.route({
  method: "POST",
  path: "/requestMetadataFromNodeAction",
  handler: httpAction(async (ctx, _request) => {
    const metadata = await ctx.runAction(api.nodeActions.fromNodeAction, {});
    return new Response(JSON.stringify(metadata));
  }),
});

export default http;
