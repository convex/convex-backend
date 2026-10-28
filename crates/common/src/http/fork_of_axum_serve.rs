//! Forked from axum in order to customize the builder.
//! https://github.com/tokio-rs/axum/blob/main/axum/src/serve.rs
//! https://linear.app/convex/issue/ENG-7171/check-if-axumserve-allows-configurability
//!
//! MIT Licensed https://github.com/tokio-rs/axum?tab=readme-ov-file#license
//!
//! Serve services.

use std::{
    convert::Infallible,
    fmt::Debug,
    future::{
        poll_fn,
        Future,
        IntoFuture,
    },
    io,
    marker::PhantomData,
    net::SocketAddr,
    sync::Arc,
    task::{
        Context,
        Poll,
    },
    time::Duration,
};

use axum::{
    body::Body,
    extract::{
        connect_info::Connected,
        Request,
    },
    handler::HandlerService,
    response::Response,
    routing::MethodRouter,
    Router,
};
use futures_util::{
    pin_mut,
    FutureExt,
};
use hyper::body::Incoming;
use hyper_util::{
    rt::{
        TokioExecutor,
        TokioIo,
    },
    server::conn::auto::Builder,
    service::TowerToHyperService,
};
use tokio::{
    net::{
        TcpListener,
        TcpStream,
    },
    sync::watch,
};
use tower::{
    Service,
    ServiceExt as _,
};
use tracing::{
    error,
    trace,
};

use crate::http::MAX_HTTP2_STREAMS;

/// Serve the service with the supplied listener.
///
/// This method of running a service is intentionally simple and doesn't support
/// any configuration. Use hyper or hyper-util if you need configuration.
///
/// It supports both HTTP/1 as well as HTTP/2.
///
/// # Examples
///
/// Serving a [`Router`]:
///
/// ```
/// use axum::{Router, routing::get};
///
/// # async {
/// let router = Router::new().route("/", get(|| async { "Hello, World!" }));
///
/// let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
/// axum::serve(listener, router).await.unwrap();
/// # };
/// ```
///
/// See also [`Router::into_make_service_with_connect_info`].
///
/// Serving a [`MethodRouter`]:
///
/// ```
/// use axum::routing::get;
///
/// # async {
/// let router = get(|| async { "Hello, World!" });
///
/// let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
/// axum::serve(listener, router).await.unwrap();
/// # };
/// ```
///
/// See also [`MethodRouter::into_make_service_with_connect_info`].
///
/// Serving a [`Handler`]:
///
/// ```
/// use axum::handler::HandlerWithoutStateExt;
///
/// # async {
/// async fn handler() -> &'static str {
///     "Hello, World!"
/// }
///
/// let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
/// axum::serve(listener, handler.into_make_service()).await.unwrap();
/// # };
/// ```
///
/// See also [`HandlerWithoutStateExt::into_make_service_with_connect_info`] and
/// [`HandlerService::into_make_service_with_connect_info`].
///
/// [`Router`]: crate::Router
/// [`Router::into_make_service_with_connect_info`]: crate::Router::into_make_service_with_connect_info
/// [`MethodRouter`]: crate::routing::MethodRouter
/// [`MethodRouter::into_make_service_with_connect_info`]: crate::routing::MethodRouter::into_make_service_with_connect_info
/// [`Handler`]: crate::handler::Handler
/// [`HandlerWithoutStateExt::into_make_service_with_connect_info`]: crate::handler::HandlerWithoutStateExt::into_make_service_with_connect_info
/// [`HandlerService::into_make_service_with_connect_info`]: crate::handler::HandlerService::into_make_service_with_connect_info
pub fn serve<M, S>(tcp_listener: TcpListener, make_service: M) -> Serve<M, S>
where
    M: for<'a> Service<IncomingStream<'a>, Error = Infallible, Response = S>,
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
{
    Serve {
        tcp_listener,
        make_service,
        tcp_nodelay: None,
        _marker: PhantomData,
    }
}

/// Future returned by [`serve`].
#[must_use = "futures must be awaited or polled"]
pub struct Serve<M, S> {
    tcp_listener: TcpListener,
    make_service: M,
    tcp_nodelay: Option<bool>,
    _marker: PhantomData<S>,
}

impl<M, S> Serve<M, S> {
    /// Prepares a server to handle graceful shutdown when the provided future
    /// completes.
    ///
    /// # Example
    ///
    /// ```
    /// use axum::{Router, routing::get};
    ///
    /// # async {
    /// let router = Router::new().route("/", get(|| async { "Hello, World!" }));
    ///
    /// let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    /// axum::serve(listener, router)
    ///     .with_graceful_shutdown(shutdown_signal())
    ///     .await
    ///     .unwrap();
    /// # };
    ///
    /// async fn shutdown_signal() {
    ///     // ...
    /// }
    /// ```
    pub fn with_graceful_shutdown<F>(self, signal: F) -> WithGracefulShutdown<M, S, F>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        WithGracefulShutdown {
            tcp_listener: self.tcp_listener,
            make_service: self.make_service,
            signal,
            tcp_nodelay: self.tcp_nodelay,
            _marker: PhantomData,
        }
    }

    /// Instructs the server to set the value of the `TCP_NODELAY` option on
    /// every accepted connection.
    ///
    /// See also [`TcpStream::set_nodelay`].
    ///
    /// # Example
    /// ```
    /// use axum::{Router, routing::get};
    ///
    /// # async {
    /// let router = Router::new().route("/", get(|| async { "Hello, World!" }));
    ///
    /// let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    /// axum::serve(listener, router)
    ///     .tcp_nodelay(true)
    ///     .await
    ///     .unwrap();
    /// # };
    /// ```
    pub fn tcp_nodelay(self, nodelay: bool) -> Self {
        Self {
            tcp_nodelay: Some(nodelay),
            ..self
        }
    }
}

impl<M, S> Debug for Serve<M, S>
where
    M: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            tcp_listener,
            make_service,
            tcp_nodelay,
            _marker: _,
        } = self;

        f.debug_struct("Serve")
            .field("tcp_listener", tcp_listener)
            .field("make_service", make_service)
            .field("tcp_nodelay", tcp_nodelay)
            .finish()
    }
}

impl<M, S> IntoFuture for Serve<M, S>
where
    M: for<'a> Service<IncomingStream<'a>, Error = Infallible, Response = S> + Send + 'static,
    for<'a> <M as Service<IncomingStream<'a>>>::Future: Send,
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
{
    type IntoFuture = private::ServeFuture;
    type Output = io::Result<()>;

    fn into_future(self) -> Self::IntoFuture {
        private::ServeFuture(Box::pin(async move {
            let Self {
                tcp_listener,
                mut make_service,
                tcp_nodelay,
                _marker: _,
            } = self;

            loop {
                let (tcp_stream, remote_addr) = match tcp_accept(&tcp_listener).await {
                    Some(conn) => conn,
                    None => continue,
                };

                if let Some(nodelay) = tcp_nodelay
                    && let Err(err) = tcp_stream.set_nodelay(nodelay)
                {
                    trace!("failed to set TCP_NODELAY on incoming connection: {err:#}");
                }

                let tcp_stream = TokioIo::new(tcp_stream);

                poll_fn(|cx| make_service.poll_ready(cx))
                    .await
                    .unwrap_or_else(|err| match err {});

                let tower_service = make_service
                    .call(IncomingStream {
                        tcp_stream: &tcp_stream,
                        remote_addr,
                    })
                    .await
                    .unwrap_or_else(|err| match err {})
                    .map_request(|req: Request<Incoming>| req.map(Body::new));

                let hyper_service = TowerToHyperService::new(tower_service);

                let mut builder = Builder::new(TokioExecutor::new());
                builder.http2().max_concurrent_streams(MAX_HTTP2_STREAMS);

                crate::runtime::tokio_spawn("axum_serve_conn", async move {
                    match builder
                        // upgrades needed for websockets
                        .serve_connection_with_upgrades(tcp_stream, hyper_service)
                        .await
                    {
                        Ok(()) => {},
                        Err(_err) => {
                            // This error only appears when the client doesn't
                            // send a request and
                            // terminate the connection.
                            //
                            // If client sends one request then terminate
                            // connection whenever, it doesn't
                            // appear.
                        },
                    }
                });
            }
        }))
    }
}

/// Serve future with graceful shutdown enabled.
#[must_use = "futures must be awaited or polled"]
pub struct WithGracefulShutdown<M, S, F> {
    tcp_listener: TcpListener,
    make_service: M,
    signal: F,
    tcp_nodelay: Option<bool>,
    _marker: PhantomData<S>,
}

impl<M, S, F> WithGracefulShutdown<M, S, F> {
    /// Instructs the server to set the value of the `TCP_NODELAY` option on
    /// every accepted connection.
    ///
    /// See also [`TcpStream::set_nodelay`].
    ///
    /// # Example
    /// ```
    /// use axum::{Router, routing::get};
    ///
    /// # async {
    /// let router = Router::new().route("/", get(|| async { "Hello, World!" }));
    ///
    /// let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    /// axum::serve(listener, router)
    ///     .with_graceful_shutdown(shutdown_signal())
    ///     .tcp_nodelay(true)
    ///     .await
    ///     .unwrap();
    /// # };
    ///
    /// async fn shutdown_signal() {
    ///     // ...
    /// }
    /// ```
    pub fn tcp_nodelay(self, nodelay: bool) -> Self {
        Self {
            tcp_nodelay: Some(nodelay),
            ..self
        }
    }
}

impl<M, S, F> Debug for WithGracefulShutdown<M, S, F>
where
    M: Debug,
    S: Debug,
    F: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            tcp_listener,
            make_service,
            signal,
            tcp_nodelay,
            _marker: _,
        } = self;

        f.debug_struct("WithGracefulShutdown")
            .field("tcp_listener", tcp_listener)
            .field("make_service", make_service)
            .field("signal", signal)
            .field("tcp_nodelay", tcp_nodelay)
            .finish()
    }
}

impl<M, S, F> IntoFuture for WithGracefulShutdown<M, S, F>
where
    M: for<'a> Service<IncomingStream<'a>, Error = Infallible, Response = S> + Send + 'static,
    for<'a> <M as Service<IncomingStream<'a>>>::Future: Send,
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
    F: Future<Output = ()> + Send + 'static,
{
    type IntoFuture = private::ServeFuture;
    type Output = io::Result<()>;

    fn into_future(self) -> Self::IntoFuture {
        let Self {
            tcp_listener,
            mut make_service,
            signal,
            tcp_nodelay,
            _marker: _,
        } = self;

        let (signal_tx, signal_rx) = watch::channel(());
        let signal_tx = Arc::new(signal_tx);
        crate::runtime::tokio_spawn("await_graceful_shutdown", async move {
            signal.await;
            trace!("received graceful shutdown signal. Telling tasks to shutdown");
            drop(signal_rx);
        });

        let (close_tx, close_rx) = watch::channel(());

        private::ServeFuture(Box::pin(async move {
            loop {
                let (tcp_stream, remote_addr) = tokio::select! {
                    conn = tcp_accept(&tcp_listener) => {
                        match conn {
                            Some(conn) => conn,
                            None => continue,
                        }
                    }
                    _ = signal_tx.closed() => {
                        trace!("signal received, not accepting new connections");
                        break;
                    }
                };

                if let Some(nodelay) = tcp_nodelay
                    && let Err(err) = tcp_stream.set_nodelay(nodelay)
                {
                    trace!("failed to set TCP_NODELAY on incoming connection: {err:#}");
                }

                let tcp_stream = TokioIo::new(tcp_stream);

                trace!("connection {remote_addr} accepted");

                poll_fn(|cx| make_service.poll_ready(cx))
                    .await
                    .unwrap_or_else(|err| match err {});

                let tower_service = make_service
                    .call(IncomingStream {
                        tcp_stream: &tcp_stream,
                        remote_addr,
                    })
                    .await
                    .unwrap_or_else(|err| match err {})
                    .map_request(|req: Request<Incoming>| req.map(Body::new));

                let hyper_service = TowerToHyperService::new(tower_service);

                let signal_tx = Arc::clone(&signal_tx);

                let close_rx = close_rx.clone();

                crate::runtime::tokio_spawn("axum_serve_conn", async move {
                    let mut builder = Builder::new(TokioExecutor::new());
                    builder.http2().max_concurrent_streams(MAX_HTTP2_STREAMS);
                    let conn = builder.serve_connection_with_upgrades(tcp_stream, hyper_service);
                    pin_mut!(conn);

                    let signal_closed = signal_tx.closed().fuse();
                    pin_mut!(signal_closed);

                    loop {
                        tokio::select! {
                            result = conn.as_mut() => {
                                if let Err(_err) = result {
                                    trace!("failed to serve connection: {_err:#}");
                                }
                                break;
                            }
                            _ = &mut signal_closed => {
                                trace!("signal received in task, starting graceful shutdown");
                                conn.as_mut().graceful_shutdown();
                            }
                        }
                    }

                    trace!("connection {remote_addr} closed");

                    drop(close_rx);
                });
            }

            drop(close_rx);
            drop(tcp_listener);

            trace!(
                "waiting for {} task(s) to finish",
                close_tx.receiver_count()
            );
            close_tx.closed().await;

            Ok(())
        }))
    }
}

fn is_connection_error(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
    )
}

async fn tcp_accept(listener: &TcpListener) -> Option<(TcpStream, SocketAddr)> {
    match listener.accept().await {
        Ok(conn) => Some(conn),
        Err(e) => {
            if is_connection_error(&e) {
                return None;
            }

            // [From `hyper::Server` in 0.14](https://github.com/hyperium/hyper/blob/v0.14.27/src/server/tcp.rs#L186)
            //
            // > A possible scenario is that the process has hit the max open files
            // > allowed, and so trying to accept a new connection will fail with
            // > `EMFILE`. In some cases, it's preferable to just wait for some time, if
            // > the application will likely close some files (or connections), and try
            // > to accept the connection again. If this option is `true`, the error
            // > will be logged at the `error` level, since it is still a big deal,
            // > and then the listener will sleep for 1 second.
            //
            // hyper allowed customizing this but axum does not.
            error!("accept error: {e}");
            tokio::time::sleep(Duration::from_secs(1)).await;
            None
        },
    }
}

mod private {
    use std::{
        future::Future,
        io,
        pin::Pin,
        task::{
            Context,
            Poll,
        },
    };

    pub struct ServeFuture(pub(super) futures_util::future::BoxFuture<'static, io::Result<()>>);

    impl Future for ServeFuture {
        type Output = io::Result<()>;

        #[inline]
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            self.0.as_mut().poll(cx)
        }
    }

    impl std::fmt::Debug for ServeFuture {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("ServeFuture").finish_non_exhaustive()
        }
    }
}

/// An incoming stream.
///
/// Used with [`serve`] and [`IntoMakeServiceWithConnectInfo`].
///
/// [`IntoMakeServiceWithConnectInfo`]: crate::extract::connect_info::IntoMakeServiceWithConnectInfo
#[derive(Debug)]
pub struct IncomingStream<'a> {
    tcp_stream: &'a TokioIo<TcpStream>,
    remote_addr: SocketAddr,
}

impl IncomingStream<'_> {
    /// Returns the local address that this stream is bound to.
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.tcp_stream.inner().local_addr()
    }

    /// Returns the remote address that this stream is bound to.
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}

const _: () = {
    impl Service<IncomingStream<'_>> for Router<()> {
        type Error = Infallible;
        type Future = std::future::Ready<Result<Self::Response, Self::Error>>;
        type Response = Self;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _req: IncomingStream<'_>) -> Self::Future {
            // call `Router::with_state` such that everything is turned into `Route` eagerly
            // rather than doing that per request
            std::future::ready(Ok(self.clone().with_state(())))
        }
    }
};

const _: () = {
    impl Connected<IncomingStream<'_>> for SocketAddr {
        fn connect_info(target: IncomingStream<'_>) -> Self {
            target.remote_addr()
        }
    }
};

const _: () = {
    impl Service<IncomingStream<'_>> for MethodRouter<()> {
        type Error = Infallible;
        type Future = std::future::Ready<Result<Self::Response, Self::Error>>;
        type Response = Self;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _req: IncomingStream<'_>) -> Self::Future {
            std::future::ready(Ok(self.clone().with_state(())))
        }
    }
};

const _: () = {
    impl<H, T, S> Service<IncomingStream<'_>> for HandlerService<H, T, S>
    where
        H: Clone,
        S: Clone,
    {
        type Error = Infallible;
        type Future = std::future::Ready<Result<Self::Response, Self::Error>>;
        type Response = Self;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _req: IncomingStream<'_>) -> Self::Future {
            std::future::ready(Ok(self.clone()))
        }
    }
};
