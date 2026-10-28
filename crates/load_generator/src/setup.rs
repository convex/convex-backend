use convex::{
    ConvexClientBuilder,
    Value,
};
use convex_sync_types::ErrorPayload;
use maplit::btreemap;

pub async fn setup(
    deployment_url: &str,
    num_messages: u64,
    num_vectors: u64,
) -> anyhow::Result<()> {
    tracing::info!("Executing setup mutations...");
    let mut client = ConvexClientBuilder::new(deployment_url)
        .with_client_id(concat!("load-generator-", env!("CARGO_PKG_VERSION")))
        .build()
        .await?;
    let result: Result<Value, ErrorPayload<Value>> = client
        .mutation(
            "setup:setupMessages",
            btreemap! {"rows".into()=> (num_messages as f64).into(), "channel".into() => "global".into()},
        )
        .await?
        .into();
    result.map_err(|e| anyhow::anyhow!(format!("setupMessages failed: {}", e.get_message())))?;
    let result: Result<Value, ErrorPayload<Value>> = client
        .mutation(
            "setup:setupVectors",
            btreemap! {"rows".into()=> (num_vectors as f64).into(), },
        )
        .await?
        .into();
    result.map_err(|e| anyhow::anyhow!(format!("setupVectors failed: {}", e.get_message())))?;

    Ok(())
}
