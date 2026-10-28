use std::collections::BTreeMap;

use url::Url;

use crate::fivetran_sdk::{
    form_field::Type,
    FormField,
    TextField,
};

const CONFIG_KEY_DEPLOYMENT_URL: &str = "url";
const CONFIG_KEY_DEPLOYMENT_KEY: &str = "key";

/// The configuration parameters used by the connector, requested to users by
/// the Fivetran UI. Users can obtain these values from the Convex dashboard in
/// the deployment’s settings page.
pub struct Config {
    /// The domain where the deployment is hosted (e.g. "https://aware-llama-900.convex.cloud").
    pub deploy_url: Url,

    /// The key giving admin permissions to the deployment
    /// (e.g. "prod:aware-llama-900|016b26d3900d5e482f1780969c2fa608a773140fb221db21785a9b2775b50263da6a258301b6374ef72b4c120e237c20ac50")
    pub deploy_key: String,
}

impl Config {
    /// Layout of the fields visible in the Fivetran UI
    pub fn fivetran_fields() -> Vec<FormField> {
        vec![
            FormField {
                name: CONFIG_KEY_DEPLOYMENT_URL.to_string(),
                label: "Deployment URL".to_string(),
                required: Some(true),
                description: Some(
                    "The domain where the deployment is hosted. You can find it in the deployment \
                     settings page of the Convex dashboard."
                        .to_string(),
                ),
                r#type: Some(Type::TextField(TextField::PlainText as i32)),
                default_value: None,
                placeholder: Some("https://aware-llama-900.convex.cloud".to_string()),
            },
            FormField {
                name: CONFIG_KEY_DEPLOYMENT_KEY.to_string(),
                label: "Deploy Key".to_string(),
                required: Some(true),
                description: Some(
                    "The key giving access to your deployment. You can find it in the deployment \
                     settings page of the Convex dashboard."
                        .to_string(),
                ),
                r#type: Some(Type::TextField(TextField::Password as i32)),
                default_value: None,
                placeholder: None,
            },
        ]
    }

    /// Validates user-supplied configuration parameters
    /// and creates a [`Config`] instance if they are valid.
    pub fn from_parameters(configuration: BTreeMap<String, String>) -> anyhow::Result<Self> {
        let Some(deploy_url) = configuration.get(CONFIG_KEY_DEPLOYMENT_URL) else {
            anyhow::bail!("Missing {CONFIG_KEY_DEPLOYMENT_URL}");
        };

        let Ok(deploy_url) = Url::parse(deploy_url) else {
            anyhow::bail!("Invalid {CONFIG_KEY_DEPLOYMENT_URL} (must be an URL)");
        };

        if deploy_url.host_str().is_none() {
            anyhow::bail!("Invalid deploy URL: must contain a host.");
        }

        if deploy_url.path() != "/"
            || deploy_url.query().is_some()
            || deploy_url.username() != ""
            || deploy_url.password().is_some()
            || deploy_url.fragment().is_some()
            || (deploy_url.scheme() != "http" && deploy_url.scheme() != "https")
        {
            anyhow::bail!("Invalid deploy URL: must be a root URL.");
        }

        let Some(deploy_key) = configuration.get(CONFIG_KEY_DEPLOYMENT_KEY) else {
            anyhow::bail!("Missing {CONFIG_KEY_DEPLOYMENT_KEY}");
        };

        Ok(Config {
            deploy_url,
            deploy_key: deploy_key.to_owned(),
        })
    }
}
