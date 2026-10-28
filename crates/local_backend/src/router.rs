use std::{
    convert::Infallible,
    sync::Arc,
    time::Duration,
};

use axum::{
    error_handling::HandleErrorLayer,
    extract::{
        DefaultBodyLimit,
        FromRef,
    },
    routing::{
        get,
        post,
        put,
    },
    Router,
};
use common::{
    http::{
        cli_cors,
        extract::{
            FromMtState,
            MtState,
        },
    },
    knobs::{
        AIRBYTE_STREAMING_IMPORT_REQUEST_SIZE_LIMIT,
        MAX_BACKEND_RPC_REQUEST_SIZE,
        MAX_ECHO_BYTES,
        MAX_PUSH_BYTES,
    },
};
use http::{
    Method,
    StatusCode,
};
use metrics::SERVER_VERSION_STR;
use tower::ServiceBuilder;
use tower_http::{
    cors::{
        AllowHeaders,
        AllowOrigin,
        CorsLayer,
    },
    decompression::RequestDecompressionLayer,
};
use utoipa::{
    openapi::security::{
        ApiKey,
        ApiKeyValue,
        SecurityScheme,
    },
    Modify,
    OpenApi,
};
use utoipa_axum::router::OpenApiRouter;

use crate::{
    app_metrics::{
        cache_hit_percentage,
        cache_hit_percentage_top_k,
        failure_percentage_top_k,
        function_call_count_top_k,
        function_concurrency,
        latency_percentiles,
        scheduled_job_lag,
        subscription_invalidations_top_k,
        table_rate,
        udf_rate,
    },
    canonical_urls::update_canonical_url,
    dashboard::{
        common_dashboard_api_router,
        local_only_dashboard_router,
        run_test_function,
    },
    deploy_config::{
        get_config,
        get_config_hashes,
        push_config,
    },
    deploy_config2,
    deployment_jwks::deployment_jwks,
    environment_variables::{
        list_environment_variables,
        platform_router,
        update_environment_variables,
    },
    http_actions::http_action_handler,
    logs::{
        stream_function_logs,
        stream_udf_execution,
    },
    node_action_callbacks::{
        action_callbacks_middleware,
        audit_log,
        cancel_developer_job,
        create_function_handle,
        create_service_token,
        get_service_url,
        internal_action_post,
        internal_mutation_post,
        internal_query_post,
        schedule_job,
        storage_delete,
        storage_generate_upload_url,
        storage_get_metadata,
        storage_get_url,
        vector_search,
    },
    public_api::public_api_router,
    scheduling::{
        cancel_all_jobs,
        cancel_job,
    },
    schema::{
        prepare_schema,
        schema_state,
    },
    snapshot_export::{
        cancel_export,
        get_zip_export,
        request_zip_export,
        request_zip_export_token,
        set_export_expiration,
    },
    snapshot_import::{
        cancel_import,
        import,
        import_finish_upload,
        import_start_upload,
        import_upload_part,
        perform_import,
    },
    storage::{
        storage_get,
        storage_upload,
    },
    streaming_export::{
        data_sync_cursor_from_deltas,
        document_deltas_get,
        document_deltas_post,
        get_table_column_names,
        json_schemas,
        list_snapshot_get,
        list_snapshot_post,
        test_streaming_export_connection,
    },
    streaming_import::{
        add_primary_key_indexes,
        apply_fivetran_operations,
        clear_tables,
        fivetran_create_table,
        fivetran_truncate_table,
        get_schema,
        import_airbyte_records,
        primary_key_indexes_ready,
        replace_tables,
    },
    subs::sync,
    LocalAppState,
    RouterState,
};

// Security addon for documenting authentication methods
#[derive(Debug)]
struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(schema) = openapi.components.as_mut() {
            /*
            // Admin keys look just like deployment keys but there's no need to
            // contact api.convex.dev to validate them) and cannot be revoked.
            // The Convex Cloud product avoids giving them out.
            // We can document this once the distinction between these is clearer.
            schema.add_security_scheme(
                "Admin Key",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .description(Some(
                            "Admin keys provide full access to a deployment. Created in the \
                             [dashboard](https://docs.convex.dev/dashboard/deployments/deployment-settings#url-and-deploy-key) \
                             or via API. Use the `Convex ` prefix (e.g., `Convex <admin_key>`).",
                        ))
                        .build(),
                ),
            );
            */
            schema.add_security_scheme(
                "Deploy Key",
                SecurityScheme::ApiKey(ApiKey::Header(
                    ApiKeyValue::with_description(
                        "Authorization",
                        "Deploy keys are used for deployment operations. See \
                         [deploy key types](https://docs.convex.dev/cli/deploy-key-types) for more information. \
                         Use the `Convex ` prefix (e.g., `Convex <deploy_key>`).",
                    ),
                )),
            );
            schema.add_security_scheme(
                "OAuth Team Token",
                SecurityScheme::ApiKey(ApiKey::Header(
                    ApiKeyValue::with_description(
                        "Authorization",
                        "Obtained through a [Convex OAuth application](https://docs.convex.dev/management-api). \
                         Use the `Convex ` prefix (e.g., `Convex <oauth_token>`).",
                    ),
                )),
            );
            schema.add_security_scheme(
                "Team Token",
                SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::with_description(
                    "Authorization",
                    "Created in the dashboard under team settings for any team you can manage. \
                     Use the `Convex ` prefix (e.g., `Convex <team_token>`).",
                ))),
            );
            schema.add_security_scheme(
                "OAuth Project Token",
                SecurityScheme::ApiKey(ApiKey::Header(
                    ApiKeyValue::with_description(
                        "Authorization",
                        "Obtained through a [Convex OAuth application](https://docs.convex.dev/management-api) \
                         with project scope. Use the `Convex ` prefix (e.g., `Convex <oauth_project_token>`).",
                    ),
                )),
            );
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    modifiers(&SecurityAddon),
    // The data sync selection tree must be registered explicitly: these types
    // are referenced only from flattened maps' `additionalProperties`, which
    // utoipa's derived `schemas()` fails to register
    // (https://github.com/juhaku/utoipa/issues/1330).
    components(schemas(
        common::types::streaming_export::selection::ComponentSelection,
        common::types::streaming_export::selection::TableSelection,
        common::types::streaming_export::selection::ColumnSelection,
    )),
    info(
        title = "Convex Deployment API",
        version = "1.0.0",
        description = "Admin API for interacting with deployments.",
    ),
    servers(
        (url = "{deployment-url}/api/v1", description = "Your Convex deployment", variables(
            ("deployment-url" = (default = "https://happy-animal-123.convex.cloud", description = "Your deployment URL"))
        ))
    )
)]
struct PlatformApiDoc;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Convex Public HTTP routes",
        version = "1.0.0",
        description = "Endpoints that require no authentication"
    ),
    servers(
        (url = "/api", description = "Deployment API")
    )
)]
struct PublicApiDoc;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Convex Dashboard HTTP routes",
        version = "1.0.0",
        description = "Endpoints intended for dashboard use"
    ),
    servers(
        (url = "/api", description = "Deployment API")
    )
)]
struct DashboardApiDoc;

pub fn router(st: LocalAppState) -> Router {
    let browser_routes = Router::new()
        // Called by the browser (and optionally authenticated by a cookie or `Authorization`
        // header). Passes version in the URL because websockets can't do it in header.
        .route("/{client_version}/sync", get(sync));

    // routes are added by common_dashboard_routes below
    let (_, common_dashboard_openapi_spec) =
        OpenApiRouter::with_openapi(DashboardApiDoc::openapi())
            .merge(common_dashboard_api_router())
            .split_for_parts();
    let (local_only_dashboard_routes, local_only_openapi_spec) =
        OpenApiRouter::with_openapi(DashboardApiDoc::openapi())
            .merge(local_only_dashboard_router())
            .split_for_parts();

    let mut dashboard_openapi_spec = common_dashboard_openapi_spec;
    dashboard_openapi_spec.merge(local_only_openapi_spec);
    let dashboard_openapi_json = dashboard_openapi_spec.to_pretty_json().unwrap();
    let dashboard_routes = common_dashboard_routes()
        .merge(local_only_dashboard_routes)
        // Environment variable routes
        .route("/update_environment_variables", post(update_environment_variables))
        .route("/list_environment_variables", get(list_environment_variables))
        // Canonical URL routes
        .route("/update_canonical_url", post(update_canonical_url))
        // Scheduled jobs routes
        .route("/cancel_all_jobs", post(cancel_all_jobs))
        .route("/cancel_job", post(cancel_job))
        .route("/dashboard_openapi.json", axum::routing::get({
            move || async { dashboard_openapi_json }
        }))
        .layer(ServiceBuilder::new());

    let cli_routes = Router::new()
        .route("/push_config", post(push_config))
        .route("/prepare_schema", post(prepare_schema))
        .route("/deploy2/start_push", post(deploy_config2::start_push))
        .route(
            "/deploy2/evaluate_push",
            post(deploy_config2::evaluate_push),
        )
        .route(
            "/deploy2/evaluate_schema",
            post(deploy_config2::evaluate_schema),
        )
        .route("/run_test_function", post(run_test_function))
        .route(
            "/deploy2/wait_for_schema",
            post(deploy_config2::wait_for_schema),
        )
        .route("/deploy2/finish_push", post(deploy_config2::finish_push))
        .route(
            "/deploy2/report_push_completed",
            post(deploy_config2::report_push_completed_handler),
        )
        .layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(|_: Infallible| async {
                    StatusCode::INTERNAL_SERVER_ERROR
                }))
                .layer(RequestDecompressionLayer::new())
                .layer(DefaultBodyLimit::max(*MAX_PUSH_BYTES)),
        )
        .route("/get_config", post(get_config))
        .route("/get_config_hashes", post(get_config_hashes))
        .route("/schema_state/{schema_id}", get(schema_state))
        .route("/stream_udf_execution", get(stream_udf_execution))
        .route("/stream_function_logs", get(stream_function_logs))
        .merge(import_routes())
        .layer(cli_cors());

    let snapshot_export_routes = Router::new()
        .route("/request/zip", post(request_zip_export))
        .route("/zip/{id}", get(get_zip_export))
        .route("/zip/{id}/token", post(request_zip_export_token))
        .route("/set_expiration/{snapshot_id}", post(set_export_expiration))
        .route("/cancel/{snapshot_id}", post(cancel_export));

    let (platform_routes, platform_openapi) =
        OpenApiRouter::with_openapi(PlatformApiDoc::openapi())
            .merge(platform_router())
            .merge(crate::deployment_audit_log::platform_router())
            .merge(crate::deployment_info::platform_router())
            .merge(crate::usage_limits::platform_router())
            .merge(crate::canonical_urls::platform_router())
            .merge(crate::log_sinks::platform_router())
            .merge(crate::deployment_state::platform_router())
            // Streaming export ("data sync") API at /api/v1/data/sync.
            .merge(crate::streaming_export::platform_router())
            .split_for_parts();
    let platform_openapi_spec = platform_openapi.to_pretty_json().unwrap();
    let platform_routes = Router::new().merge(platform_routes).route(
        "/openapi.json",
        axum::routing::get(move || async { platform_openapi_spec }),
    );

    let api_routes = Router::new()
        .merge(cli_routes)
        .merge(dashboard_routes)
        .merge(streaming_export_routes())
        .nest("/actions", action_callback_routes(st.clone()))
        .nest("/export", snapshot_export_routes)
        .nest("/streaming_import", streaming_import_routes())
        .nest("/v1", platform_routes);

    // Endpoints migrated to use the RouterState trait instead of application.
    let (public_routes, public_openapi) = OpenApiRouter::with_openapi(PublicApiDoc::openapi())
        .merge(public_api_router())
        .split_for_parts();
    let public_openapi_spec = public_openapi.to_pretty_json().unwrap();

    let migrated_api_routes = Router::new()
        .merge(browser_routes)
        .merge(public_routes)
        .route("/sync", get(sync))
        .route(
            "/public_openapi.json",
            axum::routing::get({
                let spec = public_openapi_spec.clone();
                move || async move { spec }
            }),
        )
        .nest("/storage", storage_api_routes());
    let migrated = Router::new()
        .nest("/api", migrated_api_routes)
        .layer(cors())
        // Order matters. Layers only apply to routes above them.
        // Notably, any layers added here won't apply to common routes
        // added inside `serve_http`
        .nest("/http/", http_action_routes())
        .with_state(RouterState {
            api: Arc::new(st.application.clone()),
            runtime: st.application.runtime(),
            subscription_reconnect_rate_limiter: None,
        });

    let version = SERVER_VERSION_STR.to_string();

    Router::new()
        .nest("/api", api_routes)
        .merge(health_check_routes(version))
        .route("/.well-known/jwks.json", get(deployment_jwks))
        .layer(cors())
        .with_state(st)
        .merge(migrated)
}

pub fn public_api_routes<S>() -> Router<S>
where
    RouterState: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    let (routes, _openapi_spec) = OpenApiRouter::with_openapi(PlatformApiDoc::openapi())
        .merge(public_api_router())
        .split_for_parts();
    routes.route("/sync", get(sync))
}

pub fn storage_api_routes() -> Router<RouterState> {
    Router::new()
        .route("/upload", post(storage_upload))
        .route("/{storage_id}", get(storage_get))
}

// IMPORTANT NOTE: Those routes are proxied by Usher. Any changes to the router,
// such as adding or removing a route, or changing limits, also need to be
// applied to `crates_private/usher/src/proxy.rs`.
pub fn action_callback_routes<S>(state: S) -> Router<S>
where
    LocalAppState: FromMtState<S>,
    S: Send + Sync + Clone + 'static,
{
    Router::new()
        .route("/query", post(internal_query_post))
        .route("/mutation", post(internal_mutation_post))
        .route("/action", post(internal_action_post))
        .route("/create_service_token", post(create_service_token))
        .route("/get_service_url", post(get_service_url))
        .route("/schedule_job", post(schedule_job))
        .route("/vector_search", post(vector_search))
        .route("/cancel_job", post(cancel_developer_job))
        .route("/create_function_handle", post(create_function_handle))
        // file storage endpoints
        .route("/storage_generate_upload_url", post(storage_generate_upload_url))
        .route("/storage_get_url", post(storage_get_url))
        .route("/storage_get_metadata", post(storage_get_metadata))
        .route("/storage_delete", post(storage_delete))
        .route("/audit_log", post(audit_log))
        // All routes above this line get the increased limit
        .layer(DefaultBodyLimit::max(*MAX_BACKEND_RPC_REQUEST_SIZE))
        .layer(axum::middleware::from_fn_with_state(state, action_callbacks_middleware::<S>))
}

pub fn import_routes<S>() -> Router<S>
where
    LocalAppState: FromMtState<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/import", post(import))
        .route("/import/start_upload", post(import_start_upload))
        .route("/import/upload_part", post(import_upload_part))
        .route("/import/finish_upload", post(import_finish_upload))
        .route("/perform_import", post(perform_import))
        .route("/cancel_import", post(cancel_import))
}

pub fn http_action_routes() -> Router<RouterState> {
    Router::new()
        .route("/{*rest}", http_action_handler())
        .route("/", http_action_handler())
}

pub fn app_metrics_routes<S>() -> Router<S>
where
    LocalAppState: FromMtState<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/stream_udf_execution", get(stream_udf_execution))
        .route("/stream_function_logs", get(stream_function_logs))
        .route("/udf_rate", get(udf_rate))
        .route("/failure_percentage_top_k", get(failure_percentage_top_k))
        .route(
            "/cache_hit_percentage_top_k",
            get(cache_hit_percentage_top_k),
        )
        .route("/function_call_count_top_k", get(function_call_count_top_k))
        .route(
            "/subscription_invalidations_top_k",
            get(subscription_invalidations_top_k),
        )
        .route("/cache_hit_percentage", get(cache_hit_percentage))
        .route("/table_rate", get(table_rate))
        .route("/latency_percentiles", get(latency_percentiles))
        .route("/scheduled_job_lag", get(scheduled_job_lag))
        .route("/function_concurrency", get(function_concurrency))
}

// Routes with the same handlers for the local backend + closed source backend
pub fn common_dashboard_routes<S>() -> Router<S>
where
    LocalAppState: FromMtState<S>,
    S: Clone + Send + Sync + 'static,
{
    let (dashboard_routes_from_openapi, _dashboard_openapi_spec) =
        OpenApiRouter::with_openapi(DashboardApiDoc::openapi())
            .merge(common_dashboard_api_router())
            .split_for_parts();
    Router::new()
        .merge(dashboard_routes_from_openapi)
        // Metrics routes
        .nest("/app_metrics", app_metrics_routes())
}

pub fn health_check_routes<S>(version: String) -> Router<S>
where
    LocalAppState: FromMtState<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route(
            "/instance_name",
            get(|MtState(st): MtState<LocalAppState>| async move { st.instance_name.clone() }),
        )
        .route("/instance_version", get(|| async move { version }))
        .route(
            "/",
            get(|| async { "This Convex deployment is running. See https://docs.convex.dev/." }),
        )
        .route(
            "/echo",
            post(|body: axum::body::Body| async move { body })
        // Limit requests to 128MiB to help mitigate DDoS attacks.
        .layer(DefaultBodyLimit::max(*MAX_ECHO_BYTES)),
        )
        .layer(cors())
}

// IMPORTANT NOTE: Those routes are proxied by Usher. Any changes to the router,
// such as adding or removing a route, or changing limits, also need to be
// applied to `crates_private/usher/src/proxy.rs`.
pub fn streaming_import_routes<S>() -> Router<S>
where
    LocalAppState: FromMtState<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route(
            "/import_airbyte_records",
            post(import_airbyte_records).layer(DefaultBodyLimit::max(
                *AIRBYTE_STREAMING_IMPORT_REQUEST_SIZE_LIMIT,
            )),
        )
        .route(
            "/apply_fivetran_operations",
            post(apply_fivetran_operations),
        )
        .route("/get_schema", get(get_schema))
        .route("/replace_tables", post(replace_tables))
        .route("/clear_tables", put(clear_tables))
        .route("/fivetran_truncate_table", post(fivetran_truncate_table))
        .route("/fivetran_create_table", post(fivetran_create_table))
        .route("/add_primary_key_indexes", put(add_primary_key_indexes))
        .route("/primary_key_indexes_ready", get(primary_key_indexes_ready))
}

// IMPORTANT NOTE: Those routes are proxied by Usher. Any changes to the router,
// such as adding or removing a route, or changing limits, also need to be
// applied to `crates_private/usher/src/proxy.rs`.
pub fn streaming_export_routes<S>() -> Router<S>
where
    LocalAppState: FromMtState<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/document_deltas", get(document_deltas_get))
        .route("/document_deltas", post(document_deltas_post))
        .route("/list_snapshot", get(list_snapshot_get))
        .route("/list_snapshot", post(list_snapshot_post))
        .route("/json_schemas", get(json_schemas))
        .route(
            "/test_streaming_export_connection",
            get(test_streaming_export_connection),
        )
        .route("/get_table_column_names", get(get_table_column_names))
        .route(
            "/data_sync_cursor_from_deltas",
            post(data_sync_cursor_from_deltas),
        )
}

pub fn cors() -> CorsLayer {
    CorsLayer::new()
        .allow_headers(AllowHeaders::mirror_request())
        .allow_credentials(true)
        .allow_methods(vec![
            Method::GET,
            Method::POST,
            Method::OPTIONS,
            Method::PATCH,
            Method::DELETE,
            Method::PUT,
        ])
        .allow_origin(AllowOrigin::mirror_request())
        .max_age(Duration::from_secs(86400))
}
