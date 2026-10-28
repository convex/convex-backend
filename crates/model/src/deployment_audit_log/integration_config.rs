use std::collections::BTreeMap;

use common::types::streaming_export::selection::Selection;
use serde::{
    Deserialize,
    Serialize,
};
use value::FieldName;

use crate::log_sinks::types::{
    axiom::AxiomAttribute,
    posthog_logs::DEFAULT_POSTHOG_HOST,
    s3_export::{
        S3ExportConfig,
        S3ExportCredentials,
        SyncPeriod,
    },
    serialize_topics,
    webhook::WebhookFormat,
    SinkConfig,
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
    pub period: SyncPeriod,
    pub selection: Selection,
}

impl From<&S3ExportConfig> for S3ExportAuditConfig {
    fn from(config: &S3ExportConfig) -> Self {
        Self {
            bucket: config.bucket.clone(),
            region: config.region.clone(),
            prefix: config.prefix.clone(),
            period: config.period,
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

pub fn audit_config(config: &SinkConfig) -> Option<IntegrationAuditConfig> {
    Some(match config {
        SinkConfig::Local(path) => IntegrationAuditConfig::Local { path: path.clone() },
        SinkConfig::Datadog(config) => IntegrationAuditConfig::Datadog {
            site: config.site_location.to_string(),
            service: config.service.clone(),
            tags: config.dd_tags.clone(),
            format_version: config.version.to_string(),
            topics: serialize_topics(config.topics.clone()),
        },
        SinkConfig::Axiom(config) => IntegrationAuditConfig::Axiom {
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
        SinkConfig::Webhook(config) => IntegrationAuditConfig::Webhook {
            url_origin: url_origin(config.url.as_str()),
            format: config.format.clone(),
            topics: serialize_topics(config.topics.clone()),
        },
        SinkConfig::Sentry(config) => IntegrationAuditConfig::Sentry {
            dsn_origin: url_origin(&config.dsn.0.to_string()),
            project_id: config.dsn.0.project_id().to_string(),
            tags: config.tags.clone(),
            format_version: config.version.to_string(),
        },
        SinkConfig::PostHogLogs(config) => IntegrationAuditConfig::PostHogLogs {
            host_origin: url_origin(config.host.as_deref().unwrap_or(DEFAULT_POSTHOG_HOST)),
            service_name: config.service_name.clone(),
            topics: serialize_topics(config.topics.clone()),
        },
        SinkConfig::PostHogErrorTracking(config) => IntegrationAuditConfig::PostHogErrorTracking {
            host_origin: url_origin(config.host.as_deref().unwrap_or(DEFAULT_POSTHOG_HOST)),
        },
        SinkConfig::S3Export(config) => IntegrationAuditConfig::S3Export(config.into()),
    })
}

pub fn audit_credential_changes(
    previous: &SinkConfig,
    current: &SinkConfig,
) -> Option<IntegrationAuditCredentialChanges> {
    Some(match current {
        SinkConfig::Datadog(current) => {
            let SinkConfig::Datadog(previous) = previous else {
                return None;
            };
            IntegrationAuditCredentialChanges::Datadog {
                api_key_changed: previous.dd_api_key != current.dd_api_key,
            }
        },
        SinkConfig::Axiom(current) => {
            let SinkConfig::Axiom(previous) = previous else {
                return None;
            };
            IntegrationAuditCredentialChanges::Axiom {
                api_key_changed: previous.api_key != current.api_key,
                ingest_url_changed: previous.ingest_url != current.ingest_url,
            }
        },
        SinkConfig::Webhook(current) => {
            let SinkConfig::Webhook(previous) = previous else {
                return None;
            };
            IntegrationAuditCredentialChanges::Webhook {
                hmac_secret_changed: previous.hmac_secret != current.hmac_secret,
                url_changed: previous.url != current.url,
            }
        },
        SinkConfig::Sentry(current) => {
            let SinkConfig::Sentry(previous) = previous else {
                return None;
            };
            IntegrationAuditCredentialChanges::Sentry {
                dsn_changed: previous.dsn != current.dsn,
            }
        },
        SinkConfig::PostHogLogs(current) => {
            let SinkConfig::PostHogLogs(previous) = previous else {
                return None;
            };
            IntegrationAuditCredentialChanges::PostHogLogs {
                api_key_changed: previous.api_key != current.api_key,
                host_changed: previous.host != current.host,
            }
        },
        SinkConfig::PostHogErrorTracking(current) => {
            let SinkConfig::PostHogErrorTracking(previous) = previous else {
                return None;
            };
            IntegrationAuditCredentialChanges::PostHogErrorTracking {
                api_key_changed: previous.api_key != current.api_key,
                host_changed: previous.host != current.host,
            }
        },
        SinkConfig::S3Export(current) => {
            let SinkConfig::S3Export(previous) = previous else {
                return None;
            };
            let (access_key_id_changed, secret_access_key_changed, role_arn_changed) =
                match (&previous.credentials, &current.credentials) {
                    (
                        S3ExportCredentials::AccessKey {
                            access_key_id,
                            secret_access_key,
                        },
                        S3ExportCredentials::AccessKey {
                            access_key_id: new_id,
                            secret_access_key: new_secret,
                        },
                    ) => (
                        access_key_id != new_id,
                        secret_access_key != new_secret,
                        false,
                    ),
                    (
                        S3ExportCredentials::Role { role_arn, .. },
                        S3ExportCredentials::Role {
                            role_arn: new_arn, ..
                        },
                    ) => (false, false, role_arn != new_arn),
                    // Switching between keys and a role replaces every credential.
                    _ => (true, true, true),
                };
            IntegrationAuditCredentialChanges::S3Export {
                access_key_id_changed,
                secret_access_key_changed,
                role_arn_changed,
            }
        },
        SinkConfig::Local(_) => return None,
    })
}
