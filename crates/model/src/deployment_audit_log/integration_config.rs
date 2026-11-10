use std::collections::BTreeMap;

use common::types::streaming_export::selection::Selection;
use serde::{
    Deserialize,
    Serialize,
};
use value::FieldName;

use crate::integrations::types::{
    axiom::AxiomAttribute,
    posthog_logs::DEFAULT_POSTHOG_HOST,
    s3_export::S3ExportConfig,
    serialize_topics,
    webhook::WebhookFormat,
    IntegrationConfig,
};

/// Persisted audit snapshots have an explicit allowlist of configuration
/// fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum IntegrationAuditConfig {
    Local {
        path: String,
    },
    Datadog {
        site: String,
        service: Option<String>,
        tags: Vec<String>,
        format_version: String,
        topics: Option<Vec<String>>,
    },
    Axiom {
        dataset: String,
        ingest_origin: Option<String>,
        attributes: Vec<AxiomAttribute>,
        format_version: String,
        topics: Option<Vec<String>>,
    },
    Webhook {
        url_origin: Option<String>,
        format: WebhookFormat,
        topics: Option<Vec<String>>,
    },
    Sentry {
        dsn_origin: Option<String>,
        project_id: String,
        tags: Option<BTreeMap<FieldName, String>>,
        format_version: String,
    },
    PostHogLogs {
        host_origin: Option<String>,
        service_name: Option<String>,
        topics: Option<Vec<String>>,
    },
    PostHogErrorTracking {
        host_origin: Option<String>,
    },
    S3Export(S3ExportAuditConfig),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct S3ExportAuditConfig {
    pub bucket: String,
    pub region: String,
    pub prefix: Option<String>,
    pub selection: Selection,
}

impl From<&S3ExportConfig> for S3ExportAuditConfig {
    fn from(config: &S3ExportConfig) -> Self {
        Self {
            bucket: config.bucket.clone(),
            region: config.region.clone(),
            prefix: config.prefix.clone(),
            selection: config.selection.clone(),
        }
    }
}

/// Flags preserve changes to credential-bearing fields while snapshots retain
/// safe destinations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum IntegrationAuditCredentialChanges {
    Datadog {
        api_key_changed: bool,
    },
    Axiom {
        api_key_changed: bool,
        ingest_url_changed: bool,
    },
    Webhook {
        hmac_secret_changed: bool,
        url_changed: bool,
    },
    Sentry {
        dsn_changed: bool,
    },
    PostHogLogs {
        api_key_changed: bool,
        host_changed: bool,
    },
    PostHogErrorTracking {
        api_key_changed: bool,
        host_changed: bool,
    },
    S3Export {
        access_key_id_changed: bool,
        secret_access_key_changed: bool,
        #[serde(default)]
        role_arn_changed: bool,
    },
}

/// URL paths, userinfo, queries, and fragments can all carry credentials.
/// Origins identify the service.
fn url_origin(raw: &str) -> Option<String> {
    let origin = reqwest::Url::parse(raw)
        .ok()?
        .origin()
        .ascii_serialization();
    (origin != "null").then_some(origin)
}

pub fn audit_config(config: &IntegrationConfig) -> Option<IntegrationAuditConfig> {
    Some(match config {
        IntegrationConfig::Local(path) => IntegrationAuditConfig::Local { path: path.clone() },
        IntegrationConfig::Datadog(config) => IntegrationAuditConfig::Datadog {
            site: config.site_location.to_string(),
            service: config.service.clone(),
            tags: config.dd_tags.clone(),
            format_version: config.version.to_string(),
            topics: serialize_topics(config.topics.clone()),
        },
        IntegrationConfig::Axiom(config) => IntegrationAuditConfig::Axiom {
            dataset: config.dataset_name.clone(),
            ingest_origin: url_origin(
                config
                    .ingest_url
                    .as_deref()
                    .unwrap_or("https://api.axiom.co"),
            ),
            attributes: config.attributes.clone(),
            format_version: config.version.to_string(),
            topics: serialize_topics(config.topics.clone()),
        },
        IntegrationConfig::Webhook(config) => IntegrationAuditConfig::Webhook {
            url_origin: url_origin(config.url.as_str()),
            format: config.format.clone(),
            topics: serialize_topics(config.topics.clone()),
        },
        IntegrationConfig::Sentry(config) => IntegrationAuditConfig::Sentry {
            dsn_origin: url_origin(&config.dsn.0.to_string()),
            project_id: config.dsn.0.project_id().to_string(),
            tags: config.tags.clone(),
            format_version: config.version.to_string(),
        },
        IntegrationConfig::PostHogLogs(config) => IntegrationAuditConfig::PostHogLogs {
            host_origin: url_origin(config.host.as_deref().unwrap_or(DEFAULT_POSTHOG_HOST)),
            service_name: config.service_name.clone(),
            topics: serialize_topics(config.topics.clone()),
        },
        IntegrationConfig::PostHogErrorTracking(config) => {
            IntegrationAuditConfig::PostHogErrorTracking {
                host_origin: url_origin(config.host.as_deref().unwrap_or(DEFAULT_POSTHOG_HOST)),
            }
        },
        IntegrationConfig::S3Export(config) => IntegrationAuditConfig::S3Export(config.into()),
    })
}

pub fn audit_credential_changes(
    previous: &IntegrationConfig,
    current: &IntegrationConfig,
) -> Option<IntegrationAuditCredentialChanges> {
    Some(match current {
        IntegrationConfig::Datadog(current) => {
            let IntegrationConfig::Datadog(previous) = previous else {
                return None;
            };
            IntegrationAuditCredentialChanges::Datadog {
                api_key_changed: previous.dd_api_key != current.dd_api_key,
            }
        },
        IntegrationConfig::Axiom(current) => {
            let IntegrationConfig::Axiom(previous) = previous else {
                return None;
            };
            IntegrationAuditCredentialChanges::Axiom {
                api_key_changed: previous.api_key != current.api_key,
                ingest_url_changed: previous.ingest_url != current.ingest_url,
            }
        },
        IntegrationConfig::Webhook(current) => {
            let IntegrationConfig::Webhook(previous) = previous else {
                return None;
            };
            IntegrationAuditCredentialChanges::Webhook {
                hmac_secret_changed: previous.hmac_secret != current.hmac_secret,
                url_changed: previous.url != current.url,
            }
        },
        IntegrationConfig::Sentry(current) => {
            let IntegrationConfig::Sentry(previous) = previous else {
                return None;
            };
            IntegrationAuditCredentialChanges::Sentry {
                dsn_changed: previous.dsn != current.dsn,
            }
        },
        IntegrationConfig::PostHogLogs(current) => {
            let IntegrationConfig::PostHogLogs(previous) = previous else {
                return None;
            };
            IntegrationAuditCredentialChanges::PostHogLogs {
                api_key_changed: previous.api_key != current.api_key,
                host_changed: previous.host != current.host,
            }
        },
        IntegrationConfig::PostHogErrorTracking(current) => {
            let IntegrationConfig::PostHogErrorTracking(previous) = previous else {
                return None;
            };
            IntegrationAuditCredentialChanges::PostHogErrorTracking {
                api_key_changed: previous.api_key != current.api_key,
                host_changed: previous.host != current.host,
            }
        },
        IntegrationConfig::S3Export(current) => {
            let IntegrationConfig::S3Export(previous) = previous else {
                return None;
            };
            // The key flags stay in the audit log's schema for entries
            // written while exports could use access keys.
            IntegrationAuditCredentialChanges::S3Export {
                access_key_id_changed: false,
                secret_access_key_changed: false,
                role_arn_changed: previous.credentials.role_arn != current.credentials.role_arn,
            }
        },
        IntegrationConfig::Local(_) => return None,
    })
}
