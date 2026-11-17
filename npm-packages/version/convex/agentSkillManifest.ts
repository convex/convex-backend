import { internalMutation, internalQuery } from "./_generated/server";
import { v } from "convex/values";
import { hashSha256 } from "./util/hash";
import {
  agentSkillEntry,
  AgentSkill,
  compareSkills,
  findDuplicateSkillName,
  formatDuplicateSkillNameError,
  toCanonicalSkills,
} from "../agentSkillManifestShared";

function normalizeSkills({ skills }: { skills: AgentSkill[] }) {
  const duplicateSkillName = findDuplicateSkillName({ skills });
  if (duplicateSkillName) {
    throw new Error(
      formatDuplicateSkillNameError({ skillName: duplicateSkillName }),
    );
  }

  return [...skills].sort(compareSkills);
}

async function createManifestHash({ skills }: { skills: AgentSkill[] }) {
  return await hashSha256(JSON.stringify(toCanonicalSkills({ skills })));
}

export const ingest = internalMutation({
  args: {
    repoSha: v.string(),
    skills: v.array(agentSkillEntry),
  },
  handler: async (ctx, { repoSha, skills }) => {
    const normalizedSkills = normalizeSkills({ skills });
    const manifestHash = await createManifestHash({ skills: normalizedSkills });
    const existingSnapshot = await ctx.db
      .query("agentSkillSnapshots")
      .withIndex("by_repo_sha", (q) => q.eq("repoSha", repoSha))
      .first();
    if (existingSnapshot) return existingSnapshot;

    // Known edge: an older different repoSha can still be published after a newer
    // one. We only make identical repoSha reruns idempotent here; fully ordering
    // different commits would require ancestry or monotonic metadata from CI.
    const now = Date.now();

    // Record this snapshot to the audit trail for future reference
    const snapshotId = await ctx.db.insert("agentSkillSnapshots", {
      repoSha,
      manifestHash,
      skills: normalizedSkills,
    });

    // Probably safe to to .collect() here because we know the number of skills is small and
    // will continue to remain small
    const existingCatalog = await ctx.db.query("agentSkillCatalog").collect();
    const existingBySkillName = new Map(
      existingCatalog.map((doc) => [doc.skillName, doc]),
    );
    const liveSkillNames = new Set(
      normalizedSkills.map((skill) => skill.skillName),
    );

    // Lets add or update this skill in the catalog
    for (const skill of normalizedSkills) {
      const existing = existingBySkillName.get(skill.skillName);
      const nextValue = {
        skillName: skill.skillName,
        directoryName: skill.directoryName,
        currentHash: skill.skillHash,
        lastSeenRepoSha: repoSha,
        lastSeenAt: now,
        isDeleted: false,
        deletedAt: undefined,
      };

      if (!existing) {
        await ctx.db.insert("agentSkillCatalog", nextValue);
        continue;
      }

      await ctx.db.replace("agentSkillCatalog", existing._id, nextValue);
    }

    // Lets tombstone any skills that are no longer in the convex/agent-skills repo
    for (const existing of existingCatalog) {
      if (liveSkillNames.has(existing.skillName) || existing.isDeleted)
        continue;

      await ctx.db.replace("agentSkillCatalog", existing._id, {
        skillName: existing.skillName,
        directoryName: existing.directoryName,
        currentHash: existing.currentHash,
        lastSeenRepoSha: existing.lastSeenRepoSha,
        lastSeenAt: existing.lastSeenAt,
        isDeleted: true,
        deletedAt: now,
      });
    }

    // Finally let the caller know what the snapshot is
    return (await ctx.db.get("agentSkillSnapshots", snapshotId))!;
  },
});

export const listAll = internalQuery({
  args: {},
  handler: async (ctx) => {
    const skills = await ctx.db.query("agentSkillCatalog").collect();
    return skills.sort((a, b) => a.skillName.localeCompare(b.skillName));
  },
});

export const getLatestSnapshot = internalQuery({
  args: {},
  handler: async (ctx) => {
    return await ctx.db.query("agentSkillSnapshots").order("desc").first();
  },
});
