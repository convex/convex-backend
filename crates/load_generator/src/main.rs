#![feature(exit_status_error)]
#![feature(try_blocks)]
#![feature(error_iter)]
use std::{
    io::ErrorKind,
    net::SocketAddr,
    path::{
        Path,
        PathBuf,
    },
    sync::{
        Arc,
        LazyLock,
    },
    time::Duration,
};

use ::metrics::{
    StaticMetricLabel,
    SERVER_VERSION_STR,
};
use anyhow::Context;
use axum::{
    extract::State,
    response::IntoResponse,
    routing::get,
    Router,
};
use backend_harness::{
    with_provision,
    BackendProvisioner,
    ProvisionHostCredentials,
    ProvisionRequest,
};
use clap::Parser;
use cmd_util::env::config_service;
use common::{
    errors::{
        report_error,
        MainError,
    },
    http::{
        websocket::{
            Message,
            WebSocket,
            WebSocketUpgrade,
        },
        ConvexHttpService,
        HttpResponseError,
        NoopRouteMapper,
    },
    sentry::set_sentry_tags,
};
use event_receiver::Event;
use futures::{
    stream::SplitSink,
    FutureExt,
    SinkExt,
    StreamExt,
};
use health_check::wait_for_http_health;
use log_interleaver::LogInterleaver;
use performance_stats::exporter::register_prometheus_exporter;
use runtime::prod::ProdRuntime;
use serde::{
    Deserialize,
    Serialize,
};
use strum::Display;
use tokio::{
    process::Command,
    sync::mpsc,
    time::sleep,
};

use crate::setup::setup;

mod event_receiver;
mod metrics;
mod setup;
mod stats;
use crate::{
    event_receiver::EventProcessor,
    stats::Stats,
};

static SCENARIO_RUNNER_PATH: LazyLock<&'static Path> =
    LazyLock::new(|| Path::new("npm-packages/scenario-runner"));
static MAX_JITTER_SECONDS: u64 = 30;

fn default_num_rows() -> u64 {
    500
}

#[derive(Clone, Debug, Deserialize)]
struct Workload {
    name: String,
    scenarios: Vec<ScenarioConfig>,
    /// Number of vector rows for the initial setup mutation
    #[serde(default)]
    num_vector_rows: u64,
    /// Number of messages rows for the initial setup mutation
    #[serde(default = "default_num_rows")]
    num_rows: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ScenarioConfig {
    #[serde(flatten)]
    scenario: Scenario,
    #[serde(flatten)]
    mode: Mode,
}

#[derive(Debug, Clone, Deserialize, Serialize, Display)]
#[serde(rename_all = "snake_case")]
enum FunctionType {
    Query,
    Mutation,
    Action,
}

#[derive(Deserialize, Serialize, Debug, Clone, Display)]
#[serde(tag = "name")]
enum Scenario {
    RunFunction {
        path: String,
        fn_type: FunctionType,
    },
    ObserveInsert {
        search_indexes: bool,
    },
    Search,
    VectorSearch,
    SnapshotExport,
    ManyIntersections {
        num_subscriptions: i32,
    },
    HoldSubscriptions {
        num_subscriptions: i32,
        hold_duration_secs: f64,
        #[serde(default)]
        invalidation_interval_secs: Option<f64>,
        #[serde(default)]
        num_invalidations: Option<i32>,
    },
    CloudBackup,
    RunHttpAction {
        path: String,
        method: String,
    },
}

impl Scenario {
    fn includes_action(&self) -> bool {
        match self {
            Scenario::RunFunction {
                fn_type: FunctionType::Action,
                ..
            } => true,
            Scenario::RunFunction { .. }
            | Scenario::ObserveInsert { .. }
            | Scenario::Search
            | Scenario::VectorSearch
            | Scenario::SnapshotExport
            | Scenario::CloudBackup
            | Scenario::RunHttpAction { .. } => false,
            Scenario::ManyIntersections { .. } => false,
            Scenario::HoldSubscriptions { .. } => false,
        }
    }

    fn path(&self) -> Option<String> {
        match self {
            Scenario::RunFunction { path, .. } => Some(path.clone()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum Mode {
    /// Number requests to send per second
    Rate(f64),
    /// Number of threads to run in benchmark mode. Each thread will run
    /// requests serially, waiting for a response before sending the next
    /// request.
    Benchmark(u32),
}

fn parse_workload_config(path: &str) -> anyhow::Result<Workload> {
    let s = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read workload config file at {path}"))?;
    let workload = serde_json::from_str(&s)?;
    Ok(workload)
}

#[derive(Parser, Debug)]
#[clap(group(clap::ArgGroup::new("provision").multiple(false)))]
pub struct Config {
    /// Host interface to bind to
    #[clap(short, long, default_value = "0.0.0.0")]
    pub interface: ::std::net::Ipv4Addr,
    /// Host port HTTP server should use
    #[clap(short, long, default_value = "8010")]
    pub port: u16,
    /// address:port that metrics will be served on
    /// (0.0.0.0:9100 is usually what you want in production)
    #[clap(short, long)]
    pub metrics_addr: Option<SocketAddr>,
    /// Seconds LoadGenerator should run for
    #[clap(long)]
    duration: u64,
    /// Path to workload config
    #[clap(value_parser = parse_workload_config)]
    workload: Workload,
    /// Where to provision a backend from for load generation
    #[clap(
        long,
        value_enum,
        group = "provision",
        required_unless_present = "existing_instance_url"
    )]
    provisioner: Option<BackendProvisioner>,
    /// Print out a stats report after duration has passed
    #[clap(long)]
    stats_report: bool,
    /// If set, skip rebuilding artifacts (scenario-runner)
    #[clap(long)]
    skip_build: bool,
    #[clap(long)]
    once: bool,
    #[clap(long)]
    existing_project_slug: Option<String>,
    #[clap(long, group = "provision", requires = "existing_instance_admin_key")]
    existing_instance_url: Option<String>,
    #[clap(long)]
    existing_instance_admin_key: Option<String>,
    // This is a somewhat hacky flag that allows us to skip creating actions.
    // We need it to avoid creating too many AWS lambda on production, before
    // having a way to vacuum them.
    #[clap(long)]
    skip_actions_deploy: bool,
    /// Preview deployments -- We'll create `num_preview_deployments` and push
    /// them `num_preview_deployment_pushes` times in a loop.
    #[clap(long)]
    use_preview_deployments: bool,
    #[clap(long, requires = "use_preview_deployments")]
    num_preview_deployments: Option<u64>,
    #[clap(long, requires = "use_preview_deployments")]
    num_preview_deployment_pushes: Option<u64>,
    #[clap(long)]
    use_usher_test_url: bool,
}

impl Config {
    fn load_description(&self) -> String {
        format!("{}_{}s", self.workload.name, self.duration)
    }
}

async fn run(config: &Config) -> anyhow::Result<()> {
    let logs = LogInterleaver::new();

    let load_description = config.load_description();
    let load_description_label = StaticMetricLabel::new("load_description", load_description);

    tracing::info!("provisioning with {:?}", config.provisioner);

    // HACK: Allow running load generator from any directory in our repo.
    if let Ok(current_workspace) = std::env::var("CARGO_WORKSPACE_ROOT") {
        std::env::set_current_dir(current_workspace)
            .context("Failed to set current dir to repo root")?;
    }

    if !config.skip_build {
        tracing::info!("building scenario-runner");
        logs.spawn_with_prefixed_logs(
            "turbo build".into(),
            Command::new("just")
                .arg("turbo")
                .arg("run")
                .arg("build")
                .arg("--filter=scenario-runner...")
                .current_dir("npm-packages"),
        )
        .context("Couldn't spawn turbo build in npm-packages/. Run from repo root?")?
        .wait()
        .await?
        .exit_ok()?;
    }

    // The --skip-actions-deploy flag avoids deploying node actions.
    // Since we can't tell them apart, we ensure that no scenarios include any
    // actions, even non-node actions.
    if config.skip_actions_deploy {
        anyhow::ensure!(
            config
                .workload
                .scenarios
                .iter()
                .all(|scenario_config| !scenario_config.scenario.includes_action()),
            "Can't skip actions deploy and perform actions at the same time!"
        );
        tracing::info!("Deleting the convex/actions folder");
        if let Err(e) = tokio::fs::remove_dir_all(SCENARIO_RUNNER_PATH.join("convex/actions")).await
            && e.kind() != ErrorKind::NotFound
        {
            return Err(e.into());
        };
    };
    loop {
        if let Some(ref backend_url) = config.existing_instance_url
            && let Some(ref admin_key) = config.existing_instance_admin_key
        {
            run_workload(
                None,
                backend_url.clone(),
                admin_key.clone(),
                logs.clone(),
                config,
                None,
            )
            .await?;
        } else {
            let backend_provisioner = config
                .provisioner
                .expect("Required argument if backend url wasn't present above");

            if backend_provisioner == BackendProvisioner::Production {
                // Add jitter for provisioning to avoid claiming the same instance when load
                // generator restarts
                let jitter = rand::random::<f32>();
                let sleep_dur = Duration::from_secs(MAX_JITTER_SECONDS).mul_f32(jitter);
                tracing::info!("Sleeping for jitter {sleep_dur:?}");
                sleep(sleep_dur).await;
            }

            if config.use_preview_deployments {
                run_preview_deployment_workload(config, &logs, load_description_label.clone())
                    .await?
            } else {
                let provision_request = match &config.existing_project_slug {
                    Some(project_slug) => ProvisionRequest::ExistingProject {
                        project_slug: project_slug.to_string(),
                    },
                    None => ProvisionRequest::NewProject,
                };
                let provision_host_credentials = backend_provisioner.provision_host_credentials();
                with_provision(
                    &logs,
                    backend_provisioner,
                    &provision_request,
                    &SCENARIO_RUNNER_PATH,
                    load_description_label.clone(),
                    |mut backend_url, admin_key, local_log_sink| {
                        if config.use_usher_test_url {
                            backend_url = backend_url.replace("convex.cloud", "test.convex.cloud");
                        }
                        run_workload(
                            provision_host_credentials,
                            backend_url,
                            admin_key,
                            logs.clone(),
                            config,
                            local_log_sink,
                        )
                    },
                )
                .await?;
            };
        }

        if config.once {
            break;
        }
    }
    Ok(())
}

async fn run_preview_deployment_workload(
    config: &Config,
    logs: &LogInterleaver,
    metric_label: StaticMetricLabel,
) -> anyhow::Result<()> {
    let backend_provisioner = config
        .provisioner
        .expect("Required argument if backend url wasn't present above");
    let provision_request = match &config.existing_project_slug {
        Some(project_slug) => ProvisionRequest::ExistingProject {
            project_slug: project_slug.clone(),
        },
        None => ProvisionRequest::NewProject,
    };
    let provision_host_credentials = backend_provisioner.provision_host_credentials();

    with_provision(
        logs,
        backend_provisioner,
        &provision_request,
        &SCENARIO_RUNNER_PATH,
        metric_label.clone(),
        |_a, _b, _c| async move {
            for _ in 0..config
                .num_preview_deployment_pushes
                .expect("Required argument if use_preview_deployments set")
            {
                for i in 0..config
                    .num_preview_deployments
                    .expect("Required argument if use_preview_deployments set")
                {
                    let identifier = format!("test-preview-{i}");
                    let provision_request = ProvisionRequest::Preview {
                        identifier: identifier.clone(),
                    };
                    with_provision(
                        logs,
                        backend_provisioner,
                        &provision_request,
                        &SCENARIO_RUNNER_PATH,
                        metric_label.clone(),
                        |mut backend_url, admin_key, local_log_sink| {
                            if config.use_usher_test_url {
                                backend_url =
                                    backend_url.replace("convex.cloud", "test.convex.cloud");
                            }
                            run_workload(
                                provision_host_credentials.clone(),
                                backend_url,
                                admin_key,
                                logs.clone(),
                                config,
                                local_log_sink,
                            )
                        },
                    )
                    .await?;
                }
            }
            Ok(())
        },
    )
    .await?;

    Ok(())
}

fn main() -> Result<(), MainError> {
    let _guard = config_service();
    tracing::info!("starting up");
    let sentry = sentry::init(sentry::ClientOptions {
        release: Some(format!("load-generator@{}", *SERVER_VERSION_STR).into()),
        ..Default::default()
    });
    if sentry.is_enabled() {
        tracing::info!("Sentry is enabled! Check the load-generator project for errors: https://sentry.io/organizations/convex-dev/projects/load-generator/?project=6505624");
        sentry::configure_scope(|scope| {
            set_sentry_tags(scope);
        });
    } else {
        tracing::info!("Sentry is not enabled.")
    }
    let config = Config::parse();
    let tokio = ProdRuntime::init_tokio()?;
    let runtime = ProdRuntime::new(&tokio);
    let maybe_metrics = config
        .metrics_addr
        .map(|addr| register_prometheus_exporter(runtime.clone(), addr));
    let load_generator = async move {
        run(&config).await?;
        if let Some((handle, flush)) = maybe_metrics {
            flush().await;
            handle.shutdown_and_join().await?;
        }
        Ok::<_, MainError>(())
    };
    runtime.block_on("load_generator", load_generator)?;
    Ok(())
}

async fn run_workload(
    provision_host_credentials: Option<ProvisionHostCredentials>,
    backend_url: String,
    admin_key: String,
    logs: LogInterleaver,
    config: &Config,
    local_log_sink: Option<PathBuf>,
) -> anyhow::Result<()> {
    // Get the backend version - only retry twice since the instance should be up by
    // this point.
    let backend_version = wait_for_http_health(
        &backend_url.parse()?,
        concat!("load-generator-", env!("CARGO_PKG_VERSION")),
        None,
        None,
        2,
        Duration::from_millis(250),
    )
    .await?;
    let backend_version_label = StaticMetricLabel::new("backend_version", backend_version);
    setup(
        &backend_url,
        config.workload.num_rows,
        config.workload.num_vector_rows,
    )
    .await?;

    #[derive(Serialize)]
    struct Scenarios {
        scenarios: Vec<ScenarioMessage>,
    }

    // Must directly spawn node. Spawning with `npm start` means that the
    // subprocess cannot be killed directly from the parent due to
    // https://github.com/npm/npm/issues/4603
    //
    // There might be a workaround, but I haven't figured it out.
    tracing::info!("spawning a scenario-runner");
    let scenarios = Scenarios {
        scenarios: config
            .workload
            .scenarios
            .clone()
            .into_iter()
            .map(ScenarioMessage::from)
            .collect(),
    };
    let mut scenario_runner_cmd = Command::new("node");
    let mut cmd = scenario_runner_cmd
        .arg("--enable-source-maps")
        .arg("dist/scenario-runner.js")
        .arg("--deployment-url")
        .arg(backend_url)
        .arg("--admin-key")
        .arg(admin_key)
        .arg("--load-generator-port")
        .arg(config.port.to_string())
        .arg("--scenarios")
        .arg(serde_json::to_string(&scenarios).context("Failed to serialize scenarios")?)
        .current_dir(*SCENARIO_RUNNER_PATH)
        .kill_on_drop(true);

    if let Some(ProvisionHostCredentials {
        provision_host,
        access_token,
    }) = provision_host_credentials
    {
        cmd = cmd
            .arg("--provision-host")
            .arg(provision_host)
            .arg("--access-token")
            .arg(access_token)
    }

    let scenario_runner_handle = logs.spawn_with_prefixed_logs("scenario runner".into(), cmd)?;
    let load_description = config.load_description();
    let load_description_label = StaticMetricLabel::new("load_description", load_description);
    let mut load_generator = LoadGenerator::new(
        config,
        vec![load_description_label.clone(), backend_version_label],
        local_log_sink,
    )?;
    tracing::info!("Running workload: {:?}", config.workload);
    load_generator
        .run(
            Duration::from_secs(config.duration),
            config.stats_report,
            (config.interface, config.port).into(),
            vec![scenario_runner_handle],
        )
        .await?;
    Ok(())
}

#[derive(Clone)]
struct LoadGeneratorState {
    /// Transmitter to send events to the [EventProcessor]
    event_sender: mpsc::Sender<Result<Event, serde_json::Error>>,
    /// List of websocket connections with scenario-runners
    websocket_connections: Arc<tokio::sync::Mutex<Vec<SplitSink<WebSocket, Message>>>>,
}

/// LoadGenerator provisions instances and runs scenario-runner against them. It
/// has an [EventProcessor] for processing metric events from ScenarioRunner and
/// generating a stats report.
struct LoadGenerator {
    event_processor: EventProcessor,
    /// Transmitter to send events to the [EventProcessor]
    tx: mpsc::Sender<Result<Event, serde_json::Error>>,
}

impl LoadGenerator {
    fn new(
        config: &Config,
        metric_labels: Vec<StaticMetricLabel>,
        local_log_sink: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let duration = Duration::from_secs(config.duration);
        let stats = Stats::new(duration, local_log_sink);
        let (tx, rx) = mpsc::channel(200);
        Ok(Self {
            event_processor: EventProcessor {
                rx,
                stats,
                metric_labels,
            },
            tx,
        })
    }

    async fn sync_handler(
        ws: WebSocketUpgrade,
        state: State<LoadGeneratorState>,
    ) -> Result<impl IntoResponse, HttpResponseError> {
        Ok(ws.on_upgrade(|ws| Self::process_events(ws, state)))
    }

    async fn process_events(ws: WebSocket, state: State<LoadGeneratorState>) {
        let (tx, mut rx) = ws.split();
        state.websocket_connections.lock().await.push(tx);
        while let Some(message) = rx.next().await {
            match message {
                Ok(m) => match m {
                    Message::Text(s) => {
                        let event: Result<Event, _> = serde_json::from_str(&s);
                        if let Err(e) = state.event_sender.send(event).await {
                            report_error(&mut e.into()).await;
                        }
                    },
                    Message::Pong(_) | Message::Ping(_) => {
                        continue;
                    },
                    Message::Close(_) => {
                        break;
                    },
                    Message::Binary(_) => {
                        tracing::error!("Unexpected binary message");
                    },
                },
                Err(e) => {
                    report_error(&mut e.into()).await;
                },
            }
        }
    }

    async fn run(
        &mut self,
        duration: Duration,
        stats_report: bool,
        ws_server_addr: SocketAddr,
        scenario_runner_handles: Vec<tokio::process::Child>,
    ) -> anyhow::Result<()> {
        let state = LoadGeneratorState {
            event_sender: self.tx.clone(),
            websocket_connections: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        };
        let router = Router::new()
            .route("/sync", get(Self::sync_handler))
            .with_state(state.clone());
        let http_service = ConvexHttpService::new(
            router,
            "load-generator",
            SERVER_VERSION_STR.clone(),
            100,
            Duration::from_secs(60),
            NoopRouteMapper,
        );
        let serve_http_future = http_service.serve(
            ws_server_addr,
            tokio::signal::ctrl_c().map(|_| {
                tracing::info!("Shutting down load_generator http server");
            }),
        );

        let receive_fut = self.event_processor.receive_events();
        tokio::select! {
            _ = serve_http_future => {
                tracing::error!("Http server completed unexpectedly")
            },
            _ = receive_fut => {
                tracing::error!("Event processor completed unexpectedly")
            },
            _ = tokio::time::sleep(duration) => {
                tracing::info!("{duration:?} has passed. Shutting down load generator");
                let mut websocket_connections = state.websocket_connections.lock().await;
                for mut tx in websocket_connections.drain(..) {
                    let _ = tx.send(Message::Close(None)).await;
                }
                for mut handle in scenario_runner_handles {
                    handle.kill().await?;
                }
            },
        }
        // Wait to give backend time to process any remaining log events.
        tokio::time::sleep(Duration::from_secs(10)).await;
        if stats_report {
            self.event_processor.stats.report();
        }
        self.event_processor.stats.fail_if_too_many_errors()?;
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScenarioMessage {
    scenario: Scenario,
    rate: Option<f64>,
    threads: Option<u32>,
}

impl From<ScenarioConfig> for ScenarioMessage {
    fn from(ScenarioConfig { scenario, mode }: ScenarioConfig) -> Self {
        let (rate, threads) = match mode {
            Mode::Benchmark(threads) => (None, Some(threads)),
            Mode::Rate(rate) => {
                metrics::log_target_qps(&scenario.to_string(), rate, scenario.path());
                (Some(rate), None)
            },
        };

        Self {
            scenario,
            rate,
            threads,
        }
    }
}
