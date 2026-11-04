//! RDS IAM database authentication.
//!
//! See <https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/UsingWithRDS.IAMDBAuth.Connecting.html>.

use anyhow::Context as _;
pub use aws_sdk_rds::auth_token::AuthToken;
use aws_sdk_rds::auth_token::AuthTokenGenerator;
use aws_types::region::Region;

use crate::AWS_REGION;

/// The region encoded in an RDS endpoint hostname of the form
/// `<identifier>.<region>.rds.amazonaws.com`.
pub fn region_from_rds_hostname(hostname: &str) -> Option<&str> {
    let (_, region) = hostname
        .strip_suffix(".rds.amazonaws.com")?
        .rsplit_once('.')?;
    (!region.is_empty()).then_some(region)
}

/// Get a time-limited auth token for an RDS database.
/// Note that the token only allows new connections for 15 minutes, but
/// connections may last indefinitely.
///
/// The token is signed for the region parsed out of `hostname`, falling back to
/// the global `AWS_REGION` environment variable.
///
/// The token may need to be url-encoded before use.
pub async fn get_rds_auth_token(
    hostname: &str,
    port: u16,
    username: &str,
) -> anyhow::Result<AuthToken> {
    let region = Region::new(
        region_from_rds_hostname(hostname)
            .or(AWS_REGION.as_deref())
            .context("no region configured")?
            .to_owned(),
    );
    let sdk_config = crate::config_with_credentials(
        region.clone(),
        crate::credentials::shared_credentials().await,
    )
    .load()
    .await;
    let config = aws_sdk_rds::auth_token::Config::builder()
        .hostname(hostname)
        .port(port.into())
        .username(username)
        .region(region)
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build auth token config: {}", e))?;
    let token = AuthTokenGenerator::new(config)
        .auth_token(&sdk_config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get auth token: {}", e))?;
    Ok(token)
}
