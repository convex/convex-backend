import { httpRouter } from "convex/server";
import { env, httpAction, query } from "./_generated/server";

const http = httpRouter();

http.route({
  path: "/basic",
  method: "POST",
  handler: httpAction(async (_ctx, _request) => {
    return new Response(JSON.stringify({ hello: "world" }), {
      headers: new Headers({ "content-type": "application/json" }),
      status: 200,
    });
  }),
});

http.route({
  path: "/streaming",
  method: "POST",
  handler: httpAction(async (_ctx, _request) => {
    const encoder = new TextEncoder();
    const stream = new ReadableStream({
      type: "bytes",
      start(controller) {
        controller.enqueue(encoder.encode("<html>"));
        setTimeout(() => {
          controller.enqueue(encoder.encode("</html>"));
          controller.close();
        }, 20);
      },
    });
    return new Response(stream);
  }),
});

export const siteUrl = query({
  args: {},
  handler: () => {
    return env.CONVEX_SITE_URL;
  },
});

export default http;
