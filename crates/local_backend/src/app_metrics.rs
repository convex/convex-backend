use axum::response::IntoResponse;
use common::{
    components::{
        ComponentFunctionPath,
        ComponentPath,
    },
    http::{
        extract::{
            Json,
            MtState,
            Query,
        },
        HttpResponseError,
    },
    types::{
        UdfIdentifier,
        UdfType,
    },
};
use errors::ErrorMetadata;
use serde::Deserialize;
use sync_types::UdfPath;
use value::{
    TableMapping,
    TabletId,
};

use crate::{
    authentication::ExtractIdentity,
    LocalAppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UdfRateQueryArgs {
    component_path: Option<String>,
    #[serde(alias = "path")]
    udf_path: String,
    metric: String,
    window: String,
    udf_type: Option<String>,
}

pub(crate) async fn udf_rate(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Query(UdfRateQueryArgs {
        component_path,
        udf_path,
        metric,
        window,
        udf_type,
    }): Query<UdfRateQueryArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let window_json: serde_json::Value =
        serde_json::from_str(&window).map_err(anyhow::Error::new)?;
    let window = window_json.try_into()?;
    let udf_identifier = parse_udf_identifier(udf_type, component_path, udf_path)?;

    let timeseries =
        st.application
            .metrics_log(&identity)?
            .udf_rate(udf_identifier, metric.parse()?, window)?;
    Ok(Json(timeseries))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TopKQueryArgs {
    window: String,
    k: Option<usize>,
}

pub(crate) async fn failure_percentage_top_k(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Query(TopKQueryArgs { window, k }): Query<TopKQueryArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let window_json: serde_json::Value =
        serde_json::from_str(&window).map_err(anyhow::Error::new)?;
    let window = window_json.try_into()?;

    let k = validate_k(k)?;

    let timeseries = st
        .application
        .metrics_log(&identity)?
        .failure_percentage_top_k(window, k)?;
    Ok(Json(timeseries))
}

pub(crate) async fn cache_hit_percentage_top_k(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Query(TopKQueryArgs { window, k }): Query<TopKQueryArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let window_json: serde_json::Value =
        serde_json::from_str(&window).map_err(anyhow::Error::new)?;
    let window = window_json.try_into()?;

    let k = validate_k(k)?;

    let timeseries = st
        .application
        .metrics_log(&identity)?
        .cache_hit_percentage_top_k(window, k)?;
    Ok(Json(timeseries))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SubscriptionInvalidationsTopKArgs {
    component_path: Option<String>,
    #[serde(alias = "path")]
    udf_path: Option<String>,
    window: String,
    udf_type: Option<String>,
    k: Option<usize>,
}

pub(crate) async fn subscription_invalidations_top_k(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Query(args): Query<SubscriptionInvalidationsTopKArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let window_json: serde_json::Value =
        serde_json::from_str(&args.window).map_err(anyhow::Error::new)?;
    let window = window_json.try_into()?;
    let k = validate_k(args.k)?;
    let table_mapping = st.application.latest_snapshot()?.table_mapping().clone();

    let udf_identifier = args
        .udf_path
        .map(|path| parse_udf_identifier(args.udf_type, args.component_path, path))
        .transpose()?;

    let timeseries = st
        .application
        .metrics_log(&identity)?
        .subscription_invalidations_top_k(window, k, udf_identifier.as_ref())?;

    // When filtered to a specific function, keys are tablet IDs.
    // Otherwise, keys are "{mutation}:{tablet_id}".
    let timeseries = if udf_identifier.is_some() {
        resolve_tablet_keys(timeseries, &table_mapping)
    } else {
        resolve_mutation_tablet_keys(timeseries, &table_mapping)
    };
    Ok(Json(timeseries))
}

pub(crate) async fn function_call_count_top_k(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Query(TopKQueryArgs { window, k }): Query<TopKQueryArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let window_json: serde_json::Value =
        serde_json::from_str(&window).map_err(anyhow::Error::new)?;
    let window = window_json.try_into()?;

    let k = validate_k(k)?;

    let timeseries = st
        .application
        .metrics_log(&identity)?
        .function_call_count_top_k(window, k)?;
    Ok(Json(timeseries))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CacheHitPercentageQueryArgs {
    component_path: Option<String>,
    #[serde(alias = "path")]
    udf_path: String,
    window: String,
    udf_type: Option<String>,
}
pub(crate) async fn cache_hit_percentage(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Query(query_args): Query<CacheHitPercentageQueryArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let window_json: serde_json::Value =
        serde_json::from_str(&query_args.window).map_err(anyhow::Error::new)?;
    let window = window_json.try_into()?;
    let udf_identifier = parse_udf_identifier(
        query_args.udf_type,
        query_args.component_path,
        query_args.udf_path,
    )?;
    let timeseries = st
        .application
        .metrics_log(&identity)?
        .cache_hit_percentage(udf_identifier, window)?;
    Ok(Json(timeseries))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LatencyPercentilesQueryArgs {
    component_path: Option<String>,
    #[serde(alias = "path")]
    udf_path: String,
    percentiles: String,
    window: String,
    udf_type: Option<String>,
}
pub(crate) async fn latency_percentiles(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Query(query_args): Query<LatencyPercentilesQueryArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let udf_identifier = parse_udf_identifier(
        query_args.udf_type,
        query_args.component_path,
        query_args.udf_path,
    )?;
    let percentiles: Vec<usize> =
        serde_json::from_str(&query_args.percentiles).map_err(anyhow::Error::new)?;
    let window_json: serde_json::Value =
        serde_json::from_str(&query_args.window).map_err(anyhow::Error::new)?;
    let window = window_json.try_into()?;
    let timeseries: Vec<_> = st
        .application
        .metrics_log(&identity)?
        .latency_percentiles(udf_identifier, percentiles, window)?
        .into_iter()
        .collect();
    Ok(Json(timeseries))
}

#[derive(Deserialize)]
pub(crate) struct TableRateQueryArgs {
    name: String,
    metric: String,
    window: String,
}
pub(crate) async fn table_rate(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Query(query_args): Query<TableRateQueryArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let name = query_args.name.parse()?;
    let metric = query_args.metric.parse()?;
    let window_json: serde_json::Value =
        serde_json::from_str(&query_args.window).map_err(anyhow::Error::new)?;
    let window = window_json.try_into()?;
    let timeseries = st
        .application
        .metrics_log(&identity)?
        .table_rate(name, metric, window)?;
    Ok(Json(timeseries))
}

fn parse_udf_identifier(
    udf_type: Option<String>,
    component_path: Option<String>,
    identifier: String,
) -> anyhow::Result<UdfIdentifier> {
    let component = ComponentPath::deserialize(component_path.as_deref())?;
    let udf_identifier = match udf_type {
        Some(udf_type) => {
            let udf_type: UdfType = udf_type.parse()?;
            match udf_type {
                UdfType::HttpAction => UdfIdentifier::Http(identifier.parse()?),
                _ => {
                    let udf_path: UdfPath = identifier.parse()?;
                    let path = ComponentFunctionPath {
                        component,
                        udf_path,
                    };
                    UdfIdentifier::Function(path.canonicalize())
                },
            }
        },
        None => {
            let udf_path: UdfPath = identifier.parse()?;
            let path = ComponentFunctionPath {
                component,
                udf_path,
            };
            UdfIdentifier::Function(path.canonicalize())
        },
    };
    Ok(udf_identifier)
}

#[derive(Deserialize)]
pub(crate) struct ScheduledJobLagArgs {
    window: String,
}
pub(crate) async fn scheduled_job_lag(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Query(query_args): Query<ScheduledJobLagArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let window_json: serde_json::Value =
        serde_json::from_str(&query_args.window).map_err(anyhow::Error::new)?;
    let window = window_json.try_into()?;
    let timeseries = st
        .application
        .metrics_log(&identity)?
        .scheduled_job_lag(window)?;
    Ok(Json(timeseries))
}

#[derive(Deserialize)]
pub(crate) struct FunctionConcurrencyArgs {
    window: String,
}
pub(crate) async fn function_concurrency(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Query(query_args): Query<FunctionConcurrencyArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let window_json: serde_json::Value =
        serde_json::from_str(&query_args.window).map_err(anyhow::Error::new)?;
    let window = window_json.try_into()?;
    let metrics = st
        .application
        .metrics_log(&identity)?
        .function_concurrency(window)?;
    Ok(Json(metrics))
}

fn validate_k(k: Option<usize>) -> anyhow::Result<usize> {
    const MIN_K: usize = 1;
    const MAX_K: usize = 25;
    const DEFAULT_K: usize = 5;

    let k = k.unwrap_or(DEFAULT_K);
    if !(MIN_K..=MAX_K).contains(&k) {
        anyhow::bail!(ErrorMetadata::bad_request(
            "InvalidTopKParameter",
            format!("k must be between {MIN_K} and {MAX_K}, got {k}")
        ));
    }
    Ok(k)
}

fn resolve_tablet_id(tablet_id_str: &str, table_mapping: &TableMapping) -> String {
    tablet_id_str
        .parse::<TabletId>()
        .ok()
        .and_then(|id| table_mapping.tablet_name(id).ok())
        .map(|name| name.to_string())
        .unwrap_or_else(|| tablet_id_str.to_string())
}

/// Resolve keys of the form "{tablet_id}" to table names.
fn resolve_tablet_keys<T>(
    timeseries: Vec<(String, T)>,
    table_mapping: &TableMapping,
) -> Vec<(String, T)> {
    timeseries
        .into_iter()
        .map(|(key, ts)| {
            if key == "_rest" {
                return (key, ts);
            }
            (resolve_tablet_id(&key, table_mapping), ts)
        })
        .collect()
}

/// Resolve keys of the form "{mutation}:{tablet_id}" to
/// "{mutation}:{table_name}".
fn resolve_mutation_tablet_keys<T>(
    timeseries: Vec<(String, T)>,
    table_mapping: &TableMapping,
) -> Vec<(String, T)> {
    timeseries
        .into_iter()
        .map(|(key, ts)| {
            if key == "_rest" {
                return (key, ts);
            }
            // The key is "{mutation}:{tablet_id}". The mutation path can
            // contain colons, so split from the right.
            if let Some(pos) = key.rfind(':') {
                let mutation = &key[..pos];
                let tablet_id_str = &key[pos + 1..];
                let table_name = resolve_tablet_id(tablet_id_str, table_mapping);
                (format!("{mutation}:{table_name}"), ts)
            } else {
                (key, ts)
            }
        })
        .collect()
}
