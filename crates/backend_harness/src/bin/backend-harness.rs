#![feature(exit_status_error)]
use std::path::PathBuf;

use backend_harness::{
    with_provision,
    BackendProvisioner,
    ProvisionRequest,
};
use clap::Parser;
use cmd_util::env::config_tool;
use log_interleaver::LogInterleaver;
use metrics::StaticMetricLabel;
use tokio::process::Command;

/// Run the command while a backend is provisioned.
#[derive(Parser, Debug)]
struct Args {
    /// Provisioner
    #[clap(long, value_enum)]
    provisioner: BackendProvisioner,

    /// Path under which to provision backend
    #[clap(long)]
    package_dir: PathBuf,

    /// Command to run while backend is provisioned
    cmd: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _guard = config_tool();
    let Args {
        provisioner,
        package_dir,
        cmd,
    } = Args::parse();
    let logs = LogInterleaver::new();

    let (tx, rx) = crossbeam_channel::bounded(1);
    if cmd.is_empty() {
        ctrlc::set_handler(move || {
            tx.send(()).unwrap();
        })?;
    }

    with_provision(
        &logs.clone(),
        provisioner,
        &ProvisionRequest::NewProject,
        &package_dir,
        StaticMetricLabel::new("load_description", "backend-harness"),
        |host, admin_key, _| async move {
            if cmd.is_empty() {
                println!("Provisioned {host}. Ctrl-C to quit");
                rx.recv()?;
                println!("Cleaning up {host}");
            } else {
                Command::new(cmd[0].clone())
                    .args(&cmd[1..])
                    .env("ADMIN_KEY", admin_key)
                    .spawn()?
                    .wait()
                    .await?
                    .exit_ok()?;
            }
            Ok(())
        },
    )
    .await?;

    Ok(())
}
