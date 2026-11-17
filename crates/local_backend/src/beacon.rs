//! Beacon coroutine for collecting anonymous usage metrics to help improve
//! the self-hosted product.
//!
//! This module implements a beacon that periodically logs basic system
//! information like database version. The beacon is disabled by default
//! but can be enabled by setting CONVEX_ENABLE_BEACON=1.
//!
//! No personally identifiable information is collected. The data helps Convex
//! understand how self-hosted instances are being used to improve the product.
//! This is similar to Sentry's self-hosted telemetry approach, focusing on
//! anonymous and privacy-preserving metrics.

use std::time::{
    Duration,
    SystemTime,
};

use common::{
    backoff::Backoff,
    errors::report_error,
    runtime::Runtime,
};
use database::Database;
use model::database_globals::DatabaseGlobalsModel;
use reqwest::Client;
use runtime::prod::ProdRuntime;
use serde::Deserialize;
use serde_json::Value as JsonValue;
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(900); // 15 minutes

/// The git revision a backend binary was built from.
#[derive(Clone, Copy, Debug)]
pub struct BuildRevision {
    pub git_sha: &'static str,
    pub commit_timestamp: &'static str,
}

// Allow sending in overrides for the UUID when there's a better concept of
// "unique user" than the database UUID, and also allow sending arbitrary
// extra fields.
#[derive(Deserialize)]
struct BeaconFields {
    override_uuid: Option<String>,
    fields: Option<JsonValue>,
}

pub async fn start_beacon(
    runtime: ProdRuntime,
    database: Database<ProdRuntime>,
    beacon_tag: String,
    beacon_fields: Option<JsonValue>,
    build_revision: BuildRevision,
) {
    tracing::info!("Starting beacon coroutine...");
    let start_time = SystemTime::now();
    let mut backoff = Backoff::new(INITIAL_BACKOFF, MAX_BACKOFF);

    loop {
        let result = async {
            let mut tx = database.begin_system().await?;
            let mut db_model = DatabaseGlobalsModel::new(&mut tx);
            let globals = db_model.database_globals().await?;

            let client = Client::new();
            let migration_version = globals.version;
            let compiled_revision = build_revision.git_sha;
            let commit_timestamp = build_revision.commit_timestamp;
            let uptime = SystemTime::now()
                .duration_since(start_time)
                .unwrap_or_default()
                .as_secs();

            let mut uuid = globals.id().to_string();
            let mut extra_fields = JsonValue::Null;

            if let Some(fields) = beacon_fields.clone()
                && let Ok(parsed_fields) = serde_json::from_value::<BeaconFields>(fields)
            {
                if let Some(override_uuid) = parsed_fields.override_uuid {
                    uuid = override_uuid;
                }
                if let Some(f) = parsed_fields.fields {
                    extra_fields = f;
                }
            }

            let sent_json = serde_json::json!({
                "database_uuid": uuid,
                "migration_version": migration_version,
                "compiled_revision": compiled_revision,
                "commit_timestamp": commit_timestamp,
                "uptime": uptime,
                "beacon_tag": beacon_tag,
                "beacon_fields": extra_fields,
            });
            let url = "https://api.convex.dev/api/self_host_beacon";
            let response = client.post(url).json(&sent_json).send().await?;

            if response.status().is_success() {
                tracing::info!(
                    "Beacon request with json {sent_json} sent successfully to {url}. This \
                     anonymized data is used to help Convex understand and improve the product. \
                     You can disable this telemetry by setting the --disable-beacon flag or the \
                     DISABLE_BEACON environment variable."
                );
            } else {
                tracing::warn!("Beacon request failed with status: {}", response.status());
            }
            Ok::<(), anyhow::Error>(())
        }
        .await;

        match result {
            Ok(()) => {
                backoff.reset();
                // Sleep for one hour before next heartbeat
                runtime.wait(Duration::from_secs(3600)).await;
            },
            Err(e) => {
                report_error(&mut e.context("Beacon coroutine error")).await;
                let delay = backoff.fail(&mut runtime.rng());
                tracing::error!("Beacon failed, retrying in {delay:?}");
                runtime.wait(delay).await;
            },
        }
    }
}
