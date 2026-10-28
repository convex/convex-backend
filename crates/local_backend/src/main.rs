use std::time::Duration;

use clap::Parser;
use cmd_util::env::{
    config_service,
    config_tool,
};
use common::{
    errors::MainError,
    http::ConvexHttpService,
    knobs::HTTP_SERVER_TIMEOUT_DURATION,
    runtime::Runtime,
    sentry::set_sentry_tags,
    shutdown::ShutdownSignal,
    types::{
        DeploymentId,
        MemberId,
    },
    version::SERVER_VERSION_STR,
};
use db_connection::{
    connect_persistence,
    ConnectPersistenceFlags,
};
use futures::{
    future::{
        self,
        Either,
    },
    FutureExt,
};
use keybroker::{
    DeploymentSecret,
    KeyBroker,
};
use local_backend::{
    config::{
        AdminKeyArgs,
        KeygenCommand,
        LocalConfig,
        Subcommand,
    },
    make_app,
    proxy::dev_site_proxy,
    router::router,
    HttpActionRouteMapper,
    MAX_CONCURRENT_REQUESTS,
};
use runtime::prod::ProdRuntime;
use tokio::{
    signal::{
        self,
    },
    sync::oneshot,
};

fn main() -> Result<(), MainError> {
    let config = LocalConfig::parse();
    if let Some(subcommand) = &config.subcommand {
        // Subcommands produce machine-parseable output on stdout, so configure
        // tracing to write to stderr (at ERROR level) to avoid contaminating it.
        let _guard = config_tool();
        return run_subcommand(subcommand);
    }
    let _guard = config_service();
    tracing::info!("Starting a Convex backend");
    if !config.disable_beacon {
        tracing::info!(
            "The self-host Convex backend will periodically communicate with a remote beacon \
             server. This is to help Convex understand and improve the product. You can disable \
             this telemetry by setting the --disable-beacon flag or the DISABLE_BEACON \
             environment variable."
        );
    }
    let sentry = sentry::init(sentry::ClientOptions {
        release: Some(format!("local-backend@{}", *SERVER_VERSION_STR).into()),
        ..Default::default()
    });
    if sentry.is_enabled() {
        tracing::info!(
            "Sentry is enabled. Errors will be reported to project with ID {}",
            sentry
                .dsn()
                .map(|dsn| dsn.project_id().to_string())
                .unwrap_or("unknown".to_string())
        );
        sentry::configure_scope(|scope| {
            if let Some(sentry_identifier) = config.sentry_identifier.clone() {
                scope.set_user(Some(sentry::User {
                    id: Some(sentry_identifier),
                    ..Default::default()
                }));
            }
            set_sentry_tags(scope);
        });
    } else {
        tracing::info!("Sentry is not enabled.")
    }

    let tokio = ProdRuntime::init_tokio()?;
    let runtime = ProdRuntime::new(&tokio);

    let runtime_ = runtime.clone();
    let server_future = async {
        run_server(runtime_, config).await?;
        Ok(())
    };

    runtime.block_on("main", server_future)
}

fn run_subcommand(command: &Subcommand) -> Result<(), MainError> {
    match command {
        Subcommand::Keygen {
            kind: KeygenCommand::AdminKey(args),
        } => generate_admin_key(args),
    }
}

fn generate_admin_key(args: &AdminKeyArgs) -> Result<(), MainError> {
    let secret = DeploymentSecret::try_from(args.instance_secret.as_str())?;
    let broker = KeyBroker::new(&args.instance_name, secret)?;
    let admin_key = broker.issue_admin_key(MemberId(0));
    println!("{}", admin_key.as_str());
    Ok(())
}

async fn run_server(runtime: ProdRuntime, config: LocalConfig) -> anyhow::Result<()> {
    let serve_future = async move { run_server_inner(runtime, config).await }.fuse();
    futures::pin_mut!(serve_future);

    futures::select! {
        r = serve_future => {
            r?;
            tracing::info!("Done")
        },
    };

    Ok(())
}

async fn run_server_inner(runtime: ProdRuntime, config: LocalConfig) -> anyhow::Result<()> {
    // Used to receive fatal errors from the database or /preempt endpoint.
    let (preempt_tx, preempt_rx) = oneshot::channel();
    let preempt_signal = ShutdownSignal::new(preempt_tx);
    // Use to signal to the http service to stop.
    let (shutdown_tx, shutdown_rx) = async_broadcast::broadcast(1);
    let persistence = connect_persistence(
        config.db,
        &config.db_spec,
        ConnectPersistenceFlags {
            require_ssl: !config.do_not_require_ssl,
            allow_read_only: false,
            skip_index_creation: false,
        },
        &config.name(),
        // No control plane hands a self-hosted backend an ID, so derive a
        // stable one from its name.
        Some(DeploymentId::stable_from_name(&config.name())),
        runtime.clone(),
        preempt_signal.clone(),
    )
    .await?;
    let st = make_app(
        runtime.clone(),
        config.clone(),
        persistence,
        shutdown_rx.clone(),
        preempt_signal.clone(),
    )
    .await?;
    let router = router(st.clone());
    let mut shutdown_rx_ = shutdown_rx.clone();
    let http_service = ConvexHttpService::new(
        router,
        "backend",
        SERVER_VERSION_STR.to_string(),
        MAX_CONCURRENT_REQUESTS,
        *HTTP_SERVER_TIMEOUT_DURATION,
        HttpActionRouteMapper,
    );
    let serve_http_future = http_service.serve(config.http_bind_address(), async move {
        let _ = shutdown_rx_.recv().await;
    });
    let proxy_future = dev_site_proxy(
        config.site_bind_address(),
        config.site_forward_prefix(),
        shutdown_rx,
    );

    let serve_future = future::try_join(serve_http_future, proxy_future).fuse();
    futures::pin_mut!(serve_future);

    // Start shutdown when we get a manual shutdown signal or with the first
    // ctrl-c.
    let mut force_exit_duration = None;
    futures::select! {
        r = serve_future => {
            r?;
            panic!("Serve future stopped unexpectedly!")
        },
        _err = preempt_rx.fuse() => {
            // If we fail with a fatal error, we want to exit immediately.
            tracing::info!("Received a fatal error. Shutting down immediately");
            force_exit_duration = Some(Duration::from_secs(0));
            let _: Result<_, _> = shutdown_tx.broadcast(()).await;
        }
        r = signal::ctrl_c().fuse() => {
            tracing::info!("Received Ctrl-C signal!");
            r?;
            let _: Result<_, _> = shutdown_tx.broadcast(()).await;
        },
    }

    let shutdown = async move {
        // First, drain all in-progress requests;
        tracing::info!("Shutdown initiated, draining existing requests...");
        serve_future.await?;

        // Next, shutdown all of our asynchronous workers.
        tracing::info!("Shutting down application...");
        st.shutdown().await?;

        Ok::<_, anyhow::Error>(())
    }
    .fuse();
    futures::pin_mut!(shutdown);

    let mut force_exit_future = match force_exit_duration {
        Some(force_exit_duration) => Either::Left(runtime.wait(force_exit_duration)),
        None => Either::Right(std::future::pending()),
    }
    .fuse();

    loop {
        futures::select! {
            r = shutdown => {
                r?;
                tracing::info!("Server successfully shut down.");
                // If we are not preempted we exit as soon as the requests are
                // drained. Otherwise, we have to wait for the cool down.
                if force_exit_duration.is_none() {
                    break;
                }
            },
            // Forcibly shutdown when the cool down expires
            _ = force_exit_future => {
                tracing::info!("Cool down expired. Shutting down");
                break;
            }
            // Forcibly shutdown with second ctrl-c.
            r = signal::ctrl_c().fuse() => {
                r?;
                tracing::warn!("Forcibly shutting down!");
                break;
            },
        }
    }

    Ok(())
}
