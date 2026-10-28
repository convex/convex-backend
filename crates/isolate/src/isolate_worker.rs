use async_trait::async_trait;
use common::{
    runtime::Runtime,
    types::UdfType,
};
use futures::FutureExt;
use sync_types::CanonicalizedUdfPath;
use udf::{
    metrics::is_developer_ok,
    HttpActionResult,
};

use crate::{
    client::{
        should_recreate_isolate,
        IsolateWorker,
        Request,
        RequestType,
    },
    context_cache::ContextCache,
    environment::{
        action::ActionEnvironment,
        analyze::AnalyzeEnvironment,
        auth_config::AuthConfigEnvironment,
        component_definitions::{
            AppDefinitionEvaluator,
            ComponentInitializerEvaluator,
        },
        schema::SchemaEnvironment,
        udf::DatabaseUdfEnvironment,
    },
    isolate::Isolate,
    metrics::{
        finish_service_request_timer,
        record_component_function_path,
        service_request_timer,
        RequestStatus,
    },
    ConcurrencyPermit,
    IsolateConfig,
};

#[derive(Clone)]
pub struct FunctionRunnerIsolateWorker<RT: Runtime> {
    rt: RT,
    isolate_config: IsolateConfig,
}

impl<RT: Runtime> FunctionRunnerIsolateWorker<RT> {
    pub fn new(rt: RT, isolate_config: IsolateConfig) -> Self {
        Self { rt, isolate_config }
    }
}

#[async_trait(?Send)]
impl<RT: Runtime> IsolateWorker<RT> for FunctionRunnerIsolateWorker<RT> {
    async fn handle_request_inner(
        &self,
        isolate: &mut Isolate<RT>,
        context_cache: &mut ContextCache,
        Request {
            client_id,
            inner,
            parent_trace: _,
        }: Request<RT>,
        permit: ConcurrencyPermit,
    ) -> (String, bool) {
        // Require the layer below to opt into isolate reuse by setting `isolate_clean`.
        let mut isolate_clean = false;
        let debug_str = match inner {
            RequestType::Udf {
                request,
                mut response,
                queue_timer,
                rng_seed,
                reactor_depth,
                udf_callback,
                function_started_sender,
            } => {
                drop(queue_timer);
                // TODO: Add metrics with funrun tagging
                let timer = service_request_timer(&request.udf_type);
                record_component_function_path(request.path_and_args.path());
                let udf_path = request.path_and_args.path().udf_path.to_owned();
                let (environment, args) = DatabaseUdfEnvironment::new(
                    self.rt.clone(),
                    request,
                    reactor_depth,
                    client_id.clone(),
                    rng_seed,
                );
                let r = environment
                    .run(
                        isolate,
                        context_cache,
                        permit,
                        &mut isolate_clean,
                        response.closed().boxed(),
                        args,
                        function_started_sender,
                        udf_callback,
                    )
                    .await;
                let status = match &r {
                    Ok((_tx, outcome)) => {
                        if is_developer_ok(outcome) {
                            RequestStatus::Success
                        } else {
                            RequestStatus::DeveloperError
                        }
                    },
                    Err(_) => RequestStatus::SystemError,
                };
                finish_service_request_timer(timer, status);
                let _ = response.send(r);
                format!("UDF: {udf_path:?}")
            },
            RequestType::Action {
                request,
                mut response,
                queue_timer,
                action_callbacks,
                fetch_client,
                log_line_sender,
                function_started_sender,
            } => {
                drop(queue_timer);
                let timer = service_request_timer(&UdfType::Action);
                let path = request.params.path_and_args.path();
                record_component_function_path(path);
                let udf_path = path.udf_path.to_owned();
                let component = path.component.to_owned();
                let component_path = path.component_path.clone();
                let log_string = format!("Action: {udf_path:?}");
                let environment = ActionEnvironment::new(
                    self.rt.clone(),
                    component,
                    udf_path,
                    component_path,
                    request.environment_data,
                    request.identity,
                    request.transaction,
                    action_callbacks,
                    fetch_client,
                    log_line_sender,
                    None,
                    request.context,
                );
                let r = environment
                    .run_action(
                        isolate,
                        context_cache,
                        permit,
                        &mut isolate_clean,
                        request.params.clone(),
                        response.closed().boxed(),
                        function_started_sender,
                    )
                    .await;

                let status = match &r {
                    Ok(outcome) => {
                        if outcome.result.is_ok() {
                            RequestStatus::Success
                        } else {
                            RequestStatus::DeveloperError
                        }
                    },
                    Err(_) => RequestStatus::SystemError,
                };
                finish_service_request_timer(timer, status);
                let _ = response.send(r);
                log_string
            },
            RequestType::Analyze {
                udf_config,
                modules,
                to_analyze,
                environment_variables,
                response,
            } => {
                let r = AnalyzeEnvironment::analyze::<RT>(
                    isolate,
                    context_cache,
                    permit,
                    &mut isolate_clean,
                    udf_config,
                    modules,
                    to_analyze,
                    environment_variables,
                )
                .await;
                let _ = response.send(r);
                "Analyze".to_string()
            },
            RequestType::HttpAction {
                request,
                response,
                queue_timer,
                action_callbacks,
                fetch_client,
                log_line_sender,
                http_response_streamer,
                function_started_sender,
            } => {
                drop(queue_timer);
                let timer = service_request_timer(&UdfType::HttpAction);
                let http_path = request.http_module_path.path();
                let udf_path: CanonicalizedUdfPath = http_path.udf_path.clone();
                let component_path = http_path.component_path.clone();
                let log_string = format!("Http: {udf_path:?}");
                record_component_function_path(http_path);
                let environment = ActionEnvironment::new(
                    self.rt.clone(),
                    http_path.component,
                    udf_path,
                    component_path,
                    request.environment_data,
                    request.identity,
                    request.transaction,
                    action_callbacks,
                    fetch_client,
                    log_line_sender,
                    Some(http_response_streamer),
                    request.context,
                );
                let r = environment
                    .run_http_action(
                        isolate,
                        context_cache,
                        permit,
                        &mut isolate_clean,
                        request.http_module_path,
                        request.routed_path,
                        request.http_request,
                        function_started_sender,
                    )
                    .await;
                let status = match &r {
                    Ok(outcome) => match outcome.result {
                        // Note that the stream could potentially encounter errors later
                        HttpActionResult::Streamed => RequestStatus::Success,
                        HttpActionResult::Error(_) => RequestStatus::DeveloperError,
                    },
                    Err(_) => RequestStatus::SystemError,
                };
                finish_service_request_timer(timer, status);
                let _ = response.send(r);
                log_string
            },
            RequestType::EvaluateSchema {
                schema_bundle,
                source_map,
                rng_seed,
                unix_timestamp,
                response,
            } => {
                let r = SchemaEnvironment::evaluate_schema(
                    isolate,
                    context_cache,
                    permit,
                    schema_bundle,
                    source_map,
                    rng_seed,
                    unix_timestamp,
                )
                .await;

                let _ = response.send(r);
                "EvaluateSchema".to_string()
            },
            RequestType::EvaluateAuthConfig {
                auth_config_bundle,
                source_map,
                environment_variables,
                response,
            } => {
                let r = AuthConfigEnvironment::evaluate_auth_config(
                    isolate,
                    context_cache,
                    permit,
                    auth_config_bundle,
                    source_map,
                    environment_variables,
                )
                .await;
                let _ = response.send(r);
                "EvaluateAuthConfig".to_string()
            },
            RequestType::EvaluateAppDefinitions {
                app_definition,
                component_definitions,
                dependency_graph,
                user_environment_variables,
                system_env_vars,
                response,
            } => {
                // AppDefinitionEvaluator doesn't use the prewarmed V8 context
                // because it uses an arbitrary number of contexts. This call is
                // rare and not particularly latency-sensitive though
                context_cache.clear();
                let env = AppDefinitionEvaluator::new(
                    app_definition,
                    component_definitions,
                    dependency_graph,
                    user_environment_variables,
                    system_env_vars,
                );
                let r = env.evaluate(context_cache, permit, isolate).await;
                let _ = response.send(r);
                "EvaluateAppDefinitions".to_string()
            },
            RequestType::EvaluateComponentInitializer {
                evaluated_definitions,
                path,
                definition,
                args,
                name,
                response,
            } => {
                let env = ComponentInitializerEvaluator::new(
                    evaluated_definitions,
                    path,
                    definition,
                    args,
                    name,
                );
                let r = env.evaluate(context_cache, permit, isolate).await;
                let _ = response.send(r);
                "EvaluateComponentInitializer".to_string()
            },
        };
        if isolate_clean && should_recreate_isolate(isolate, context_cache, &debug_str) {
            isolate_clean = false;
        }
        (debug_str, isolate_clean)
    }

    fn config(&self) -> &IsolateConfig {
        &self.isolate_config
    }

    fn rt(&self) -> &RT {
        &self.rt
    }
}
