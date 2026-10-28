use std::{
    fmt::Debug,
    sync::LazyLock,
    time::Duration,
};

use anyhow::Context;
use aws_config::{
    retry::RetryConfig,
    timeout::TimeoutConfig,
};
use aws_sdk_s3::{
    types::{
        Delete,
        DeleteMarkerEntry,
        Object,
        ObjectIdentifier,
    },
    Client,
};
use aws_smithy_types_convert::stream::PaginationStreamExt;
use cmd_util::env::env_config;
use futures::{
    stream::TryStreamExt,
    Stream,
    StreamExt,
};

use crate::must_s3_config_from_env;

#[derive(Clone, Debug)]
pub struct S3Client(pub Client);

static S3_TRASH_FOLDER: &str = ".trash/";

static S3_OPERATION_ATTEMPT_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("S3_OPERATION_ATTEMPT_TIMEOUT_SECONDS", 30)));

impl S3Client {
    pub async fn new(enable_retries: bool) -> anyhow::Result<Self> {
        let retry_config = match enable_retries {
            true => RetryConfig::standard().with_max_attempts(6),
            false => RetryConfig::disabled(),
        };
        let config = must_s3_config_from_env()
            .await
            .context(
                "Failed to create S3 configuration. Check AWS env variables or IAM permissions.",
            )?
            .timeout_config(
                TimeoutConfig::builder()
                    .operation_attempt_timeout(*S3_OPERATION_ATTEMPT_TIMEOUT)
                    .build(),
            )
            .retry_config(retry_config)
            .build();

        let s3_client = Client::from_conf(config);
        Ok(Self(s3_client))
    }

    /// Create an S3 client from a pre-configured SDK config.
    pub fn new_with_config(config: aws_config::SdkConfig) -> Self {
        Self(Client::new(&config))
    }

    /// Lists all keys in a bucket, grouped by the substring from the start of
    /// the key to the delimiter (inclusive). E.g. for a bucket with the
    /// following keys: a/1.txt
    /// a/2.txt
    /// b/1.txt
    /// b/2/3.txt
    /// and a delimiter of "/", the prefixes will be ["a/", "b/"].
    /// This can be useful for buckets that contain a directory for every
    /// instance
    /// Any files inside the `.trash` directory are not included.
    pub fn list_all_prefixes(
        &self,
        bucket: String,
        delimiter: String,
    ) -> impl Stream<Item = anyhow::Result<String>> + Send + Unpin + use<> {
        let stream = self
            .0
            .list_objects_v2()
            .bucket(bucket)
            .delimiter(delimiter)
            .into_paginator()
            .send()
            .into_stream_03x();
        stream
            .map_err(anyhow::Error::from)
            .map_ok(|output| {
                futures::stream::iter(output.common_prefixes.unwrap_or_default()).map(Ok)
            })
            .try_flatten()
            .map_ok(|p| p.prefix.expect("inner field must be present"))
            .try_filter(|p| futures::future::ready(!p.starts_with(S3_TRASH_FOLDER)))
    }

    /// Lists all files in a bucket, optionally filtered to a prefix.
    /// Any files inside the `.trash` directory are not included.
    pub fn list_all_s3_files_from_bucket(
        &self,
        bucket: String,
        prefix: Option<String>,
    ) -> impl Stream<Item = anyhow::Result<Object>> + Send + Unpin + use<> {
        let stream = self
            .0
            .list_objects_v2()
            .bucket(bucket)
            .set_prefix(prefix)
            .into_paginator()
            .send()
            .into_stream_03x();
        stream
            .map_err(anyhow::Error::from)
            .map_ok(|output| futures::stream::iter(output.contents.unwrap_or_default()).map(Ok))
            .try_flatten()
            .try_filter(|obj| {
                futures::future::ready(!obj.key().unwrap_or_default().starts_with(S3_TRASH_FOLDER))
            })
    }

    pub async fn delete_s3_files(
        &self,
        bucket: String,
        objects: impl Stream<Item = ObjectIdentifier> + Unpin,
    ) -> anyhow::Result<()> {
        let mut stream = objects.chunks(1000);
        while let Some(chunk) = stream.next().await {
            let req = self.0.delete_objects().bucket(&bucket).delete(
                Delete::builder()
                    .set_objects(Some(chunk))
                    .quiet(true)
                    .build()?,
            );
            req.send().await?;
        }
        Ok(())
    }

    pub async fn delete_s3_file(
        &self,
        bucket: String,
        key: String,
        version_id: Option<String>,
    ) -> anyhow::Result<()> {
        // Deleting is safe because our S3 buckets have versioning turned on and only
        // perform a soft delete when deleteObject is called
        let mut builder = self.0.delete_object().bucket(bucket).key(key.clone());
        builder = match version_id {
            Some(id) => builder.version_id(id),
            None => builder,
        };
        let result = builder.send().await;

        result.with_context(|| format!("Failed to delete S3 file with key {key}"))?;

        tracing::info!("Delete of S3 file with key {} was successful", key);

        Ok(())
    }

    pub async fn recover_s3_files_from_bucket(
        &self,
        bucket: String,
        deployment_name: String,
        for_real: bool,
    ) -> anyhow::Result<()> {
        let mut all_delete_markers: Vec<DeleteMarkerEntry> = vec![];
        let mut key_marker: Option<String> = None;
        let mut version_id_marker: Option<String> = None;

        loop {
            let mut req_builder = self
                .0
                .list_object_versions()
                .bucket(bucket.clone())
                .prefix(deployment_name.clone());

            if let Some(ref marker) = key_marker {
                req_builder = req_builder.key_marker(marker.clone());
            }
            if let Some(ref marker) = version_id_marker {
                req_builder = req_builder.version_id_marker(marker.clone());
            }

            let resp = req_builder.send().await?;

            // Get the delete markers slice, defaulting to an empty slice if None.
            let markers: &[DeleteMarkerEntry] = resp.delete_markers();
            // Extend the collected vector with the markers from the current page.
            all_delete_markers.extend_from_slice(markers);

            if resp.is_truncated() == Some(true) {
                key_marker = resp.next_key_marker().map(String::from);
                version_id_marker = resp.next_version_id_marker().map(String::from);
                // Ensure both markers are present if truncated, otherwise break.
                if key_marker.is_none() && version_id_marker.is_none() {
                    tracing::warn!(
                        "ListObjectVersions response was truncated but missing next markers. \
                         Stopping pagination."
                    );
                    break;
                }
            } else {
                break; // Exit loop if not truncated
            }
        }

        tracing::info!(
            "Retrieved {} total delete markers from AWS. Starting undelete",
            all_delete_markers.len()
        );

        let num_markers_found = all_delete_markers.len();

        if !for_real {
            // Simulate the deletion for a dry run
            for marker in &all_delete_markers {
                let key_str = marker.key.as_deref().unwrap_or("[missing key]");
                let version_str = marker
                    .version_id
                    .as_deref()
                    .unwrap_or("[missing version_id]");
                println!("DRY RUN: Would delete marker for key {key_str} version {version_str}");
            }
            tracing::info!(
                "DRY RUN: Would have recovered {num_markers_found} deleted files for instance \
                 {deployment_name}"
            );
        } else {
            // Convert markers to ObjectIdentifiers for batch deletion.
            let identifiers_iter = all_delete_markers.into_iter().map(|marker| {
                let key = marker.key.expect("DeleteMarkerEntry missing key");
                let version_id = marker
                    .version_id
                    .expect("DeleteMarkerEntry missing version_id");
                ObjectIdentifier::builder()
                    .key(key)
                    .version_id(version_id)
                    .build()
                    .expect("Building ObjectIdentifier failed unexpectedly")
            });

            let stream = futures::stream::iter(identifiers_iter);
            self.delete_s3_files(bucket.clone(), stream).await?;
            tracing::info!(
                "Recovered {num_markers_found} deleted files for instance {deployment_name}"
            );
        }

        Ok(())
    }
}
