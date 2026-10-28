import { v } from "convex/values";

export const agentSkillEntry = v.object({
  skillName: v.string(),
  directoryName: v.string(),
  skillHash: v.string(),
});

export type AgentSkill = {
  skillName: string;
  directoryName: string;
  skillHash: string;
};

export type AgentSkillManifestRequest = {
  repoSha: string;
  skills: AgentSkill[];
};

export type AgentSkillStatus =
  | { kind: "active" }
  | { kind: "deleted"; deletedAt: number };

export type AgentSkillCatalogEntry = {
  skillName: string;
  status: AgentSkillStatus;
  hash: string;
  lastSeenRepoSha: string;
  lastSeenAt: number;
};

export type AgentSkillCatalogResponse = {
  latestRepoSha: string | null;
  skills: AgentSkillCatalogEntry[];
};

const FULL_COMMIT_SHA_REGEX = /^[0-9a-f]{40}$/i;

export function compareSkills(a: AgentSkill, b: AgentSkill) {
  return a.skillName.localeCompare(b.skillName);
}

export function findDuplicateSkillName({ skills }: { skills: AgentSkill[] }) {
  const seenSkillNames = new Set<string>();
  for (const { skillName } of skills) {
    if (seenSkillNames.has(skillName)) return skillName;
    seenSkillNames.add(skillName);
  }
  return null;
}

export function formatDuplicateSkillNameError({
  skillName,
}: {
  skillName: string;
}) {
  return `Duplicate skill name '${skillName}' in manifest`;
}

export function toCanonicalSkills({ skills }: { skills: AgentSkill[] }) {
  return skills.map(({ skillName, directoryName, skillHash }) => [
    skillName,
    directoryName,
    skillHash,
  ]);
}

export function validateAgentSkillManifestRequest(
  json: unknown,
):
  | { kind: "ok"; data: AgentSkillManifestRequest }
  | { kind: "error"; message: string } {
  if (typeof json !== "object" || json === null)
    return { kind: "error", message: "Invalid agent skill manifest payload" };

  const { repoSha, skills } = json as Record<string, unknown>;
  if (typeof repoSha !== "string" || !FULL_COMMIT_SHA_REGEX.test(repoSha))
    return { kind: "error", message: "Invalid agent skill manifest payload" };
  if (!Array.isArray(skills))
    return { kind: "error", message: "Invalid agent skill manifest payload" };

  const parsedSkills: AgentSkill[] = [];
  for (const skill of skills) {
    if (typeof skill !== "object" || skill === null) {
      return {
        kind: "error",
        message: "Invalid agent skill manifest payload",
      };
    }

    const { skillName, directoryName, skillHash } = skill as Record<
      string,
      unknown
    >;
    if (
      typeof skillName !== "string" ||
      typeof directoryName !== "string" ||
      typeof skillHash !== "string"
    ) {
      return {
        kind: "error",
        message: "Invalid agent skill manifest payload",
      };
    }

    parsedSkills.push({ skillName, directoryName, skillHash });
  }

  const duplicateSkillName = findDuplicateSkillName({ skills: parsedSkills });
  if (duplicateSkillName) {
    return {
      kind: "error",
      message: formatDuplicateSkillNameError({ skillName: duplicateSkillName }),
    };
  }

  return { kind: "ok", data: { repoSha, skills: parsedSkills } };
}
