import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";
import { agentSkillEntry } from "../agentSkillManifestShared";

export default defineSchema({
  npmVersion: defineTable({
    value: v.string(),
  }),
  cursorRules: defineTable({
    hash: v.string(),
    version: v.string(),
    content: v.string(),
  }),
  guidelines: defineTable({
    hash: v.string(),
    version: v.string(),
    content: v.string(),
  }),
  localBackendVersion: defineTable({
    version: v.string(),
  }),

  // Stores the audit trail of skill snapshots and is uploaded by the convex/agent-skills CI pipeline
  agentSkillSnapshots: defineTable({
    repoSha: v.string(),
    manifestHash: v.string(),
    skills: v.array(agentSkillEntry),
  }).index("by_repo_sha", ["repoSha"]),

  // Stores a cached copy of each skill from the convex/agent-skills repo and its state
  agentSkillCatalog: defineTable({
    skillName: v.string(),
    directoryName: v.string(),
    currentHash: v.string(),
    lastSeenRepoSha: v.string(),
    lastSeenAt: v.number(),
    isDeleted: v.boolean(),
    deletedAt: v.optional(v.number()),
  })
    .index("by_skill_name", ["skillName"])
    .index("by_is_deleted", ["isDeleted"]),
});
