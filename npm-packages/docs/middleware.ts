// ⚠️ KEEP IN SYNC WITH `netlify/edge-functions/serve-markdown.ts` ⚠️
//
// Vercel Routing Middleware that serves the Markdown version of a page, which
// the llms-txt plugin generates next to its HTML, to clients that ask for
// `text/markdown`, such as coding agents. The Netlify deployment does the same
// in `netlify/edge-functions/serve-markdown.ts`.

export const config = {
  // Asset directories and the `.md` files themselves never have a Markdown
  // version, and the fetch below must not run this middleware again.
  matcher: "/((?!assets/|img/|fonts/|screenshots/|video/)(?!.*\\.md$).*)",
};

export default async function middleware(
  request: Request,
): Promise<Response | undefined> {
  if (!acceptsMarkdown(request.headers.get("accept"))) {
    return undefined;
  }

  const url = new URL(request.url);
  let pathname = url.pathname;
  if (pathname.endsWith("/") && pathname !== "/") {
    pathname = pathname.slice(0, -1);
  }

  // The home page's Markdown version is llms.txt.
  const mdPath =
    pathname === "/" || pathname === "/home" ? "/llms.txt" : `${pathname}.md`;

  // Forwards what passes Vercel's Deployment Protection, so on a protected
  // preview the fetch is let through like the request was.
  const headers = new Headers();
  for (const name of ["cookie", "x-vercel-protection-bypass"]) {
    const value = request.headers.get(name);
    if (value !== null) {
      headers.set(name, value);
    }
  }
  // Protected content must stay out of shared caches, which would otherwise
  // serve it to visitors who never passed the protection.
  const isProtected =
    request.headers.has("x-vercel-protection-bypass") ||
    /(?:^|;\s*)_vercel_jwt=/.test(request.headers.get("cookie") ?? "");
  const response = await fetch(new URL(mdPath, url), { headers });

  // Without a .md file for this path, fall through to the HTML page. The
  // content-type check catches a fallback page served with a 200.
  if (
    !response.ok ||
    response.headers.get("content-type")?.startsWith("text/html")
  ) {
    return undefined;
  }

  return new Response(response.body, {
    status: response.status,
    headers: {
      "content-type": "text/markdown; charset=utf-8",
      "cache-control": `${isProtected ? "private" : "public"}, max-age=3600, stale-while-revalidate=86400`,
      vary: "Accept",
    },
  });
}

// Whether the Accept header lists `text/markdown` with a nonzero quality,
// which is what the Netlify version checks with `Negotiator#mediaTypes()`.
// Wildcards like `*/*` don't count: browsers send those.
function acceptsMarkdown(accept: string | null): boolean {
  if (accept === null) {
    return false;
  }
  return accept.split(",").some((entry) => {
    const [mediaType, ...params] = entry
      .split(";")
      .map((part) => part.trim().toLowerCase());
    if (mediaType !== "text/markdown") {
      return false;
    }
    const quality = params.find((param) => param.startsWith("q="));
    return quality === undefined || Number(quality.slice(2)) > 0;
  });
}
