use std::sync::LazyLock;

use errors::ErrorMetadata;
use regex::Regex;

// `LocalDeploymentName` assumes that slugs never contain `_`.
// If changing this, also update `LocalDeploymentName` to work with the new
// format.
static SLUG_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\w-]+$").unwrap());

pub fn validate_slug(slug: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        slug.len() >= 3,
        ErrorMetadata::bad_request("InvalidSlug", "Slug must be at least 3 characters long.",)
    );
    anyhow::ensure!(
        slug.len() <= 64,
        ErrorMetadata::bad_request("InvalidSlug", "Slug must be at most 64 characters long.",)
    );
    anyhow::ensure!(
        SLUG_REGEX.is_match(slug),
        ErrorMetadata::bad_request(
            "InvalidSlug",
            "Slug must contain only numbers, letters, and '-'.",
        )
    );
    Ok(())
}

pub fn validate_team_slug(team_slug: &str) -> anyhow::Result<()> {
    validate_slug(team_slug)?;
    if team_slug.contains("convex") {
        anyhow::bail!(ErrorMetadata::bad_request(
            "InvalidTeamName",
            "Team slug cannot contain substring `convex`",
        ))
    }
    Ok(())
}
