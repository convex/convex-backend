const { withSentryConfig } = require("@sentry/nextjs");

const ContentSecurityPolicy = `
  frame-ancestors 'self';
`;

const allowedImageDomains = [
  {
    protocol: "https",
    hostname: "**.convex.cloud",
    pathname: "/api/storage/**",
  },
  {
    protocol: "https",
    hostname: "workoscdn.com",
    pathname: "**",
  },
];

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
    key: "X-Frame-Options",
    value: "SAMEORIGIN",
  },
  {
    key: "Referrer-Policy",
    value: "origin-when-cross-origin",
  },
  {
    key: "Content-Security-Policy",
    value: ContentSecurityPolicy.replace(/\s{2,}/g, " ").trim(),
  },
];

/** @type {import('next').NextConfig} */
const nextConfig = {
  transpilePackages: [],
  reactStrictMode: true,
  // Next 16 writes an AGENTS.md and a CLAUDE.md into the package on every
  // dev/build run; this repo keeps those files under its own conventions.
  agentRules: false,
  async rewrites() {
    return [
      {
        source: "/try-chef/:code",
        destination: "/referral/:code",
      },
      {
        source: "/try-chef/:code/apply",
        destination: "/referral/:code/apply",
      },
    ];
  },
  async headers() {
    return [
      {
        // Apply these headers to all routes in your application.
        source: "/:path*",
        headers: securityHeaders,
      },
    ];
  },
  async redirects() {
    return [
      // The security policy lives on the marketing site; keep this temporary so
      // the canonical location can move without browsers caching a stale target.
      {
        source: "/security.txt",
        destination: "https://www.convex.dev/.well-known/security.txt",
        permanent: false,
      },
      {
        source: "/.well-known/security.txt",
        destination: "https://www.convex.dev/.well-known/security.txt",
        permanent: false,
      },
      {
        source: "/login",
        destination: "/api/auth/login",
        permanent: false,
      },
      {
        source: "/signup",
        destination: `${process.env.WORKOS_LOGIN_URL}/sign-up`,
        permanent: false,
      },
      {
        source: "/teams/:teamId*",
        destination: "/",
        permanent: false,
      },
      {
        source: "/projects/:projectId*",
        destination: "/",
        permanent: false,
      },
      {
        source: "/t/:team/settings/sso",
        destination: "/t/:team/settings/team-authentication",
        permanent: false,
      },
      {
        source: "/t/:team/:project/:deploymentName/cron-jobs",
        destination: "/t/:team/:project/:deploymentName/schedules/functions",
        permanent: false,
      },
      {
        source: "/t/:team/:project/:deploymentName/schedules",
        destination: "/t/:team/:project/:deploymentName/schedules/functions",
        permanent: false,
      },
      {
        source: "/t/:team/:project/:deploymentName/settings/snapshot-export",
        destination: "/t/:team/:project/:deploymentName/settings/snapshots",
        permanent: false,
      },
    ];
  },
  images: {
    remotePatterns:
      process.env.VERCEL_ENV === "production"
        ? allowedImageDomains
        : [
            ...allowedImageDomains,
            {
              protocol: "http",
              hostname: "127.0.0.1",
              pathname: "/api/storage/**",
            },
          ],
    // The image optimizer refuses local IPs by default. Local deployments
    // serve storage from 127.0.0.1, which is only used outside of production.
    dangerouslyAllowLocalIP: process.env.VERCEL_ENV !== "production",
  },
  turbopack: {
    rules: {
      "*.svg": {
        loaders: ["@svgr/webpack"],
        as: "*.js",
      },
    },
  },
  compiler: {
    define: {
      // Consider also defining __SENTRY_TRACING__ as false if we stop caring
      // about web vitals.
      __SENTRY_DEBUG__: false,
    },
  },
  env: {
    NEXT_PUBLIC_VERCEL_GIT_COMMIT_SHA: process.env.VERCEL_GIT_COMMIT_SHA || "",
  },
};

// Uploading sourcemaps requires a Sentry auth token. In this monorepo all
// builds are production builds, so restrict it to the only real deployment of
// this app: Vercel production. Preview builds are excluded.
const uploadSourceMaps =
  !!process.env.VERCEL && process.env.VERCEL_ENV === "production";

module.exports = withSentryConfig(nextConfig, {
  org: "convex-dev",
  project: "dashboard",
  release: {
    name: process.env.VERCEL_GIT_COMMIT_SHA,
    create: uploadSourceMaps,
    finalize: uploadSourceMaps,
  },
  sourcemaps: {
    disable: !uploadSourceMaps,
  },
  silent: true,
  widenClientFileUpload: true,
});
