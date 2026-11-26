//! Common utilities across multiple files.

use std::{
    env,
    ffi::OsString,
    ops::Deref,
    path::{
        Path,
        PathBuf,
    },
};

pub use value::utils::{
    display_map,
    display_sequence,
};

#[derive(Clone)]
pub struct ReadOnly<T>(T);

impl<T> ReadOnly<T> {
    pub fn new(inner: T) -> Self {
        Self(inner)
    }
}

impl<T> Deref for ReadOnly<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

/// Ensures that we are always running Convex services in UTC.
pub fn ensure_utc() -> anyhow::Result<()> {
    if let Ok(val) = std::env::var("TZ")
        && val != "UTC"
    {
        anyhow::bail!("TZ is set, but Convex requires UTC. Unset TZ to continue.")
    }
    unsafe { std::env::set_var("TZ", "UTC") };

    Ok(())
}

/// The Convex checkout that dev tooling (harnesses, local provisioning, tests
/// that read files outside their crate) works in: `CONVEX_REPO_ROOT` if set,
/// else the checkout this crate was built from.
pub fn checkout_root() -> PathBuf {
    resolve_checkout_root(
        env::var_os("CONVEX_REPO_ROOT"),
        Path::new(env!("CARGO_MANIFEST_DIR")),
    )
}

fn resolve_checkout_root(repo_root: Option<OsString>, build_manifest_dir: &Path) -> PathBuf {
    if let Some(root) = repo_root.filter(|root| !root.is_empty()) {
        return std::path::absolute(root).expect("resolve CONVEX_REPO_ROOT");
    }
    assert!(
        build_manifest_dir.is_absolute(),
        "set CONVEX_REPO_ROOT: this build has no absolute path to its checkout"
    );
    build_manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("crates/common is two levels below the checkout")
        .to_path_buf()
}

/// The directory of prebuilt Convex binaries named by `CONVEX_BIN_DIR`, made
/// absolute. Unset or empty means binaries come from this checkout's `target/`.
pub fn convex_bin_dir() -> Option<PathBuf> {
    env::var_os("CONVEX_BIN_DIR")
        .filter(|dir| !dir.is_empty())
        .map(|dir| std::path::absolute(dir).expect("resolve CONVEX_BIN_DIR"))
}
