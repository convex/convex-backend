//! Host target discovery and small mode helpers (WN-07).
//! Owned by the WN task listed in .plan/DESIGN.md §9.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HostTarget {
    pub triple: String,
    pub os: String,
    pub arch: String,
    /// `gnu`, `musl`, or `null` (e.g. macOS).
    pub env: Option<String>,
    /// Runtime glibc version on linux-gnu, otherwise `None`.
    pub glibc: Option<String>,
}

fn target_env() -> Option<&'static str> {
    if cfg!(target_env = "gnu") {
        Some("gnu")
    } else if cfg!(target_env = "musl") {
        Some("musl")
    } else {
        None
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn glibc_version() -> Option<String> {
    // SAFETY: returns a pointer to a static NUL-terminated string.
    unsafe {
        let p = libc::gnu_get_libc_version();
        if p.is_null() {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned())
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn glibc_version() -> Option<String> {
    None
}

/// Rust-style target triple of the running backend.
pub fn host_target() -> HostTarget {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    let env = target_env();
    let triple = match os {
        "macos" => format!("{arch}-apple-darwin"),
        "linux" => format!("{arch}-unknown-linux-{}", env.unwrap_or("gnu")),
        "windows" => format!("{arch}-pc-windows-{}", env.unwrap_or("msvc")),
        other => format!("{arch}-unknown-{other}"),
    };
    HostTarget {
        triple,
        os: os.to_string(),
        arch: arch.to_string(),
        env: env.map(str::to_string),
        glibc: glibc_version(),
    }
}

/// `RUST_NATIVE_MODE`: `Some("worker")` when native workers are enabled.
pub fn native_mode() -> Option<String> {
    match common::knobs::RUST_NATIVE_MODE.as_str() {
        "worker" => Some("worker".to_string()),
        _ => None,
    }
}

/// Worker process entry (argv[1] == `WORKER_ARG`). Never returns.
pub fn worker_entry() -> ! {
    super::worker::worker_main()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_target_is_consistent() {
        let t = host_target();
        assert!(t.triple.starts_with(&t.arch));
        if cfg!(target_os = "macos") {
            assert_eq!(t.glibc, None);
            assert!(t.triple.ends_with("apple-darwin"));
        }
    }
}
