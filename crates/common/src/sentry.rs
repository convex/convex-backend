use std::sync::Arc;

use futures::Stream;
use reqwest::Url;
use sentry::{
    transports::ReqwestHttpTransport,
    ClientOptions,
    Hub,
    Scope,
    Transport,
    TransportFactory,
};

/// Creates a Sentry transport factory with Convex's egress proxy and TLS
/// policy. Defers HTTP client allocation until `create_transport` is called.
pub fn proxied_sentry_transport_factory(
    proxy_url: Option<Url>,
    client_id: String,
) -> Arc<dyn TransportFactory> {
    Arc::new(move |options: &ClientOptions| -> Arc<dyn Transport> {
        let client = crate::http::fetch::build_proxied_reqwest_client(
            proxy_url.clone(),
            client_id.clone(),
            reqwest::redirect::Policy::none(),
        );
        Arc::new(ReqwestHttpTransport::with_client(options, client))
    })
}

pub fn set_sentry_tags(scope: &mut Scope) {
    if let Ok(alloc_id) = std::env::var("NOMAD_ALLOC_ID") {
        scope.set_tag("nomad_alloc_id", alloc_id);
    }
    if let Ok(job_name) = std::env::var("NOMAD_JOB_NAME") {
        scope.set_tag("nomad_job_name", job_name);
    }
    if let Ok(group_name) = std::env::var("NOMAD_GROUP_NAME") {
        scope.set_tag("nomad_group_name", group_name);
    }
    if let Ok(task_name) = std::env::var("NOMAD_TASK_NAME") {
        scope.set_tag("nomad_task_name", task_name);
    }
    if let Ok(dc) = std::env::var("NOMAD_DC") {
        scope.set_tag("nomad_dc", dc);
    }
    if let Ok(shell_user) = std::env::var("USER") {
        scope.set_tag("shell_user", shell_user);
    }
}

pub trait SentryStreamExt: Stream + Sized {
    /// Like [sentry::SentryFutureExt::bind_hub], but for streams.
    fn bind_hub(self, hub: Arc<Hub>) -> BindHubStream<Self>;
}
impl<S: Stream> SentryStreamExt for S {
    fn bind_hub(self, hub: Arc<Hub>) -> BindHubStream<Self> {
        BindHubStream { stream: self, hub }
    }
}

#[pin_project::pin_project]
pub struct BindHubStream<S> {
    #[pin]
    stream: S,
    hub: Arc<Hub>,
}

impl<S: Stream> Stream for BindHubStream<S> {
    type Item = S::Item;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.project();
        Hub::run(this.hub.clone(), || this.stream.poll_next(cx))
    }
}
