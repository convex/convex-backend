use big_brain_private_api_types::{
    ProjectName,
    ProjectSlug,
};
use common::types::{
    DeploymentId,
    DeploymentType,
    MemberId,
    ProjectId,
};
use serde::{
    de::Error as _,
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
};
use utoipa::ToSchema;

/// Selectors valid on project resources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ProjectSelector {
    Any,
    Id(ProjectId),
    Slug(ProjectSlug),
}

/// Selectors valid on deployment resources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DeploymentSelector {
    Any,
    Id(DeploymentId),
    Type(DeploymentType),
    Creator(CreatorMatcher),
}

/// Selectors valid on token resources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum TokenSelector {
    Any,
    Creator(CreatorMatcher),
}

/// Selectors valid on component resources, nested under a deployment.
///
/// A component statement reads e.g. `…:deployment:*:component:path=foo/bar`.
/// Selection is over the component's path in the deployment's component tree,
/// matched as a string (the canonical slash-joined path; the root app component
/// is `""`). A leading `/` on the written path is optional and ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComponentSelector {
    /// `*` — every component in the deployment.
    Any,
    /// `path=foo/bar` — exactly this component (`path=/` selects the root app
    /// component, stored as `""`).
    Path(String),
    /// `path=<prefix>*` — any component whose path string starts with `prefix`.
    /// This is a literal prefix: `foo` matches `foo`, `foo/bar`, *and*
    /// `foobar`. End the prefix with `/` for path-boundary matching: `foo/`
    /// matches `foo` and `foo/bar` but not `foobar`.
    PathStartsWith(String),
}

/// Right-hand side of `creator=` on a deployment or token selector. Either
/// a fixed [`MemberId`] or `self`, which resolves to the evaluating actor's
/// member id at match time so a single statement covers "things I created"
/// without naming the actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CreatorMatcher {
    /// `creator=self` — match if the resource's creator is the actor doing
    /// the access check.
    SelfActor,
    /// `creator=<id>` — match if the resource's creator is this specific
    /// member.
    Member(MemberId),
}

impl CreatorMatcher {
    /// Resolves the matcher against the evaluating actor's member id,
    /// returning the [`MemberId`] the resource's `creator` field must equal
    /// for the selector to match.
    pub(crate) fn resolve(self, actor: MemberId) -> MemberId {
        match self {
            CreatorMatcher::SelfActor => actor,
            CreatorMatcher::Member(mid) => mid,
        }
    }
}

/// The kind of resource in the hierarchy.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::EnumString, strum::Display,
)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum ResourceKind {
    Team,
    Project,
    Deployment,
    Component,
    Member,
    Token,
    CustomRole,
    Billing,
    OauthApplication,
    Sso,
    DirectorySync,
    Integration,
    DefaultEnvironmentVariable,
}

/// A single segment in a resource specifier path.
/// Each variant carries only the selectors valid for that resource kind,
/// making invalid selector/kind combinations unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ResourceSegment {
    Team,
    Project(Vec<ProjectSelector>),
    Deployment(Vec<DeploymentSelector>),
    Component(Vec<ComponentSelector>),
    Member,
    Token(Vec<TokenSelector>),
    CustomRole,
    Billing,
    OauthApplication,
    Sso,
    DirectorySync,
    Integration,
    DefaultEnvironmentVariable,
}

impl ResourceSegment {
    pub fn kind(&self) -> ResourceKind {
        match self {
            ResourceSegment::Team => ResourceKind::Team,
            ResourceSegment::Project(_) => ResourceKind::Project,
            ResourceSegment::Deployment(_) => ResourceKind::Deployment,
            ResourceSegment::Component(_) => ResourceKind::Component,
            ResourceSegment::Member => ResourceKind::Member,
            ResourceSegment::Token(_) => ResourceKind::Token,
            ResourceSegment::CustomRole => ResourceKind::CustomRole,
            ResourceSegment::Billing => ResourceKind::Billing,
            ResourceSegment::OauthApplication => ResourceKind::OauthApplication,
            ResourceSegment::Sso => ResourceKind::Sso,
            ResourceSegment::DirectorySync => ResourceKind::DirectorySync,
            ResourceSegment::Integration => ResourceKind::Integration,
            ResourceSegment::DefaultEnvironmentVariable => ResourceKind::DefaultEnvironmentVariable,
        }
    }
}

/// Distinguishes the kinds of token that authorize via
/// [`RolePolicyAction::CreateProjectAccessToken`]. The two kinds share the
/// same role-evaluation path (both grant on `project:_:token:_`) but only
/// `PreviewDeployKey` carries an implicit op set on preview deployments,
/// so only it triggers the preview escalation guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectTokenKind {
    PlatformToken,
    PreviewDeployKey,
}

/// Actions a member can perform. Some variants carry the ID of the specific
/// resource being acted on; those IDs are used at runtime for permission
/// checks and never appear on the wire. See [`RoleStatementAction`] for the
/// wire-format mirror.
#[derive(Debug, PartialEq, Eq, Clone, strum::Display)]
pub enum RolePolicyAction {
    // Team
    UpdateTeam,
    DeleteTeam,
    // Projects
    CreateProject(ProjectSlug),
    TransferProject(ProjectId),
    /// Authorizes accepting a project transfer on the destination team.
    /// Carries the inbound project's slug — not its id — because the
    /// project still belongs to the source team when this check runs, so
    /// authorization targets a proposed-project slot on the destination
    /// team (symmetric with [`Self::ReceiveDeployment`]).
    ReceiveProject {
        slug: ProjectSlug,
    },
    UpdateProject(ProjectId),
    DeleteProject(ProjectId),
    ViewProjects(ProjectId),
    UpdateMemberProjectRole(ProjectId),
    CreateProjectEnvironmentVariable(ProjectId),
    UpdateProjectEnvironmentVariable(ProjectId),
    DeleteProjectEnvironmentVariable(ProjectId),
    ViewProjectEnvironmentVariables(ProjectId),
    CreateDeployment {
        project_id: ProjectId,
        deployment_type: DeploymentType,
        creator: Option<MemberId>,
    },
    TransferDeployment(DeploymentId),
    ReceiveDeployment {
        project_id: ProjectId,
        deployment_type: DeploymentType,
        creator: Option<MemberId>,
    },
    /// Per-field permissions for the public update-deployment API. Each
    /// corresponds to a single field on `PlatformUpdateDeploymentArgs` and is
    /// only checked when that field is being changed. All map to
    /// `AuditLogAction::UpdateDeployment`.
    UpdateDeploymentReference(DeploymentId),
    UpdateDeploymentDashboardEditConfirmation(DeploymentId),
    UpdateDeploymentExpiresAt(DeploymentId),
    UpdateDeploymentSendLogsToClient(DeploymentId),
    UpdateDeploymentClass(DeploymentId),
    UpdateDeploymentIsDefault(DeploymentId),
    UpdateDeploymentType(DeploymentId),
    DeleteDeployment(DeploymentId),
    ViewDeployments(DeploymentId),
    UseAiGateway {
        project_id: ProjectId,
        owner: MemberId,
    },
    /// Read access to deployment-scoped integration metadata (e.g. WorkOS
    /// environment association). Granted to any team member.
    ViewDeploymentIntegrations(DeploymentId),
    /// Write access to deployment-scoped integrations (provisioning, deletion,
    /// etc.). Same permission rules as the deployment-write actions above.
    /// Coarse permission check — handlers emit their own granular audit log
    /// entries (e.g. CreateWorkosEnvironment, DeleteWorkosEnvironment).
    WriteDeploymentIntegrations(DeploymentId),
    // Custom Domains
    CreateCustomDomain(DeploymentId),
    DeleteCustomDomain(DeploymentId),
    ViewCustomDomains(DeploymentId),
    // Members
    InviteMember,
    CancelMemberInvitation,
    RemoveMember,
    UpdateMemberRole,
    /// Read access to team members, pending invitations, and per-member
    /// project-role assignments. Granted to admins and developers.
    ViewMembers,
    // Billing
    UpdatePaymentMethod,
    UpdateBillingContact,
    UpdateBillingAddress,
    /// All subscription lifecycle operations — creating a subscription,
    /// changing the plan, cancelling, and resuming — share a single
    /// permission. Handlers emit their own audit log action
    /// (`CreateSubscription` / `CancelSubscription` / `ResumeSubscription`
    /// / `ChangeSubscriptionPlan`) so the audit trail stays granular even
    /// though the gate doesn't.
    ChangeSubscriptionPlan,
    UpdateSpendingLimit,
    ViewBillingDetails,
    ViewInvoices,
    // Audit Log
    ViewTeamAuditLog,
    // Team Domains — the domains the team has claimed in WorkOS, shared by SSO
    // and directory sync. Creation happens in the WorkOS admin portal, so
    // `CreateTeamDomain` gates the portal link rather than a write of ours.
    CreateTeamDomain,
    DeleteTeamDomain,
    ViewTeamDomains,
    // Team Access Tokens
    CreateTeamAccessToken {
        creator: Option<MemberId>,
    },
    UpdateTeamAccessToken {
        creator: Option<MemberId>,
    },
    DeleteTeamAccessToken {
        creator: Option<MemberId>,
    },
    ViewTeamAccessTokens {
        creator: Option<MemberId>,
    },
    // Project Access Tokens
    CreateProjectAccessToken {
        project_id: ProjectId,
        creator: Option<MemberId>,
        /// Which kind of project-scoped token is being minted. Used inside
        /// `TeamMemberModel::ensure_actor_has_permission` (in big_brain_lib)
        /// to fire the preview-key escalation guard only for
        /// [`ProjectTokenKind::PreviewDeployKey`]; platform tokens go
        /// through the same role check without the extra subset step.
        token_kind: ProjectTokenKind,
    },
    UpdateProjectAccessToken {
        project_id: ProjectId,
        creator: Option<MemberId>,
    },
    DeleteProjectAccessToken {
        project_id: ProjectId,
        creator: Option<MemberId>,
    },
    ViewProjectAccessTokens(ProjectId),
    // Deployment Access Tokens
    CreateDeploymentAccessToken {
        deployment_id: DeploymentId,
        creator: Option<MemberId>,
        /// The op set the new deploy key will carry. `None` means it
        /// inherits every [`keybroker::DeploymentOp`]. Used inside
        /// `TeamMemberModel::ensure_actor_has_permission` (in big_brain_lib)
        /// to run the privilege-escalation guard alongside the regular
        /// role check, so callers don't have to remember the second call.
        allowed_operations: Option<Vec<keybroker::DeploymentOp>>,
    },
    UpdateDeploymentAccessToken {
        deployment_id: DeploymentId,
        creator: Option<MemberId>,
    },
    DeleteDeploymentAccessToken {
        deployment_id: DeploymentId,
        creator: Option<MemberId>,
    },
    ViewDeploymentAccessTokens(DeploymentId),
    // OAuth Apps
    CreateOAuthApplication,
    UpdateOAuthApplication,
    DeleteOAuthApplication,
    ViewOAuthApplications,
    GenerateOAuthClientSecret,
    // Usage
    ViewUsage,
    // Insights
    ViewInsights(DeploymentId),
    // Backups
    CreateBackups(DeploymentId),
    ImportBackups(DeploymentId),
    ConfigurePeriodicBackups(DeploymentId),
    DisablePeriodicBackups(DeploymentId),
    DeleteBackups(DeploymentId),
    ViewBackups(DeploymentId),
    // Referrals
    ApplyReferralCode,
    // SSO
    EnableSSO,
    DisableSSO,
    UpdateSSO,
    ViewSSO,
    // Directory Sync
    EnableDirectorySync,
    DisableDirectorySync,
    UpdateDirectorySyncGroupMapping,
    DeleteDirectorySyncGroupMapping,
    ViewDirectorySync,
    // Custom Roles
    CreateCustomRole,
    UpdateCustomRole,
    DeleteCustomRole,
    ViewCustomRoles,
    // Team integrations (WorkOS, future GitHub/Vercel/etc.). Each integration's
    // handlers are responsible for emitting their own granular audit log entries.
    ViewTeamIntegrations,
    CreateTeamIntegrations,
    UpdateTeamIntegrations,
    DeleteTeamIntegrations,
}

/// Path through the resource hierarchy that a [`RolePolicyAction`]
/// targets, broken out so the resource construction logic and the
/// resource-kind logic share a single source of truth.
///
/// Variants describe both the segment shape (so a builder knows what to
/// load and how to nest segments) and the runtime ids that need to be
/// resolved. The [`ResourceKind`] always matches what the
/// evaluator expects to see at the tail of a [`ConcreteResource`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionResourcePath {
    /// `[Kind]` — a singleton segment that carries no ids.
    /// Valid kinds: Team, Member, Billing, Sso, OauthApplication, Integration,
    /// CustomRole.
    Singleton(ResourceKind),
    /// `[Project(loaded by id)]`.
    Project(ProjectId),
    /// `[Project(loaded by id), DefaultEnvironmentVariable]`.
    ProjectDefaultEnvVar(ProjectId),
    /// `[Project(loaded by deployment.project_id), Deployment(loaded by id)]`.
    Deployment(DeploymentId),
    /// `[ProposedProject { slug }]` — create-project authorization. No id
    /// exists yet.
    ProposedProject(ProjectSlug),
    /// `[Project(loaded by id), ProposedDeployment { type, creator }]` —
    /// create-deployment / receive-deployment authorization.
    ProposedDeploymentInProject {
        project_id: ProjectId,
        deployment_type: DeploymentType,
        creator: Option<MemberId>,
    },
    /// `[Project(loaded by id), LocalDeployment { owner }]`.
    LocalDeploymentInProject {
        project_id: ProjectId,
        owner: MemberId,
    },
    /// `[Team, Token(ConcreteToken { creator })]`.
    TeamToken { creator: Option<MemberId> },
    /// `[Project(loaded by id), Token(ConcreteToken { creator })]`.
    ProjectToken {
        project_id: ProjectId,
        creator: Option<MemberId>,
    },
    /// `[Project(loaded by deployment.project_id), Deployment(loaded by
    /// id), Token(ConcreteToken { creator })]`.
    DeploymentToken {
        deployment_id: DeploymentId,
        creator: Option<MemberId>,
    },
    /// Action is intentionally ungrantable by custom roles, regardless of
    /// any statement, to prevent privilege escalation. The builder treats
    /// custom-role evaluation as "no grant" for these. The carried
    /// [`ResourceKind`] is the kind the action targets, kept for
    /// diagnostics.
    Forbidden(ResourceKind),
}

impl RolePolicyAction {
    /// The resource path this action targets. Single source of truth for
    /// both [`Self::resource_kind`] and the resource-construction logic
    /// in the resource builder in big_brain_lib.
    pub fn resource_path(&self) -> ActionResourcePath {
        use ActionResourcePath as Path;
        use RolePolicyAction as P;
        match self {
            // Team singletons.
            P::UpdateTeam
            | P::DeleteTeam
            | P::ApplyReferralCode
            | P::ViewTeamAuditLog
            | P::CreateTeamDomain
            | P::DeleteTeamDomain
            | P::ViewTeamDomains
            | P::ViewUsage => Path::Singleton(ResourceKind::Team),
            // Billing singletons.
            P::UpdatePaymentMethod
            | P::UpdateBillingContact
            | P::UpdateBillingAddress
            | P::ChangeSubscriptionPlan
            | P::UpdateSpendingLimit
            | P::ViewBillingDetails
            | P::ViewInvoices => Path::Singleton(ResourceKind::Billing),
            // OAuth Application singletons.
            P::CreateOAuthApplication
            | P::UpdateOAuthApplication
            | P::DeleteOAuthApplication
            | P::ViewOAuthApplications
            | P::GenerateOAuthClientSecret => Path::Singleton(ResourceKind::OauthApplication),
            // SSO singletons.
            P::EnableSSO | P::DisableSSO | P::UpdateSSO | P::ViewSSO => {
                Path::Singleton(ResourceKind::Sso)
            },
            // Directory Sync singletons.
            P::EnableDirectorySync
            | P::DisableDirectorySync
            | P::UpdateDirectorySyncGroupMapping
            | P::DeleteDirectorySyncGroupMapping
            | P::ViewDirectorySync => Path::Singleton(ResourceKind::DirectorySync),
            // Team Integration singletons.
            P::ViewTeamIntegrations
            | P::CreateTeamIntegrations
            | P::UpdateTeamIntegrations
            | P::DeleteTeamIntegrations => Path::Singleton(ResourceKind::Integration),
            // Custom roles. ViewCustomRoles is grantable; the
            // create/update/delete variants are intentionally ungrantable
            // (see `to_statement_action`).
            P::ViewCustomRoles => Path::Singleton(ResourceKind::CustomRole),
            P::CreateCustomRole | P::UpdateCustomRole | P::DeleteCustomRole => {
                Path::Forbidden(ResourceKind::CustomRole)
            },
            // Project actions whose target is an existing project.
            P::TransferProject(id)
            | P::UpdateProject(id)
            | P::DeleteProject(id)
            | P::ViewProjects(id)
            | P::UpdateMemberProjectRole(id) => Path::Project(*id),
            // Project default environment variables.
            P::CreateProjectEnvironmentVariable(id)
            | P::UpdateProjectEnvironmentVariable(id)
            | P::DeleteProjectEnvironmentVariable(id)
            | P::ViewProjectEnvironmentVariables(id) => Path::ProjectDefaultEnvVar(*id),
            // Deployment actions whose target is an existing deployment.
            P::TransferDeployment(id)
            | P::UpdateDeploymentReference(id)
            | P::UpdateDeploymentDashboardEditConfirmation(id)
            | P::UpdateDeploymentExpiresAt(id)
            | P::UpdateDeploymentSendLogsToClient(id)
            | P::UpdateDeploymentClass(id)
            | P::UpdateDeploymentIsDefault(id)
            | P::UpdateDeploymentType(id)
            | P::DeleteDeployment(id)
            | P::ViewDeployments(id)
            | P::ViewDeploymentIntegrations(id)
            | P::WriteDeploymentIntegrations(id)
            | P::CreateCustomDomain(id)
            | P::DeleteCustomDomain(id)
            | P::ViewCustomDomains(id)
            | P::ViewInsights(id)
            | P::CreateBackups(id)
            | P::ImportBackups(id)
            | P::ConfigurePeriodicBackups(id)
            | P::DisablePeriodicBackups(id)
            | P::DeleteBackups(id)
            | P::ViewBackups(id) => Path::Deployment(*id),
            P::UseAiGateway { project_id, owner } => Path::LocalDeploymentInProject {
                project_id: *project_id,
                owner: *owner,
            },
            // Project create / receive — synthesize a proposed-project
            // segment on the destination team. Receive can't use
            // `Path::Project(id)` because the project still belongs to the
            // source team at perm-check time, so the cross-team load would
            // bail.
            P::CreateProject(slug) | P::ReceiveProject { slug } => {
                Path::ProposedProject(slug.clone())
            },
            // Deployment create / receive — synthesize a proposed-deployment
            // segment under the (existing) parent project.
            P::CreateDeployment {
                project_id,
                deployment_type,
                creator,
            }
            | P::ReceiveDeployment {
                project_id,
                deployment_type,
                creator,
            } => Path::ProposedDeploymentInProject {
                project_id: *project_id,
                deployment_type: *deployment_type,
                creator: *creator,
            },
            // Member actions evaluate against a selectorless Member segment
            // — `member:*` is the only matching specifier today.
            P::InviteMember | P::CancelMemberInvitation | P::RemoveMember | P::UpdateMemberRole => {
                Path::Singleton(ResourceKind::Member)
            },
            // Team-scoped tokens.
            P::CreateTeamAccessToken { creator }
            | P::UpdateTeamAccessToken { creator }
            | P::DeleteTeamAccessToken { creator }
            | P::ViewTeamAccessTokens { creator } => Path::TeamToken { creator: *creator },
            // Project-scoped tokens.
            P::CreateProjectAccessToken {
                project_id,
                creator,
                ..
            }
            | P::UpdateProjectAccessToken {
                project_id,
                creator,
            }
            | P::DeleteProjectAccessToken {
                project_id,
                creator,
            } => Path::ProjectToken {
                project_id: *project_id,
                creator: *creator,
            },
            P::ViewProjectAccessTokens(project_id) => Path::ProjectToken {
                project_id: *project_id,
                creator: None,
            },
            // Deployment-scoped tokens.
            P::CreateDeploymentAccessToken {
                deployment_id,
                creator,
                ..
            }
            | P::UpdateDeploymentAccessToken {
                deployment_id,
                creator,
            }
            | P::DeleteDeploymentAccessToken {
                deployment_id,
                creator,
            } => Path::DeploymentToken {
                deployment_id: *deployment_id,
                creator: *creator,
            },
            P::ViewDeploymentAccessTokens(deployment_id) => Path::DeploymentToken {
                deployment_id: *deployment_id,
                creator: None,
            },
            // ViewMembers is a wildcard read over the member directory.
            // Custom-role statements grant it with `resource: "member:*"`.
            P::ViewMembers => Path::Singleton(ResourceKind::Member),
        }
    }

    /// Projects this runtime action onto its wire-format mirror, dropping any
    /// resource ID parameters.
    ///
    /// Returns `None` for actions that are intentionally ungrantable by custom
    /// roles:
    ///
    /// - `CreateCustomRole`, `UpdateCustomRole`, and `DeleteCustomRole` —
    ///   allowing a custom role to grant or modify custom-role permissions
    ///   would let it escalate itself.
    /// - `ApplyReferralCode` — referral redemption is a one-time team-
    ///   lifecycle event reserved for built-in team admins and cannot be
    ///   delegated to custom roles (including via wildcard rules).
    pub fn to_statement_action(&self) -> Option<RoleStatementAction> {
        use RolePolicyAction as P;
        use RoleStatementAction as S;
        Some(match self {
            P::UpdateTeam => S::UpdateTeam,
            P::DeleteTeam => S::DeleteTeam,
            P::CreateProject(_) => S::CreateProject,
            P::TransferProject(_) => S::TransferProject,
            P::ReceiveProject { .. } => S::ReceiveProject,
            P::UpdateProject(_) => S::UpdateProject,
            P::DeleteProject(_) => S::DeleteProject,
            P::ViewProjects(_) => S::ViewProjects,
            P::UpdateMemberProjectRole(_) => S::UpdateMemberProjectRole,
            P::CreateProjectEnvironmentVariable(_) => S::CreateProjectEnvironmentVariable,
            P::UpdateProjectEnvironmentVariable(_) => S::UpdateProjectEnvironmentVariable,
            P::DeleteProjectEnvironmentVariable(_) => S::DeleteProjectEnvironmentVariable,
            P::ViewProjectEnvironmentVariables(_) => S::ViewProjectEnvironmentVariables,
            P::CreateDeployment { .. } => S::CreateDeployment,
            P::TransferDeployment(_) => S::TransferDeployment,
            P::ReceiveDeployment { .. } => S::ReceiveDeployment,
            P::UpdateDeploymentReference(_) => S::UpdateDeploymentReference,
            P::UpdateDeploymentDashboardEditConfirmation(_) => {
                S::UpdateDeploymentDashboardEditConfirmation
            },
            P::UpdateDeploymentExpiresAt(_) => S::UpdateDeploymentExpiresAt,
            P::UpdateDeploymentSendLogsToClient(_) => S::UpdateDeploymentSendLogsToClient,
            P::UpdateDeploymentClass(_) => S::UpdateDeploymentClass,
            P::UpdateDeploymentIsDefault(_) => S::UpdateDeploymentIsDefault,
            P::UpdateDeploymentType(_) => S::UpdateDeploymentType,
            P::DeleteDeployment(_) => S::DeleteDeployment,
            P::ViewDeployments(_) => S::ViewDeployments,
            P::ViewDeploymentIntegrations(_) => S::ViewDeploymentIntegrations,
            P::WriteDeploymentIntegrations(_) => S::WriteDeploymentIntegrations,
            P::CreateCustomDomain(_) => S::CreateCustomDomain,
            P::DeleteCustomDomain(_) => S::DeleteCustomDomain,
            P::ViewCustomDomains(_) => S::ViewCustomDomains,
            P::InviteMember => S::InviteMember,
            P::CancelMemberInvitation => S::CancelMemberInvitation,
            P::RemoveMember => S::RemoveMember,
            P::UpdateMemberRole => S::UpdateMemberRole,
            P::ViewMembers => S::ViewMembers,
            P::UpdatePaymentMethod => S::UpdatePaymentMethod,
            P::UpdateBillingContact => S::UpdateBillingContact,
            P::UpdateBillingAddress => S::UpdateBillingAddress,
            P::ChangeSubscriptionPlan => S::ChangeSubscriptionPlan,
            P::UpdateSpendingLimit => S::UpdateSpendingLimit,
            P::ViewBillingDetails => S::ViewBillingDetails,
            P::ViewInvoices => S::ViewInvoices,
            P::ViewTeamAuditLog => S::ViewTeamAuditLog,
            P::CreateTeamDomain => S::CreateTeamDomain,
            P::DeleteTeamDomain => S::DeleteTeamDomain,
            P::ViewTeamDomains => S::ViewTeamDomains,
            P::CreateTeamAccessToken { .. } => S::CreateTeamAccessToken,
            P::UpdateTeamAccessToken { .. } => S::UpdateTeamAccessToken,
            P::DeleteTeamAccessToken { .. } => S::DeleteTeamAccessToken,
            P::ViewTeamAccessTokens { .. } => S::ViewTeamAccessTokens,
            P::CreateProjectAccessToken { .. } => S::CreateProjectAccessToken,
            P::UpdateProjectAccessToken { .. } => S::UpdateProjectAccessToken,
            P::DeleteProjectAccessToken { .. } => S::DeleteProjectAccessToken,
            P::ViewProjectAccessTokens(_) => S::ViewProjectAccessTokens,
            P::CreateDeploymentAccessToken { .. } => S::CreateDeploymentAccessToken,
            P::UpdateDeploymentAccessToken { .. } => S::UpdateDeploymentAccessToken,
            P::DeleteDeploymentAccessToken { .. } => S::DeleteDeploymentAccessToken,
            P::ViewDeploymentAccessTokens(_) => S::ViewDeploymentAccessTokens,
            P::CreateOAuthApplication => S::CreateOAuthApplication,
            P::UpdateOAuthApplication => S::UpdateOAuthApplication,
            P::DeleteOAuthApplication => S::DeleteOAuthApplication,
            P::ViewOAuthApplications => S::ViewOAuthApplications,
            P::GenerateOAuthClientSecret => S::GenerateOAuthClientSecret,
            P::ViewUsage => S::ViewUsage,
            P::UseAiGateway { .. } => S::UseAiGateway,
            P::ViewInsights(_) => S::ViewInsights,
            P::CreateBackups(_) => S::CreateBackups,
            P::ImportBackups(_) => S::ImportBackups,
            P::ConfigurePeriodicBackups(_) => S::ConfigurePeriodicBackups,
            P::DisablePeriodicBackups(_) => S::DisablePeriodicBackups,
            P::DeleteBackups(_) => S::DeleteBackups,
            P::ViewBackups(_) => S::ViewBackups,
            // `ApplyReferralCode` is intentionally not in `RoleStatementAction`
            // — see the doc on `to_statement_action`.
            P::ApplyReferralCode => return None,
            P::EnableSSO => S::EnableSSO,
            P::DisableSSO => S::DisableSSO,
            P::UpdateSSO => S::UpdateSSO,
            P::ViewSSO => S::ViewSSO,
            P::EnableDirectorySync => S::EnableDirectorySync,
            P::DisableDirectorySync => S::DisableDirectorySync,
            P::UpdateDirectorySyncGroupMapping => S::UpdateDirectorySyncGroupMapping,
            P::DeleteDirectorySyncGroupMapping => S::DeleteDirectorySyncGroupMapping,
            P::ViewDirectorySync => S::ViewDirectorySync,
            P::ViewCustomRoles => S::ViewCustomRoles,
            P::CreateCustomRole | P::UpdateCustomRole | P::DeleteCustomRole => return None,
            P::ViewTeamIntegrations => S::ViewTeamIntegrations,
            P::CreateTeamIntegrations => S::CreateTeamIntegrations,
            P::UpdateTeamIntegrations => S::UpdateTeamIntegrations,
            P::DeleteTeamIntegrations => S::DeleteTeamIntegrations,
        })
    }
}

/// An action that can be allowed or denied by a custom role statement.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema, strum::Display)]
pub enum RoleStatementAction {
    // Team
    #[serde(rename = "team:update")]
    #[strum(serialize = "team:update")]
    UpdateTeam,
    #[serde(rename = "team:delete")]
    #[strum(serialize = "team:delete")]
    DeleteTeam,
    // Projects
    #[serde(rename = "project:create")]
    #[strum(serialize = "project:create")]
    CreateProject,
    #[serde(rename = "project:transfer")]
    #[strum(serialize = "project:transfer")]
    TransferProject,
    #[serde(rename = "project:receive")]
    #[strum(serialize = "project:receive")]
    ReceiveProject,
    #[serde(rename = "project:update")]
    #[strum(serialize = "project:update")]
    UpdateProject,
    #[serde(rename = "project:delete")]
    #[strum(serialize = "project:delete")]
    DeleteProject,
    #[serde(rename = "project:view")]
    #[strum(serialize = "project:view")]
    ViewProjects,
    #[serde(rename = "project:updateMemberRole")]
    #[strum(serialize = "project:updateMemberRole")]
    UpdateMemberProjectRole,
    #[serde(rename = "defaultEnvironmentVariable:create")]
    #[strum(serialize = "defaultEnvironmentVariable:create")]
    CreateProjectEnvironmentVariable,
    #[serde(rename = "defaultEnvironmentVariable:update")]
    #[strum(serialize = "defaultEnvironmentVariable:update")]
    UpdateProjectEnvironmentVariable,
    #[serde(rename = "defaultEnvironmentVariable:delete")]
    #[strum(serialize = "defaultEnvironmentVariable:delete")]
    DeleteProjectEnvironmentVariable,
    #[serde(rename = "defaultEnvironmentVariable:view")]
    #[strum(serialize = "defaultEnvironmentVariable:view")]
    ViewProjectEnvironmentVariables,
    #[serde(rename = "deployment:create")]
    #[strum(serialize = "deployment:create")]
    CreateDeployment,
    #[serde(rename = "deployment:transfer")]
    #[strum(serialize = "deployment:transfer")]
    TransferDeployment,
    #[serde(rename = "deployment:receive")]
    #[strum(serialize = "deployment:receive")]
    ReceiveDeployment,
    #[serde(rename = "deployment:updateReference")]
    #[strum(serialize = "deployment:updateReference")]
    UpdateDeploymentReference,
    #[serde(rename = "deployment:updateDashboardEditConfirmation")]
    #[strum(serialize = "deployment:updateDashboardEditConfirmation")]
    UpdateDeploymentDashboardEditConfirmation,
    #[serde(rename = "deployment:updateExpiresAt")]
    #[strum(serialize = "deployment:updateExpiresAt")]
    UpdateDeploymentExpiresAt,
    #[serde(rename = "deployment:updateSendLogsToClient")]
    #[strum(serialize = "deployment:updateSendLogsToClient")]
    UpdateDeploymentSendLogsToClient,
    #[serde(rename = "deployment:updateClass")]
    #[strum(serialize = "deployment:updateClass")]
    UpdateDeploymentClass,
    #[serde(rename = "deployment:updateIsDefault")]
    #[strum(serialize = "deployment:updateIsDefault")]
    UpdateDeploymentIsDefault,
    #[serde(rename = "deployment:updateType")]
    #[strum(serialize = "deployment:updateType")]
    UpdateDeploymentType,
    #[serde(rename = "deployment:delete")]
    #[strum(serialize = "deployment:delete")]
    DeleteDeployment,
    #[serde(rename = "deployment:view")]
    #[strum(serialize = "deployment:view")]
    ViewDeployments,
    #[serde(rename = "deployment:integrations:view")]
    #[strum(serialize = "deployment:integrations:view")]
    ViewDeploymentIntegrations,
    #[serde(rename = "deployment:integrations:write")]
    #[strum(serialize = "deployment:integrations:write")]
    WriteDeploymentIntegrations,
    // Custom Domains
    #[serde(rename = "deployment:customDomain:create")]
    #[strum(serialize = "deployment:customDomain:create")]
    CreateCustomDomain,
    #[serde(rename = "deployment:customDomain:delete")]
    #[strum(serialize = "deployment:customDomain:delete")]
    DeleteCustomDomain,
    #[serde(rename = "deployment:customDomain:view")]
    #[strum(serialize = "deployment:customDomain:view")]
    ViewCustomDomains,
    // Members
    #[serde(rename = "member:invite")]
    #[strum(serialize = "member:invite")]
    InviteMember,
    #[serde(rename = "member:cancelInvitation")]
    #[strum(serialize = "member:cancelInvitation")]
    CancelMemberInvitation,
    #[serde(rename = "member:remove")]
    #[strum(serialize = "member:remove")]
    RemoveMember,
    #[serde(rename = "member:updateRole")]
    #[strum(serialize = "member:updateRole")]
    UpdateMemberRole,
    #[serde(rename = "member:view")]
    #[strum(serialize = "member:view")]
    ViewMembers,
    // Billing
    #[serde(rename = "billing:paymentMethod:update")]
    #[strum(serialize = "billing:paymentMethod:update")]
    UpdatePaymentMethod,
    #[serde(rename = "billing:contact:update")]
    #[strum(serialize = "billing:contact:update")]
    UpdateBillingContact,
    #[serde(rename = "billing:address:update")]
    #[strum(serialize = "billing:address:update")]
    UpdateBillingAddress,
    /// Authorizes all subscription lifecycle actions — creating, changing the
    /// plan, cancelling, and resuming. Teams using custom roles already have a
    /// paid plan, so splitting these into separate gates would only matter if
    /// a custom role intentionally allowed e.g. *changing* a plan while
    /// forbidding *cancelling* it; that's not a distinction we expose.
    #[serde(rename = "billing:subscription:changePlan")]
    #[strum(serialize = "billing:subscription:changePlan")]
    ChangeSubscriptionPlan,
    #[serde(rename = "billing:spendingLimit:update")]
    #[strum(serialize = "billing:spendingLimit:update")]
    UpdateSpendingLimit,
    #[serde(rename = "billing:view")]
    #[strum(serialize = "billing:view")]
    ViewBillingDetails,
    #[serde(rename = "billing:invoices:view")]
    #[strum(serialize = "billing:invoices:view")]
    ViewInvoices,
    // Audit Log
    #[serde(rename = "team:auditLog:view")]
    #[strum(serialize = "team:auditLog:view")]
    ViewTeamAuditLog,
    // Team Domains
    #[serde(rename = "team:domain:create")]
    #[strum(serialize = "team:domain:create")]
    CreateTeamDomain,
    #[serde(rename = "team:domain:delete")]
    #[strum(serialize = "team:domain:delete")]
    DeleteTeamDomain,
    #[serde(rename = "team:domain:view")]
    #[strum(serialize = "team:domain:view")]
    ViewTeamDomains,
    // Team Access Tokens
    #[serde(rename = "team:token:create")]
    #[strum(serialize = "team:token:create")]
    CreateTeamAccessToken,
    #[serde(rename = "team:token:update")]
    #[strum(serialize = "team:token:update")]
    UpdateTeamAccessToken,
    #[serde(rename = "team:token:delete")]
    #[strum(serialize = "team:token:delete")]
    DeleteTeamAccessToken,
    #[serde(rename = "team:token:view")]
    #[strum(serialize = "team:token:view")]
    ViewTeamAccessTokens,
    // Project Access Tokens
    #[serde(rename = "project:token:create")]
    #[strum(serialize = "project:token:create")]
    CreateProjectAccessToken,
    #[serde(rename = "project:token:update")]
    #[strum(serialize = "project:token:update")]
    UpdateProjectAccessToken,
    #[serde(rename = "project:token:delete")]
    #[strum(serialize = "project:token:delete")]
    DeleteProjectAccessToken,
    #[serde(rename = "project:token:view")]
    #[strum(serialize = "project:token:view")]
    ViewProjectAccessTokens,
    // Deployment Access Tokens
    #[serde(rename = "deployment:token:create")]
    #[strum(serialize = "deployment:token:create")]
    CreateDeploymentAccessToken,
    #[serde(rename = "deployment:token:update")]
    #[strum(serialize = "deployment:token:update")]
    UpdateDeploymentAccessToken,
    #[serde(rename = "deployment:token:delete")]
    #[strum(serialize = "deployment:token:delete")]
    DeleteDeploymentAccessToken,
    #[serde(rename = "deployment:token:view")]
    #[strum(serialize = "deployment:token:view")]
    ViewDeploymentAccessTokens,
    // OAuth Apps
    #[serde(rename = "oauthApplication:create")]
    #[strum(serialize = "oauthApplication:create")]
    CreateOAuthApplication,
    #[serde(rename = "oauthApplication:update")]
    #[strum(serialize = "oauthApplication:update")]
    UpdateOAuthApplication,
    #[serde(rename = "oauthApplication:delete")]
    #[strum(serialize = "oauthApplication:delete")]
    DeleteOAuthApplication,
    #[serde(rename = "oauthApplication:view")]
    #[strum(serialize = "oauthApplication:view")]
    ViewOAuthApplications,
    #[serde(rename = "oauthApplication:generateClientSecret")]
    #[strum(serialize = "oauthApplication:generateClientSecret")]
    GenerateOAuthClientSecret,
    // Usage
    #[serde(rename = "team:usage:view")]
    #[strum(serialize = "team:usage:view")]
    ViewUsage,
    // Insights
    #[serde(rename = "deployment:insights:view")]
    #[strum(serialize = "deployment:insights:view")]
    ViewInsights,
    // Backups
    #[serde(rename = "deployment:backups:create")]
    #[strum(serialize = "deployment:backups:create")]
    CreateBackups,
    #[serde(rename = "deployment:backups:import")]
    #[strum(serialize = "deployment:backups:import")]
    ImportBackups,
    #[serde(rename = "deployment:backups:configurePeriodic")]
    #[strum(serialize = "deployment:backups:configurePeriodic")]
    ConfigurePeriodicBackups,
    #[serde(rename = "deployment:backups:disablePeriodic")]
    #[strum(serialize = "deployment:backups:disablePeriodic")]
    DisablePeriodicBackups,
    #[serde(rename = "deployment:backups:delete")]
    #[strum(serialize = "deployment:backups:delete")]
    DeleteBackups,
    #[serde(rename = "deployment:backups:view")]
    #[strum(serialize = "deployment:backups:view")]
    ViewBackups,
    // SSO
    #[serde(rename = "sso:enable")]
    #[strum(serialize = "sso:enable")]
    EnableSSO,
    #[serde(rename = "sso:disable")]
    #[strum(serialize = "sso:disable")]
    DisableSSO,
    #[serde(rename = "sso:update")]
    #[strum(serialize = "sso:update")]
    UpdateSSO,
    #[serde(rename = "sso:view")]
    #[strum(serialize = "sso:view")]
    ViewSSO,
    // Directory Sync
    #[serde(rename = "directorySync:enable")]
    #[strum(serialize = "directorySync:enable")]
    EnableDirectorySync,
    #[serde(rename = "directorySync:disable")]
    #[strum(serialize = "directorySync:disable")]
    DisableDirectorySync,
    #[serde(rename = "directorySync:updateGroupMapping")]
    #[strum(serialize = "directorySync:updateGroupMapping")]
    UpdateDirectorySyncGroupMapping,
    #[serde(rename = "directorySync:deleteGroupMapping")]
    #[strum(serialize = "directorySync:deleteGroupMapping")]
    DeleteDirectorySyncGroupMapping,
    #[serde(rename = "directorySync:view")]
    #[strum(serialize = "directorySync:view")]
    ViewDirectorySync,
    // Custom Roles
    #[serde(rename = "customRole:view")]
    #[strum(serialize = "customRole:view")]
    ViewCustomRoles,
    // Team integrations
    #[serde(rename = "integration:view")]
    #[strum(serialize = "integration:view")]
    ViewTeamIntegrations,
    #[serde(rename = "integration:create")]
    #[strum(serialize = "integration:create")]
    CreateTeamIntegrations,
    #[serde(rename = "integration:update")]
    #[strum(serialize = "integration:update")]
    UpdateTeamIntegrations,
    #[serde(rename = "integration:delete")]
    #[strum(serialize = "integration:delete")]
    DeleteTeamIntegrations,
    // Deployment operations that mirror keybroker's `DeploymentOp`. See
    // `deployment_op_action` in `eval.rs` for the mapping.
    #[serde(rename = "deployment:deploy")]
    #[strum(serialize = "deployment:deploy")]
    Deploy,
    #[serde(rename = "deployment:env:view")]
    #[strum(serialize = "deployment:env:view")]
    ViewEnvironmentVariables,
    #[serde(rename = "deployment:env:write")]
    #[strum(serialize = "deployment:env:write")]
    WriteEnvironmentVariables,
    #[serde(rename = "deployment:pause")]
    #[strum(serialize = "deployment:pause")]
    PauseDeployment,
    #[serde(rename = "deployment:unpause")]
    #[strum(serialize = "deployment:unpause")]
    UnpauseDeployment,
    #[serde(rename = "deployment:logs:view")]
    #[strum(serialize = "deployment:logs:view")]
    ViewLogs,
    #[serde(rename = "deployment:metrics:view")]
    #[strum(serialize = "deployment:metrics:view")]
    ViewMetrics,
    #[serde(rename = "deployment:data:view")]
    #[strum(serialize = "deployment:data:view")]
    ViewData,
    #[serde(rename = "deployment:data:write")]
    #[strum(serialize = "deployment:data:write")]
    WriteData,
    #[serde(rename = "deployment:backups:download")]
    #[strum(serialize = "deployment:backups:download")]
    DownloadBackups,
    #[serde(rename = "deployment:functions:actAsUser")]
    #[strum(serialize = "deployment:functions:actAsUser")]
    ActAsUser,
    #[serde(rename = "deployment:functions:runInternalQueries")]
    #[strum(serialize = "deployment:functions:runInternalQueries")]
    RunInternalQueries,
    #[serde(rename = "deployment:functions:runInternalMutations")]
    #[strum(serialize = "deployment:functions:runInternalMutations")]
    RunInternalMutations,
    #[serde(rename = "deployment:functions:runInternalActions")]
    #[strum(serialize = "deployment:functions:runInternalActions")]
    RunInternalActions,
    #[serde(rename = "deployment:functions:runTestQuery")]
    #[strum(serialize = "deployment:functions:runTestQuery")]
    RunTestQuery,
    #[serde(rename = "deployment:auditLog:view")]
    #[strum(serialize = "deployment:auditLog:view")]
    ViewAuditLog,
    #[serde(rename = "deployment:usageLimits:view")]
    #[strum(serialize = "deployment:usageLimits:view")]
    ViewUsageLimits,
    #[serde(rename = "deployment:usageLimits:write")]
    #[strum(serialize = "deployment:usageLimits:write")]
    WriteUsageLimits,
    #[serde(rename = "deployment:usage:view")]
    #[strum(serialize = "deployment:usage:view")]
    ViewDeploymentUsage,
    /// Mint an AI gateway credential from an action running on the
    /// deployment. Only checked for identities that carry an op set: deploy
    /// keys and dashboard members.
    #[serde(rename = "deployment:aiGateway:use")]
    #[strum(serialize = "deployment:aiGateway:use")]
    UseAiGateway,
}

impl RoleStatementAction {
    pub(crate) fn resource_kind(&self) -> ResourceKind {
        use RoleStatementAction as A;
        match self {
            // Team
            A::UpdateTeam
            | A::DeleteTeam
            | A::ViewTeamAuditLog
            | A::CreateTeamDomain
            | A::DeleteTeamDomain
            | A::ViewTeamDomains
            | A::ViewUsage => ResourceKind::Team,
            // Billing
            A::UpdatePaymentMethod
            | A::UpdateBillingContact
            | A::UpdateBillingAddress
            | A::ChangeSubscriptionPlan
            | A::UpdateSpendingLimit
            | A::ViewBillingDetails
            | A::ViewInvoices => ResourceKind::Billing,
            // OAuth Applications
            A::CreateOAuthApplication
            | A::UpdateOAuthApplication
            | A::DeleteOAuthApplication
            | A::ViewOAuthApplications
            | A::GenerateOAuthClientSecret => ResourceKind::OauthApplication,
            // SSO
            A::EnableSSO | A::DisableSSO | A::UpdateSSO | A::ViewSSO => ResourceKind::Sso,
            // Directory Sync
            A::EnableDirectorySync
            | A::DisableDirectorySync
            | A::UpdateDirectorySyncGroupMapping
            | A::DeleteDirectorySyncGroupMapping
            | A::ViewDirectorySync => ResourceKind::DirectorySync,
            // Team Integrations
            A::ViewTeamIntegrations
            | A::CreateTeamIntegrations
            | A::UpdateTeamIntegrations
            | A::DeleteTeamIntegrations => ResourceKind::Integration,
            // Project
            A::CreateProject
            | A::TransferProject
            | A::ReceiveProject
            | A::UpdateProject
            | A::DeleteProject
            | A::ViewProjects
            | A::UpdateMemberProjectRole => ResourceKind::Project,
            // Default (project-scoped) Environment Variables
            A::CreateProjectEnvironmentVariable
            | A::UpdateProjectEnvironmentVariable
            | A::DeleteProjectEnvironmentVariable
            | A::ViewProjectEnvironmentVariables => ResourceKind::DefaultEnvironmentVariable,
            // Deployment
            A::CreateDeployment
            | A::ReceiveDeployment
            | A::TransferDeployment
            | A::UpdateDeploymentReference
            | A::UpdateDeploymentDashboardEditConfirmation
            | A::UpdateDeploymentExpiresAt
            | A::UpdateDeploymentSendLogsToClient
            | A::UpdateDeploymentClass
            | A::UpdateDeploymentIsDefault
            | A::UpdateDeploymentType
            | A::DeleteDeployment
            | A::ViewDeployments
            | A::ViewDeploymentIntegrations
            | A::WriteDeploymentIntegrations
            | A::CreateCustomDomain
            | A::DeleteCustomDomain
            | A::ViewCustomDomains
            | A::ViewInsights
            | A::CreateBackups
            | A::ImportBackups
            | A::ConfigurePeriodicBackups
            | A::DisablePeriodicBackups
            | A::DeleteBackups
            | A::ViewBackups
            | A::Deploy
            | A::ViewEnvironmentVariables
            | A::WriteEnvironmentVariables
            | A::PauseDeployment
            | A::UnpauseDeployment
            | A::ViewLogs
            | A::ViewMetrics
            | A::ViewData
            | A::WriteData
            | A::DownloadBackups
            | A::ActAsUser
            | A::RunInternalQueries
            | A::RunInternalMutations
            | A::RunInternalActions
            | A::RunTestQuery
            | A::ViewAuditLog
            | A::ViewUsageLimits
            | A::WriteUsageLimits
            | A::ViewDeploymentUsage
            | A::UseAiGateway => ResourceKind::Deployment,
            // Member
            A::InviteMember
            | A::CancelMemberInvitation
            | A::RemoveMember
            | A::UpdateMemberRole
            | A::ViewMembers => ResourceKind::Member,
            // Custom Role
            A::ViewCustomRoles => ResourceKind::CustomRole,
            // Token
            A::CreateTeamAccessToken
            | A::UpdateTeamAccessToken
            | A::DeleteTeamAccessToken
            | A::ViewTeamAccessTokens
            | A::CreateProjectAccessToken
            | A::UpdateProjectAccessToken
            | A::DeleteProjectAccessToken
            | A::ViewProjectAccessTokens
            | A::CreateDeploymentAccessToken
            | A::UpdateDeploymentAccessToken
            | A::DeleteDeploymentAccessToken
            | A::ViewDeploymentAccessTokens => ResourceKind::Token,
        }
    }

    /// For token actions, the resource kind that owns the token — `Team`,
    /// `Project`, or `Deployment`. Returns `None` for non-token actions.
    /// Used by [`RoleStatement::validate`] to require that a token-scoped
    /// statement's parent segment matches the action's owner.
    pub(crate) fn token_scope(&self) -> Option<ResourceKind> {
        use RoleStatementAction as A;
        match self {
            A::CreateTeamAccessToken
            | A::UpdateTeamAccessToken
            | A::DeleteTeamAccessToken
            | A::ViewTeamAccessTokens => Some(ResourceKind::Team),
            A::CreateProjectAccessToken
            | A::UpdateProjectAccessToken
            | A::DeleteProjectAccessToken
            | A::ViewProjectAccessTokens => Some(ResourceKind::Project),
            A::CreateDeploymentAccessToken
            | A::UpdateDeploymentAccessToken
            | A::DeleteDeploymentAccessToken
            | A::ViewDeploymentAccessTokens => Some(ResourceKind::Deployment),
            _ => None,
        }
    }

    pub fn is_component_scopable(&self) -> bool {
        use RoleStatementAction as A;
        matches!(
            self,
            A::ViewData
                | A::WriteData
                | A::RunInternalQueries
                | A::RunInternalMutations
                | A::RunInternalActions
        )
    }
}

/// A parsed resource specifier describing which resources a rule applies to.
///
/// Wire format is a string like `"project:*"` or
/// `"project:*:deployment:type=prod"`, parsed via the `FromStr` impl.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceSpecifier {
    pub(crate) segments: Vec<ResourceSegment>,
}

impl Serialize for ResourceSpecifier {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ResourceSpecifier {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(D::Error::custom)
    }
}

/// Whether a rule grants or revokes access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum RoleStatementEffect {
    Allow,
    Deny,
}

/// An action pattern: either a wildcard matching all actions, or a list of
/// specific actions.
///
/// Wire format is either the string `"*"` (wildcard) or an array of actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionPattern {
    Wildcard,
    Specific(Vec<RoleStatementAction>),
}

impl Serialize for ActionPattern {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            ActionPattern::Wildcard => serializer.serialize_str("*"),
            ActionPattern::Specific(actions) => actions.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ActionPattern {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Peek at the JSON shape so that we can dispatch to the right
        // deserializer and surface its specific error (e.g. "unknown variant
        // `bogusAction`, expected one of ..."). An untagged enum here would
        // collapse those errors into the unhelpful "data did not match any
        // variant of untagged enum".
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(s) if s == "*" => Ok(ActionPattern::Wildcard),
            serde_json::Value::String(s) => Err(D::Error::custom(format!(
                "Invalid action pattern: \"{s}\" (expected \"*\" or an array of actions)",
            ))),
            serde_json::Value::Array(_) => {
                let actions: Vec<RoleStatementAction> =
                    serde_json::from_value(value).map_err(D::Error::custom)?;
                Ok(ActionPattern::Specific(actions))
            },
            other => Err(D::Error::custom(format!(
                "Invalid action pattern: {other} (expected \"*\" or an array of actions)",
            ))),
        }
    }
}

#[derive(Serialize, Deserialize, ToSchema)]
#[allow(dead_code)]
enum RoleStatementWildcardAction {
    #[serde(rename = "*")]
    All,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum RoleStatementActions {
    Wildcard(RoleStatementWildcardAction),
    Specific(Vec<RoleStatementAction>),
}

/// A single permission rule within a custom role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RoleStatement {
    pub effect: RoleStatementEffect,
    #[schema(value_type = RoleStatementActions)]
    pub actions: ActionPattern,
    /// Resource path like `project:*`, `project:slug=my-app`, or
    /// `project:*:deployment:type=prod`.
    #[schema(value_type = String, example = "project:*")]
    pub resource: ResourceSpecifier,
}

impl RoleStatement {
    pub(crate) fn leaf_kind(&self) -> Option<ResourceKind> {
        self.resource.segments.last().map(|s| s.kind())
    }

    /// Whether this statement's action pattern applies to `action`, taking
    /// into account that on a `component` leaf the wildcard spans only the
    /// component-scopable actions.
    pub(crate) fn covers_action(&self, action: RoleStatementAction) -> bool {
        let pattern_match = match &self.actions {
            ActionPattern::Wildcard => true,
            ActionPattern::Specific(actions) => actions.contains(&action),
        };
        pattern_match
            && (self.leaf_kind() != Some(ResourceKind::Component) || action.is_component_scopable())
    }

    /// Validates that every action in this rule targets the same resource kind
    /// as the leaf segment of the resource specifier. For token actions, also
    /// requires the parent segment kind to match the action's token scope so
    /// e.g. `team:*:token:*` rejects `createProjectAccessToken`. A `component`
    /// leaf is the one place an action may target a *parent* kind: it accepts
    /// exactly the component-scopable deployment actions.
    pub fn validate(&self) -> anyhow::Result<()> {
        let leaf_kind = self
            .resource
            .segments
            .last()
            .map(|s| s.kind())
            .ok_or_else(|| anyhow::anyhow!("RoleStatement has empty resource specifier"))?;
        let parent_kind = if leaf_kind == ResourceKind::Token {
            // Parser guarantees a token segment has a parent (team / project /
            // deployment); treat its absence as a logic error.
            Some(
                self.resource
                    .segments
                    .iter()
                    .rev()
                    .nth(1)
                    .map(|s| s.kind())
                    .ok_or_else(|| {
                        anyhow::anyhow!("Token resource specifier missing parent segment")
                    })?,
            )
        } else {
            None
        };
        if let ActionPattern::Specific(actions) = &self.actions {
            for (i, action) in actions.iter().enumerate() {
                if actions[..i].contains(action) {
                    anyhow::bail!("Duplicate action {action} in statement");
                }
                let action_kind = action.resource_kind();
                if leaf_kind == ResourceKind::Component {
                    if !action.is_component_scopable() {
                        anyhow::bail!(
                            "Action {action} cannot be scoped to a component; a statement \
                             granting it must target a deployment"
                        );
                    }
                } else if action_kind != leaf_kind {
                    anyhow::bail!(
                        "Action {action} targets {action_kind} resources, but statement resource \
                         specifier targets {leaf_kind}"
                    );
                }
                if let (Some(parent_kind), Some(action_scope)) = (parent_kind, action.token_scope())
                    && parent_kind != action_scope
                {
                    anyhow::bail!(
                        "Action {action} targets {action_scope} tokens, but statement resource \
                         specifier nests the token under {parent_kind}"
                    );
                }
            }
        }
        Ok(())
    }
}

/// A custom role definition containing an ordered list of statements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomRole {
    pub name: String,
    pub description: Option<String>,
    pub statements: Vec<RoleStatement>,
}

/// Minimal representation of a project needed for custom role enforcement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConcreteProject {
    pub id: ProjectId,
    pub slug: ProjectSlug,
    pub name: ProjectName,
}

/// Minimal representation of a deployment needed for custom role enforcement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConcreteDeployment {
    pub id: DeploymentId,
    pub deployment_type: DeploymentType,
    pub creator: Option<MemberId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConcreteComponent {
    pub path: String,
}

/// Minimal representation of an access token needed for custom role
/// enforcement. Carries only the fields token selectors read so callers
/// don't have to materialize a full access token just to evaluate a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConcreteToken {
    pub creator: Option<MemberId>,
}

/// A concrete resource segment representing an actual resource being accessed.
/// Each variant wraps only the data the corresponding selectors read:
/// `Project`/`Deployment` use the slim
/// [`ConcreteProject`]/[`ConcreteDeployment`] projections, and singleton-shaped
/// segments (`Team`, `Member`, etc.) carry no payload because their
/// `ResourceSegment` counterparts have no selectors.
#[derive(Debug, Clone)]
pub enum ConcreteSegment {
    Team,
    Project(ConcreteProject),
    /// A project that does not yet exist (the create-project authorization
    /// path). `ProjectSelector::Id` cannot match — there is no id — but
    /// `Slug` and `Any` selectors evaluate against the proposed slug.
    ProposedProject {
        slug: ProjectSlug,
    },
    Deployment(ConcreteDeployment),
    /// A deployment that does not yet exist (the create-deployment and
    /// receive-deployment authorization paths). `DeploymentSelector::Id`
    /// cannot match; `Type`, `Creator`, and `Any` evaluate against the
    /// proposed fields.
    ProposedDeployment {
        deployment_type: DeploymentType,
        creator: Option<MemberId>,
    },
    /// An existing local deployment. For role matching it behaves as a Dev
    /// deployment, has no cloud deployment id, and is created by `owner`.
    LocalDeployment {
        owner: MemberId,
    },
    Component(ConcreteComponent),
    Member,
    Token(ConcreteToken),
    CustomRole,
    Billing,
    OauthApplication,
    Sso,
    DirectorySync,
    Integration,
    DefaultEnvironmentVariable,
}

impl ConcreteSegment {
    pub fn kind(&self) -> ResourceKind {
        match self {
            ConcreteSegment::Team => ResourceKind::Team,
            ConcreteSegment::Project(_) | ConcreteSegment::ProposedProject { .. } => {
                ResourceKind::Project
            },
            ConcreteSegment::Deployment(_)
            | ConcreteSegment::ProposedDeployment { .. }
            | ConcreteSegment::LocalDeployment { .. } => ResourceKind::Deployment,
            ConcreteSegment::Component(_) => ResourceKind::Component,
            ConcreteSegment::Member => ResourceKind::Member,
            ConcreteSegment::Token(_) => ResourceKind::Token,
            ConcreteSegment::CustomRole => ResourceKind::CustomRole,
            ConcreteSegment::Billing => ResourceKind::Billing,
            ConcreteSegment::OauthApplication => ResourceKind::OauthApplication,
            ConcreteSegment::Sso => ResourceKind::Sso,
            ConcreteSegment::DirectorySync => ResourceKind::DirectorySync,
            ConcreteSegment::Integration => ResourceKind::Integration,
            ConcreteSegment::DefaultEnvironmentVariable => ResourceKind::DefaultEnvironmentVariable,
        }
    }

    /// Returns the segment's selectable attributes formatted as
    /// `key=value, ...` if the segment carries any, or `None` for
    /// attribute-less singletons (Team, Member, Billing, etc.).
    fn describe_attributes(&self) -> Option<String> {
        match self {
            ConcreteSegment::Project(p) => Some(format!(
                "id={}, name={:?}, slug={:?}",
                p.id,
                p.name.as_str(),
                p.slug.as_str(),
            )),
            ConcreteSegment::ProposedProject { slug } => Some(format!("slug={:?}", slug.as_str())),
            ConcreteSegment::Deployment(d) => {
                let creator = match d.creator {
                    Some(m) => m.to_string(),
                    None => "none".to_string(),
                };
                Some(format!(
                    "id={}, type={}, creator={creator}",
                    d.id, d.deployment_type,
                ))
            },
            ConcreteSegment::ProposedDeployment {
                deployment_type,
                creator,
            } => {
                let creator = match creator {
                    Some(m) => m.to_string(),
                    None => "none".to_string(),
                };
                Some(format!("type={deployment_type}, creator={creator}"))
            },
            ConcreteSegment::LocalDeployment { owner } => {
                Some(format!("type=dev, creator={owner}"))
            },
            // Render the root app component as `/`, matching how it is
            // written in a selector (`path=/`).
            ConcreteSegment::Component(c) if c.path.is_empty() => Some("path=/".to_string()),
            ConcreteSegment::Component(c) => Some(format!("path={}", c.path)),
            ConcreteSegment::Token(t) => {
                let creator = match t.creator {
                    Some(m) => m.to_string(),
                    None => "none".to_string(),
                };
                Some(format!("creator={creator}"))
            },
            ConcreteSegment::Team
            | ConcreteSegment::Member
            | ConcreteSegment::CustomRole
            | ConcreteSegment::Billing
            | ConcreteSegment::OauthApplication
            | ConcreteSegment::Sso
            | ConcreteSegment::DirectorySync
            | ConcreteSegment::Integration
            | ConcreteSegment::DefaultEnvironmentVariable => None,
        }
    }
}

/// A concrete resource path describing the actual resource being accessed.
#[derive(Debug, Clone)]
pub struct ConcreteResource {
    pub segments: Vec<ConcreteSegment>,
}

impl ConcreteResource {
    /// Returns a `kind(attr=val, ...) > kind(...)` description of the
    /// resource if any segment carries selectable attributes; otherwise
    /// `None`. Used to enrich custom-role denial error messages so the
    /// caller can see exactly which resource attributes were evaluated.
    pub fn describe_attributes(&self) -> Option<String> {
        let parts: Vec<String> = self
            .segments
            .iter()
            .filter_map(|seg| {
                let attrs = seg.describe_attributes()?;
                Some(format!("{}({attrs})", seg.kind()))
            })
            .collect();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" > "))
        }
    }
}
