// Local development CORS proxy for talking to production Big Brain from the
// dashboard running on localhost. Takes the same URL shape as the
// `cors-anywhere` package it replaces
// (http://localhost:8080/https://api.convex.dev/...), but only ever forwards to
// Big Brain so it can't be pointed at other hosts reachable from this machine.
const http = require("node:http");
const https = require("node:https");

const host = process.env.HOST || "127.0.0.1";
const port = process.env.PORT || 8080;

const ALLOWED_TARGET = "https://api.convex.dev";

// Headers that are meaningless or harmful to copy to the upstream request.
const STRIPPED_REQUEST_HEADERS = [
  "host",
  "cookie",
  "cookie2",
  "connection",
  "keep-alive",
  "proxy-connection",
  "transfer-encoding",
  "upgrade",
];

// Upstream sends its own CORS headers for dashboard.convex.dev; ours replace
// them rather than being appended alongside.
const STRIPPED_RESPONSE_HEADERS = [
  "access-control-allow-origin",
  "access-control-allow-credentials",
  "access-control-expose-headers",
  "connection",
  "keep-alive",
  "transfer-encoding",
];

function corsHeaders(req) {
  return {
    "access-control-allow-origin": req.headers.origin || "*",
    vary: "Origin",
  };
}

function parseTarget(req) {
  const raw = req.url.slice(1);
  let target;
  try {
    target = new URL(raw);
  } catch {
    return null;
  }
  return target.origin === ALLOWED_TARGET ? target : null;
}

const server = http.createServer((req, res) => {
  if (req.method === "OPTIONS") {
    res.writeHead(204, {
      ...corsHeaders(req),
      "access-control-allow-methods":
        "GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS",
      "access-control-allow-headers":
        req.headers["access-control-request-headers"] || "*",
      "access-control-max-age": "86400",
    });
    res.end();
    return;
  }

  const target = parseTarget(req);
  if (target === null) {
    res.writeHead(403, {
      ...corsHeaders(req),
      "content-type": "text/plain",
    });
    res.end(
      `This proxy only forwards requests to ${ALLOWED_TARGET}. ` +
        `Request the target as a path, e.g. http://${host}:${port}/${ALLOWED_TARGET}/instances\n`,
    );
    return;
  }

  const headers = { ...req.headers };
  for (const header of STRIPPED_REQUEST_HEADERS) {
    delete headers[header];
  }

  const upstream = https.request(
    target,
    { method: req.method, headers },
    (upstreamRes) => {
      const responseHeaders = { ...upstreamRes.headers };
      for (const header of STRIPPED_RESPONSE_HEADERS) {
        delete responseHeaders[header];
      }
      // Redirects would send the browser straight to Big Brain, which rejects
      // the localhost origin, so keep them pointed back through the proxy.
      if (responseHeaders.location !== undefined) {
        const location = new URL(responseHeaders.location, target);
        if (location.origin === ALLOWED_TARGET) {
          responseHeaders.location = `http://${req.headers.host}/${location.href}`;
        }
      }
      res.writeHead(upstreamRes.statusCode, {
        ...responseHeaders,
        ...corsHeaders(req),
        // Without this the dashboard can't read any non-safelisted response
        // header (e.g. the ones it uses for error reporting).
        "access-control-expose-headers":
          Object.keys(responseHeaders).join(", "),
      });
      upstreamRes.pipe(res);
    },
  );

  upstream.on("error", (error) => {
    console.error(`Proxy request to ${target.href} failed:`, error.message);
    if (!res.headersSent) {
      res.writeHead(502, { ...corsHeaders(req), "content-type": "text/plain" });
    }
    res.end("Proxy request failed\n");
  });

  req.pipe(upstream);
});

server.listen(port, host, () => {
  console.log(`Proxying ${ALLOWED_TARGET} on http://${host}:${port}`);
});
