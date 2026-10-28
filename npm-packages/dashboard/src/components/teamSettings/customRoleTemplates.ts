import {
  RoleStatement,
  RoleStatementAction,
} from "@convex-dev/platform/managementApi";
import { ACTIONS_BY_CATEGORY } from "./customRoleActions";

export type CustomRoleTemplate = {
  id: string;
  label: string;
  description: string;
  defaultName: string;
  defaultRoleDescription: string;
  statements: RoleStatement[];
};

// Lifecycle and class-changing actions stay admin-only — notably
// `deployment:updateType`, which would let an editor reclassify a
// non-production deployment as production.
const VIEW_DEPLOYMENT_ACTIONS: RoleStatementAction[] = [
  "deployment:view",
  "deployment:insights:view",
  "deployment:integrations:view",
  "deployment:env:view",
  "deployment:logs:view",
  "deployment:metrics:view",
  "deployment:auditLog:view",
  "deployment:usageLimits:view",
  "deployment:usage:view",
  "deployment:data:view",
  "deployment:customDomain:view",
  "deployment:backups:view",
];

const EDIT_DEPLOYMENT_ACTIONS: RoleStatementAction[] = [
  "deployment:create",
  "deployment:view",
  "deployment:updateReference",
  "deployment:updateDashboardEditConfirmation",
  "deployment:updateExpiresAt",
  "deployment:updateSendLogsToClient",
  "deployment:insights:view",
  "deployment:integrations:view",
  "deployment:integrations:write",
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
  "deployment:deploy",
  "deployment:pause",
  "deployment:unpause",
  "deployment:functions:actAsUser",
  "deployment:functions:runInternalQueries",
  "deployment:functions:runInternalMutations",
  "deployment:functions:runInternalActions",
  "deployment:functions:runTestQuery",
  "deployment:customDomain:view",
  "deployment:customDomain:create",
  "deployment:customDomain:delete",
  "deployment:backups:view",
  "deployment:backups:create",
  "deployment:backups:import",
  "deployment:backups:configurePeriodic",
  "deployment:backups:disablePeriodic",
  "deployment:backups:delete",
  "deployment:backups:download",
];

export const CUSTOM_ROLE_TEMPLATES: CustomRoleTemplate[] = [
  {
    id: "billing",
    label: "Billing",
    description: "Access to billing and usage.",
    defaultName: "Billing",
    defaultRoleDescription:
      "Manage payment, subscriptions, and view billing and usage.",
    statements: [
      {
        effect: "allow",
        resource: "billing:*",
        actions: ACTIONS_BY_CATEGORY.billing,
      },
      {
        effect: "allow",
        resource: "team:*",
        actions: ["team:usage:view"],
      },
      {
        effect: "allow",
        resource: "member:*",
        actions: ["member:view"],
      },
    ],
  },
  {
    id: "team-config",
    label: "Team Configuration",
    description:
      "Manage team settings, billing, OAuth, SSO, integrations, and members. No project access.",
    defaultName: "Team Configuration",
    defaultRoleDescription:
      "Configure team settings, billing, OAuth applications, SSO, integrations, and members. No project or deployment access.",
    statements: [
      {
        effect: "allow",
        resource: "team:*",
        actions: ["team:update", "team:usage:view", "team:auditLog:view"],
      },
      {
        effect: "allow",
        resource: "billing:*",
        actions: ACTIONS_BY_CATEGORY.billing,
      },
      {
        effect: "allow",
        resource: "oauthApplication:*",
        actions: ACTIONS_BY_CATEGORY.oauthApplication,
      },
      {
        effect: "allow",
        resource: "sso:*",
        actions: ACTIONS_BY_CATEGORY.sso,
      },
      {
        effect: "allow",
        resource: "integration:*",
        actions: ACTIONS_BY_CATEGORY.integration,
      },
      {
        effect: "allow",
        resource: "member:*",
        actions: ACTIONS_BY_CATEGORY.member,
      },
    ],
  },
  {
    id: "project-admin",
    label: "Admin for all projects",
    description:
      "Full admin on every project and deployment. No team-level settings.",
    defaultName: "Project Admin",
    defaultRoleDescription:
      "Full administrative access to all projects and deployments. No team-level settings.",
    statements: [
      {
        effect: "allow",
        resource: "member:*",
        actions: ["member:view"],
      },
      {
        effect: "allow",
        resource: "project:*",
        actions: ACTIONS_BY_CATEGORY.project,
      },
      {
        effect: "allow",
        resource: "project:*:deployment:*",
        actions: ACTIONS_BY_CATEGORY.deployment,
      },
      {
        effect: "allow",
        resource: "project:*:defaultEnvironmentVariable:*",
        actions: ACTIONS_BY_CATEGORY.defaultEnvironmentVariable,
      },
      {
        effect: "allow",
        resource: "project:*:token:*",
        actions: ACTIONS_BY_CATEGORY.projectToken,
      },
      {
        effect: "allow",
        resource: "project:*:deployment:*:token:*",
        actions: ACTIONS_BY_CATEGORY.deploymentToken,
      },
    ],
  },
  {
    id: "view-non-prod",
    label: "View all non-production",
    description:
      "View projects and non-production deployments. No production access.",
    defaultName: "View Non-Production",
    defaultRoleDescription:
      "View all projects and non-production deployments. No access to production deployments.",
    statements: [
      {
        effect: "allow",
        resource: "member:*",
        actions: ["member:view"],
      },
      {
        effect: "allow",
        resource: "project:*",
        actions: ["project:view"],
      },
      {
        effect: "allow",
        resource: "project:*:deployment:*",
        actions: VIEW_DEPLOYMENT_ACTIONS,
      },
      {
        effect: "deny",
        resource: "project:*:deployment:type=prod",
        actions: "*",
      },
    ],
  },
  {
    id: "edit-non-prod",
    label: "Edit all non-production",
    description:
      "View and edit projects, deploy to non-production. No production access.",
    defaultName: "Edit Non-Production",
    defaultRoleDescription:
      "View and edit all projects, deploy to non-production deployments. No access to production deployments.",
    statements: [
      {
        effect: "allow",
        resource: "member:*",
        actions: ["member:view"],
      },
      {
        effect: "allow",
        resource: "project:*",
        actions: ["project:view", "project:update", "project:create"],
      },
      {
        effect: "allow",
        resource: "project:*:deployment:*",
        actions: EDIT_DEPLOYMENT_ACTIONS,
      },
      {
        effect: "allow",
        resource: "project:*:defaultEnvironmentVariable:*",
        actions: ACTIONS_BY_CATEGORY.defaultEnvironmentVariable,
      },
      {
        effect: "deny",
        resource: "project:*:deployment:type=prod",
        actions: "*",
      },
    ],
  },
];

export const CUSTOM_ROLE_TEMPLATES_BY_ID: Record<string, CustomRoleTemplate> =
  Object.fromEntries(CUSTOM_ROLE_TEMPLATES.map((t) => [t.id, t]));
