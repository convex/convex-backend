use std::time::{
    Duration,
    Instant,
};

use convex_sync_types::backoff::Backoff;
use rand::rng;
use reqwest::Url;

/// Every request sends the caller's `client_id` in this header, so the
/// backend's client-version metrics attribute health checks to the service
/// that made them.
const CONVEX_CLIENT_HEADER: &str = "Convex-Client";

pub async fn health_check(service_url: &Url, client_id: &str) -> anyhow::Result<Option<String>> {
    health_check_with_retries(
        service_url,
        client_id,
        None,
        None,
        "instance_version",
        1,
        Duration::ZERO,
    )
    .await
}

pub async fn wait_for_http_health(
    service_url: &Url,
    client_id: &str,
    expected_version: Option<&str>,
    expected_deployment_name: Option<&str>,
    num_retries: usize,
    initial_backoff: Duration,
) -> anyhow::Result<String> {
    let start = Instant::now();
    tracing::info!("Waiting for health to {service_url}");
    match health_check_with_retries(
        service_url,
        client_id,
        expected_version,
        expected_deployment_name,
        "instance_version",
        num_retries,
        initial_backoff,
    )
    .await?
    {
        Some(version) => {
            tracing::info!(
                "{service_url} healthy at version:{version} after {:?}",
                start.elapsed()
            );
            Ok(version)
        },
        None => anyhow::bail!(
            "Timed out waiting for service health: {service_url} after {:?}",
            start.elapsed()
        ),
    }
}

async fn health_check_with_retries(
    service_url: &Url,
    client_id: &str,
    expected_version: Option<&str>,
    expected_deployment_name: Option<&str>,
    health_check_endpoint: &str,
    num_retries: usize,
    initial_backoff: Duration,
) -> anyhow::Result<Option<String>> {
    let mut backoff = Backoff::new(initial_backoff, Duration::from_secs(30));
    let mut last_error = None;
    for i in 0..=num_retries {
        if i != 0 {
            let delay = backoff.fail(&mut rng());
            tokio::time::sleep(delay).await;
        }
        match health_check_once(
            service_url,
            client_id,
            expected_version,
            health_check_endpoint,
        )
        .await
        {
            Ok(version) => {
                if let Some(expected_deployment_name) = expected_deployment_name
                    && let Err(e) =
                        verify_deployment_name(service_url, expected_deployment_name, client_id)
                            .await
                {
                    last_error = Some(e);
                    continue;
                }
                return Ok(Some(version));
            },
            Err(e) => last_error = Some(e),
        }
    }
    tracing::error!("{:?}", last_error.unwrap());
    Ok(None)
}

async fn health_check_once(
    service_url: &Url,
    client_id: &str,
    expected_version: Option<&str>,
    health_check_endpoint: &str,
) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let health_url = service_url.join(health_check_endpoint)?;
    let response = client
        .get(health_url.clone())
        .header(CONVEX_CLIENT_HEADER, client_id)
        .timeout(Duration::from_secs(15))
        .send()
        .await;

    match response {
        Ok(r) => {
            if r.status().is_success() {
                let version = r.text().await?;
                if let Some(expected_version) = expected_version {
                    anyhow::ensure!(expected_version == &version);
                }

                Ok(version)
            } else {
                anyhow::bail!("Health check on {service_url} failed: {r:?}");
            }
        },
        Err(e) => anyhow::bail!("Health check on {service_url} failed: {e:?}"),
    }
}

pub async fn verify_deployment_name(
    service_url: &Url,
    deployment_name: &str,
    client_id: &str,
) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let instance_name_url = service_url.join("instance_name")?;
    let response = client
        .get(instance_name_url)
        .header("x-convex-instance", deployment_name)
        .header(CONVEX_CLIENT_HEADER, client_id)
        .timeout(Duration::from_secs(5))
        .send()
        .await?;
    anyhow::ensure!(
        response.status().is_success(),
        "Health check on {service_url} failed: {response:?}"
    );
    let response_instance_name = response.text().await?;
    anyhow::ensure!(
        response_instance_name == deployment_name,
        "Health checking {service_url}. Expected {deployment_name}. Got {response_instance_name}."
    );
    Ok(())
}
