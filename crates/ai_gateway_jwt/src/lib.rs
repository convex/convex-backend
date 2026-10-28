//! Claims and validation policy for AI gateway credentials. Conductor signs for
//! cloud deployments; Big Brain signs for local deployments. Identity and
//! attribution come from trusted state, and the gateway receives public keys.
//! The underlying RS256 implementation lives in [`jwt`].

use biscuit::{
    jwk::{
        JWKSet,
        JWK,
    },
    ClaimPresenceOptions,
    ClaimsSet,
    Empty,
    Presence,
    RegisteredClaims,
    SingleOrMultiple,
    TemporalOptions,
    Validation,
    ValidationOptions,
};
use chrono::{
    DateTime,
    Duration,
    Utc,
};
use common::{
    execution_context::RequestId,
    types::{
        AttributionClaims,
        DeploymentMetadata,
        MemberId,
        ProjectId,
        TeamId,
    },
};
use serde::{
    Deserialize,
    Serialize,
};

mod jwt;

pub use crate::jwt::{
    unverified_expiration,
    Jwt,
    JwtError,
    JwtSigner,
    JwtVerifier,
    MAX_JWT_SIZE,
};

/// Issuers separate cloud deployment claims from local project-owned claims.
pub const AI_GATEWAY_JWT_ISSUER: &str = "convex-cloud";
pub const LOCAL_AI_GATEWAY_JWT_ISSUER: &str = "convex-local";
/// A service audience keeps tokens valid across gateway hostname changes.
pub const AI_GATEWAY_JWT_AUDIENCE: &str = "ai";
pub const AI_GATEWAY_JWT_VERSION: u16 = 1;
pub const AI_GATEWAY_JWT_LIFETIME: Duration = Duration::minutes(5);
pub const AI_GATEWAY_JWT_CLOCK_SKEW: Duration = Duration::seconds(5);

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AiGatewayJwtClaims {
    /// Allows verifiers to reject incompatible claim semantics during rollout.
    pub version: u16,
    /// Strings allow the gateway to accept new region and deployment-class
    /// names.
    #[serde(
        rename = "convex.region",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub region: Option<String>,
    #[serde(
        rename = "convex.deploymentClass",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub deployment_class: Option<String>,
    /// Local JWTs carry project and team as signed billing identity because
    /// `AttributionClaims` only describes the calling function or component.
    /// The local verifier requires both fields. memberId points to the local
    /// user that is calling the AI gateway.
    #[serde(
        rename = "convex.projectId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub project_id: Option<ProjectId>,
    #[serde(
        rename = "convex.teamId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub team_id: Option<TeamId>,
    #[serde(
        rename = "convex.memberId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub member_id: Option<MemberId>,
    /// The team and project that own the deployment, for any consumer.
    /// Stable and unique, but opaque: don't parse them. Both or neither.
    /// Optional so gateways can be deployed before every token minter.
    #[serde(
        rename = "convex.team",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub team: Option<String>,
    #[serde(
        rename = "convex.project",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub project: Option<String>,
    #[serde(
        rename = "convex.componentPath",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub component_path: Option<String>,
    #[serde(
        rename = "convex.functionName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub function_name: Option<String>,
    /// Whether `convex.functionName` is a function path or an HTTP route.
    /// Present when `convex.functionName` is.
    #[serde(
        rename = "convex.functionType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub function_type: Option<String>,
    /// Signed association with the backend request. Optional so gateways can
    /// be deployed before every token minter during a rolling release.
    #[serde(
        rename = "convex.requestId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub request_id: Option<String>,
}

/// A caller the gateway has authenticated, and what it may be billed for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedDeployment {
    /// The billing identity, proven by signature. Usage rows key on it.
    instance_name: String,
    region: Option<String>,
    deployment_class: Option<String>,
    attribution: AttributionClaims,
    member_id: Option<MemberId>,
    /// The opaque `convex.team` and `convex.project` claims.
    team: Option<String>,
    project: Option<String>,
    /// The verifier converts the issuer-specific wire claims above into this
    /// validated billing identity, eliminating partial project/team states.
    usage_owner: UsageOwner,
    request_id: Option<String>,
}

/// Who owns usage after the JWT's issuer-specific claims have been validated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageOwner {
    Deployment,
    Project {
        project_id: ProjectId,
        team_id: TeamId,
    },
}

impl AuthenticatedDeployment {
    pub fn instance_name(&self) -> &str {
        &self.instance_name
    }

    pub fn region(&self) -> Option<&str> {
        self.region.as_deref()
    }

    pub fn deployment_class(&self) -> Option<&str> {
        self.deployment_class.as_deref()
    }

    pub fn attribution(&self) -> &AttributionClaims {
        &self.attribution
    }

    pub fn member_id(&self) -> Option<MemberId> {
        self.member_id
    }

    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    pub fn team(&self) -> Option<&str> {
        self.team.as_deref()
    }

    pub fn project(&self) -> Option<&str> {
        self.project.as_deref()
    }

    pub fn usage_owner(&self) -> UsageOwner {
        self.usage_owner
    }
}

fn gateway_claims(
    issuer: &str,
    subject: &str,
    custom_claims: AiGatewayJwtClaims,
    now: DateTime<Utc>,
) -> ClaimsSet<AiGatewayJwtClaims> {
    ClaimsSet {
        registered: RegisteredClaims {
            issuer: Some(issuer.to_owned()),
            subject: Some(subject.to_owned()),
            audience: Some(SingleOrMultiple::Single(AI_GATEWAY_JWT_AUDIENCE.to_owned())),
            expiry: Some((now + AI_GATEWAY_JWT_LIFETIME).into()),
            issued_at: Some(now.into()),
            ..Default::default()
        },
        private: custom_claims,
    }
}

fn gateway_validation(now: DateTime<Utc>, issuer: &str) -> ValidationOptions {
    ValidationOptions {
        claim_presence_options: ClaimPresenceOptions {
            issued_at: Presence::Required,
            expiry: Presence::Required,
            issuer: Presence::Required,
            audience: Presence::Required,
            subject: Presence::Required,
            ..Default::default()
        },
        temporal_options: TemporalOptions {
            now: Some(now),
            epsilon: AI_GATEWAY_JWT_CLOCK_SKEW,
        },
        issuer: Validation::Validate(issuer.to_owned()),
        audience: Validation::Validate(AI_GATEWAY_JWT_AUDIENCE.to_owned()),
        // Token age is bounded by the required `exp` plus the lifetime check
        // below. Biscuit's default `iat` validation stays on so the expiry
        // keeps its clock-skew allowance.
        ..Default::default()
    }
}

fn verify_claims(
    verifier: &JwtVerifier,
    token: &str,
    now: DateTime<Utc>,
    issuer: &str,
) -> Result<ClaimsSet<AiGatewayJwtClaims>, JwtError> {
    let claims: ClaimsSet<AiGatewayJwtClaims> =
        verifier.verify(token, gateway_validation(now, issuer))?;

    if claims.registered.audience.as_ref()
        != Some(&SingleOrMultiple::Single(
            AI_GATEWAY_JWT_AUDIENCE.to_owned(),
        ))
        || claims.private.version != AI_GATEWAY_JWT_VERSION
    {
        return Err(JwtError::InvalidToken);
    }

    let issued_at: DateTime<Utc> = claims
        .registered
        .issued_at
        .ok_or(JwtError::InvalidToken)?
        .into();
    let expires_at: DateTime<Utc> = claims
        .registered
        .expiry
        .ok_or(JwtError::InvalidToken)?
        .into();
    let token_lifetime = expires_at.signed_duration_since(issued_at);
    if token_lifetime <= Duration::zero() || token_lifetime > AI_GATEWAY_JWT_LIFETIME {
        return Err(JwtError::InvalidToken);
    }
    Ok(claims)
}

/// The one place that turns real IDs into the opaque `convex.team` and
/// `convex.project` claims, so the encoding can change without touching
/// signers' callers or consumers.
fn team_and_project_claims(team_id: TeamId, project_id: ProjectId) -> (String, String) {
    (team_id.0.to_string(), project_id.0.to_string())
}

fn has_team_and_project_pair(claims: &AiGatewayJwtClaims) -> bool {
    claims.team.is_some() == claims.project.is_some()
}

/// Signs the gateway's contract with a Convex-held key.
#[derive(Clone, PartialEq)]
pub struct AiGatewayJwtSigner(pub(crate) JwtSigner);

impl AiGatewayJwtSigner {
    pub fn new(private_jwk: JWK<Empty>) -> anyhow::Result<Self> {
        Ok(Self(JwtSigner::new(private_jwk)?))
    }

    /// Signs a token for a deployment already authenticated by Conductor.
    ///
    /// `deployment` and `attribution` must both come from trusted backend
    /// state.
    pub fn sign(
        &self,
        deployment: &DeploymentMetadata,
        attribution: AttributionClaims,
        request_id: &RequestId,
        now: DateTime<Utc>,
    ) -> Result<Jwt, JwtError> {
        self.sign_with_team_and_project(deployment, None, None, attribution, request_id, now)
    }

    /// Like [`Self::sign`], but also signs the team and project that own the
    /// deployment, which must come from trusted backend state. Pass both or
    /// neither. Usage is still billed to the deployment.
    pub fn sign_with_team_and_project(
        &self,
        deployment: &DeploymentMetadata,
        team_id: Option<TeamId>,
        project_id: Option<ProjectId>,
        attribution: AttributionClaims,
        request_id: &RequestId,
        now: DateTime<Utc>,
    ) -> Result<Jwt, JwtError> {
        let (team, project) = match (team_id, project_id) {
            (Some(team_id), Some(project_id)) => {
                let (team, project) = team_and_project_claims(team_id, project_id);
                (Some(team), Some(project))
            },
            (None, None) => (None, None),
            _ => return Err(JwtError::SigningFailed),
        };
        let AttributionClaims {
            component_path,
            function_name,
            function_type,
        } = attribution;
        self.0.sign(&gateway_claims(
            AI_GATEWAY_JWT_ISSUER,
            &deployment.name,
            AiGatewayJwtClaims {
                version: AI_GATEWAY_JWT_VERSION,
                region: deployment.region.as_ref().map(ToString::to_string),
                deployment_class: Some(deployment.class.to_string()),
                request_id: Some(request_id.to_string()),
                team,
                project,
                component_path,
                function_name,
                function_type,
                ..Default::default()
            },
            now,
        ))
    }
}

#[derive(Clone, PartialEq)]
pub struct LocalAiGatewayJwtSigner(pub(crate) JwtSigner);

impl LocalAiGatewayJwtSigner {
    pub fn new(private_jwk: JWK<Empty>) -> anyhow::Result<Self> {
        Ok(Self(JwtSigner::new(private_jwk)?))
    }

    /// Signs identity and attribution after Big Brain has authenticated the
    /// access token and authorized the local deployment.
    pub fn sign(
        &self,
        instance_name: &str,
        project_id: ProjectId,
        team_id: TeamId,
        member_id: MemberId,
        attribution: AttributionClaims,
        now: DateTime<Utc>,
    ) -> Result<Jwt, JwtError> {
        let AttributionClaims {
            component_path,
            function_name,
            function_type,
        } = attribution;
        let (team, project) = team_and_project_claims(team_id, project_id);
        self.0.sign(&gateway_claims(
            LOCAL_AI_GATEWAY_JWT_ISSUER,
            instance_name,
            AiGatewayJwtClaims {
                version: AI_GATEWAY_JWT_VERSION,
                project_id: Some(project_id),
                team_id: Some(team_id),
                member_id: Some(member_id),
                team: Some(team),
                project: Some(project),
                component_path,
                function_name,
                function_type,
                ..Default::default()
            },
            now,
        ))
    }
}

/// Accepts only tokens that satisfy the gateway's contract.
#[derive(Clone, PartialEq)]
pub struct AiGatewayJwtVerifier(JwtVerifier);

impl AiGatewayJwtVerifier {
    pub fn new(public_keys: JWKSet<Empty>) -> anyhow::Result<Self> {
        Ok(Self(JwtVerifier::new(public_keys)?))
    }

    pub fn verify(
        &self,
        token: &str,
        now: DateTime<Utc>,
    ) -> Result<AuthenticatedDeployment, JwtError> {
        let claims = verify_claims(&self.0, token, now, AI_GATEWAY_JWT_ISSUER)?;
        if claims.private.project_id.is_some()
            || claims.private.team_id.is_some()
            || claims.private.member_id.is_some()
            || !has_team_and_project_pair(&claims.private)
        {
            return Err(JwtError::InvalidToken);
        }
        let instance_name = claims.registered.subject.ok_or(JwtError::InvalidToken)?;
        let attribution = AttributionClaims {
            component_path: claims.private.component_path,
            function_name: claims.private.function_name,
            function_type: claims.private.function_type,
        };
        attribution.validate().map_err(|_| JwtError::InvalidToken)?;
        Ok(AuthenticatedDeployment {
            instance_name,
            region: claims.private.region,
            deployment_class: claims.private.deployment_class,
            attribution,
            member_id: None,
            team: claims.private.team,
            project: claims.private.project,
            usage_owner: UsageOwner::Deployment,
            request_id: claims.private.request_id,
        })
    }
}

#[derive(Clone, PartialEq)]
pub struct LocalAiGatewayJwtVerifier(JwtVerifier);

impl LocalAiGatewayJwtVerifier {
    pub fn new(public_keys: JWKSet<Empty>) -> anyhow::Result<Self> {
        Ok(Self(JwtVerifier::new(public_keys)?))
    }

    pub fn verify(
        &self,
        token: &str,
        now: DateTime<Utc>,
    ) -> Result<AuthenticatedDeployment, JwtError> {
        let claims = verify_claims(&self.0, token, now, LOCAL_AI_GATEWAY_JWT_ISSUER)?;
        if claims.private.region.is_some()
            || claims.private.deployment_class.is_some()
            || !has_team_and_project_pair(&claims.private)
        {
            return Err(JwtError::InvalidToken);
        }
        let instance_name = claims.registered.subject.ok_or(JwtError::InvalidToken)?;
        let project_id = claims.private.project_id.ok_or(JwtError::InvalidToken)?;
        let team_id = claims.private.team_id.ok_or(JwtError::InvalidToken)?;
        let member_id = claims.private.member_id;
        let attribution = AttributionClaims {
            component_path: claims.private.component_path,
            function_name: claims.private.function_name,
            function_type: claims.private.function_type,
        };
        attribution.validate().map_err(|_| JwtError::InvalidToken)?;
        Ok(AuthenticatedDeployment {
            instance_name,
            region: None,
            deployment_class: None,
            request_id: None,
            attribution,
            member_id,
            team: claims.private.team,
            project: claims.private.project,
            usage_owner: UsageOwner::Project {
                project_id,
                team_id,
            },
        })
    }
}
