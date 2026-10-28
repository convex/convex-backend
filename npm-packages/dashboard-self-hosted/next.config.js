const ContentSecurityPolicy = `
  frame-ancestors 'self';
`;

const securityHeaders = [
  {
    key: "X-DNS-Prefetch-Control",
    value: "on",
  },
  {
    key: "Strict-Transport-Security",
    value: "max-age=63072000; includeSubDomains; preload",
  },
  {
    key: "X-XSS-Protection",
    value: "1; mode=block",
  },
  {
    key: "Referrer-Policy",
    value: "origin-when-cross-origin",
  },
];

const optionsForExport = {
  output: "export",
  images: {
    unoptimized: true,
  },
};

const optionsForBuild = {
  output: "standalone",
  async headers() {
    return [
      {
        // Apply these headers to all routes in your application.
        source: "/:path*",
        headers: [
          ...securityHeaders,
          ...(process.env.EMBEDDED_CORS_HEADERS
            ? [
                {
                  key: "Cross-Origin-Resource-Policy",
                  value: "cross-origin",
                },
                {
                  key: "Cross-Origin-Embedder-Policy",
                  value: "require-corp",
                },
              ]
            : [
                {
                  key: "X-Frame-Options",
                  value: "SAMEORIGIN",
                },
                {
                  key: "Content-Security-Policy",
                  value: ContentSecurityPolicy.replace(/\s{2,}/g, " ").trim(),
                },
              ]),
        ],
      },
    ];
  },
};

/** @type {import('next').NextConfig} */
const nextConfig = {
  transpilePackages: [],
  reactStrictMode: true,
  // Next 16 writes an AGENTS.md and a CLAUDE.md into the package on every
  // dev/build run; this repo keeps those files under its own conventions.
  agentRules: false,
  ...(process.env.BUILD_TYPE === "export" ? optionsForExport : optionsForBuild),
  turbopack: {
    rules: {
      "*.svg": {
        loaders: ["@svgr/webpack"],
        as: "*.js",
      },
    },
  },
};

module.exports = nextConfig;
