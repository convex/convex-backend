// Custom-role evaluation for dashboard UI gating.
//
// Mirrors `crates_private/big_brain_lib/src/roles/eval.rs` so that the
// dashboard can locally decide whether the current member's custom roles
// permit a given action against a concrete resource. The parser also
// mirrors `crates_private/big_brain_lib/src/roles/parse.rs` because
// `RoleStatement.resource` is serialized over the wire as the string form
// (e.g. "project:*:deployment:type=prod").
//
// The big-brain server remains the source of truth for authorization;
// keep this in sync with the Rust eval/parse modules when they change.

import type {
  RoleStatement,
  RoleStatementAction,
  RoleStatementActions,
} from "@convex-dev/platform/managementApi";
import type { DeploymentOp } from "system-udfs/convex/_system/server";

export type ResourceKind =
  | "team"
  | "project"
  | "deployment"
  | "member"
  | "token"
  | "customRole"
  | "billing"
  | "oauthApplication"
  | "sso"
  | "directorySync"
  | "integration"
  | "defaultEnvironmentVariable";

export type ProjectSelector =
  | { kind: "any" }
  | { kind: "id"; id: number }
  | { kind: "slug"; slug: string };

// Right-hand side of `creator=` on a deployment or token selector. Mirrors
// `CreatorMatcher` in `crates_private/big_brain_lib/src/roles/types.rs` —
// `creator=self` resolves to the evaluating actor's member id at match
// time, so a single rule can grant "things I created".
export type CreatorMatcher =
  | { kind: "self" }
  | { kind: "member"; memberId: number };

export type DeploymentSelector =
  | { kind: "any" }
  | { kind: "id"; id: number }
  | { kind: "type"; deploymentType: string }
  | { kind: "creator"; matcher: CreatorMatcher };

export type TokenSelector =
  | { kind: "any" }
  | { kind: "creator"; matcher: CreatorMatcher };

export type ResourceSegment =
  | { kind: "team" }
  | { kind: "project"; selectors: ProjectSelector[] }
  | { kind: "deployment"; selectors: DeploymentSelector[] }
  | { kind: "member" }
  | { kind: "token"; selectors: TokenSelector[] }
  | { kind: "customRole" }
  | { kind: "billing" }
  | { kind: "oauthApplication" }
  | { kind: "sso" }
  | { kind: "directorySync" }
  | { kind: "integration" }
  | { kind: "defaultEnvironmentVariable" };

export type ResourceSpecifier = { segments: ResourceSegment[] };

export type ConcreteSegment =
  | { kind: "team" }
  | { kind: "project"; id: number; slug: string }
  | {
      kind: "deployment";
      id: number;
      deploymentType: string;
      creator: number | null;
    }
  | { kind: "member" }
  | {
      kind: "token";
      tokenKind: "team" | "project" | "deployment";
      creator: number | null;
    }
  | { kind: "customRole" }
  | { kind: "billing" }
  | { kind: "oauthApplication" }
  | { kind: "sso" }
  | { kind: "directorySync" }
  | { kind: "integration" }
  | { kind: "defaultEnvironmentVariable" };

export type ConcreteResource = { segments: ConcreteSegment[] };

export type AccessDecision = "allowed" | "denied";

// --- DeploymentOp → RoleStatementAction mapping --------------------------

// TODO(ari): Remove this mapping once we migrate DeploymentOps to use the new format.
export const DEPLOYMENT_OP_TO_ACTION: Record<
  DeploymentOp,
  RoleStatementAction
> = {
  Deploy: "deployment:deploy",
  PauseDeployment: "deployment:pause",
  UnpauseDeployment: "deployment:unpause",
  ViewEnvironmentVariables: "deployment:env:view",
  WriteEnvironmentVariables: "deployment:env:write",
  ViewData: "deployment:data:view",
  WriteData: "deployment:data:write",
  RunInternalQueries: "deployment:functions:runInternalQueries",
  RunInternalMutations: "deployment:functions:runInternalMutations",
  RunInternalActions: "deployment:functions:runInternalActions",
  RunTestQuery: "deployment:functions:runTestQuery",
  ActAsUser: "deployment:functions:actAsUser",
  ViewBackups: "deployment:backups:view",
  CreateBackups: "deployment:backups:create",
  DownloadBackups: "deployment:backups:download",
  DeleteBackups: "deployment:backups:delete",
  ImportBackups: "deployment:backups:import",
  ViewLogs: "deployment:logs:view",
  ViewMetrics: "deployment:metrics:view",
  ViewAuditLog: "deployment:auditLog:view",
  ViewUsage: "deployment:usage:view",
  ViewUsageLimits: "deployment:usageLimits:view",
  WriteUsageLimits: "deployment:usageLimits:write",
  ViewIntegrations: "deployment:integrations:view",
  WriteIntegrations: "deployment:integrations:write",
  UseAiGateway: "deployment:aiGateway:use",
};

const READ_ONLY_ACTIONS: RoleStatementAction[] = [
  "deployment:data:view",
  "deployment:env:view",
  "deployment:functions:runInternalQueries",
  "deployment:functions:runTestQuery",
  "deployment:auditLog:view",
  "deployment:usageLimits:view",
  "deployment:usage:view",
  "deployment:logs:view",
  "deployment:metrics:view",
  "deployment:integrations:view",
  "deployment:backups:view",
  "deployment:backups:download",
];

export function isReadOnlyAction(action: RoleStatementAction): boolean {
  return READ_ONLY_ACTIONS.includes(action);
}

// --- Parsing ------------------------------------------------------------

const VALID_KINDS: readonly ResourceKind[] = [
  "team",
  "project",
  "deployment",
  "member",
  "token",
  "customRole",
  "billing",
  "oauthApplication",
  "sso",
  "directorySync",
  "integration",
  "defaultEnvironmentVariable",
];

function parseProjectSelector(s: string): ProjectSelector {
  if (s === "*") return { kind: "any" };
  if (s.startsWith("id=")) {
    const id = Number(s.slice(3));
    if (!Number.isFinite(id)) {
      throw new Error(`Invalid id value: ${s.slice(3)}`);
    }
    return { kind: "id", id };
  }
  if (s.startsWith("slug=")) return { kind: "slug", slug: s.slice(5) };
  throw new Error(`Invalid selector for project resource: ${s}`);
}

function parseCreatorMatcher(value: string): CreatorMatcher {
  if (value === "self") return { kind: "self" };
  const memberId = Number(value);
  if (!Number.isFinite(memberId)) {
    throw new Error(`Invalid creator value: ${value} (expected self or id)`);
  }
  return { kind: "member", memberId };
}

function parseDeploymentSelector(s: string): DeploymentSelector {
  if (s === "*") return { kind: "any" };
  if (s.startsWith("id=")) {
    const id = Number(s.slice(3));
    if (!Number.isFinite(id)) {
      throw new Error(`Invalid id value: ${s.slice(3)}`);
    }
    return { kind: "id", id };
  }
  if (s.startsWith("type="))
    return { kind: "type", deploymentType: s.slice(5) };
  if (s.startsWith("creator=")) {
    return { kind: "creator", matcher: parseCreatorMatcher(s.slice(8)) };
  }
  throw new Error(`Invalid selector for deployment resource: ${s}`);
}

function parseTokenSelector(s: string): TokenSelector {
  if (s === "*") return { kind: "any" };
  if (s.startsWith("creator=")) {
    return { kind: "creator", matcher: parseCreatorMatcher(s.slice(8)) };
  }
  throw new Error(`Invalid selector for token resource: ${s}`);
}

function parseSegment(
  kind: ResourceKind,
  selectorStr: string,
): ResourceSegment {
  const parts = selectorStr.split(",").map((p) => p.trim());
  switch (kind) {
    case "team":
      if (parts.length !== 1 || parts[0] !== "*") {
        throw new Error(`Team segment only accepts wildcard (*)`);
      }
      return { kind: "team" };
    case "project":
      return { kind: "project", selectors: parts.map(parseProjectSelector) };
    case "deployment":
      return {
        kind: "deployment",
        selectors: parts.map(parseDeploymentSelector),
      };
    case "member":
      if (parts.length !== 1 || parts[0] !== "*") {
        throw new Error(`Member segment only accepts wildcard (*)`);
      }
      return { kind: "member" };
    case "token":
      return { kind: "token", selectors: parts.map(parseTokenSelector) };
    case "customRole":
    case "billing":
    case "oauthApplication":
    case "sso":
    case "directorySync":
    case "integration":
    case "defaultEnvironmentVariable":
      if (parts.length !== 1 || parts[0] !== "*") {
        throw new Error(`${kind} segment only accepts wildcard (*)`);
      }
      return { kind };
    default: {
      const _exhaustive: never = kind;
      throw new Error(`Unknown resource kind: ${_exhaustive}`);
    }
  }
}

export function parseResourceSpecifier(s: string): ResourceSpecifier {
  const parts = s.split(":");
  if (parts.length % 2 !== 0) {
    throw new Error(`Resource specifier must have kind:selector pairs: ${s}`);
  }
  const segments: ResourceSegment[] = [];
  for (let i = 0; i < parts.length; i += 2) {
    const rawKind = parts[i];
    if (!VALID_KINDS.includes(rawKind as ResourceKind)) {
      throw new Error(`Unknown resource kind: ${rawKind}`);
    }
    segments.push(parseSegment(rawKind as ResourceKind, parts[i + 1]));
  }
  return { segments };
}

// --- Action → resource kind --------------------------------------------

// Mirrors `RoleStatementAction::resource_kind()` in
// `crates_private/big_brain_lib/src/roles/types.rs`. Keep in sync.
const ACTION_RESOURCE_KIND: Record<RoleStatementAction, ResourceKind> = {
  // Team
  "team:update": "team",
  "team:delete": "team",
  "team:auditLog:view": "team",
  "team:domain:create": "team",
  "team:domain:delete": "team",
  "team:domain:view": "team",
  "team:usage:view": "team",
  // Billing
  "billing:paymentMethod:update": "billing",
  "billing:contact:update": "billing",
  "billing:address:update": "billing",
  "billing:subscription:changePlan": "billing",
  "billing:spendingLimit:update": "billing",
  "billing:view": "billing",
  "billing:invoices:view": "billing",
  // OAuth Applications
  "oauthApplication:create": "oauthApplication",
  "oauthApplication:update": "oauthApplication",
  "oauthApplication:delete": "oauthApplication",
  "oauthApplication:view": "oauthApplication",
  "oauthApplication:generateClientSecret": "oauthApplication",
  // SSO
  "sso:enable": "sso",
  "sso:disable": "sso",
  "sso:update": "sso",
  "sso:view": "sso",
  // Directory Sync
  "directorySync:enable": "directorySync",
  "directorySync:disable": "directorySync",
  "directorySync:updateGroupMapping": "directorySync",
  "directorySync:deleteGroupMapping": "directorySync",
  "directorySync:view": "directorySync",
  // Team Integrations
  "integration:view": "integration",
  "integration:create": "integration",
  "integration:update": "integration",
  "integration:delete": "integration",
  // Project
  "project:create": "project",
  "project:transfer": "project",
  "project:receive": "project",
  "project:update": "project",
  "project:delete": "project",
  "project:view": "project",
  "project:updateMemberRole": "project",
  // Default (project-scoped) Environment Variables
  "defaultEnvironmentVariable:create": "defaultEnvironmentVariable",
  "defaultEnvironmentVariable:update": "defaultEnvironmentVariable",
  "defaultEnvironmentVariable:delete": "defaultEnvironmentVariable",
  "defaultEnvironmentVariable:view": "defaultEnvironmentVariable",
  // Deployment
  "deployment:create": "deployment",
  "deployment:receive": "deployment",
  "deployment:transfer": "deployment",
  "deployment:updateReference": "deployment",
  "deployment:updateDashboardEditConfirmation": "deployment",
  "deployment:updateExpiresAt": "deployment",
  "deployment:updateSendLogsToClient": "deployment",
  "deployment:updateClass": "deployment",
  "deployment:updateIsDefault": "deployment",
  "deployment:updateType": "deployment",
  "deployment:delete": "deployment",
  "deployment:view": "deployment",
  "deployment:integrations:view": "deployment",
  "deployment:integrations:write": "deployment",
  "deployment:customDomain:create": "deployment",
  "deployment:customDomain:delete": "deployment",
  "deployment:customDomain:view": "deployment",
  "deployment:insights:view": "deployment",
  "deployment:backups:create": "deployment",
  "deployment:backups:import": "deployment",
  "deployment:backups:configurePeriodic": "deployment",
  "deployment:backups:disablePeriodic": "deployment",
  "deployment:backups:delete": "deployment",
  "deployment:backups:view": "deployment",
  "deployment:deploy": "deployment",
  "deployment:env:view": "deployment",
  "deployment:env:write": "deployment",
  "deployment:pause": "deployment",
  "deployment:unpause": "deployment",
  "deployment:logs:view": "deployment",
  "deployment:metrics:view": "deployment",
  "deployment:data:view": "deployment",
  "deployment:data:write": "deployment",
  "deployment:backups:download": "deployment",
  "deployment:functions:actAsUser": "deployment",
  "deployment:functions:runInternalQueries": "deployment",
  "deployment:functions:runInternalMutations": "deployment",
  "deployment:functions:runInternalActions": "deployment",
  "deployment:functions:runTestQuery": "deployment",
  "deployment:auditLog:view": "deployment",
  "deployment:usageLimits:view": "deployment",
  "deployment:usageLimits:write": "deployment",
  "deployment:usage:view": "deployment",
  "deployment:aiGateway:use": "deployment",
  // Member
  "member:invite": "member",
  "member:cancelInvitation": "member",
  "member:remove": "member",
  "member:updateRole": "member",
  "member:view": "member",
  // Custom Role
  "customRole:view": "customRole",
  // Token
  "team:token:create": "token",
  "team:token:update": "token",
  "team:token:delete": "token",
  "team:token:view": "token",
  "project:token:create": "token",
  "project:token:update": "token",
  "project:token:delete": "token",
  "project:token:view": "token",
  "deployment:token:create": "token",
  "deployment:token:update": "token",
  "deployment:token:delete": "token",
  "deployment:token:view": "token",
};

export function actionResourceKind(action: RoleStatementAction): ResourceKind {
  return ACTION_RESOURCE_KIND[action];
}

// --- Selector matching --------------------------------------------------

function projectSelectorMatches(
  selector: ProjectSelector,
  segment: { id: number; slug: string },
): boolean {
  switch (selector.kind) {
    case "any":
      return true;
    case "id":
      return segment.id === selector.id;
    case "slug":
      return segment.slug === selector.slug;
    default: {
      const _exhaustive: never = selector;
      return _exhaustive;
    }
  }
}

// `creator=self` only matches when the caller passed an actor id, so a
// rule that only grants "things I created" denies if we can't identify
// the actor. Returning `undefined` here makes the caller skip the
// equality check entirely.
function resolveCreatorMatcher(
  matcher: CreatorMatcher,
  actor: number | undefined,
): number | undefined {
  return matcher.kind === "self" ? actor : matcher.memberId;
}

function deploymentSelectorMatches(
  selector: DeploymentSelector,
  segment: { id: number; deploymentType: string; creator: number | null },
  actor: number | undefined,
): boolean {
  switch (selector.kind) {
    case "any":
      return true;
    case "id":
      return segment.id === selector.id;
    case "type":
      return segment.deploymentType === selector.deploymentType;
    case "creator": {
      const resolved = resolveCreatorMatcher(selector.matcher, actor);
      if (resolved === undefined) return false;
      return segment.creator === resolved;
    }
    default: {
      const _exhaustive: never = selector;
      return _exhaustive;
    }
  }
}

function tokenSelectorMatches(
  selector: TokenSelector,
  segment: { creator: number | null },
  actor: number | undefined,
): boolean {
  switch (selector.kind) {
    case "any":
      return true;
    case "creator": {
      const resolved = resolveCreatorMatcher(selector.matcher, actor);
      if (resolved === undefined) return false;
      return segment.creator === resolved;
    }
    default: {
      const _exhaustive: never = selector;
      return _exhaustive;
    }
  }
}

function segmentMatches(
  spec: ResourceSegment,
  concrete: ConcreteSegment,
  actor: number | undefined,
): boolean {
  if (spec.kind !== concrete.kind) return false;
  switch (spec.kind) {
    case "project":
      if (concrete.kind !== "project") return false;
      return spec.selectors.some((s) => projectSelectorMatches(s, concrete));
    case "deployment":
      if (concrete.kind !== "deployment") return false;
      return spec.selectors.some((s) =>
        deploymentSelectorMatches(s, concrete, actor),
      );
    case "token":
      if (concrete.kind !== "token") return false;
      return spec.selectors.some((s) =>
        tokenSelectorMatches(s, concrete, actor),
      );
    default:
      return true;
  }
}

function specifierMatches(
  spec: ResourceSpecifier,
  concrete: ConcreteResource,
  actor: number | undefined,
): boolean {
  if (spec.segments.length !== concrete.segments.length) return false;
  return spec.segments.every((seg, i) =>
    segmentMatches(seg, concrete.segments[i], actor),
  );
}

// --- Evaluation ---------------------------------------------------------

function actionMatches(
  pattern: RoleStatementActions,
  action: RoleStatementAction,
): boolean {
  if (pattern === "*") return true;
  return pattern.includes(action);
}

/**
 * Evaluate whether the given collection of role statements grants `action`
 * on `resource`. Mirrors `evaluate_statements` in `eval.rs`:
 *
 *   1. The resource's leaf kind must match the action's resource kind, else
 *      Denied (the action would never apply to this resource shape).
 *   2. A matching `Deny` short-circuits to Denied.
 *   3. Otherwise, a matching `Allow` produces Allowed.
 *   4. Default Denied.
 *
 * `actor` is the evaluating member's id; it resolves `creator=self`
 * selectors against the resource. Pass `undefined` only when the caller
 * has no profile available — those `self` selectors will then never
 * match, which fails closed.
 */
export function evaluateStatements(
  statements: readonly RoleStatement[],
  action: RoleStatementAction,
  resource: ConcreteResource,
  actor?: number,
): AccessDecision {
  const leaf = resource.segments[resource.segments.length - 1];
  if (!leaf || leaf.kind !== actionResourceKind(action)) {
    return "denied";
  }

  let anyAllow = false;
  for (const statement of statements) {
    if (!actionMatches(statement.actions, action)) continue;
    let spec: ResourceSpecifier;
    try {
      spec = parseResourceSpecifier(statement.resource);
    } catch {
      // Malformed statements never match; the server-side validation should
      // prevent these from being persisted in the first place.
      continue;
    }
    if (!specifierMatches(spec, resource, actor)) continue;
    if (statement.effect === "deny") return "denied";
    anyAllow = true;
  }
  return anyAllow ? "allowed" : "denied";
}

/**
 * Roles are additive across the list: `Allow` from any role permits the
 * action even if a different role would have denied it. Within a single
 * role, `Deny` still overrides `Allow` (per `evaluateStatements`). Mirrors
 * `allowed_deployment_ops` in `eval.rs`.
 */
export function evaluateRoles(
  roles: readonly { statements: readonly RoleStatement[] }[],
  action: RoleStatementAction,
  resource: ConcreteResource,
  actor?: number,
): AccessDecision {
  for (const role of roles) {
    if (
      evaluateStatements(role.statements, action, resource, actor) === "allowed"
    ) {
      return "allowed";
    }
  }
  return "denied";
}

// --- Result type and shared resource constants -------------------------

// Discriminated union returned by gated read hooks. Forcing callers to
// pattern-match on `status` to reach `data` makes "I forgot to handle the
// no-permission case" a compile error rather than a runtime surprise.
export type Permissioned<T> =
  | { status: "loading" }
  | { status: "denied"; deniedAction: RoleStatementAction }
  | { status: "ok"; data: T };

export const BILLING_RESOURCE: ConcreteResource = {
  segments: [{ kind: "billing" }],
};

export const MEMBER_RESOURCE: ConcreteResource = {
  segments: [{ kind: "member" }],
};

export const CUSTOM_ROLE_RESOURCE: ConcreteResource = {
  segments: [{ kind: "customRole" }],
};

export const TEAM_RESOURCE: ConcreteResource = {
  segments: [{ kind: "team" }],
};

export const SSO_RESOURCE: ConcreteResource = {
  segments: [{ kind: "sso" }],
};

export const DIRECTORY_SYNC_RESOURCE: ConcreteResource = {
  segments: [{ kind: "directorySync" }],
};

export const OAUTH_APPLICATION_RESOURCE: ConcreteResource = {
  segments: [{ kind: "oauthApplication" }],
};

// Team-scoped tokens carry a creator selector so a role like
// "team tokens you created" still matches. Callers gating "can I create
// my own team token" should pass the current member's id; for view checks
// over the whole team-token list, leave `creator` null so a role limited
// to "creator=me" denies.
export function teamTokenResource(creator: number | null): ConcreteResource {
  return {
    segments: [{ kind: "team" }, { kind: "token", tokenKind: "team", creator }],
  };
}

// Project-scoped resource: a single project segment carrying id and slug
// so roles can match on either selector form.
export function projectResource(project: {
  id: number;
  slug: string;
}): ConcreteResource {
  return {
    segments: [{ kind: "project", id: project.id, slug: project.slug }],
  };
}

// Project-scoped default environment variable resource. Lives at
// `project:<sel>:defaultEnvironmentVariable:*` on the wire, so the
// concrete resource must carry the project segment too.
export function defaultEnvironmentVariableResource(project: {
  id: number;
  slug: string;
}): ConcreteResource {
  return {
    segments: [
      { kind: "project", id: project.id, slug: project.slug },
      { kind: "defaultEnvironmentVariable" },
    ],
  };
}

// Project-scoped tokens live at `project:<sel>:token:<sel>`, so the
// concrete resource carries the project segment plus a token segment with
// a creator selector. Pass the current member's id when gating "can I
// create my own preview deploy key"; pass null for whole-list view checks
// so a role limited to `creator=me` denies.
export function projectTokenResource(
  project: { id: number; slug: string },
  tokenCreator: number | null,
): ConcreteResource {
  return {
    segments: [
      ...projectResource(project).segments,
      { kind: "token", tokenKind: "project", creator: tokenCreator },
    ],
  };
}

// Deployment-scoped resource: project + deployment segments. The
// deployment segment carries the creator id so a role like
// "deployments you created" still matches.
export function deploymentResource(
  project: { id: number; slug: string },
  deployment: {
    id: number;
    deploymentType: string;
    creator: number | null;
  },
): ConcreteResource {
  return {
    segments: [
      { kind: "project", id: project.id, slug: project.slug },
      {
        kind: "deployment",
        id: deployment.id,
        deploymentType: deployment.deploymentType,
        creator: deployment.creator,
      },
    ],
  };
}

// Deployment-scoped tokens live at `project:*:deployment:*:token:*`,
// so the concrete resource must carry the full project + deployment
// path — a leaf-only `[{ kind: "token", ... }]` would never match a
// real role statement (segment lengths differ). The token segment
// carries a creator selector so a role like "deployment tokens you
// created" still matches: pass the current member's id when gating
// "can I create my own deploy key"; pass null for whole-list view
// checks so a role limited to `creator=me` denies.
export function deploymentTokenResource(
  project: { id: number; slug: string },
  deployment: {
    id: number;
    deploymentType: string;
    creator: number | null;
  },
  tokenCreator: number | null,
): ConcreteResource {
  return {
    segments: [
      ...deploymentResource(project, deployment).segments,
      { kind: "token", tokenKind: "deployment", creator: tokenCreator },
    ],
  };
}
