//! macOS (dev only, DESIGN §4.2): no seccomp, Landlock or cgroups. The
//! sandbox is env_clear + setsid + rlimits (core, nofile) + the host's
//! SIGKILL + the RSS/orphan watchdog. Everything portable lives in
//! `sandbox/mod.rs`; this module only documents the platform's level.

/// Warning logged once when a worker starts unsandboxed on macOS.
pub const UNSANDBOXED_WARNING: &str =
    "native Rust actions run unsandboxed on macOS (dev mode)";
