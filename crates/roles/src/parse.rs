use std::{
    fmt,
    str::FromStr,
};

use anyhow::bail;
use big_brain_private_api_types::{
    validate_slug,
    ProjectSlug,
};
use common::types::{
    DeploymentId,
    DeploymentType,
    MemberId,
    ProjectId,
};

use super::types::{
    ComponentSelector,
    CreatorMatcher,
    DeploymentSelector,
    ProjectSelector,
    ResourceKind,
    ResourceSegment,
    ResourceSpecifier,
    TokenSelector,
};

fn parse_project_selector(s: &str) -> anyhow::Result<ProjectSelector> {
    if s == "*" {
        return Ok(ProjectSelector::Any);
    }
    if let Some(id_str) = s.strip_prefix("id=") {
        let id: ProjectId = id_str
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid id value: {id_str}"))?;
        return Ok(ProjectSelector::Id(id));
    }
    if let Some(slug) = s.strip_prefix("slug=") {
        validate_slug(slug).map_err(|e| anyhow::anyhow!("Invalid slug in selector: {e}"))?;
        return Ok(ProjectSelector::Slug(ProjectSlug::from(slug.to_string())));
    }
    bail!("Invalid selector for project resource: {s} (valid: *, id=value, slug=value)")
}

fn parse_creator_matcher(value: &str) -> anyhow::Result<CreatorMatcher> {
    if value == "self" {
        return Ok(CreatorMatcher::SelfActor);
    }
    let id: u64 = value
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid creator value: {value} (expected self or id)"))?;
    Ok(CreatorMatcher::Member(MemberId(id)))
}

fn parse_deployment_selector(s: &str) -> anyhow::Result<DeploymentSelector> {
    if s == "*" {
        return Ok(DeploymentSelector::Any);
    }
    if let Some(id_str) = s.strip_prefix("id=") {
        let id: DeploymentId = id_str
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid id value: {id_str}"))?;
        return Ok(DeploymentSelector::Id(id));
    }
    if let Some(t) = s.strip_prefix("type=") {
        let deployment_type: DeploymentType = t
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid deployment type: {t}"))?;
        return Ok(DeploymentSelector::Type(deployment_type));
    }
    if let Some(creator) = s.strip_prefix("creator=") {
        return Ok(DeploymentSelector::Creator(parse_creator_matcher(creator)?));
    }
    bail!(
        "Invalid selector for deployment resource: {s} (valid: *, id=value, type=value, \
         creator=id, creator=self)"
    )
}

fn parse_token_selector(s: &str) -> anyhow::Result<TokenSelector> {
    if s == "*" {
        return Ok(TokenSelector::Any);
    }
    if let Some(creator) = s.strip_prefix("creator=") {
        return Ok(TokenSelector::Creator(parse_creator_matcher(creator)?));
    }
    bail!("Invalid selector for token resource: {s} (valid: *, creator=id, creator=self)")
}

/// A leading `/` on a written component path is optional and ignored, so
/// `path=/foo` and `path=foo` are equivalent.
fn strip_leading_slash(value: &str) -> &str {
    value.strip_prefix('/').unwrap_or(value)
}

fn parse_component_selector(s: &str) -> anyhow::Result<ComponentSelector> {
    if s == "*" {
        return Ok(ComponentSelector::Any);
    }
    if let Some(value) = s.strip_prefix("path=") {
        // A trailing `*` makes it a (literal) prefix match; otherwise it's an
        // exact path. The component path is matched as a string, so the prefix
        // is stored verbatim (a trailing `/` in the prefix gives path-boundary
        // matching — see `ComponentSelector::PathStartsWith`).
        if let Some(prefix) = value.strip_suffix('*') {
            return Ok(ComponentSelector::PathStartsWith(
                strip_leading_slash(prefix).to_string(),
            ));
        }
        if value == "/" {
            return Ok(ComponentSelector::Path(String::new()));
        }
        if value.is_empty() {
            bail!("Component path cannot be empty; use `path=/` for the root app component");
        }
        return Ok(ComponentSelector::Path(
            strip_leading_slash(value).to_string(),
        ));
    }
    bail!(
        "Invalid selector for component resource: {s} (valid: *, path=/ for the root app, \
         path=foo/bar, path=foo/bar/*, path=foo*)"
    )
}

fn parse_segment(kind: ResourceKind, selector_str: &str) -> anyhow::Result<ResourceSegment> {
    let parts: Vec<&str> = selector_str.split(',').map(|p| p.trim()).collect();
    match kind {
        ResourceKind::Team => {
            if parts != ["*"] {
                bail!("Team segment only accepts wildcard (*), got: {selector_str}");
            }
            Ok(ResourceSegment::Team)
        },
        ResourceKind::Project => {
            let selectors: Vec<ProjectSelector> = parts
                .iter()
                .map(|s| parse_project_selector(s))
                .collect::<anyhow::Result<_>>()?;
            Ok(ResourceSegment::Project(selectors))
        },
        ResourceKind::Deployment => {
            let selectors: Vec<DeploymentSelector> = parts
                .iter()
                .map(|s| parse_deployment_selector(s))
                .collect::<anyhow::Result<_>>()?;
            Ok(ResourceSegment::Deployment(selectors))
        },
        ResourceKind::Component => {
            let selectors: Vec<ComponentSelector> = parts
                .iter()
                .map(|s| parse_component_selector(s))
                .collect::<anyhow::Result<_>>()?;
            Ok(ResourceSegment::Component(selectors))
        },
        ResourceKind::Member => {
            if parts != ["*"] {
                bail!("Member segment only accepts wildcard (*), got: {selector_str}");
            }
            Ok(ResourceSegment::Member)
        },
        ResourceKind::Token => {
            let selectors: Vec<TokenSelector> = parts
                .iter()
                .map(|s| parse_token_selector(s))
                .collect::<anyhow::Result<_>>()?;
            Ok(ResourceSegment::Token(selectors))
        },
        ResourceKind::CustomRole => {
            if parts != ["*"] {
                bail!("CustomRole segment only accepts wildcard (*), got: {selector_str}");
            }
            Ok(ResourceSegment::CustomRole)
        },
        ResourceKind::Billing => {
            if parts != ["*"] {
                bail!("Billing segment only accepts wildcard (*), got: {selector_str}");
            }
            Ok(ResourceSegment::Billing)
        },
        ResourceKind::OauthApplication => {
            if parts != ["*"] {
                bail!("OauthApplication segment only accepts wildcard (*), got: {selector_str}");
            }
            Ok(ResourceSegment::OauthApplication)
        },
        ResourceKind::Sso => {
            if parts != ["*"] {
                bail!("Sso segment only accepts wildcard (*), got: {selector_str}");
            }
            Ok(ResourceSegment::Sso)
        },
        ResourceKind::DirectorySync => {
            if parts != ["*"] {
                bail!("DirectorySync segment only accepts wildcard (*), got: {selector_str}");
            }
            Ok(ResourceSegment::DirectorySync)
        },
        ResourceKind::Integration => {
            if parts != ["*"] {
                bail!("Integration segment only accepts wildcard (*), got: {selector_str}");
            }
            Ok(ResourceSegment::Integration)
        },
        ResourceKind::DefaultEnvironmentVariable => {
            if parts != ["*"] {
                bail!(
                    "DefaultEnvironmentVariable segment only accepts wildcard (*), got: \
                     {selector_str}"
                );
            }
            Ok(ResourceSegment::DefaultEnvironmentVariable)
        },
    }
}

/// Returns the valid child resource kinds for a given parent kind. Deployment
/// is always nested under project; tokens nest under their owning resource
/// (team, project, or deployment).
fn valid_children(kind: ResourceKind) -> &'static [ResourceKind] {
    match kind {
        ResourceKind::Team => &[ResourceKind::Token],
        ResourceKind::Project => &[
            ResourceKind::Deployment,
            ResourceKind::Token,
            ResourceKind::DefaultEnvironmentVariable,
        ],
        ResourceKind::Deployment => &[ResourceKind::Token, ResourceKind::Component],
        ResourceKind::Component
        | ResourceKind::Member
        | ResourceKind::Token
        | ResourceKind::CustomRole
        | ResourceKind::Billing
        | ResourceKind::OauthApplication
        | ResourceKind::Sso
        | ResourceKind::DirectorySync
        | ResourceKind::Integration
        | ResourceKind::DefaultEnvironmentVariable => &[],
    }
}

/// Resource kinds that can appear as the first segment in a specifier.
/// Deployment must appear under a project; Token must appear under a
/// team / project / deployment; DefaultEnvironmentVariable must appear under
/// a project.
fn is_valid_root(kind: ResourceKind) -> bool {
    !matches!(
        kind,
        ResourceKind::Deployment
            | ResourceKind::Component
            | ResourceKind::Token
            | ResourceKind::DefaultEnvironmentVariable
    )
}

impl FromStr for ResourceSpecifier {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() % 2 != 0 {
            bail!("Resource specifier must have kind:selector pairs: {s}");
        }
        let mut segments: Vec<ResourceSegment> = Vec::new();
        for chunk in parts.chunks(2) {
            let kind: ResourceKind = chunk[0]
                .parse()
                .map_err(|_| anyhow::anyhow!("Unknown resource kind: {}", chunk[0]))?;

            // Validate nesting
            if segments.is_empty() {
                if !is_valid_root(kind) {
                    bail!("Invalid root resource kind: {kind}");
                }
            } else {
                let parent_kind = segments.last().unwrap().kind();
                if !valid_children(parent_kind).contains(&kind) {
                    bail!("Invalid nesting: {kind} cannot appear under {parent_kind}");
                }
            }

            segments.push(parse_segment(kind, chunk[1])?);
        }
        Ok(ResourceSpecifier { segments })
    }
}

// --- Display impls ---

impl fmt::Display for ProjectSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectSelector::Any => write!(f, "*"),
            ProjectSelector::Id(id) => write!(f, "id={id}"),
            ProjectSelector::Slug(slug) => write!(f, "slug={slug}"),
        }
    }
}

impl fmt::Display for CreatorMatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CreatorMatcher::SelfActor => write!(f, "self"),
            CreatorMatcher::Member(mid) => write!(f, "{}", mid.0),
        }
    }
}

impl fmt::Display for DeploymentSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeploymentSelector::Any => write!(f, "*"),
            DeploymentSelector::Id(id) => write!(f, "id={id}"),
            DeploymentSelector::Type(t) => write!(f, "type={t}"),
            DeploymentSelector::Creator(c) => write!(f, "creator={c}"),
        }
    }
}

impl fmt::Display for TokenSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenSelector::Any => write!(f, "*"),
            TokenSelector::Creator(c) => write!(f, "creator={c}"),
        }
    }
}

impl fmt::Display for ComponentSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComponentSelector::Any => write!(f, "*"),
            // The path is stored canonically (root = ""); the root renders as
            // `/`.
            ComponentSelector::Path(p) if p.is_empty() => write!(f, "path=/"),
            ComponentSelector::Path(p) => write!(f, "path={p}"),
            // The stored prefix keeps its (significant) trailing slash, so just
            // re-append the `*`: `foo` -> `path=foo*`, `foo/` -> `path=foo/*`.
            ComponentSelector::PathStartsWith(p) => write!(f, "path={p}*"),
        }
    }
}

/// Helper macro to write comma-separated selectors for a segment.
macro_rules! write_segment {
    ($f:expr, $kind:expr, $selectors:expr) => {{
        write!($f, "{}:", $kind)?;
        for (j, selector) in $selectors.iter().enumerate() {
            if j > 0 {
                write!($f, ",")?;
            }
            write!($f, "{selector}")?;
        }
        Ok(())
    }};
}

impl fmt::Display for ResourceSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceSegment::Team => write!(f, "team:*"),
            ResourceSegment::Project(s) => write_segment!(f, ResourceKind::Project, s),
            ResourceSegment::Deployment(s) => write_segment!(f, ResourceKind::Deployment, s),
            ResourceSegment::Component(s) => write_segment!(f, ResourceKind::Component, s),
            ResourceSegment::Member => write!(f, "member:*"),
            ResourceSegment::Token(s) => write_segment!(f, ResourceKind::Token, s),
            ResourceSegment::CustomRole => write!(f, "customRole:*"),
            ResourceSegment::Billing => write!(f, "billing:*"),
            ResourceSegment::OauthApplication => write!(f, "oauthApplication:*"),
            ResourceSegment::Sso => write!(f, "sso:*"),
            ResourceSegment::DirectorySync => write!(f, "directorySync:*"),
            ResourceSegment::Integration => write!(f, "integration:*"),
            ResourceSegment::DefaultEnvironmentVariable => {
                write!(f, "defaultEnvironmentVariable:*")
            },
        }
    }
}

impl fmt::Display for ResourceSpecifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, segment) in self.segments.iter().enumerate() {
            if i > 0 {
                write!(f, ":")?;
            }
            write!(f, "{segment}")?;
        }
        Ok(())
    }
}
