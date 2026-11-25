import { defineTable } from "convex/server";
import { v } from "convex/values";

// Fields with `v.optional` are missing on documents written before they were
// added.
export const awsLambdaVersionsTable = defineTable({
  lambdaName: v.string(),
  lambdaVersion: v.string(),

  // Deprecated: superseded by `typeConfig`. Rust still writes it, a leftover
  // from rolling out `typeConfig` in 2023: the source package ID of a static
  // lambda, and null for a dynamic one. It only reads it for versions written
  // before `typeConfig`.
  sourcePackageId: v.union(v.string(), v.null()),

  nodeExecutorSha256: v.bytes(),
  lambdaConfig: v.object({
    env: v.record(v.string(), v.string()),
    runtime: v.string(),
    handler: v.string(),
    memorySizeMb: v.int64(),
    diskSizeMb: v.optional(v.int64()),
    timeoutSec: v.int64(),
    subnetIds: v.optional(v.array(v.string())),
    securityGroupIds: v.optional(v.array(v.string())),
    ipv6Enabled: v.optional(v.boolean()),
  }),
  typeConfig: v.optional(
    v.union(
      v.object({
        type: v.literal("static"),
        sourcePackageId: v.union(v.string(), v.null()),
      }),
      v.object({
        type: v.literal("dynamic"),
        externalDepsPackageId: v.union(v.string(), v.null()),
      }),
    ),
  ),
  codeStorage: v.optional(
    v.object({
      s3Bucket: v.string(),
      s3Key: v.string(),
    }),
  ),
});
