use common::types::{
    DeploymentType,
    MemberId,
};
use errors::ErrorMetadata;
use keybroker::{
    bad_admin_key_error,
    DeploymentOp,
    Identity,
};
use serde::{
    Deserialize,
    Serialize,
};

use super::types::{
    ComponentSelector,
    ConcreteComponent,
    ConcreteDeployment,
    ConcreteProject,
    ConcreteResource,
    ConcreteSegment,
    ConcreteToken,
    CustomRole,
    DeploymentSelector,
    ProjectSelector,
    ResourceKind,
    ResourceSegment,
    ResourceSpecifier,
    RolePolicyAction,
    RoleStatement,
    RoleStatementAction,
    RoleStatementEffect,
    TokenSelector,
};

/// The result of evaluating a custom role against an action and resource.
#[derive(Debug, PartialEq, Eq)]
pub enum AccessDecision {
    Allowed,
    Denied,
}

impl ProjectSelector {
    fn matches(&self, project: &ConcreteProject) -> bool {
        match self {
            ProjectSelector::Any => true,
            ProjectSelector::Id(id) => project.id == *id,
            ProjectSelector::Slug(slug) => project.slug == *slug,
        }
    }
}

impl DeploymentSelector {
    fn matches(&self, deployment: &ConcreteDeployment, actor: MemberId) -> bool {
        match self {
            DeploymentSelector::Any => true,
            DeploymentSelector::Id(id) => deployment.id == *id,
            DeploymentSelector::Type(t) => deployment.deployment_type == *t,
            DeploymentSelector::Creator(c) => deployment.creator == Some(c.resolve(actor)),
        }
    }
}

impl TokenSelector {
    fn matches(&self, token: &ConcreteToken, actor: MemberId) -> bool {
        match self {
            TokenSelector::Any => true,
            TokenSelector::Creator(c) => token.creator == Some(c.resolve(actor)),
        }
    }
}

impl ComponentSelector {
    /// Matches against the canonical component path (root app = `""`).
    pub fn matches(&self, component: &ConcreteComponent) -> bool {
        match self {
            ComponentSelector::Any => true,
            ComponentSelector::Path(p) => component.path == *p,
            // A prefix written with a trailing `/` (`path=foo/*`) also selects
            // the component at the prefix itself, so `foo/` covers `foo` and
            // its whole subtree while still excluding `foobar`.
            ComponentSelector::PathStartsWith(prefix) => {
                component.path.starts_with(prefix.as_str())
                    || prefix.strip_suffix('/') == Some(component.path.as_str())
            },
        }
    }
}

impl ResourceSegment {
    /// Returns true if this segment matches the given concrete segment.
    /// Multiple selectors within a segment are OR'd — any match suffices.
    /// `actor` is the member id of the principal whose permissions are
    /// being evaluated; used to resolve `creator=self` selectors.
    pub(crate) fn matches(&self, concrete: &ConcreteSegment, actor: MemberId) -> bool {
        match (self, concrete) {
            (ResourceSegment::Team, ConcreteSegment::Team) => true,
            (ResourceSegment::Project(selectors), ConcreteSegment::Project(project)) => {
                selectors.iter().any(|s| s.matches(project))
            },
            (ResourceSegment::Project(selectors), ConcreteSegment::ProposedProject { slug }) => {
                selectors.iter().any(|s| match s {
                    ProjectSelector::Any => true,
                    ProjectSelector::Id(_) => false,
                    ProjectSelector::Slug(s) => s == slug,
                })
            },
            (ResourceSegment::Deployment(selectors), ConcreteSegment::Deployment(deployment)) => {
                selectors.iter().any(|s| s.matches(deployment, actor))
            },
            (
                ResourceSegment::Deployment(selectors),
                ConcreteSegment::ProposedDeployment {
                    deployment_type,
                    creator,
                },
            ) => selectors.iter().any(|s| match s {
                DeploymentSelector::Any => true,
                DeploymentSelector::Id(_) => false,
                DeploymentSelector::Type(t) => t == deployment_type,
                DeploymentSelector::Creator(c) => *creator == Some(c.resolve(actor)),
            }),
            (
                ResourceSegment::Deployment(selectors),
                ConcreteSegment::LocalDeployment { owner },
            ) => selectors.iter().any(|s| match s {
                DeploymentSelector::Any => true,
                DeploymentSelector::Id(_) => false,
                DeploymentSelector::Type(t) => *t == DeploymentType::Dev,
                DeploymentSelector::Creator(c) => *owner == c.resolve(actor),
            }),
            (ResourceSegment::Component(selectors), ConcreteSegment::Component(component)) => {
                selectors.iter().any(|s| s.matches(component))
            },
            (ResourceSegment::Member, ConcreteSegment::Member) => true,
            (ResourceSegment::Token(selectors), ConcreteSegment::Token(token)) => {
                selectors.iter().any(|s| s.matches(token, actor))
            },
            (ResourceSegment::CustomRole, ConcreteSegment::CustomRole) => true,
            (ResourceSegment::Billing, ConcreteSegment::Billing) => true,
            (ResourceSegment::OauthApplication, ConcreteSegment::OauthApplication) => true,
            (ResourceSegment::Sso, ConcreteSegment::Sso) => true,
            (ResourceSegment::DirectorySync, ConcreteSegment::DirectorySync) => true,
            (ResourceSegment::Integration, ConcreteSegment::Integration) => true,
            (
                ResourceSegment::DefaultEnvironmentVariable,
                ConcreteSegment::DefaultEnvironmentVariable,
            ) => true,
            // Mismatched kinds never match.
            _ => false,
        }
    }
}

impl ResourceSpecifier {
    fn leaf_kind(&self) -> Option<ResourceKind> {
        self.segments.last().map(|s| s.kind())
    }

    /// Returns true if this specifier matches the given concrete resource.
    ///
    /// Requires an exact segment count match, with one parent-matches-child
    /// exception: a specifier ending at a deployment also matches a component
    /// of that deployment, so a `project:*:deployment:*` grant
    /// covers every component.
    fn matches(&self, resource: &ConcreteResource, actor: MemberId) -> bool {
        let deployment_covers_component = self.segments.len() + 1 == resource.segments.len()
            && self.leaf_kind() == Some(ResourceKind::Deployment)
            && resource.segments.last().map(|s| s.kind()) == Some(ResourceKind::Component);
        if self.segments.len() != resource.segments.len() && !deployment_covers_component {
            return false;
        }
        segments_match(&self.segments, &resource.segments, actor)
    }
}

fn segments_match(spec: &[ResourceSegment], concrete: &[ConcreteSegment], actor: MemberId) -> bool {
    spec.iter()
        .zip(concrete.iter())
        .all(|(spec_seg, concrete_seg)| spec_seg.matches(concrete_seg, actor))
}

impl CustomRole {
    /// Evaluate whether this role grants the given action on the given
    /// resource.
    ///
    /// `actor` is the member id of the principal whose permissions are being
    /// evaluated, used to resolve `creator=self` selectors against the
    /// resource's creator.
    ///
    /// Uses deny-overrides-allow on a default-deny baseline:
    /// 1. If any matching rule has effect Deny, the result is Denied.
    /// 2. If at least one matching rule has effect Allow (and none deny), the
    ///    result is Allowed.
    /// 3. If no rules match, the result is Denied.
    pub fn evaluate(
        &self,
        action: &RolePolicyAction,
        resource: &ConcreteResource,
        actor: MemberId,
    ) -> AccessDecision {
        // `*CustomRole` actions are intentionally ungrantable by a custom role
        // (see `RolePolicyAction::to_statement_action`); they always deny.
        let Some(stmt_action) = action.to_statement_action() else {
            return AccessDecision::Denied;
        };
        evaluate_statements(self.statements.iter(), stmt_action, resource, actor)
    }
}

/// Same deny-overrides-allow eval used by [`CustomRole::evaluate`], but driven
/// by a [`RoleStatementAction`] so it can be applied to actions that have no
/// [`RolePolicyAction`] counterpart yet, and by an iterator so it can flatten
/// statements across multiple roles.
pub(crate) fn evaluate_statements<'a>(
    statements: impl IntoIterator<Item = &'a RoleStatement>,
    action: RoleStatementAction,
    resource: &ConcreteResource,
    actor: MemberId,
) -> AccessDecision {
    let leaf_kind = resource.segments.last().map(|s| s.kind());
    let leaf_matches_action = match leaf_kind {
        Some(kind) if kind == action.resource_kind() => true,
        // A component resource is only meaningful for the deployment actions
        // that can be narrowed to a component.
        Some(ResourceKind::Component) => action.is_component_scopable(),
        _ => false,
    };
    if !leaf_matches_action {
        return AccessDecision::Denied;
    }

    let mut any_allow = false;
    for rule in statements {
        if rule.covers_action(action) && rule.resource.matches(resource, actor) {
            match rule.effect {
                RoleStatementEffect::Deny => return AccessDecision::Denied,
                RoleStatementEffect::Allow => any_allow = true,
            }
        }
    }

    if any_allow {
        AccessDecision::Allowed
    } else {
        AccessDecision::Denied
    }
}

/// Every [`DeploymentOp`] except `Unknown`, in the same order as the variant
/// declaration. Used to enumerate ops when computing what a set of roles
/// allows on a deployment.
pub const ALL_DEPLOYMENT_OPS: &[DeploymentOp] = &[
    DeploymentOp::Deploy,
    DeploymentOp::ViewEnvironmentVariables,
    DeploymentOp::WriteEnvironmentVariables,
    DeploymentOp::PauseDeployment,
    DeploymentOp::UnpauseDeployment,
    DeploymentOp::ViewLogs,
    DeploymentOp::ViewMetrics,
    DeploymentOp::ViewIntegrations,
    DeploymentOp::WriteIntegrations,
    DeploymentOp::ViewData,
    DeploymentOp::WriteData,
    DeploymentOp::ViewBackups,
    DeploymentOp::CreateBackups,
    DeploymentOp::DownloadBackups,
    DeploymentOp::DeleteBackups,
    DeploymentOp::ImportBackups,
    DeploymentOp::ActAsUser,
    DeploymentOp::RunInternalQueries,
    DeploymentOp::RunInternalMutations,
    DeploymentOp::RunInternalActions,
    DeploymentOp::RunTestQuery,
    DeploymentOp::ViewAuditLog,
    DeploymentOp::ViewUsageLimits,
    DeploymentOp::WriteUsageLimits,
    DeploymentOp::ViewUsage,
    DeploymentOp::UseAiGateway,
];

/// Authoritative mapping from a keybroker [`DeploymentOp`] to the
/// [`RoleStatementAction`] that gates it.
pub fn deployment_op_action(op: DeploymentOp) -> Option<RoleStatementAction> {
    use DeploymentOp as O;
    use RoleStatementAction as A;
    Some(match op {
        O::Deploy => A::Deploy,
        O::ViewEnvironmentVariables => A::ViewEnvironmentVariables,
        O::WriteEnvironmentVariables => A::WriteEnvironmentVariables,
        O::PauseDeployment => A::PauseDeployment,
        O::UnpauseDeployment => A::UnpauseDeployment,
        O::ViewLogs => A::ViewLogs,
        O::ViewMetrics => A::ViewMetrics,
        O::ViewIntegrations => A::ViewDeploymentIntegrations,
        O::WriteIntegrations => A::WriteDeploymentIntegrations,
        O::ViewData => A::ViewData,
        O::WriteData => A::WriteData,
        O::ViewBackups => A::ViewBackups,
        O::CreateBackups => A::CreateBackups,
        O::DownloadBackups => A::DownloadBackups,
        O::DeleteBackups => A::DeleteBackups,
        O::ImportBackups => A::ImportBackups,
        O::ActAsUser => A::ActAsUser,
        O::RunInternalQueries => A::RunInternalQueries,
        O::RunInternalMutations => A::RunInternalMutations,
        O::RunInternalActions => A::RunInternalActions,
        O::RunTestQuery => A::RunTestQuery,
        O::ViewAuditLog => A::ViewAuditLog,
        O::ViewUsageLimits => A::ViewUsageLimits,
        O::WriteUsageLimits => A::WriteUsageLimits,
        O::ViewUsage => A::ViewDeploymentUsage,
        O::UseAiGateway => A::UseAiGateway,
        O::Unknown => return None,
    })
}

pub trait RequireDeploymentOp {
    fn require_operation(&self, operation: DeploymentOp) -> anyhow::Result<()>;
}

impl RequireDeploymentOp for Identity {
    /// Check that this identity is an admin allowed to perform `operation`.
    /// System identities are always allowed. Admin identities are checked
    /// against their allowed operations. All other identities are rejected.
    fn require_operation(&self, operation: DeploymentOp) -> anyhow::Result<()> {
        let admin_identity = match self {
            Identity::System(_) => return Ok(()),
            Identity::DeploymentAdmin(admin_identity) | Identity::ActingUser(admin_identity, _) => {
                admin_identity
            },
            Identity::User(_) | Identity::Unknown(_) => {
                return Err(bad_admin_key_error(self.instance_name()).into());
            },
        };
        if !admin_identity.is_operation_allowed(operation)? {
            let action = deployment_op_action(operation)
                .map_or_else(|| format!("{operation:?}"), |action| action.to_string());
            anyhow::bail!(ErrorMetadata::forbidden(
                "OperationNotPermitted",
                format!("You do not have permission to perform this operation ({action})."),
            ));
        }
        Ok(())
    }
}

/// Returns the [`DeploymentOp`]s that `roles` collectively allow
/// deployment-wide on `deployment` (which lives under `project`). Roles are
/// additive: an op is allowed if *any* role evaluates to `Allowed` for it.
/// Within a single role, `Deny` still overrides `Allow` (per
/// [`CustomRole::evaluate`]), but a `Deny` in one role does not override an
/// `Allow` in another.
///
/// An op a role denies on any component of the deployment is left out of this
/// list for that role, even though the role allows it elsewhere in the
/// deployment: callers treat the list as "allowed everywhere", so a partially
/// denied op must not appear. Component-level access is instead described by
/// [`component_op_rules`].
pub fn allowed_deployment_ops(
    roles: &[CustomRole],
    project: &ConcreteProject,
    deployment: &ConcreteDeployment,
    actor: MemberId,
) -> Vec<DeploymentOp> {
    let resource = ConcreteResource {
        segments: vec![
            ConcreteSegment::Project(project.clone()),
            ConcreteSegment::Deployment(deployment.clone()),
        ],
    };
    allowed_deployment_ops_for_resource(roles, &resource, actor)
}

/// Same as [`allowed_deployment_ops`] but evaluates against an
/// already-built [`ConcreteResource`]. Used by the deploy-key escalation
/// guard, which needs to evaluate ops against synthesized
/// `ProposedDeployment` segments under a project.
pub fn allowed_deployment_ops_for_resource(
    roles: &[CustomRole],
    resource: &ConcreteResource,
    actor: MemberId,
) -> Vec<DeploymentOp> {
    ALL_DEPLOYMENT_OPS
        .iter()
        .copied()
        .filter(|op| {
            let Some(action) = deployment_op_action(*op) else {
                return false;
            };
            roles.iter().any(|role| {
                evaluate_statements(role.statements.iter(), action, resource, actor)
                    == AccessDecision::Allowed
                    && !has_component_scoped_deny(role, action, resource, actor)
            })
        })
        .collect()
}

/// Whether `role` has a `Deny` statement for `action` on some component of
/// `resource` (a `[Project, Deployment]`-shaped resource). The component
/// selectors are irrelevant here: any such statement means the op is not
/// allowed deployment-wide.
fn has_component_scoped_deny(
    role: &CustomRole,
    action: RoleStatementAction,
    resource: &ConcreteResource,
    actor: MemberId,
) -> bool {
    role.statements.iter().any(|stmt| {
        stmt.effect == RoleStatementEffect::Deny
            && stmt.leaf_kind() == Some(ResourceKind::Component)
            && stmt.resource.segments.len() == resource.segments.len() + 1
            && stmt.covers_action(action)
            && segments_match(&stmt.resource.segments, &resource.segments, actor)
    })
}

/// The [`DeploymentOp`]s whose actions are component-scopable, in
/// [`ALL_DEPLOYMENT_OPS`] order.
fn component_scopable_ops() -> impl Iterator<Item = DeploymentOp> {
    ALL_DEPLOYMENT_OPS
        .iter()
        .copied()
        .filter(|op| deployment_op_action(*op).is_some_and(|action| action.is_component_scopable()))
}

/// One statement of a custom role, reduced to what matters for a component of
/// a particular deployment: which component-scopable ops it covers and which
/// components it selects. The project and deployment segments have already
/// been matched away by [`component_op_rules`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentOpRule {
    pub effect: RoleStatementEffect,
    /// Never empty; a statement that covers no component-scopable op produces
    /// no rule.
    pub ops: Vec<DeploymentOp>,
    /// OR'd, like the selectors of a resource segment. A statement whose leaf
    /// is the deployment itself becomes `[ComponentSelector::Any]`.
    pub selectors: Vec<ComponentSelector>,
}

/// The residual component-level rules of a single custom role for a single
/// deployment. Kept per role so that [`evaluate_component_op_rules`] can apply
/// deny-overrides-allow within the role without letting a deny in one role
/// override an allow in another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentOpRoleRules {
    pub rules: Vec<ComponentOpRule>,
}

impl ComponentOpRule {
    fn matches(&self, op: DeploymentOp, component: &ConcreteComponent) -> bool {
        self.ops.contains(&op) && self.selectors.iter().any(|s| s.matches(component))
    }
}

impl ComponentOpRoleRules {
    fn evaluate(&self, op: DeploymentOp, component: &ConcreteComponent) -> AccessDecision {
        let mut any_allow = false;
        for rule in &self.rules {
            if rule.matches(op, component) {
                match rule.effect {
                    RoleStatementEffect::Deny => return AccessDecision::Denied,
                    RoleStatementEffect::Allow => any_allow = true,
                }
            }
        }
        if any_allow {
            AccessDecision::Allowed
        } else {
            AccessDecision::Denied
        }
    }
}

pub fn component_op_rules(
    roles: &[CustomRole],
    project: &ConcreteProject,
    deployment: &ConcreteDeployment,
    actor: MemberId,
) -> Vec<ComponentOpRoleRules> {
    let parent_segments = [
        ConcreteSegment::Project(project.clone()),
        ConcreteSegment::Deployment(deployment.clone()),
    ];
    roles
        .iter()
        .filter_map(|role| {
            let rules: Vec<ComponentOpRule> = role
                .statements
                .iter()
                .filter_map(|stmt| component_op_rule(stmt, &parent_segments, actor))
                .collect();
            (!rules.is_empty()).then_some(ComponentOpRoleRules { rules })
        })
        .collect()
}

fn component_op_rule(
    stmt: &RoleStatement,
    parent_segments: &[ConcreteSegment; 2],
    actor: MemberId,
) -> Option<ComponentOpRule> {
    let selectors = match stmt.resource.segments.as_slice() {
        [_, _] if stmt.leaf_kind() == Some(ResourceKind::Deployment) => {
            vec![ComponentSelector::Any]
        },
        [_, _, ResourceSegment::Component(selectors)] => selectors.clone(),
        _ => return None,
    };
    if !segments_match(&stmt.resource.segments, parent_segments, actor) {
        return None;
    }
    let ops: Vec<DeploymentOp> = component_scopable_ops()
        .filter(|op| deployment_op_action(*op).is_some_and(|action| stmt.covers_action(action)))
        .collect();
    (!ops.is_empty()).then_some(ComponentOpRule {
        effect: stmt.effect,
        ops,
        selectors,
    })
}

pub fn evaluate_component_op_rules(
    roles: &[ComponentOpRoleRules],
    op: DeploymentOp,
    component_path: &str,
) -> AccessDecision {
    let component = ConcreteComponent {
        path: component_path.to_string(),
    };
    if roles
        .iter()
        .any(|role| role.evaluate(op, &component) == AccessDecision::Allowed)
    {
        AccessDecision::Allowed
    } else {
        AccessDecision::Denied
    }
}
