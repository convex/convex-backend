import { httpRouter } from "convex/server";
import { httpAction } from "./_generated/server";
import { internal } from "./_generated/api";

const http = httpRouter();

// Serves `https://<deployment>.convex.site/postMessage`
http.route({
  path: "/postMessage",
  method: "POST",
  handler: httpAction(async (ctx, request) => {
    const { author, body } = await request.json();
    await ctx.runMutation(internal.messages.send, { author, body });
    return new Response(null, { status: 200 });
  }),
});

// Serves `https://<deployment>.convex.site/getMessages/<author>`
http.route({
  pathPrefix: "/getMessages/",
  method: "GET",
  handler: httpAction(async (ctx, request) => {
    const author = new URL(request.url).pathname.slice("/getMessages/".length);
    const messages = await ctx.runQuery(internal.messages.listByAuthor, {
      author,
    });
    return Response.json(messages);
  }),
});

export default http;
