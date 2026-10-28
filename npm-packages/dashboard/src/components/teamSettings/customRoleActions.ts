import { RoleStatementAction } from "@convex-dev/platform/managementApi";

// Server-side validation pairs each action with the resource kind it can target
// (e.g. `team:*:token:*` only accepts team-token actions). Listing the same
// pairing here lets the JSON editor flag mismatches inline before submit.
export type ActionCategory =
  | "team"
  | "project"
  | "deployment"
  | "member"
  | "teamToken"
  | "projectToken"
  | "deploymentToken"
  | "customRole"
  | "billing"
  | "oauthApplication"
  | "sso"
  | "directorySync"
  | "integration"
  | "defaultEnvironmentVariable";

export const ACTIONS_BY_CATEGORY: Record<
  ActionCategory,
  RoleStatementAction[]
> = {
  // `team:applyReferralCode` intentionally omitted: redemption is a one-time
  // team-lifecycle event that we want to keep gated to built-in team admins,
  // not delegable via custom roles.
  team: [
    "team:update",
    "team:delete",
    "team:auditLog:view",
    "team:domain:create",
    "team:domain:delete",
    "team:domain:view",
    "team:usage:view",
  ],
  billing: [
    "billing:paymentMethod:update",
    "billing:contact:update",
    "billing:address:update",
    "billing:subscription:changePlan",
    "billing:spendingLimit:update",
    "billing:view",
    "billing:invoices:view",
  ],
  oauthApplication: [
    "oauthApplication:create",
    "oauthApplication:update",
    "oauthApplication:delete",
    "oauthApplication:view",
    "oauthApplication:generateClientSecret",
  ],
  sso: ["sso:enable", "sso:disable", "sso:update", "sso:view"],
  directorySync: [
    "directorySync:enable",
    "directorySync:disable",
    "directorySync:updateGroupMapping",
    "directorySync:deleteGroupMapping",
    "directorySync:view",
  ],
  integration: [
    "integration:view",
    "integration:create",
    "integration:update",
    "integration:delete",
    "team:auditLog:view",
  ],
  project: [
    "project:create",
    "project:transfer",
    "project:receive",
    "project:update",
    "project:delete",
    "project:view",
    "project:updateMemberRole",
  ],
  defaultEnvironmentVariable: [
    "defaultEnvironmentVariable:create",
    "defaultEnvironmentVariable:update",
    "defaultEnvironmentVariable:delete",
    "defaultEnvironmentVariable:view",
  ],
  deployment: [
    "deployment:create",
    "deployment:receive",
    "deployment:transfer",
    "deployment:delete",
    "deployment:view",
    "deployment:updateReference",
    "deployment:updateDashboardEditConfirmation",
    "deployment:updateExpiresAt",
    "deployment:updateSendLogsToClient",
    "deployment:updateClass",
    "deployment:updateIsDefault",
    "deployment:updateType",
    "deployment:integrations:view",
    "deployment:integrations:write",
    "deployment:customDomain:create",
    "deployment:customDomain:delete",
    "deployment:customDomain:view",
    "deployment:insights:view",
    "deployment:backups:create",
    "deployment:backups:import",
    "deployment:backups:configurePeriodic",
    "deployment:backups:disablePeriodic",
    "deployment:backups:delete",
    "deployment:backups:view",
    "deployment:backups:download",
    "deployment:deploy",
    "deployment:pause",
    "deployment:unpause",
    "deployment:env:view",
    "deployment:env:write",
    "deployment:logs:view",
    "deployment:metrics:view",
    "deployment:auditLog:view",
    "deployment:usageLimits:view",
    "deployment:usageLimits:write",
    "deployment:usage:view",
    "deployment:data:view",
    "deployment:data:write",
    "deployment:functions:actAsUser",
    "deployment:functions:runInternalQueries",
    "deployment:functions:runInternalMutations",
    "deployment:functions:runInternalActions",
    "deployment:functions:runTestQuery",
    "deployment:aiGateway:use",
  ],
  member: [
    "member:invite",
    "member:cancelInvitation",
    "member:remove",
    "member:updateRole",
    "member:view",
  ],
  teamToken: [
    "team:token:create",
    "team:token:update",
    "team:token:delete",
    "team:token:view",
  ],
  projectToken: [
    "project:token:create",
    "project:token:update",
    "project:token:delete",
    "project:token:view",
  ],
  deploymentToken: [
    "deployment:token:create",
    "deployment:token:update",
    "deployment:token:delete",
    "deployment:token:view",
  ],
  customRole: ["customRole:view"],
};
