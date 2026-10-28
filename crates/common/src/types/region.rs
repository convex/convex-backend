use std::sync::{
    LazyLock,
    OnceLock,
};

use tuple_struct::tuple_struct_string;

tuple_struct_string!(
    #[schema(value_type = RegionNameForDocs)]
    RegionName
);

#[allow(dead_code)]
#[derive(utoipa::ToSchema)]
#[schema(as = RegionName)]
enum RegionNameForDocs {
    #[schema(rename = "aws-us-east-1")]
    AwsUsEast1,
    #[schema(rename = "aws-eu-west-1")]
    AwsEuWest1,
    #[schema(rename = "aws-ca-central-1")]
    AwsCaCentral1,
    #[schema(rename = "aws-ap-southeast-2")]
    AwsApSoutheast2,
}

static DEFAULT_REGION_NAME: LazyLock<RegionName> = LazyLock::new(|| "aws-us-east-1".into());
pub static TEST_REGION_NAME: LazyLock<RegionName> = LazyLock::new(|| "local".into());

// We are only able to determine what the default region should be at runtime,
// so we set the region in local_dev_bootstrap or setup_db, and then use the
// default_region function everywhere
static RUNTIME_DEFAULT_REGION: OnceLock<RegionName> = OnceLock::new();

pub fn set_test_region_as_default() -> anyhow::Result<()> {
    RUNTIME_DEFAULT_REGION
        .set(TEST_REGION_NAME.clone())
        .or_else(|_| {
            // Allow calling this multiple times if it's already set to the test region name
            if RUNTIME_DEFAULT_REGION.get() == Some(&*TEST_REGION_NAME) {
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "Default region already set to non-test region"
                ))
            }
        })
}

/// Returns the default region for the current environment.
pub fn default_region() -> &'static RegionName {
    RUNTIME_DEFAULT_REGION.get_or_init(|| DEFAULT_REGION_NAME.clone())
}

impl RegionName {
    /// Returns the AWS region portion (e.g. "us-east-1") if this region name
    /// is prefixed with "aws-".
    pub fn aws_region(&self) -> Option<&str> {
        self.as_str().strip_prefix("aws-")
    }
}
