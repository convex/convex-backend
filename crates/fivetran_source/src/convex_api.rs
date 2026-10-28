use std::{
    collections::BTreeMap,
    fmt::Display,
    sync::LazyLock,
};

use anyhow::Context;
use async_trait::async_trait;
use common::types::streaming_export::{
    selection::Selection,
    ActiveDataSync,
    DataSyncArgs,
    DataSyncCursorFromDeltasArgs,
    DataSyncCursorFromDeltasResponse,
    DataSyncResponse,
    SyncId,
};
use derive_more::Display;
use fivetran_common::config::Config;
use headers::{
    HeaderName,
    HeaderValue,
};
use serde::{
    de::DeserializeOwned,
    Deserialize,
    Serialize,
};

use crate::conversions::selection::DEFAULT_FIVETRAN_SCHEMA_NAME;

#[allow(clippy::declare_interior_mutable_const)]
const CONVEX_CLIENT_HEADER: HeaderName = HeaderName::from_static("convex-client");

static CONVEX_CLIENT_HEADER_VALUE: LazyLock<HeaderValue> = LazyLock::new(|| {
    let connector_version = env!("CARGO_PKG_VERSION");
    HeaderValue::from_str(&format!("fivetran-export-{connector_version}")).unwrap()
});

/// The APIs exposed by a Convex backend for streaming export.
#[async_trait]
pub trait Source: Display + Send + Sync {
    /// An endpoint that confirms the Convex backend is accessible with
    /// streaming export enabled
    async fn test_streaming_export_connection(&self) -> anyhow::Result<()>;

    /// Fetch one page of the data sync stream. `cursor` is `None` to start a
    /// fresh sync, otherwise the opaque cursor from the previous page.
    async fn data_sync(
        &self,
        cursor: Option<String>,
        selection: Selection,
    ) -> anyhow::Result<DataSyncResponse>;

    /// Convert a cursor from the legacy `document_deltas` API into a data sync
    /// cursor covering the same data, so connections created before the data
    /// sync migration resume where they left off instead of resyncing.
    async fn data_sync_cursor_from_deltas(
        &self,
        cursor: i64,
        selection: Selection,
    ) -> anyhow::Result<String>;

    /// The deployment's view of this data sync's progress.
    async fn get_active_sync(&self, sync_id: &SyncId) -> anyhow::Result<ActiveDataSync>;

    /// Get a list of columns for each table and component on the Convex
    /// backend.
    async fn get_table_column_names(
        &self,
    ) -> anyhow::Result<BTreeMap<ComponentPath, BTreeMap<TableName, Vec<FieldName>>>>;
}

/// Implementation of [`Source`] accessing a real Convex deployment over HTTP.
pub struct ConvexApi {
    pub config: Config,
}

impl ConvexApi {
    fn url(&self, endpoint: &str) -> reqwest::Url {
        // `Url::join` treats the last path segment as a file name unless it ends
        // in `/`, so the prefix is joined separately from the endpoint.
        self.config
            .deploy_url
            .join("api/")
            .unwrap()
            .join(endpoint)
            .unwrap()
    }

    /// Turns a non-2xx response into an error carrying the response body, which
    /// is Convex's `{"code", "message"}` error payload. Passed through verbatim
    /// rather than parsed: the connector has no error it can recover from, and
    /// Fivetran surfaces the message to the customer as a task.
    async fn error_for_response(&self, endpoint: &str, resp: reqwest::Response) -> anyhow::Error {
        let status = resp.status();
        match resp.text().await {
            Ok(body) => anyhow::anyhow!(
                "Call to {endpoint} on {} returned an unsuccessful response ({status}): {body}",
                self.config.deploy_url,
            ),
            Err(_) => anyhow::anyhow!(
                "Call to {endpoint} on {} returned an unsuccessful response with no content \
                 ({status})",
                self.config.deploy_url,
            ),
        }
    }

    /// Performs a GET HTTP request to a given endpoint of the Convex API.
    async fn get<T: DeserializeOwned>(&self, endpoint: &str) -> anyhow::Result<T> {
        match reqwest::Client::new()
            .get(self.url(endpoint))
            .header(CONVEX_CLIENT_HEADER, &*CONVEX_CLIENT_HEADER_VALUE)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Convex {}", self.config.deploy_key),
            )
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => Ok(resp
                .json::<T>()
                .await
                .context("Failed to deserialize query result")?),
            Ok(resp) => Err(self.error_for_response(endpoint, resp).await),
            Err(e) => anyhow::bail!(e.to_string()),
        }
    }

    /// Performs a POST HTTP request to a given endpoint of the Convex API using
    /// the given parameters as a JSON body.
    async fn post<P: Serialize, T: DeserializeOwned>(
        &self,
        endpoint: &str,
        parameters: P,
    ) -> anyhow::Result<T> {
        match reqwest::Client::new()
            .post(self.url(endpoint))
            .header(CONVEX_CLIENT_HEADER, &*CONVEX_CLIENT_HEADER_VALUE)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Convex {}", self.config.deploy_key),
            )
            .json(&parameters)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => Ok(resp
                .json::<T>()
                .await
                .context("Failed to deserialize query result")?),
            Ok(resp) => Err(self.error_for_response(endpoint, resp).await),
            Err(e) => anyhow::bail!(e.to_string()),
        }
    }
}

#[async_trait]
impl Source for ConvexApi {
    async fn test_streaming_export_connection(&self) -> anyhow::Result<()> {
        self.get("test_streaming_export_connection").await
    }

    async fn data_sync(
        &self,
        cursor: Option<String>,
        selection: Selection,
    ) -> anyhow::Result<DataSyncResponse> {
        self.post("v1/data/sync", DataSyncArgs { cursor, selection })
            .await
    }

    async fn data_sync_cursor_from_deltas(
        &self,
        cursor: i64,
        selection: Selection,
    ) -> anyhow::Result<String> {
        let response: DataSyncCursorFromDeltasResponse = self
            .post(
                "data_sync_cursor_from_deltas",
                DataSyncCursorFromDeltasArgs { cursor, selection },
            )
            .await?;
        Ok(response.cursor)
    }

    async fn get_active_sync(&self, sync_id: &SyncId) -> anyhow::Result<ActiveDataSync> {
        self.get(&format!("v1/data/sync/{sync_id}")).await
    }

    async fn get_table_column_names(
        &self,
    ) -> anyhow::Result<BTreeMap<ComponentPath, BTreeMap<TableName, Vec<FieldName>>>> {
        let response: GetTableColumnNamesResponse = self.get("get_table_column_names").await?;

        let by_component = response
            .by_component
            .into_iter()
            .map(|(component_path, tables)| {
                (
                    ComponentPath(component_path),
                    tables
                        .into_iter()
                        .map(|table| {
                            let columns: Vec<_> =
                                table.columns.into_iter().map(FieldName).collect();
                            (TableName(table.name), columns)
                        })
                        .collect(),
                )
            })
            .collect();

        Ok(by_component)
    }
}

impl Display for ConvexApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.config.deploy_url.as_ref())
    }
}

#[derive(Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Display, Debug)]
pub struct TableName(pub String);

#[derive(Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Display, Clone, Debug)]
pub struct ComponentPath(pub String);

#[derive(Display, Debug)]
pub struct FieldName(pub String);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTableColumnNamesResponse {
    pub by_component: BTreeMap<String, Vec<GetTableColumnNameTable>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTableColumnNameTable {
    pub name: String,
    pub columns: Vec<String>,
}

/// The Fivetran schema a Convex component's tables are synced into. Convex's
/// root component has no name, so it maps to Fivetran's default schema.
pub fn fivetran_schema_name(component: &str) -> String {
    match component {
        "" => DEFAULT_FIVETRAN_SCHEMA_NAME.to_string(),
        _ => component.to_string(),
    }
}
