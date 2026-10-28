use common::components::ComponentPath;
use keybroker::Identity;
use serde::Deserialize;
use udf::helpers::UdfArgsJson;
use utoipa::ToSchema;

use crate::admin::must_be_admin;

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
/// This struct should only be used for endpoints that allow calling functions
/// inside components. Requires admin key.
pub struct UdfPostRequestWithComponent {
    component_path: Option<String>,
    pub path: String,
    #[schema(value_type = Object)]
    pub args: UdfArgsJson,

    pub format: Option<String>,
}

impl UdfPostRequestWithComponent {
    pub fn component_path(&self, identity: &Identity) -> anyhow::Result<ComponentPath> {
        must_be_admin(identity)?;
        ComponentPath::deserialize(self.component_path.as_deref())
    }
}
