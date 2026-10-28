use std::time::Duration;

use anyhow::Context;
use common::{
    components::{
        CanonicalizedComponentFunctionPath,
        ComponentPath,
        Reference,
    },
    errors::JsError,
    execution_context::{
        ExecutionContext,
        RequestContext,
    },
    http::RoutedHttpPath,
    log_lines::{
        run_function_and_collect_log_lines,
        LogLevel,
        LogLine,
        SystemLogMetadata,
    },
    runtime::{
        tokio_spawn,
        Runtime,
    },
    types::{
        FunctionCaller,
        ModuleEnvironment,
        RoutableMethod,
    },
};
use database::{
    BootstrapComponentsModel,
    Transaction,
};
use errors::ErrorMetadataAnyhowExt;
use function_runner::server::HttpActionMetadata;
use futures::{
    FutureExt,
    StreamExt,
};
use http::StatusCode;
use keybroker::Identity;
use model::modules::{
    ModuleModel,
    HTTP_MODULE_PATH,
};
use sync_types::{
    CanonicalizedUdfPath,
    FunctionName,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use udf::{
    validation::ValidatedHttpPath,
    HttpActionOutcome,
    HttpActionRequest,
    HttpActionRequestHead,
    HttpActionResponsePart,
    HttpActionResponseStreamer,
    HttpActionResult,
};
use usage_tracking::FunctionUsageTracker;
use value::sha256::Sha256Digest;

use super::ApplicationFunctionRunner;
use crate::function_log::HttpActionStatusCode;

impl<RT: Runtime> ApplicationFunctionRunner<RT> {
    #[fastrace::trace]
    pub async fn run_http_action(
        &self,
        request_context: RequestContext,
        http_request: HttpActionRequest,
        mut response_streamer: HttpActionResponseStreamer,
        identity: Identity,
        caller: FunctionCaller,
    ) -> anyhow::Result<udf::HttpActionResult> {
        let start = self.runtime.monotonic_now();
        let usage_tracker = FunctionUsageTracker::new();

        let mut tx = self
            .database
            .begin_with_usage(identity.clone(), usage_tracker.clone())
            .await?;

        let (component_path, routed_path) =
            match self.route_http_action(&mut tx, &http_request.head).await? {
                Some(r) => r,
                None => {
                    drop(tx);
                    let response_parts = udf::HttpActionResponsePart::from_text(
                        StatusCode::NOT_FOUND,
                        "This Convex deployment does not have HTTP actions enabled.".to_string(),
                    );
                    for part in response_parts {
                        response_streamer.send_part(part)??;
                    }
                    return Ok(udf::HttpActionResult::Streamed);
                },
            };
        let path = CanonicalizedComponentFunctionPath {
            component: component_path,
            udf_path: CanonicalizedUdfPath::new(
                HTTP_MODULE_PATH.clone(),
                FunctionName::default_export(),
            ),
        };
        let validated_path = match ValidatedHttpPath::new(&mut tx, path).await? {
            Ok(validated_path) => validated_path,
            Err(e) => return Ok(udf::HttpActionResult::Error(e)),
        };
        let unix_timestamp = self.runtime.unix_timestamp();
        let context = ExecutionContext::new(request_context, &caller);

        let request_head = http_request.head.clone();
        let route = http_request.head.route_for_failure();
        let (log_line_sender, log_line_receiver) = mpsc::unbounded_channel();
        // We want to intercept the response head so we can log it on function
        // completion, but still stream the response as it comes in, so we
        // create another channel here.
        let (isolate_response_sender, isolate_response_receiver) = mpsc::unbounded_channel();
        let http_response_streamer = HttpActionResponseStreamer::new(isolate_response_sender);

        let outcome_future = self
            .isolate_functions
            .execute_http_action(
                tx,
                log_line_sender,
                HttpActionMetadata {
                    http_response_streamer,
                    http_module_path: validated_path,
                    routed_path,
                    http_request,
                },
                context.clone(),
            )
            .boxed();

        let context_ = context.clone();

        // Stream `response_stream` from the isolate, to `response_streamer`
        // in the application.
        let stream_result_fut = tokio_spawn(
            "http_action_response_streamer",
            Self::forward_http_action_stream(
                UnboundedReceiverStream::new(isolate_response_receiver),
                response_streamer,
            ),
        );

        // NOTE: this will run in parallel with `stream_result_fut`, which is
        // running on a spawned coroutine.
        let send_log_line = |log_line| {
            self.function_log.log_http_action_progress(
                route.clone(),
                unix_timestamp,
                context_.clone(),
                vec![log_line].into(),
                // http actions are always run in Isolate
                ModuleEnvironment::Isolate,
            )
        };
        let (outcome_result, mut log_lines) =
            run_function_and_collect_log_lines(outcome_future, log_line_receiver, &send_log_line)
                .await;

        let (result_for_logging, response_sha256) = stream_result_fut.await??;

        match outcome_result {
            Ok(outcome) => {
                let result = outcome.result.clone();
                let result_for_logging = match &result {
                    HttpActionResult::Error(e) => Err(e.clone()),
                    HttpActionResult::Streamed => Ok(result_for_logging.ok_or_else(|| {
                        anyhow::anyhow!(
                            "Result should be populated for successfully completed HTTP action"
                        )
                    })?),
                };
                self.function_log
                    .log_http_action(
                        outcome,
                        result_for_logging,
                        log_lines,
                        start.elapsed(),
                        caller,
                        usage_tracker,
                        context,
                        response_sha256,
                    )
                    .await;
                Ok(result)
            },
            Err(e) if e.is_deterministic_user_error() || e.is_client_disconnect() => {
                let is_client_disconnect = e.is_client_disconnect();
                let js_err = JsError::from_error(e);
                match result_for_logging {
                    Some(r) => {
                        let outcome = HttpActionOutcome::new(
                            None,
                            request_head,
                            identity.into(),
                            unix_timestamp,
                            HttpActionResult::Streamed,
                            None,
                            None,
                            Duration::ZERO,
                        );
                        let new_log_line = LogLine::new_system_log_line(
                            if is_client_disconnect {
                                // Not developer's fault, but we should let them know
                                // since it indicates there will be no more logs or
                                // response parts.
                                LogLevel::Info
                            } else {
                                LogLevel::Warn
                            },
                            vec![js_err.to_string()],
                            outcome.unix_timestamp,
                            SystemLogMetadata {
                                code: if is_client_disconnect {
                                    "info:httpActionClientDisconnect".to_string()
                                } else {
                                    "error:httpAction".to_string()
                                },
                            },
                        );
                        send_log_line(new_log_line.clone());
                        log_lines.push(new_log_line);
                        self.function_log
                            .log_http_action(
                                outcome.clone(),
                                Ok(r),
                                log_lines,
                                start.elapsed(),
                                caller,
                                usage_tracker,
                                context,
                                response_sha256,
                            )
                            .await;
                        Ok(HttpActionResult::Streamed)
                    },
                    None => {
                        let result = udf::HttpActionResult::Error(js_err.clone());
                        let outcome = HttpActionOutcome::new(
                            None,
                            request_head,
                            identity.into(),
                            unix_timestamp,
                            result.clone(),
                            None,
                            None,
                            Duration::ZERO,
                        );
                        self.function_log
                            .log_http_action(
                                outcome.clone(),
                                Err(js_err),
                                log_lines,
                                start.elapsed(),
                                caller,
                                usage_tracker,
                                context,
                                response_sha256,
                            )
                            .await;
                        Ok(result)
                    },
                }
            },
            Err(e) => {
                self.function_log
                    .log_http_action_system_error(
                        &e,
                        request_head,
                        identity.into(),
                        start,
                        caller,
                        log_lines,
                        context,
                        response_sha256,
                    )
                    .await;
                Err(e)
            },
        }
    }

    // Forwards from `response_stream` to `response_streamer`.
    async fn forward_http_action_stream(
        mut response_stream: UnboundedReceiverStream<HttpActionResponsePart>,
        mut response_streamer: HttpActionResponseStreamer,
    ) -> anyhow::Result<(Option<HttpActionStatusCode>, Sha256Digest)> {
        let mut result_for_logging = None;
        loop {
            // If the `response_stream` is still open, detect when `response_streamer`
            // closes, which makes us close `response_stream`.
            // This signals to the isolate that the client has disconnected.
            let streamer_close = if response_stream.as_ref().is_closed() {
                // We need the conditional to avoid a busy-loop. If `response_streamer`
                // is closed, `response_streamer.sender.closed()` will resolve immediately,
                // so we make sure that only happens once.
                futures::future::Either::Left(futures::future::pending())
            } else {
                futures::future::Either::Right(response_streamer.sender.closed())
            };
            tokio::select! {
                _ = streamer_close => {
                    response_stream.close();
                },
                part = response_stream.next() => {
                    let Some(part) = part else {
                        break;
                    };
                    if let HttpActionResponsePart::Head(h) = &part {
                        result_for_logging = Some(HttpActionStatusCode(h.status));
                    }
                    // If the `response_streamer` is closed, the inner Result
                    // will have an error. That's fine; we want to keep letting
                    // the isolate send data and `response_streamer` will keep
                    // accumulating data into its hash.
                    let _ = response_streamer.send_part(part)?;
                }
            }
        }
        let response_sha256 = response_streamer.complete();
        Ok((result_for_logging, response_sha256))
    }

    async fn route_http_action(
        &self,
        tx: &mut Transaction<RT>,
        head: &HttpActionRequestHead,
    ) -> anyhow::Result<Option<(ComponentPath, RoutedHttpPath)>> {
        let mut model = BootstrapComponentsModel::new(tx);
        let mut current_component_path = ComponentPath::root();
        let mut routed_path = RoutedHttpPath(head.url.path().to_string());
        let method = RoutableMethod::try_from(head.method.clone())?;
        loop {
            let (definition_id, current_id) =
                model.must_component_path_to_ids(&current_component_path)?;
            let definition = model.load_definition_metadata(definition_id).await?;
            let module = ModuleModel::new(model.tx).get_http(current_id).await?;
            let http_routes = module
                .as_ref()
                .map(|m| {
                    m.analyze_result
                        .as_ref()
                        .context("Missing analyze result for http module")?
                        .http_routes
                        .as_ref()
                        .context("Missing http routes")
                })
                .transpose()?;

            if http_routes.is_none() && definition.http_mounts.is_empty() {
                return Ok(None);
            }

            // Apps may define an http_prefix for their http.js routes. If a prefix
            // was defined, we strip it off ahead of matching the routes against
            // http.js (which themselves are defined without the prefix).
            let http_js_routed_path = if definition.is_app()
                && let Some(ref prefix) = definition.http_prefix
            {
                routed_path
                    .strip_prefix(&prefix[..])
                    .map(|suffix| RoutedHttpPath(format!("/{suffix}")))
            } else {
                Some(routed_path.clone())
            };

            // First, try matching an exact path from `http.js`, which will always
            // be the most specific match.
            if let Some(ref effective_path) = http_js_routed_path
                && let Some(http_routes) = http_routes
                && http_routes.route_exact(&effective_path[..], method)
            {
                return Ok(Some((current_component_path, effective_path.clone())));
            }

            // Next, try finding the most specific prefix match from both `http.js`
            // and the component-level mounts.
            enum CurrentMatch<'a> {
                CurrentHttpJs,
                MountedComponent(&'a Reference),
            }
            let mut longest_match = None;

            if let Some(ref effective_path) = http_js_routed_path
                && let Some(http_routes) = http_routes
                && let Some(match_suffix) = http_routes.route_prefix(effective_path, method)
            {
                longest_match = Some((match_suffix, CurrentMatch::CurrentHttpJs));
            }
            // http_mounts are not nested under a (possible) App http_prefix - they always
            // use the original routed_path (absolute paths from root)
            for (mount_path, reference) in &definition.http_mounts {
                let Some(match_suffix) = routed_path.strip_prefix(&mount_path[..]) else {
                    continue;
                };
                let new_match = RoutedHttpPath(format!("/{match_suffix}"));
                if let Some((ref existing_suffix, _)) = longest_match {
                    // If the existing longest match has a shorter suffix, then it
                    // matches a longer prefix.
                    if existing_suffix.len() < match_suffix.len() {
                        continue;
                    }
                }
                longest_match = Some((new_match, CurrentMatch::MountedComponent(reference)));
            }
            match longest_match {
                None => {
                    // If we couldn't match the route, forward the request to the current
                    // component's `http.js` if present. This lets the JS layer uniformly handle
                    // 404s when defined.
                    if let Some(effective_path) = http_js_routed_path
                        && http_routes.is_some()
                    {
                        return Ok(Some((current_component_path, effective_path)));
                    } else {
                        return Ok(None);
                    }
                },
                Some((_, CurrentMatch::CurrentHttpJs)) => {
                    return Ok(Some((
                        current_component_path,
                        // Use the effective (prefix-stripped) path for http.js
                        http_js_routed_path
                            .expect("CurrentHttpJs match requires http_js_routed_path"),
                    )));
                },
                Some((match_suffix, CurrentMatch::MountedComponent(reference))) => {
                    let Reference::ChildComponent {
                        component: name,
                        attributes,
                    } = reference
                    else {
                        anyhow::bail!("Invalid reference in component definition: {reference:?}");
                    };
                    anyhow::ensure!(attributes.is_empty());

                    current_component_path = current_component_path.join(name.clone());
                    routed_path = match_suffix;
                    continue;
                },
            }
        }
    }
}
