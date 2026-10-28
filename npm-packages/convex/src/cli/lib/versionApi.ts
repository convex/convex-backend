import * as Sentry from "@sentry/node";
import { z } from "zod";
import { version } from "../version.js";

const DEFAULT_VERSION_API_ORIGIN = "https://version.convex.dev";
const VERSION_API_ORIGIN_ENV_VAR = "CONVEX_VERSION_API_ORIGIN";

function versionApiOrigin() {
  return process.env[VERSION_API_ORIGIN_ENV_VAR] ?? DEFAULT_VERSION_API_ORIGIN;
}

function versionApiEndpoint(path: string) {
  return `${versionApiOrigin()}${path}`;
}

const HEADERS: Record<string, string> = {
  "Convex-Client": `npm-cli-${version}`,
  // Useful telemetry proxy for "human at a terminal" vs automated/background execution.
  "Convex-Interactive": process.stdin.isTTY === true ? "true" : "false",
};
if (process.env.CONVEX_AGENT_MODE) {
  HEADERS["Convex-Agent-Mode"] = process.env.CONVEX_AGENT_MODE;
}

const optionalStringToNullSchema = z
  .unknown()
  .optional()
  .transform((value) => (typeof value === "string" ? value : null));

const optionalTrueToBooleanSchema = z
  .unknown()
  .optional()
  .transform((value) => value === true);

const versionResultSchema = z.object({
  message: z.string().nullable(),
  guidelinesHash: optionalStringToNullSchema,
  agentSkillsSha: optionalStringToNullSchema,
  disableSkillsCli: optionalTrueToBooleanSchema,
  disableSkillsCliMessage: optionalStringToNullSchema,
});

const agentSkillStatusSchema = z.discriminatedUnion("kind", [
  z.object({
    kind: z.literal("active"),
  }),
  z.object({
    kind: z.literal("deleted"),
    deletedAt: z.number(),
  }),
]);

const agentSkillCatalogEntrySchema = z.object({
  skillName: z.string(),
  status: agentSkillStatusSchema,
  hash: z.string(),
  lastSeenRepoSha: z.string(),
  lastSeenAt: z.number(),
});

const agentSkillCatalogResultSchema = z.object({
  latestRepoSha: z.string().nullable(),
  skills: z.array(agentSkillCatalogEntrySchema),
});

export type VersionResult = z.infer<typeof versionResultSchema>;

export type VersionFetchResult =
  | { kind: "ok"; data: VersionResult }
  | { kind: "error" };

export type AgentSkillStatus = z.infer<typeof agentSkillStatusSchema>;

export type AgentSkillCatalogEntry = z.infer<
  typeof agentSkillCatalogEntrySchema
>;

export type AgentSkillCatalogResult = z.infer<
  typeof agentSkillCatalogResultSchema
>;

export type AgentSkillCatalogFetchResult =
  | { kind: "ok"; data: AgentSkillCatalogResult }
  | { kind: "error" };

export async function getVersion(): Promise<VersionFetchResult> {
  try {
    const req = await fetch(versionApiEndpoint("/v1/version"), {
      headers: HEADERS,
    });

    if (!req.ok) {
      Sentry.captureException(
        new Error(`Failed to fetch version: status = ${req.status}`),
      );
      return { kind: "error" };
    }

    const json = await req.json();
    const result = validateVersionResult(json);

    if (result === null) return { kind: "error" };
    return { kind: "ok", data: result };
  } catch (error) {
    Sentry.captureException(error);
    return { kind: "error" };
  }
}

export function validateVersionResult(json: unknown): VersionResult | null {
  const result = versionResultSchema.safeParse(json);
  if (!result.success) {
    Sentry.captureMessage("Invalid version result", "error");
    return null;
  }
  return result.data;
}

export function validateAgentSkillCatalogResult(
  json: unknown,
): AgentSkillCatalogResult | null {
  const result = agentSkillCatalogResultSchema.safeParse(json);
  if (!result.success) {
    Sentry.captureMessage("Invalid agent skill catalog result", "error");
    return null;
  }
  return result.data;
}

/** Fetch the latest agent skills SHA from version.convex.dev. */
export async function fetchAgentSkillsSha(): Promise<string | null> {
  const versionData = await getVersion();
  if (versionData.kind === "error") return null;
  return versionData.data.agentSkillsSha;
}

export async function fetchAgentSkillsCatalog(): Promise<AgentSkillCatalogFetchResult> {
  try {
    const req = await fetch(versionApiEndpoint("/v1/agent_skills"), {
      headers: HEADERS,
    });

    if (!req.ok) {
      Sentry.captureException(
        new Error(
          `Failed to fetch agent skills catalog: status = ${req.status}`,
        ),
      );
      return { kind: "error" };
    }

    const json = await req.json();
    const result = validateAgentSkillCatalogResult(json);

    if (result === null) return { kind: "error" };
    return { kind: "ok", data: result };
  } catch (error) {
    Sentry.captureException(error);
    return { kind: "error" };
  }
}

export async function downloadGuidelines(): Promise<string | null> {
  try {
    const req = await fetch(versionApiEndpoint("/v1/guidelines"), {
      headers: HEADERS,
    });

    if (!req.ok) {
      Sentry.captureMessage(
        `Failed to fetch Convex guidelines: status = ${req.status}`,
      );
      return null;
    }

    const text = await req.text();
    return text;
  } catch (error) {
    Sentry.captureException(error);
    return null;
  }
}
