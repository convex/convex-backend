use std::{
    self,
    convert::Infallible,
    sync::Arc,
};

use errors::ErrorMetadata;
use fnv::FnvHashMap;
use futures::Future;
use pb::error_metadata::ErrorMetadataStatusExt;
use pb_extras::ReflectionService;
use sentry::integrations::tower as sentry_tower;
use tonic::{
    server::NamedService,
    service::Routes,
    Response,
    Status,
};
use tonic_health::{
    server::{
        health_reporter,
        HealthReporter,
    },
    ServingStatus,
};
use tonic_middleware::MiddlewareLayer;
use tower::ServiceBuilder;

use crate::{
    http::MakeSocket,
    knobs::HTTP_SERVER_TCP_BACKLOG,
};

mod middleware;

// maps the full route `/service.Service/Method` to just `Method`
type KnownMethods = FnvHashMap<String, &'static str>;
pub struct ConvexGrpcService {
    routes: Routes,
    known_methods: KnownMethods,
    health_reporter: HealthReporter,
    service_names: Vec<&'static str>,
}

impl ConvexGrpcService {
    pub fn new() -> Self {
        let (health_reporter, health_service) = health_reporter();
        let routes = Routes::new(health_service);
        Self {
            routes,
            known_methods: FnvHashMap::default(),
            health_reporter,
            service_names: Vec::new(),
        }
    }

    pub fn add_service<S>(mut self, service: S) -> Self
    where
        S: tower::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<tonic::body::Body>,
                Error = Infallible,
            > + ReflectionService
            + Clone
            + Send
            + Sync
            + 'static,
        S::Future: Send + 'static,
    {
        self.routes = self.routes.add_service(service);
        // Gather all service names so we can mark them all as healthy and print one
        // line with all names when we start serving.
        let service_name = <S as NamedService>::NAME;
        self.service_names.push(service_name);
        for method_name in S::METHODS {
            self.known_methods
                .insert(format!("/{service_name}/{method_name}"), method_name);
        }
        self
    }

    pub async fn serve<F>(self, addr: impl MakeSocket, shutdown: F) -> anyhow::Result<()>
    where
        F: Future<Output = ()>,
    {
        let known_methods = Arc::new(self.known_methods);
        let convex_layers = ServiceBuilder::new()
            .layer(MiddlewareLayer::new(middleware::LoggingMiddleware::new(
                known_methods.clone(),
            )))
            .layer(crate::fastrace_helpers::layer::TraceparentReceivingLayer)
            .layer_fn(|s| middleware::TokioInstrumentationService::new(known_methods.clone(), s))
            .layer(sentry_tower::NewSentryLayer::new_from_top())
            .layer(sentry_tower::SentryHttpLayer::new());

        let socket = addr.make_socket()?;
        let local_addr = socket.local_addr()?;
        tracing::info!(
            "gRPC services {} listening on ipv4://{local_addr}",
            self.service_names.join(",")
        );
        for service_name in self.service_names {
            self.health_reporter
                .set_service_status(service_name, ServingStatus::Serving)
                .await;
        }
        let listener = socket.listen(*HTTP_SERVER_TCP_BACKLOG)?;
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        tonic::transport::Server::builder()
            // Internal Convex gRPC clients can validly cancel many streams on
            // long-lived connections; don't let h2's external-peer abuse
            // heuristic close those trusted connections.
            // TODO: Apply the same policy to internal gRPC clients once tonic
            // exposes h2's client-side max_local_error_reset_streams setting.
            // The Sentry "too_many_internal_resets" issues are likely from
            // client-side channels, so this server setting alone is incomplete.
            .http2_max_local_error_reset_streams(None)
            .layer(convex_layers)
            .add_routes(self.routes)
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await?;
        tracing::info!("GRPC server shutdown complete");
        Ok(())
    }
}

pub fn handle_response<T>(response: Result<Response<T>, Status>) -> anyhow::Result<T> {
    match response {
        Ok(response) => Ok(response.into_inner()),
        Err(status) => Err(status.into_anyhow()),
    }
}

/// Returns true if `status` is a failure to *establish* the connection to the
/// server (connection refused, host down, connect timeout, …).
///
/// Tonic wraps every connection-establishment error in [`tonic::ConnectError`]
/// and nothing on the request/response data path produces one, so its presence
/// anywhere in the error's source chain proves the request was never
/// dispatched — making it safe to retry on another upstream even for
/// non-idempotent operations. We key on `ConnectError` rather than
/// [`tonic::Code::Unavailable`] precisely because some mid-flight failures
/// (e.g. an h2 `REFUSED_STREAM`) also map to `Unavailable` but may have reached
/// the server.
pub fn is_connect_error(status: &Status) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(status);
    while let Some(err) = source {
        if err.is::<tonic::ConnectError>() {
            return true;
        }
        source = err.source();
    }
    false
}

/// Like [`handle_response`], but tags connection-establishment failures as
/// [`ErrorMetadata::rejected_before_execution`] so callers that fail over to
/// another upstream retry them (see [`is_connect_error`]). Apply this to the
/// *initial* response of an RPC, not to errors surfaced while consuming a
/// response stream — those may be mid-flight and unsafe to retry.
pub fn handle_response_retry_on_connect<T>(
    response: Result<Response<T>, Status>,
) -> anyhow::Result<T> {
    match response {
        Ok(response) => Ok(response.into_inner()),
        Err(status) if is_connect_error(&status) => {
            Err(status
                .into_anyhow()
                .context(ErrorMetadata::rejected_before_execution(
                    "UpstreamConnectError",
                    "Failed to connect to upstream",
                )))
        },
        Err(status) => Err(status.into_anyhow()),
    }
}
