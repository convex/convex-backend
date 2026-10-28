import {
  DefaultFunctionArgs,
  FunctionReference,
  httpRouter,
} from "convex/server";
import { ActionCtx, env, httpAction } from "./_generated/server";
import { internal } from "./_generated/api";
import { generateMessage } from "./util/message";
import { extractVersionFromHeader } from "./util/convexClientHeader";
import {
  OLD_CURSOR_RULES,
  shouldUseOldCursorRules,
} from "./util/oldCursorRules";
import { hashSha256 } from "./util/hash";
import {
  AgentSkillCatalogResponse,
  AgentSkillManifestRequest,
  validateAgentSkillManifestRequest,
} from "../agentSkillManifestShared";

const http = httpRouter();

function logClientTelemetryHeaders(path: string, req: Request) {
  // Temporary telemetry sink: emit all inbound request headers as JSON so we can
  // forward service logs to Axiom and analyze usage by header dimensions
  // (for example Convex-Client, Convex-Interactive, and Convex-Agent-Mode).
  const headers = Object.fromEntries(req.headers.entries());
  console.log(
    JSON.stringify({
      event: "version_api_headers",
      path,
      headers,
    }),
  );
}

const COMMON_HEADERS = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Methods": "GET, OPTIONS",
  "Access-Control-Allow-Headers":
    "Content-Type, convex-client, convex-interactive, convex-agent-mode",
  "Cache-Control": "public, max-age=3600",
  Vary: "Convex-Client",
};

type VersionResponse = {
  message: string | null;
  cursorRulesHash: string | null;
  guidelinesHash: string | null;
  agentSkillsSha: string | null;
};

function getAgentSkillStatus({
  isDeleted,
  deletedAt,
}: {
  isDeleted: boolean;
  deletedAt: number | undefined;
}) {
  if (!isDeleted) return { kind: "active" } as const;
  if (deletedAt === undefined) {
    console.error(
      "Deleted agent skill catalog entry is missing deletedAt; omitting it from /v1/agent_skills",
    );
    return null;
  }
  return { kind: "deleted", deletedAt } as const;
}

function validateAgentSkillSyncAuth(req: Request) {
  const expectedToken = env.AGENT_SKILLS_SYNC_TOKEN;
  if (!expectedToken) {
    console.error("AGENT_SKILLS_SYNC_TOKEN is not configured");
    return new Response("Server is not configured for agent skill sync", {
      status: 500,
    });
  }

  const authHeader = req.headers.get("Authorization");
  if (authHeader !== `Bearer ${expectedToken}`) {
    return new Response("Unauthorized", { status: 401 });
  }

  return null;
}

http.route({
  path: "/v1/version",
  method: "GET",
  handler: httpAction(async (ctx, req) => {
    logClientTelemetryHeaders("/v1/version", req);
    const convexClientHeader = req.headers.get("Convex-Client");
    const clientVersion = extractVersionFromHeader(convexClientHeader);

    const [npmVersionData, cursorRulesData, guidelinesData, latestSnapshot] =
      await Promise.all([
        getCachedOrRefresh(ctx, internal.npm),
        getCursorRulesForVersion(ctx, clientVersion),
        getCachedOrRefresh(ctx, internal.guidelines),
        ctx.runQuery(internal.agentSkillManifest.getLatestSnapshot, {}),
      ]);

    const message = npmVersionData
      ? generateMessage(npmVersionData, convexClientHeader)
      : null;

    return new Response(
      JSON.stringify({
        message,
        cursorRulesHash: cursorRulesData?.hash ?? null,
        guidelinesHash: guidelinesData?.hash ?? null,
        agentSkillsSha: latestSnapshot?.repoSha ?? null,
      } satisfies VersionResponse),
      {
        status: 200,
        headers: {
          ...COMMON_HEADERS,
          "Content-Type": "application/json",
        },
      },
    );
  }),
});

http.route({
  path: "/v1/agent_skills",
  method: "GET",
  handler: httpAction(async (ctx) => {
    const [latestSnapshot, skills] = await Promise.all([
      ctx.runQuery(internal.agentSkillManifest.getLatestSnapshot, {}),
      ctx.runQuery(internal.agentSkillManifest.listAll, {}),
    ]);

    return new Response(
      JSON.stringify({
        latestRepoSha: latestSnapshot?.repoSha ?? null,
        skills: skills.flatMap((skill) => {
          const status = getAgentSkillStatus({
            isDeleted: skill.isDeleted,
            deletedAt: skill.deletedAt,
          });
          if (status === null) return [];

          return [
            {
              skillName: skill.skillName,
              status,
              hash: skill.currentHash,
              lastSeenRepoSha: skill.lastSeenRepoSha,
              lastSeenAt: skill.lastSeenAt,
            },
          ];
        }),
      } satisfies AgentSkillCatalogResponse),
      {
        status: 200,
        headers: {
          ...COMMON_HEADERS,
          "Content-Type": "application/json",
        },
      },
    );
  }),
});

/**
 * This gets called by the convex/agent-skills CI pipeline to sync the agent skill catalog
 * with the latest skills from the convex/agent-skills repo.
 */
http.route({
  path: "/v1/agent_skills/publish",
  method: "POST",
  handler: httpAction(async (ctx, req) => {
    const authError = validateAgentSkillSyncAuth(req);
    if (authError) return authError;

    let json: unknown;
    try {
      json = await req.json();
    } catch {
      return new Response("Invalid JSON body", { status: 400 });
    }

    const payloadResult = validateAgentSkillManifestRequest(json);
    if (payloadResult.kind === "error") {
      return new Response(payloadResult.message, { status: 400 });
    }
    const payload: AgentSkillManifestRequest = payloadResult.data;

    try {
      const snapshot = await ctx.runMutation(
        internal.agentSkillManifest.ingest,
        {
          repoSha: payload.repoSha,
          skills: payload.skills,
        },
      );
      return new Response(
        JSON.stringify({
          ok: true,
          repoSha: snapshot.repoSha,
          manifestHash: snapshot.manifestHash,
          skillCount: snapshot.skills.length,
        }),
        {
          status: 200,
          headers: {
            "Content-Type": "application/json",
          },
        },
      );
    } catch (error) {
      console.error("Failed to ingest agent skill manifest:", error);
      return new Response("Failed to ingest agent skill manifest", {
        status: 500,
      });
    }
  }),
});

http.route({
  path: "/v1/cursor_rules",
  method: "GET",
  handler: httpAction(async (ctx, req) => {
    const convexClientHeader = req.headers.get("Convex-Client");
    const clientVersion = extractVersionFromHeader(convexClientHeader);

    const cursorRulesData = await getCursorRulesForVersion(ctx, clientVersion);

    if (!cursorRulesData) {
      return new Response("Can't get the Cursor rules", {
        status: 500,
        headers: COMMON_HEADERS,
      });
    }

    return new Response(cursorRulesData.content, {
      status: 200,
      headers: {
        ...COMMON_HEADERS,
        "Content-Type": "text/plain",
      },
    });
  }),
});

http.route({
  path: "/v1/guidelines",
  method: "GET",
  handler: httpAction(async (ctx, req) => {
    logClientTelemetryHeaders("/v1/guidelines", req);
    const guidelinesData = await getCachedOrRefresh(ctx, internal.guidelines);

    if (!guidelinesData) {
      return new Response("Can't get guidelines", {
        status: 500,
        headers: COMMON_HEADERS,
      });
    }

    return new Response(guidelinesData.content, {
      status: 200,
      headers: {
        ...COMMON_HEADERS,
        "Content-Type": "text/plain",
      },
    });
  }),
});

// Handle CORS preflight requests
http.route({
  path: "/v1/guidelines",
  method: "OPTIONS",
  handler: httpAction(async () => {
    return new Response(null, {
      status: 200,
      headers: COMMON_HEADERS,
    });
  }),
});

http.route({
  path: "/v1/version",
  method: "OPTIONS",
  handler: httpAction(async () => {
    return new Response(null, {
      status: 200,
      headers: COMMON_HEADERS,
    });
  }),
});

http.route({
  path: "/v1/agent_skills",
  method: "OPTIONS",
  handler: httpAction(async () => {
    return new Response(null, {
      status: 200,
      headers: COMMON_HEADERS,
    });
  }),
});

http.route({
  path: "/v1/cursor_rules",
  method: "OPTIONS",
  handler: httpAction(async () => {
    return new Response(null, {
      status: 200,
      headers: COMMON_HEADERS,
    });
  }),
});

http.route({
  path: "/v1/local_backend_version",
  method: "GET",
  handler: httpAction(async (ctx) => {
    const localBackendVersionData = await getCachedOrRefresh(
      ctx,
      internal.localBackend,
    );

    if (!localBackendVersionData) {
      return new Response("Failed to get local backend version", {
        status: 500,
        headers: COMMON_HEADERS,
      });
    }

    return new Response(
      JSON.stringify({
        version: localBackendVersionData.version,
      }),
      {
        status: 200,
        headers: {
          ...COMMON_HEADERS,
          "Content-Type": "application/json",
        },
      },
    );
  }),
});

http.route({
  path: "/v1/local_backend_version",
  method: "OPTIONS",
  handler: httpAction(async () => {
    return new Response(null, {
      status: 200,
      headers: COMMON_HEADERS,
    });
  }),
});

/**
 * Return the cached value if one exists, otherwise fetch and cache a fresh one.
 * Periodic background refreshes are handled by the cron jobs in `crons.ts`.
 */
export async function getCachedOrRefresh<Doc extends { _creationTime: number }>(
  ctx: ActionCtx,
  module: {
    getCached: FunctionReference<
      "query",
      "internal",
      DefaultFunctionArgs,
      Doc | null
    >;
    refresh: FunctionReference<
      "action",
      "internal",
      DefaultFunctionArgs,
      Doc | null
    >;
  },
) {
  const cached = await ctx.runQuery(module.getCached, {});
  if (!cached) {
    return await ctx.runAction(module.refresh, {});
  }

  return cached;
}

async function getCursorRulesForVersion(
  ctx: ActionCtx,
  clientVersion: string | null,
): Promise<{
  hash: string;
  content: string;
} | null> {
  if (shouldUseOldCursorRules(clientVersion)) {
    return {
      content: OLD_CURSOR_RULES,
      hash: await hashSha256(OLD_CURSOR_RULES),
    };
  }

  return await getCachedOrRefresh(ctx, internal.cursorRules);
}

export default http;
