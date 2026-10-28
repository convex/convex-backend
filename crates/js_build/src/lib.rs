//! Helpers for build scripts that need JS from `npm-packages` compiled before
//! the crate that embeds or bundles it.
//!
//! Callers run the pinned pnpm and turbo from `scripts/node_modules`, so a
//! cargo build uses the same tool versions as `just turbo` and shares its
//! caches.

use std::{
    env,
    fs,
    io::{
        self,
        Write,
    },
    path::{
        Path,
        PathBuf,
    },
    process::Command,
};

use anyhow::Context;

/// The pinned JS tools in `scripts/node_modules`.
#[derive(Clone, Copy)]
enum JsTool {
    Pnpm,
    Turbo,
}

impl JsTool {
    #[cfg(not(target_os = "windows"))]
    fn binary(self) -> &'static str {
        match self {
            JsTool::Pnpm => "pnpm",
            JsTool::Turbo => "turbo",
        }
    }

    #[cfg(target_os = "windows")]
    fn binary(self) -> &'static str {
        match self {
            JsTool::Pnpm => "pnpm.cmd",
            JsTool::Turbo => "turbo.cmd",
        }
    }
}

/// Cargo silently drops `rerun-if-changed` paths that don't exist and then
/// reruns the build script on every invocation. That fallback isn't great,
/// since it'll silently degrade build times, so check that the path actually
/// exists.
pub fn rerun_if_changed(path: impl AsRef<Path>) -> anyhow::Result<()> {
    let path = path.as_ref();
    anyhow::ensure!(path.exists(), "Non-existent dependency path: {path:?}");
    println!("cargo:rerun-if-changed={}", path.display());
    Ok(())
}

/// Whether the JS this build script depends on was already installed and built
/// before cargo ran, as CI jobs that build it in an earlier step declare by
/// setting `CONVEX_PREBUILT_JS`.
///
/// Only sound where the JS build strictly precedes cargo: with concurrent JS
/// builds the [`pnpm_install`] and [`turbo_build`] calls must stay (under the
/// JS tool flock) so they block until the dist outputs are complete.
pub fn js_prebuilt() -> bool {
    println!("cargo:rerun-if-env-changed=CONVEX_PREBUILT_JS");
    env::var_os("CONVEX_PREBUILT_JS").is_some()
}

/// The `npm-packages` directory, found by walking up from the calling build
/// script's crate.
pub fn packages_dir() -> anyhow::Result<PathBuf> {
    let manifest_dir =
        env::var_os("CARGO_MANIFEST_DIR").context("CARGO_MANIFEST_DIR must be set")?;
    let manifest_dir = fs::canonicalize(PathBuf::from(manifest_dir))?;
    for ancestor in manifest_dir.ancestors() {
        let candidate = ancestor.join("npm-packages");
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }
    anyhow::bail!("No npm-packages directory above {manifest_dir:?}")
}

/// Install the workspace's JS dependencies, resolved strictly from the
/// lockfile.
pub fn pnpm_install() -> anyhow::Result<()> {
    run_js_tool(
        JsTool::Pnpm,
        &[
            "install",
            "--frozen-lockfile",
            "--ignore-scripts",
            "--config.confirmModulesPurge=false",
        ],
    )
}

/// Run each package's `build` task, along with the tasks of the workspace
/// packages it depends on (`--filter=pkg...`, matching `rush build -t pkg`).
pub fn turbo_build(packages: &[&str]) -> anyhow::Result<()> {
    let mut args = vec!["run".to_owned(), "build".to_owned()];
    args.extend(packages.iter().map(|pkg| format!("--filter={pkg}...")));
    run_js_tool(
        JsTool::Turbo,
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    )
}

// Neither tool is safe to run concurrently against one checkout: pnpm races on
// its store writes (pnpm/pnpm#7335, and a git dependency resolved with `path:`
// can land in the store indexed under the tarball's root package, which then
// fails pnpm's store content check), and turbo has no cross-process task lock,
// so concurrent runs (`just turbo` racing a build script, or two build scripts
// in one cargo invocation) can execute a task twice and race writes to shared
// outputs/cache entries. flock on a per-checkout lock file (shared with the
// `just turbo` recipe) serializes every JS tool run; hosts without flock(1)
// (macOS, Windows) run unlocked, where such races are transient and a rerun
// fixes them.
fn flock_available() -> bool {
    Command::new("flock")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

fn run_js_tool(tool: JsTool, args: &[&str]) -> anyhow::Result<()> {
    let packages_dir = packages_dir()?;
    let repo_root = packages_dir
        .parent()
        .with_context(|| format!("{packages_dir:?} has no parent"))?;
    // turbo shells out to `pnpm` by name, so the pinned copy in
    // scripts/node_modules must be on PATH.
    let bin_dir = repo_root.join("scripts/node_modules/.bin");
    let mut paths = vec![bin_dir.clone()];
    if let Some(path) = env::var_os("PATH") {
        paths.extend(env::split_paths(&path));
    }
    let tool_path = bin_dir.join(tool.binary());
    let mut command = if flock_available() {
        fs::create_dir_all(packages_dir.join(".turbo"))?;
        let mut c = Command::new("flock");
        // Relative to the child's working directory below, which is where the
        // `just turbo` recipe takes its lock too.
        c.arg(".turbo/turbo.lock").arg(&tool_path);
        c
    } else {
        Command::new(&tool_path)
    };
    let output = command
        .current_dir(&packages_dir)
        .env("PATH", env::join_paths(paths)?)
        // Keep turbo hermetic inside cargo builds: no first-run telemetry
        // banner/phone-home in the output, and no user-exported TURBO_* (UI
        // mode, remote-cache tokens) changing behavior.
        .env("TURBO_TELEMETRY_DISABLED", "1")
        .env_remove("TURBO_UI")
        .env_remove("TURBO_TOKEN")
        .env_remove("TURBO_TEAM")
        .args(args)
        .output()
        .with_context(|| format!("Failed to run {tool_path:?} {}", args.join(" ")))?;
    io::stdout().write_all(&output.stdout).unwrap();
    io::stderr().write_all(&output.stderr).unwrap();
    anyhow::ensure!(
        output.status.success(),
        "Failed on {tool_path:?} {}",
        args.join(" ")
    );
    Ok(())
}
