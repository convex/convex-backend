/**
 * Help the developer store the CONVEX_URL environment variable.
 */
import { chalkStderr } from "chalk";
import * as dotenv from "dotenv";

import { Context } from "../../bundler/context.js";
import { logWarning } from "../../bundler/log.js";
import { loadPackageJson } from "./utils/utils.js";

const _FRAMEWORKS = [
  "create-react-app",
  "Next.js",
  "Vite",
  "Remix",
  "SvelteKit",
  "Expo",
  "TanStackStart",
] as const;
type Framework = (typeof _FRAMEWORKS)[number];

/**
 * A configuration for writing the actual (framework specific) `CONVEX_URL`
 * and `CONVEX_SITE_URL` environment variables to a ".env" type file.
 *
 * May be `null` if there was an error determining any of the field values.
 */
type EnvFileUrlConfig = {
  /** The name of the file - typically `.env.local` */
  envFile: string;
  /**
   * The framework specific `CONVEX_URL`
   *
   * If `null`, ignore and don't update that environment variable.
   */
  convexUrlEnvVar: string | null;
  /**
   * The framework specific `CONVEX_SITE_URL`
   *
   * If `null`, ignore and don't update that environment variable.
   */
  siteUrlEnvVar: string | null;
  /** Existing content loaded from the `envFile`, if it exists */
  existingFileContent: string | null;
} | null;

export async function writeUrlsToEnvFile(
  ctx: Context,
  options: {
    convexUrl: string;
    siteUrl?: string | null | undefined;
  },
): Promise<EnvFileUrlConfig> {
  const envFileConfig = await loadEnvFileUrlConfig(ctx, options);

  if (envFileConfig === null) {
    return null;
  }

  const { envFile, convexUrlEnvVar, siteUrlEnvVar, existingFileContent } =
    envFileConfig;
  let updatedFileContent: string | null = null;
  if (convexUrlEnvVar) {
    updatedFileContent = changedEnvVarFile({
      existingFileContent,
      envVarName: convexUrlEnvVar,
      envVarValue: options.convexUrl,
      commentAfterValue: null,
      commentOnPreviousLine: null,
    })!;
  }
  if (siteUrlEnvVar && options.siteUrl) {
    updatedFileContent = changedEnvVarFile({
      existingFileContent: updatedFileContent ?? existingFileContent,
      envVarName: siteUrlEnvVar,
      envVarValue: options.siteUrl,
      commentAfterValue: null,
      commentOnPreviousLine: null,
    })!;
  }
  if (updatedFileContent) {
    ctx.fs.writeUtf8File(envFile, updatedFileContent);
  }

  return envFileConfig;
}

export function changedEnvVarFile({
  existingFileContent,
  envVarName,
  envVarValue,
  commentAfterValue,
  commentOnPreviousLine,
}: {
  existingFileContent: string | null;
  envVarName: string;
  envVarValue: string;
  commentAfterValue: string | null;
  commentOnPreviousLine: string | null;
}): string | null {
  const varAssignment = `${envVarName}=${envVarValue}${
    commentAfterValue === null ? "" : ` # ${commentAfterValue}`
  }`;
  const commentOnPreviousLineWithLineBreak =
    commentOnPreviousLine === null ? "" : `${commentOnPreviousLine}\n`;
  if (existingFileContent === null) {
    return `${commentOnPreviousLineWithLineBreak}${varAssignment}\n`;
  }
  const config = dotenv.parse(existingFileContent);
  const existing = config[envVarName];
  if (existing === envVarValue) {
    return null;
  }
  if (existing !== undefined) {
    return existingFileContent.replace(
      getEnvVarRegex(envVarName),
      `${varAssignment}`,
    );
  } else {
    const doubleLineBreak = existingFileContent.endsWith("\n") ? "\n" : "\n\n";
    return (
      existingFileContent +
      doubleLineBreak +
      commentOnPreviousLineWithLineBreak +
      varAssignment +
      "\n"
    );
  }
}

export function getEnvVarRegex(envVarName: string) {
  return new RegExp(`^${envVarName}.*$`, "m");
}

export async function suggestedEnvVarNames(ctx: Context): Promise<{
  detectedFramework?: Framework;
  convexUrlEnvVar: ConvexUrlEnvVar;
  convexSiteEnvVar: ConvexSiteUrlEnvVar;
  frontendDevUrl?: string;
  publicPrefix?: string;
}> {
  // no package.json, that's fine, just guess
  if (!ctx.fs.exists("package.json")) {
    return {
      convexUrlEnvVar: "CONVEX_URL",
      convexSiteEnvVar: "CONVEX_SITE_URL",
    };
  }

  const packages = await loadPackageJson(ctx);

  // Is it create-react-app?
  const isCreateReactApp = "react-scripts" in packages;
  if (isCreateReactApp) {
    return {
      detectedFramework: "create-react-app",
      convexUrlEnvVar: "REACT_APP_CONVEX_URL",
      convexSiteEnvVar: "REACT_APP_CONVEX_SITE_URL",
      frontendDevUrl: "http://localhost:3000",
      publicPrefix: "REACT_APP_",
    };
  }

  const isNextJs = "next" in packages;
  if (isNextJs) {
    return {
      detectedFramework: "Next.js",
      convexUrlEnvVar: "NEXT_PUBLIC_CONVEX_URL",
      convexSiteEnvVar: "NEXT_PUBLIC_CONVEX_SITE_URL",
      frontendDevUrl: "http://localhost:3000",
      publicPrefix: "NEXT_PUBLIC_",
    };
  }

  const isExpo = "expo" in packages;
  if (isExpo) {
    return {
      detectedFramework: "Expo",
      convexUrlEnvVar: "EXPO_PUBLIC_CONVEX_URL",
      convexSiteEnvVar: "EXPO_PUBLIC_CONVEX_SITE_URL",
      publicPrefix: "EXPO_PUBLIC_",
    };
  }

  const isSvelteKit = "@sveltejs/kit" in packages;
  if (isSvelteKit) {
    return {
      detectedFramework: "SvelteKit",
      convexUrlEnvVar: "PUBLIC_CONVEX_URL",
      convexSiteEnvVar: "PUBLIC_CONVEX_SITE_URL",
      frontendDevUrl: "http://localhost:5173",
      publicPrefix: "PUBLIC_",
    };
  }

  // TanStackStart currently supports VITE_FOO for browser-side envvars.
  const isTanStackStart =
    "@tanstack/start" in packages || "@tanstack/react-start" in packages;

  if (isTanStackStart) {
    return {
      detectedFramework: "TanStackStart",
      convexUrlEnvVar: "VITE_CONVEX_URL",
      convexSiteEnvVar: "VITE_CONVEX_SITE_URL",
      frontendDevUrl: "http://localhost:3000",
      publicPrefix: "VITE_",
    };
  }

  // Vite is a dependency of a lot of things; vite appearing in dependencies is not a strong indicator.
  const isVite = "vite" in packages;

  if (isVite) {
    return {
      detectedFramework: "Vite",
      convexUrlEnvVar: "VITE_CONVEX_URL",
      convexSiteEnvVar: "VITE_CONVEX_SITE_URL",
      frontendDevUrl: "http://localhost:5173",
      publicPrefix: "VITE_",
    };
  }

  // We detect Remix after Vite because when using Remix as a plugin of Vite
  // (Remix Vite), we want to use Vite-style environment variables.
  const isRemix = "@remix-run/dev" in packages;
  if (isRemix) {
    return {
      detectedFramework: "Remix",
      convexUrlEnvVar: "CONVEX_URL",
      convexSiteEnvVar: "CONVEX_SITE_URL",
      frontendDevUrl: "http://localhost:3000",
    };
  }

  return {
    convexUrlEnvVar: "CONVEX_URL",
    convexSiteEnvVar: "CONVEX_SITE_URL",
  };
}

async function loadEnvFileUrlConfig(
  ctx: Context,
  options: {
    convexUrl: string;
    siteUrl?: string | null | undefined;
  },
): Promise<EnvFileUrlConfig> {
  const { detectedFramework, convexUrlEnvVar, convexSiteEnvVar } =
    await suggestedEnvVarNames(ctx);

  const { envFile, existing } = suggestedDevEnvFile(ctx, detectedFramework);

  if (!existing) {
    return {
      envFile,
      convexUrlEnvVar,
      siteUrlEnvVar: convexSiteEnvVar,
      existingFileContent: null,
    };
  }

  const existingFileContent = ctx.fs.readUtf8File(envFile);
  const config = dotenv.parse(existingFileContent);

  const resolvedConvexUrlEnvVar = resolveEnvVarName(
    convexUrlEnvVar,
    options.convexUrl,
    envFile,
    config,
    EXPECTED_CONVEX_URL_NAMES,
  );
  const resolvedSiteUrlEnvVar = resolveEnvVarName(
    convexSiteEnvVar,
    options.siteUrl ?? "",
    envFile,
    config,
    EXPECTED_SITE_URL_NAMES,
  );
  if (
    resolvedConvexUrlEnvVar.kind === "invalid" ||
    resolvedSiteUrlEnvVar.kind === "invalid"
  ) {
    return null;
  }
  return {
    envFile,
    convexUrlEnvVar: resolvedConvexUrlEnvVar.envVarName,
    siteUrlEnvVar: resolvedSiteUrlEnvVar.envVarName,
    existingFileContent,
  };
}

function resolveEnvVarName(
  envVarName: string,
  envVarValue: string,
  envFile: string,
  config: dotenv.DotenvParseOutput,
  expectedNames: Set<string>,
):
  | {
      kind: "invalid";
    }
  | {
      kind: "valid";
      envVarName: string | null;
    } {
  const matching = Object.keys(config).filter((key) => expectedNames.has(key));
  if (matching.length > 1) {
    logWarning(
      chalkStderr.yellow(
        `Found multiple ${envVarName} environment variables in ${envFile} so cannot update automatically.`,
      ),
    );
    return { kind: "invalid" };
  }
  if (matching.length === 1) {
    const [existingEnvVarName, oldValue] = [matching[0], config[matching[0]]];
    if (oldValue === envVarValue) {
      // Set envVarName to null to indicate that it shouldn't be updated.
      return { kind: "valid", envVarName: null };
    }
    if (
      oldValue !== "" &&
      Object.values(config).filter((v) => v === oldValue).length !== 1
    ) {
      logWarning(
        chalkStderr.yellow(
          `Can't safely modify ${envFile} for ${envVarName}, please edit manually.`,
        ),
      );
      return { kind: "invalid" };
    }
    return { kind: "valid", envVarName: existingEnvVarName };
  }
  return { kind: "valid", envVarName };
}

function suggestedDevEnvFile(
  ctx: Context,
  framework?: Framework,
): {
  existing: boolean;
  envFile: string;
} {
  // If a .env.local file exists, that's unequivocally the right file
  if (ctx.fs.exists(".env.local")) {
    return {
      existing: true,
      envFile: ".env.local",
    };
  }

  // Remix is on team "don't commit the .env file," so .env is for dev.
  if (framework === "Remix") {
    return {
      existing: ctx.fs.exists(".env"),
      envFile: ".env",
    };
  }

  // The most dev-looking env file that exists, or .env.local
  return {
    existing: ctx.fs.exists(".env.local"),
    envFile: ".env.local",
  };
}

export const EXPECTED_CONVEX_URL_NAMES = new Set([
  "CONVEX_URL" as const,
  "PUBLIC_CONVEX_URL" as const,
  "NEXT_PUBLIC_CONVEX_URL" as const,
  "VITE_CONVEX_URL" as const,
  "REACT_APP_CONVEX_URL" as const,
  "EXPO_PUBLIC_CONVEX_URL" as const,
]);
type ConvexUrlEnvVar =
  typeof EXPECTED_CONVEX_URL_NAMES extends Set<infer T> ? T : never;

export const EXPECTED_SITE_URL_NAMES = new Set([
  "CONVEX_SITE_URL" as const,
  "PUBLIC_CONVEX_SITE_URL" as const,
  "NEXT_PUBLIC_CONVEX_SITE_URL" as const,
  "VITE_CONVEX_SITE_URL" as const,
  "REACT_APP_CONVEX_SITE_URL" as const,
  "EXPO_PUBLIC_CONVEX_SITE_URL" as const,
]);
type ConvexSiteUrlEnvVar =
  typeof EXPECTED_SITE_URL_NAMES extends Set<infer T> ? T : never;

// Crash or warn on
// CONVEX_DEPLOY_KEY=project:me:new-project|eyABCD0= npx convex
// which parses as
// CONVEX_DEPLOY_KEY=project:me:new-project | eyABCD0='' npx convex
// when what was intended was
// CONVEX_DEPLOY_KEY=project:me:new-project|eyABCD0= npx convex
// This only fails so catastrophically when the key ends with `=`.
export async function detectSuspiciousEnvironmentVariables(
  ctx: Context,
  ignoreSuspiciousEnvVars = false,
) {
  for (const [key, value] of Object.entries(process.env)) {
    if (value === "" && key.startsWith("ey")) {
      try {
        // add a "=" to the end and try to base64 decode (expected format of Convex keys)
        const decoded = JSON.parse(
          Buffer.from(key + "=", "base64").toString("utf8"),
        );
        // Only parseable v2 tokens to be sure this is a Convex token before complaining.
        if (!("v2" in decoded)) {
          continue;
        }
      } catch {
        continue;
      }

      if (ignoreSuspiciousEnvVars) {
        logWarning(
          `ignoring suspicious environment variable ${key}, did you mean to use quotes like CONVEX_DEPLOY_KEY='...'?`,
        );
      } else {
        return await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage: `Quotes are required around environment variable values by your shell: CONVEX_DEPLOY_KEY='project:name:project|${key.slice(0, 4)}...${key.slice(key.length - 4)}=' npx convex dev`,
        });
      }
    }
  }
}

export function getBuildEnvironment(): string | false {
  return process.env.VERCEL
    ? "Vercel"
    : process.env.NETLIFY
      ? "Netlify"
      : process.env.CF_PAGES || process.env.WORKERS_CI
        ? "Cloudflare"
        : false;
}

export function getDefaultDeployMessage(): string | null {
  const platforms: Array<{
    name: string;
    detect: () => boolean;
    commitShaVar: string;
  }> = [
    {
      name: "GitHub Actions",
      detect: () => !!process.env.GITHUB_ACTIONS,
      commitShaVar: "GITHUB_SHA",
    },
    {
      name: "Vercel",
      detect: () => !!process.env.VERCEL,
      commitShaVar: "VERCEL_GIT_COMMIT_SHA",
    },
    {
      name: "Netlify",
      detect: () => !!process.env.NETLIFY,
      commitShaVar: "COMMIT_REF",
    },
    {
      name: "Cloudflare Pages",
      detect: () => !!process.env.CF_PAGES,
      commitShaVar: "CF_PAGES_COMMIT_SHA",
    },
    {
      name: "Cloudflare Workers",
      detect: () => !!process.env.WORKERS_CI,
      commitShaVar: "WORKERS_CI_COMMIT_SHA",
    },
    {
      name: "Render",
      detect: () => !!process.env.RENDER,
      commitShaVar: "RENDER_GIT_COMMIT",
    },
    {
      name: "Railway",
      detect: () => !!process.env.RAILWAY_ENVIRONMENT,
      commitShaVar: "RAILWAY_GIT_COMMIT_SHA",
    },
    {
      name: "GitLab CI",
      detect: () => !!process.env.GITLAB_CI,
      commitShaVar: "CI_COMMIT_SHA",
    },
    {
      name: "CircleCI",
      detect: () => !!process.env.CIRCLECI,
      commitShaVar: "CIRCLE_SHA1",
    },
    {
      name: "Google Cloud Build",
      detect: () =>
        !!process.env.BUILD_ID &&
        !!process.env.PROJECT_ID &&
        !!process.env.PROJECT_NUMBER &&
        !!process.env.LOCATION,
      commitShaVar: "SHORT_SHA",
    },
    {
      name: "Heroku",
      detect: () => !!process.env.HEROKU_APP_NAME || !!process.env.DYNO,
      commitShaVar: "HEROKU_BUILD_COMMIT",
    },
  ];

  const platform = platforms.find((p) => p.detect());
  if (!platform) {
    return null;
  }

  const commitSha = process.env[platform.commitShaVar];
  if (!commitSha) {
    return `Deployed from ${platform.name}`;
  }

  const shortSha = commitSha.slice(0, 7);
  return `Deployed from ${platform.name} • ${shortSha}`;
}

export function gitBranchFromEnvironment(): string | null {
  if (process.env.VERCEL) {
    // https://vercel.com/docs/projects/environment-variables/system-environment-variables
    return process.env.VERCEL_GIT_COMMIT_REF ?? null;
  }
  if (process.env.NETLIFY) {
    // https://docs.netlify.com/configure-builds/environment-variables/
    return process.env.HEAD ?? null;
  }
  if (process.env.CF_PAGES || process.env.WORKERS_CI) {
    // https://developers.cloudflare.com/pages/configuration/build-configuration/#environment-variables
    // https://developers.cloudflare.com/workers/ci-cd/builds/configuration/#environment-variables
    return process.env.CF_PAGES_BRANCH ?? process.env.WORKERS_CI_BRANCH ?? null;
  }

  if (process.env.CI) {
    // https://docs.github.com/en/actions/learn-github-actions/variables
    // https://docs.gitlab.com/ee/ci/variables/predefined_variables.html
    return (
      process.env.GITHUB_HEAD_REF ?? process.env.CI_COMMIT_REF_NAME ?? null
    );
  }

  return null;
}

export function isNonProdBuildEnvironment(): boolean {
  if (process.env.VERCEL) {
    // https://vercel.com/docs/projects/environment-variables/system-environment-variables
    return process.env.VERCEL_ENV !== "production";
  }
  if (process.env.NETLIFY) {
    // https://docs.netlify.com/configure-builds/environment-variables/
    return process.env.CONTEXT !== "production";
  }
  if (process.env.CF_PAGES || process.env.WORKERS_CI) {
    // https://developers.cloudflare.com/pages/configuration/build-configuration/#environment-variables
    // https://developers.cloudflare.com/workers/ci-cd/builds/configuration/#environment-variables
    // Branch !== "main" is the closest heuristic; Cloudflare Pages
    // does not expose a dedicated production/preview flag.
    const branch = process.env.CF_PAGES_BRANCH ?? process.env.WORKERS_CI_BRANCH;
    return branch !== "main";
  }
  return false;
}
