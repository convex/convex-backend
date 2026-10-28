use std::{
    env,
    fs,
    future::Future,
    path::{
        Path,
        PathBuf,
    },
    sync::LazyLock,
    time::Duration,
};

use ::metrics::StaticMetricLabel;
use anyhow::Context;
use backoff::{
    future::retry,
    ExponentialBackoff,
};
use big_brain_client::BigBrainClient;
use big_brain_private_api_types::{
    DeploymentAuthPreviewArgs,
    DeploymentAuthProdArgs,
    DeploymentAuthResponse,
    ProjectSelectionArgs,
};
use clap::ValueEnum;
use cmd_util::env::env_config;
use common::types::MemberId;
use futures::FutureExt;
use health_check::wait_for_http_health;
use keybroker::{
    DeploymentSecret,
    KeyBroker,
    DEV_INSTANCE_NAME,
};
use log_interleaver::LogInterleaver;
use serde::Deserialize;
use tokio::process::{
    Child,
    Command,
};

use crate::metrics;

static REPO_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
});

static SELF_HOSTED_DOCKER_COMPOSE: LazyLock<PathBuf> =
    LazyLock::new(|| REPO_ROOT.join("self-hosted/docker/docker-compose.yml"));

const PROD_PROVISION_HOST: &str = "https://api.convex.dev";
const BACKEND_HARNESS_CLIENT_ID: &str = concat!("backend-harness-", env!("CARGO_PKG_VERSION"));
/// Port usher runs on locally
const USHER_PORT: u16 = 8002;
/// Port funrun runs on locally, overriding its default of 40994. Must stay
/// below the Linux ephemeral range (32768–60999): a port inside it can already
/// be held by an unrelated outbound connection, so the bind fails with
/// EADDRINUSE.
const FUNRUN_PORT: u16 = 8005;
/// Address of local test backend through usher
static USHER_INSTANCE_URL: LazyLock<String> =
    LazyLock::new(|| format!("http://carnitas.local.convex.cloud:{USHER_PORT}"));

const LOCAL_LOG_SINK_FILENAME: &str = "log_sink.jsonl";

/// Ports the open-source `convex-local-backend` binds. Overridable via env so a
/// second backend stack (e.g. the conductor stack, which uses 8000) can run
/// concurrently on the same host without a port collision.
fn open_source_backend_port() -> u16 {
    env_config("BACKEND_HARNESS_OSS_PORT", 8000)
}
fn open_source_site_proxy_port() -> u16 {
    env_config("BACKEND_HARNESS_OSS_SITE_PROXY_PORT", 8001)
}

#[cfg(unix)]
const NPX: &str = "npx";
#[cfg(windows)]
const NPX: &str = "npx.cmd";

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendProvisioner {
    /// Provision from production with npx convex deploy
    Production,
    /// Provision from 127.0.0.1:8050 big brain with npx convex deploy
    LocalBigBrain,
    /// Spawn an open source backend on localhost
    OpenSourceDebug,
    /// Spawn an open source backend on localhost with --release build
    OpenSourceRelease,
    /// Spawn a localhost conductor
    ConductorDebug,
    /// Spawn a localhost conductor with --release build
    ConductorRelease,
    /// Use a backend in a docker container
    SelfHostedBackend,
}

impl BackendProvisioner {
    pub fn provision_host_credentials(&self) -> Option<ProvisionHostCredentials> {
        let host = match self {
            BackendProvisioner::Production => Some(PROD_PROVISION_HOST),
            BackendProvisioner::LocalBigBrain => Some("http://127.0.0.1:8050"),
            BackendProvisioner::OpenSourceDebug
            | BackendProvisioner::OpenSourceRelease
            | BackendProvisioner::ConductorDebug
            | BackendProvisioner::ConductorRelease
            | BackendProvisioner::SelfHostedBackend => None,
        };

        host.map(|provision_host| {
            let access_token = if *self == BackendProvisioner::Production {
                env::var("CONVEX_OVERRIDE_ACCESS_TOKEN").expect(
                    "Must provide CONVEX_OVERRIDE_ACCESS_TOKEN. For production, look in 1password.",
                )
            } else {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct ConfigJson {
                    access_token: String,
                }

                let config_file = home::home_dir().unwrap().join(".convex-test/config.json");
                let ConfigJson { access_token } =
                    serde_json::from_str(&fs::read_to_string(&config_file).unwrap_or_else(|_| {
                        panic!(
                            "Could not find {config_file:?}. Run the dev npx convex login printed \
                             out during big brain startup."
                        )
                    }))
                    .expect("Could not deserialize config file");

                access_token
            };
            ProvisionHostCredentials {
                provision_host,
                access_token,
            }
        })
    }
}

#[derive(Clone, Debug)]
pub struct ProvisionHostCredentials {
    pub provision_host: &'static str,
    pub access_token: String,
}

/// Handle to provisioned backend. Deactivates/cleans up on drop
#[must_use]
enum ProvisionHandle {
    BigBrain {
        provision_host_credentials: ProvisionHostCredentials,
        package_dir: PathBuf,
        metric_label: StaticMetricLabel,
    },
    LocalBackend {
        _backend_handle: tokio::process::Child,
        _usher_handle: Option<tokio::process::Child>,
        _funrun_handle: Option<tokio::process::Child>,
        tempdir: tempfile::TempDir,
    },
    LocalConductor {
        _conductor_handle: tokio::process::Child,
        _usher_handle: tokio::process::Child,
        _funrun_handle: Option<tokio::process::Child>,
        tempdir: tempfile::TempDir,
    },
}

impl ProvisionHandle {
    fn local_log_sink(&self) -> Option<PathBuf> {
        match self {
            ProvisionHandle::BigBrain { .. } => None,
            ProvisionHandle::LocalBackend { tempdir, .. }
            | ProvisionHandle::LocalConductor { tempdir, .. } => {
                Some(tempdir.path().join(LOCAL_LOG_SINK_FILENAME))
            },
        }
    }

    fn url(&self) -> Option<String> {
        match self {
            ProvisionHandle::BigBrain { .. } => None,
            ProvisionHandle::LocalBackend { _usher_handle, .. } => {
                let url = if _usher_handle.is_some() {
                    USHER_INSTANCE_URL.clone()
                } else {
                    format!("http://127.0.0.1:{}", open_source_backend_port())
                };
                Some(url)
            },
            ProvisionHandle::LocalConductor { .. } => Some(USHER_INSTANCE_URL.clone()),
        }
    }

    async fn cleanup(self) -> anyhow::Result<()> {
        match self {
            ProvisionHandle::BigBrain {
                provision_host_credentials,
                package_dir,
                metric_label,
            } => {
                delete_project(provision_host_credentials, &package_dir, metric_label).await?;
            },
            ProvisionHandle::LocalBackend { .. } => (),
            ProvisionHandle::LocalConductor { .. } => (),
        }

        Ok(())
    }
}

/// Provision an instance and run the given function
/// passing in the backend_url to the provisioned instance
pub async fn with_provision<T, F, Fut>(
    logs: &LogInterleaver,
    backend_provisioner: BackendProvisioner,
    provision_request: &ProvisionRequest,
    package_dir: &Path,
    metric_label: StaticMetricLabel,
    func: F,
) -> anyhow::Result<T>
where
    F: FnOnce(String, String, Option<PathBuf>) -> Fut,
    Fut: Future<Output = anyhow::Result<T>>,
{
    let Provision {
        deployment_url,
        admin_key,
        handle,
    } = provision(
        logs,
        backend_provisioner,
        provision_request,
        package_dir,
        metric_label,
    )
    .await?;

    let result = func(deployment_url, admin_key, handle.local_log_sink()).await;
    match provision_request {
        ProvisionRequest::NewProject => {
            handle.cleanup().await?;
        },
        ProvisionRequest::ExistingProject { .. } => (),

        ProvisionRequest::Preview { .. } => (),
    };
    result
}

pub fn get_configured_deployment_name(package_dir: &Path) -> anyhow::Result<String> {
    let env_local = package_dir.join(".env.local");
    dotenvy::from_filename_iter(&env_local)
        .with_context(|| format!(".env.local not found at {env_local:?}"))?
        .find_map(|item| {
            item.ok()
                .and_then(|(key, value)| (key == "CONVEX_DEPLOYMENT").then_some(value))
        })
        .map(|name| {
            // Remove deployment type prefix
            // Duplicates logic from common/src/admin_key.rs - to avoid dependency on common
            name.split_once(':')
                .map(|(_, name)| name.to_owned())
                .unwrap_or(name)
        })
        .context("CONVEX_DEPLOYMENT not found in .env.local")
}

#[derive(Clone, Debug)]
enum DeploymentSelector {
    Preview(String),
    Prod,
}

async fn deployment_credentials(
    ProvisionHostCredentials {
        provision_host,
        access_token,
    }: ProvisionHostCredentials,
    package_dir: &Path,
    deployment_selector: DeploymentSelector,
) -> anyhow::Result<DeploymentAuthResponse> {
    let client = BigBrainClient::new(provision_host.to_string(), access_token);
    let configured_deployment_name = get_configured_deployment_name(package_dir)?;
    match deployment_selector {
        DeploymentSelector::Preview(preview_name) => {
            client
                .preview_deployment_credentials(DeploymentAuthPreviewArgs {
                    project_selection: ProjectSelectionArgs::DeploymentName {
                        deployment_name: configured_deployment_name,
                        deployment_type: None,
                    },
                    preview_name,
                })
                .await
        },
        DeploymentSelector::Prod => {
            client
                .prod_deployment_credentials(DeploymentAuthProdArgs {
                    deployment_name: configured_deployment_name,
                    deployment_class: None,
                })
                .await
        },
    }
}

async fn preview_deploy_key(
    ProvisionHostCredentials {
        provision_host,
        access_token,
    }: ProvisionHostCredentials,
    package_dir: &Path,
) -> anyhow::Result<String> {
    let deployment_name = get_configured_deployment_name(package_dir)?;
    let client = BigBrainClient::new(provision_host.to_string(), access_token);
    let team_and_project = client
        .get_project_and_team_for_deployment(deployment_name.clone())
        .await?;
    let prod_credentials = client
        .prod_deployment_credentials(DeploymentAuthProdArgs {
            deployment_name,
            deployment_class: None,
        })
        .await?;

    let admin_key_parts = prod_credentials.admin_key.split_once('|');
    let admin_key = match admin_key_parts {
        Some(parts) => parts.1,
        None => prod_credentials.admin_key.as_str(),
    };

    Ok(format!(
        "preview:{}:{}|{}",
        team_and_project.team, team_and_project.project, admin_key
    ))
}

struct Provision {
    deployment_url: String,
    admin_key: String,
    handle: ProvisionHandle,
}

fn start_local_usher(logs: &LogInterleaver, release: bool) -> anyhow::Result<Child> {
    let usher_binary = if release {
        REPO_ROOT.join("target/release/usher")
    } else {
        REPO_ROOT.join("target/debug/usher")
    };
    logs.spawn_with_prefixed_logs(
        "usher".into(),
        Command::new(usher_binary)
            .arg("--port")
            .arg(USHER_PORT.to_string())
            .arg("--register-service")
            .arg("convex-conductor-0=127.0.0.1:8000,grpc.port=7999")
            .arg("--partition-mapping")
            .arg("carnitas=0")
            .arg("--region=local")
            .kill_on_drop(true),
    )
}

fn start_local_funrun(logs: &LogInterleaver, release: bool) -> anyhow::Result<Child> {
    let funrun_binary = if release {
        REPO_ROOT.join("target/release/funrun")
    } else {
        REPO_ROOT.join("target/debug/funrun")
    };
    logs.spawn_with_prefixed_logs(
        "funrun".into(),
        Command::new(funrun_binary)
            .arg("--port")
            .arg(FUNRUN_PORT.to_string())
            .arg("--metrics-addr")
            .arg("0.0.0.0:9101")
            .kill_on_drop(true),
    )
}

async fn provision(
    logs: &LogInterleaver,
    backend_provisioner: BackendProvisioner,
    provision_request: &ProvisionRequest,
    package_dir: &Path,
    metric_label: StaticMetricLabel,
) -> anyhow::Result<Provision> {
    let local_secret = DeploymentSecret::random();
    let local_admin_key = KeyBroker::new(DEV_INSTANCE_NAME, local_secret)?
        .issue_admin_key(MemberId(0))
        .as_string();
    let (admin_key, handle, deployment_url) = match backend_provisioner {
        BackendProvisioner::Production | BackendProvisioner::LocalBigBrain => {
            let provision_host_credentials = backend_provisioner
                .provision_host_credentials()
                .expect("BigBrain and Production provisioners must have hosts!");
            let opt_deployment_info = provision_from_big_brain(
                logs,
                provision_host_credentials.clone(),
                package_dir,
                provision_request,
                metric_label.clone(),
            )
            .await?;
            let configured_deployment_name = get_configured_deployment_name(package_dir)?;
            tracing::info!("{package_dir:?}: Completed provision of {configured_deployment_name}");
            let DeploymentAuthResponse {
                url,
                admin_key,
                deployment_name,
                ..
            } = deployment_credentials(
                provision_host_credentials.clone(),
                package_dir,
                opt_deployment_info.clone(),
            )
            .await?;
            tracing::info!("{deployment_name} provisioned for {opt_deployment_info:?} at {url}");
            (
                admin_key.to_string(),
                ProvisionHandle::BigBrain {
                    provision_host_credentials,
                    package_dir: package_dir.to_path_buf(),
                    metric_label: metric_label.clone(),
                },
                url,
            )
        },
        BackendProvisioner::ConductorDebug | BackendProvisioner::ConductorRelease => {
            let release = matches!(backend_provisioner, BackendProvisioner::ConductorRelease);
            let mut build_cmd = Command::new("cargo");
            build_cmd.arg("build").arg("--bin").arg("conductor");
            let udf_use_funrun = env_config("UDF_USE_FUNRUN", true);
            if udf_use_funrun {
                build_cmd.arg("--bin").arg("funrun");
            }
            if release {
                build_cmd.arg("--release");
            }
            logs.spawn_with_prefixed_logs("cargo build".into(), &mut build_cmd)?
                .wait()
                .map(|result| anyhow::Ok(result?.exit_ok()?))
                .await?;
            let conductor_binary = if release {
                REPO_ROOT.join("target/release/conductor")
            } else {
                REPO_ROOT.join("target/debug/conductor")
            };
            let tempdir_handle = tempfile::tempdir()?;
            let db_path = tempdir_handle.path().join("convex_local_backend.sqlite3");
            let mut run_conductor_cmd = Command::new(conductor_binary);
            run_conductor_cmd
                .arg("--local-storage")
                .arg(tempdir_handle.path())
                .arg("--db-cluster-name")
                .arg("local")
                .arg("--in-process-searchlight")
                .arg("--do-not-require-ssl")
                .arg(db_path.clone())
                .arg("--local-log-sink")
                .arg(tempdir_handle.path().join(LOCAL_LOG_SINK_FILENAME))
                .arg("--convex-origin-base")
                .arg(format!("http://local.convex.cloud:{USHER_PORT}"))
                .arg("--convex-site-base")
                .arg(format!("http://local.convex.site:{USHER_PORT}"))
                .arg("--instance")
                .arg(format!("carnitas={local_secret}"))
                .env("UDF_USE_FUNRUN", udf_use_funrun.to_string())
                .env("CONVEX_RELEASE_VERSION_DEV", "0.0.0-backendharness");
            if udf_use_funrun {
                run_conductor_cmd
                    .arg("--register-service")
                    .arg(format!("funrun-default=http://0.0.0.0:{FUNRUN_PORT}"));
            }
            let conductor_handle = logs.spawn_with_prefixed_logs(
                "conductor".into(),
                run_conductor_cmd.kill_on_drop(true),
            )?;
            let usher_handle = start_local_usher(logs, release)?;
            let funrun_handle = udf_use_funrun
                .then(|| start_local_funrun(logs, release))
                .transpose()?;
            // Give it ~15 seconds to start up (5 retries with 500ms exponential backoff)
            wait_for_http_health(
                &USHER_INSTANCE_URL.parse()?,
                BACKEND_HARNESS_CLIENT_ID,
                Some("0.0.0-backendharness"),
                // We should include a random instance name here, but it would need to match our
                // admin key. Right now the admin key is static, so either we use a static name
                // which will always match, or we refactor to allow dynamic admin keys here.
                None,
                5,
                Duration::from_millis(500),
            )
            .await
            .context("Timed out waiting for backend startup. Might have a second one running?")?;

            (
                local_admin_key.clone(),
                ProvisionHandle::LocalConductor {
                    _conductor_handle: conductor_handle,
                    _usher_handle: usher_handle,
                    _funrun_handle: funrun_handle,
                    tempdir: tempdir_handle,
                },
                USHER_INSTANCE_URL.clone(),
            )
        },
        BackendProvisioner::OpenSourceDebug | BackendProvisioner::OpenSourceRelease => {
            let release = matches!(backend_provisioner, BackendProvisioner::OpenSourceRelease);
            let mut cmd = Command::new("cargo");
            cmd.arg("build").arg("--bin").arg("convex-local-backend");
            if release {
                cmd.arg("--release");
            }

            logs.spawn_with_prefixed_logs("cargo build".into(), &mut cmd)?
                .wait()
                .await?
                .exit_ok()?;

            let backend_binary = if release {
                REPO_ROOT.join("target/release/convex-local-backend")
            } else {
                REPO_ROOT.join("target/debug/convex-local-backend")
            };

            let tempdir_handle = tempfile::tempdir()?;
            let db_path = tempdir_handle.path().join("convex_local_backend.sqlite3");
            let backend_handle = logs.spawn_with_prefixed_logs(
                "backend".into(),
                Command::new(backend_binary)
                    .arg(db_path)
                    .arg("--port")
                    .arg(open_source_backend_port().to_string())
                    .arg("--site-proxy-port")
                    .arg(open_source_site_proxy_port().to_string())
                    .arg("--disable-beacon")
                    .arg("--instance-name")
                    .arg(DEV_INSTANCE_NAME)
                    .arg("--instance-secret")
                    .arg(local_secret.to_string())
                    .env("CONVEX_RELEASE_VERSION_DEV", "0.0.0-backendharness")
                    .kill_on_drop(true),
            )?;
            let backend_url = format!("http://127.0.0.1:{}", open_source_backend_port());
            // Give it ~15 seconds to start up (5 retries with 500ms exponential backoff)
            wait_for_http_health(
                &backend_url.parse()?,
                BACKEND_HARNESS_CLIENT_ID,
                Some("0.0.0-backendharness"),
                // We should include a random instance name here, but it would need to match our
                // admin key. Right now the admin key is static, so either we use a static name
                // which will always match, or we refactor to allow dynamic admin keys here.
                None,
                5,
                Duration::from_millis(500),
            )
            .await
            .context("Timed out waiting for backend startup. Might have a second one running?")?;

            (
                local_admin_key.clone(),
                ProvisionHandle::LocalBackend {
                    _backend_handle: backend_handle,
                    _usher_handle: None,
                    _funrun_handle: None,
                    tempdir: tempdir_handle,
                },
                backend_url,
            )
        },
        BackendProvisioner::SelfHostedBackend => {
            let mut docker_pull_cmd = Command::new("docker");
            docker_pull_cmd
                .arg("compose")
                .arg("-f")
                .arg(SELF_HOSTED_DOCKER_COMPOSE.to_string_lossy().to_string())
                .arg("pull")
                .env("PORT", "8000")
                .env("SITE_PROXY_PORT", "8001");
            let mut docker_pull_handle =
                logs.spawn_with_prefixed_logs("docker pull".into(), &mut docker_pull_cmd)?;
            docker_pull_handle.wait().await?.exit_ok()?;
            let mut docker_up_cmd = Command::new("docker");
            docker_up_cmd
                .arg("compose")
                .arg("-f")
                .arg(SELF_HOSTED_DOCKER_COMPOSE.to_string_lossy().to_string())
                .arg("up")
                .env("PORT", "8000")
                .env("SITE_PROXY_PORT", "8001")
                .env("CONVEX_RELEASE_VERSION_DEV", "0.0.0-backendharness")
                .env("INSTANCE_NAME", "carnitas")
                .env("INSTANCE_SECRET", local_secret.to_string())
                .kill_on_drop(true);
            let tempdir_handle = tempfile::tempdir()?;
            let backend_handle =
                logs.spawn_with_prefixed_logs("docker up".into(), &mut docker_up_cmd)?;
            let backend_url = "http://127.0.0.1:8000".to_string();
            // Give it ~15 seconds to start up (5 retries with 500ms exponential backoff)
            wait_for_http_health(
                &backend_url.parse()?,
                BACKEND_HARNESS_CLIENT_ID,
                Some("0.0.0-backendharness"),
                // We should include a random instance name here, but it would need to match our
                // admin key. Right now the admin key is static, so either we use a static name
                // which will always match, or we refactor to allow dynamic admin keys here.
                None,
                5,
                Duration::from_millis(500),
            )
            .await
            .context("Timed out waiting for backend startup. Might have a second one running?")?;

            (
                local_admin_key.clone(),
                ProvisionHandle::LocalBackend {
                    _backend_handle: backend_handle,
                    _usher_handle: None,
                    _funrun_handle: None,
                    tempdir: tempdir_handle,
                },
                backend_url,
            )
        },
    };

    match provision_request {
        ProvisionRequest::ExistingProject { .. } | ProvisionRequest::NewProject => {
            deploy(logs, package_dir, metric_label, &handle, &admin_key).await?;
        },
        ProvisionRequest::Preview { .. } => (),
    };

    Ok(Provision {
        deployment_url,
        admin_key,
        handle,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProvisionRequest {
    ExistingProject { project_slug: String },
    NewProject,
    Preview { identifier: String },
}

async fn provision_from_big_brain(
    logs: &LogInterleaver,
    provision_host_credentials: ProvisionHostCredentials,
    package_dir: &Path,
    provision_request: &ProvisionRequest,
    metric_label: StaticMetricLabel,
) -> anyhow::Result<DeploymentSelector> {
    let result: anyhow::Result<_> = try bikeshed anyhow::Result<_> {
        let ProvisionHostCredentials {
            provision_host,
            access_token,
        } = provision_host_credentials.clone();
        match provision_request {
            ProvisionRequest::ExistingProject { project_slug } => {
                tracing::info!("{package_dir:?}: npx convex dev --configure=existing");
                logs.spawn_with_prefixed_logs(
                    "npx convex dev --configure=existing".into(),
                    Command::new(NPX)
                        .arg("convex")
                        .arg("dev")
                        .arg("--once")
                        .arg("--skip-push")
                        .arg("--configure=existing")
                        .arg("--team")
                        .arg("engineering-loadgenerator")
                        .arg("--project")
                        .arg(project_slug)
                        .env("CONVEX_PROVISION_HOST", provision_host)
                        .env("CONVEX_OVERRIDE_ACCESS_TOKEN", access_token)
                        .current_dir(package_dir),
                )?
                .wait()
                .await?
                .exit_ok()
                .context("`convex dev --once --configure=existing` unsuccessful")?;
                DeploymentSelector::Prod
            },
            ProvisionRequest::NewProject => {
                tracing::info!("{package_dir:?}: npx convex dev --configure=new");
                let mut cmd = Command::new(NPX);
                cmd.arg("convex")
                    .arg("dev")
                    .arg("--once")
                    .arg("--skip-push")
                    .arg("--configure=new")
                    .arg("--project")
                    .arg("load_generator");
                logs.spawn_with_prefixed_logs(
                    "npx convex dev --configure=new".into(),
                    cmd.env("CONVEX_PROVISION_HOST", provision_host)
                        .env("CONVEX_OVERRIDE_ACCESS_TOKEN", access_token)
                        .current_dir(package_dir),
                )?
                .wait()
                .await?
                .exit_ok()
                .context("`convex dev --once --configure=new` unsuccessful")?;
                DeploymentSelector::Prod
            },
            ProvisionRequest::Preview { identifier, .. } => {
                tracing::info!("{package_dir:?}: npx convex deploy --preview-create");
                let deploy_key =
                    preview_deploy_key(provision_host_credentials, package_dir).await?;
                let mut cmd = Command::new(NPX);
                cmd.arg("convex")
                    .arg("deploy")
                    .arg("--preview-create")
                    .arg(identifier);
                logs.spawn_with_prefixed_logs(
                    format!("npx convex deploy --preview-create {identifier}"),
                    cmd.env("CONVEX_PROVISION_HOST", provision_host)
                        .env("CONVEX_OVERRIDE_ACCESS_TOKEN", access_token)
                        .env("CONVEX_DEPLOY_KEY", deploy_key)
                        .current_dir(package_dir),
                )?
                .wait()
                .await?
                .exit_ok()
                .context("`convex deploy --preview-create` unsuccessful")?;

                DeploymentSelector::Preview(identifier.to_string())
            },
        }
    };
    metrics::log_provision(metric_label, result.is_ok(), provision_request);
    result
}

pub async fn get_cli_version(package_dir: &Path) -> anyhow::Result<String> {
    let output = Command::new(NPX)
        .arg("convex")
        .arg("--version")
        .current_dir(package_dir)
        .output()
        .await?;
    output.status.exit_ok().with_context(|| {
        format!(
            "npx convex --version failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    String::from_utf8(output.stdout)
        .context("Could not convert `npx convex --version` output to string")
}

async fn deploy(
    logs: &LogInterleaver,
    package_dir: &Path,
    metric_label: StaticMetricLabel,
    provision_handle: &ProvisionHandle,
    admin_key: &str,
) -> anyhow::Result<()> {
    tracing::info!("{package_dir:?}: npx convex deploy");
    // Retry with exponential backoff because there's a race condition where traefik
    // nodes might not have provisioned instance's domain by the time we try to
    // deploy
    let backoff = ExponentialBackoff {
        max_elapsed_time: Some(Duration::from_secs(5)),
        ..Default::default()
    };

    retry(backoff, || async {
        let mut command = Command::new(NPX);
        let push_command = match provision_handle {
            ProvisionHandle::BigBrain {
                provision_host_credentials:
                    ProvisionHostCredentials {
                        provision_host,
                        access_token,
                    },
                ..
            } => command
                .arg("convex")
                .arg("deploy")
                .arg("--yes")
                .env("CONVEX_PROVISION_HOST", provision_host)
                .env("CONVEX_OVERRIDE_ACCESS_TOKEN", access_token),
            // Only pass the admin key in directly with local backend to bypass dependency on
            // big-brain
            ProvisionHandle::LocalBackend { .. } | ProvisionHandle::LocalConductor { .. } => {
                let url = provision_handle
                    .url()
                    .expect("Local backend or conductor have a URL");
                command
                    .arg("convex")
                    .arg("deploy")
                    .arg("--yes")
                    // Perform codegen even though this might change the local
                    // repo state during tests, because codegen should be kept
                    // up-to-date anyway.
                    .arg("--codegen")
                    .arg("enable")
                    .arg("--admin-key")
                    .arg(admin_key)
                    .arg("--url")
                    .arg(url)
            },
        };
        let push_result: anyhow::Result<()> = try bikeshed anyhow::Result<_> {
            logs.spawn_with_prefixed_logs(
                "npx convex deploy".into(),
                push_command.current_dir(package_dir),
            )?
            .wait()
            .await?
            .exit_ok()
            .context("`convex deploy` unsuccessful")?;
        };
        metrics::log_push(metric_label.clone(), push_result.is_ok());
        Ok(push_result?)
    })
    .await?;
    Ok(())
}

async fn delete_project(
    ProvisionHostCredentials {
        provision_host,
        access_token,
    }: ProvisionHostCredentials,
    package_dir: &Path,
    metric_label: StaticMetricLabel,
) -> anyhow::Result<()> {
    tracing::info!("Tearing down project");
    let big_brain_client = BigBrainClient::new(provision_host.into(), access_token);

    let result: anyhow::Result<()> = try bikeshed anyhow::Result<_> {
        let deployment_name = get_configured_deployment_name(package_dir)?;
        let project_id = big_brain_client
            .get_project_and_team_for_deployment(deployment_name)
            .await?
            .project_id;
        big_brain_client.delete_project(project_id).await?;
        tracing::info!("Delete project of {project_id} succeeded");
    };
    metrics::log_deactivate(metric_label, result.is_ok());
    result
}
