//! rust-native mode: user cdylibs run in a pool of worker processes.
//!
//! Contract: `contracts/NATIVE.md` (convex-rs). WN-00 skeleton; owners per
//! `.plan/DESIGN.md` §9 fill in the submodules. WN-06 takes this file over.
#![allow(dead_code)]

pub mod artifact;
pub mod handle;
pub mod metrics;
pub mod pool;
pub mod proto;
pub mod pump;
pub mod sandbox;
pub mod target;
pub mod worker;

use std::{
    collections::{
        HashMap,
        HashSet,
    },
    hash::{
        Hash,
        Hasher,
    },
    path::PathBuf,
    sync::{
        atomic::{
            AtomicU64,
            Ordering,
        },
        Arc,
        OnceLock,
        Weak,
    },
    time::{
        Duration,
        Instant,
    },
};

use common::knobs::{
    FUNCTION_MAX_RESULT_SIZE,
    RUST_MAX_CONCURRENT_ACTIONS,
    RUST_MAX_OUTSTANDING_TASKS,
    RUST_MAX_TASK_REQUEST_BYTES,
    RUST_NATIVE_ALLOW_NETWORK,
    RUST_NATIVE_ALLOW_UNSANDBOXED,
    RUST_NATIVE_CGROUP,
    RUST_NATIVE_EXTRA_LIBS,
    RUST_NATIVE_GID,
    RUST_NATIVE_IDLE_RECYCLE_RSS_BYTES,
    RUST_NATIVE_IDLE_TIMEOUT_SECS,
    RUST_NATIVE_MAX_CALLS_PER_WORKER,
    RUST_NATIVE_MAX_IDLE,
    RUST_NATIVE_MEMORY_BYTES,
    RUST_NATIVE_MIN_IDLE,
    RUST_NATIVE_PREWARM,
    RUST_NATIVE_PROBE_EVERY,
    RUST_NATIVE_PROBE_SLOW_CALL_MS,
    RUST_NATIVE_RECYCLE_RSS_BYTES,
    RUST_NATIVE_RSS_SLACK_BYTES,
    RUST_NATIVE_SANDBOX,
    RUST_NATIVE_SPAWN_THREADS,
    RUST_NATIVE_SPAWN_TIMEOUT_MS,
    RUST_NATIVE_STACK_BYTES,
    RUST_NATIVE_UID,
};
use parking_lot::Mutex;
use serde_json::{
    json,
    Value as JsonValue,
};
use tokio::sync::Semaphore;

use self::{
    artifact::ArtifactStore,
    handle::{
        Launcher,
        NativeSpawner,
        SpawnerConfig,
    },
    pool::{
        Pool,
        PoolConfig,
    },
    pump::{
        CallLimits,
        CallPump,
        CallSpec,
    },
    sandbox::SandboxLevel,
};
use super::core::{
    parse_envelope,
    CallFailure,
    GuestErrorKind,
    GuestResult,
    HostBackend,
};

/// First line of a carrier module source.
pub const NATIVE_MAGIC: &str = "//convex-rust-native v1\n";
/// First line of a ref module source.
pub const NATIVE_REF_MAGIC: &str = "//convex-rust-native-ref v1\n";
/// argv[1] that turns the backend binary into a worker.
pub const WORKER_ARG: &str = "__rust_native_worker";
pub const NATIVE_ABI_VERSION: u32 = 1;

/// A decoded, verified artifact written to `RUST_NATIVE_DIR/bin/<sha>.so`.
#[derive(Debug)]
pub struct Artifact {
    /// sha256 of the binary (hex).
    pub sha256: String,
    pub path: PathBuf,
    pub size: u64,
    /// `module:function` -> exported `convex_run:` symbol.
    pub functions: std::collections::BTreeMap<String, String>,
}

/// Why a native call failed. Mapped to `CallFailure` and the messages in
/// NATIVE.md §4.3/§4.4 by WN-06.
#[derive(Debug, Clone)]
pub enum NativeFailure {
    Timeout,
    OutOfMemory { limit: u64 },
    StackOverflow { stack: u64 },
    /// Signal, sandbox kill or exit without a Result.
    Crashed(String),
    Protocol(String),
    ResultTooLarge { len: usize, max: usize },
    /// Spawn, dlopen or Ready failure (developer error, breaker applies).
    StartFailed(String),
}

/// What the host applies to a worker before exec (rlimits, Landlock,
/// seccomp, uid drop). Built by `sandbox`, applied by the worker itself (`sandbox::enter`) before it loads the dylib.
#[derive(Debug, Clone, Default)]
pub struct SandboxPlan {
    pub memory_bytes: u64,
    pub stack_bytes: u64,
    pub allow_network: bool,
    pub tmp_dir: Option<PathBuf>,
    pub uid_gid: Option<(u32, u32)>,
    pub cgroup: Option<PathBuf>,
    /// The deployment environment the worker process runs with (the process
    /// env is cleared first; NATIVE.md D3).
    pub env: Vec<(String, String)>,
}

/// Starts worker processes; faked in pool tests.
pub trait Spawner: Send + Sync + 'static {
    type Worker: Send + 'static;
    fn spawn(
        &self,
        artifact: &Arc<Artifact>,
        plan: &SandboxPlan,
    ) -> anyhow::Result<Self::Worker>;
}

/// A loaded guest inside the worker; faked in worker-loop tests (no dlopen).
pub trait GuestModule: Send + 'static {
    /// Calls `convex_native_init` with the host vtable.
    fn init(&mut self) -> anyhow::Result<()>;
    /// Runs `convex_run:<mod>:<name>` and returns the result envelope bytes.
    fn call(&mut self, export: &str, request: &[u8]) -> anyhow::Result<Vec<u8>>;
    /// Optional (guest export `convex_native_trim`): whether the guest wants
    /// its allocator trimmed after the call that just replied.
    fn wants_trim(&mut self) -> bool {
        false
    }
    /// Optional: trims the guest's allocator (run when idle).
    fn trim(&mut self) {}
}

/// Prefix shared by both native module kinds; wasm sources are base64 and
/// never start with `//`.
pub const NATIVE_PREFIX: &str = "//convex-rust-native";

/// What a call on a backend with `RUST_NATIVE_MODE=off` gets for a native
/// module (DESIGN.md §6).
pub const MODE_OFF_MESSAGE: &str = "This backend does not run native Rust actions \
                                    (RUST_NATIVE_MODE is off). Push with --rust-runtime wasm, or \
                                    enable it.";

/// The result of one native call.
pub struct NativeCallOutput {
    pub result: Result<GuestResult, CallFailure>,
    /// Call sent until result received (excludes queueing and acquiring).
    pub elapsed: Duration,
    /// Phase timings; `runtime` is the native-specific host overhead.
    pub phases: metrics::CallPhases,
}

impl From<NativeFailure> for CallFailure {
    fn from(f: NativeFailure) -> Self {
        match f {
            NativeFailure::Timeout => CallFailure::Timeout,
            NativeFailure::OutOfMemory { limit } => CallFailure::OutOfMemory { limit },
            NativeFailure::StackOverflow { stack } => CallFailure::StackOverflow { stack },
            NativeFailure::Crashed(m) => CallFailure::Crashed(m),
            NativeFailure::Protocol(m) => CallFailure::Protocol(m),
            NativeFailure::ResultTooLarge { len, max } => CallFailure::ResultTooLarge { len, max },
            NativeFailure::StartFailed(m) => CallFailure::StartFailed(m),
        }
    }
}

static GLOBAL: OnceLock<Weak<NativeRuntime>> = OnceLock::new();

/// The runtime of this process, if one was built (for `/api/rust_native/info`).
pub fn global_runtime() -> Option<Arc<NativeRuntime>> {
    GLOBAL.get()?.upgrade()
}

/// Entry point from `RustActionRuntime`; constructed only when
/// `RUST_NATIVE_MODE=worker`, inside a tokio runtime.
pub struct NativeRuntime {
    store: ArtifactStore,
    pool: Pool<NativeSpawner>,
    /// Bounds concurrent native calls; separate from the wasm and V8 limits.
    limiter: Semaphore,
    /// Source keys (`ModuleMetadata.sha256`) of ref modules -> artifact sha.
    refs: Mutex<HashMap<Vec<u8>, String>>,
    next_call_id: AtomicU64,
    sandbox: SandboxLevel,
    memory_bytes: u64,
    stack_bytes: u64,
    allow_network: bool,
    uid_gid: Option<(u32, u32)>,
    cgroup: Option<PathBuf>,
    limits: CallLimits,
    /// `limits.to_json()`, built once.
    limits_json: Arc<[u8]>,
    /// The sandbox plan minus the environment; cloned per call.
    plan_template: SandboxPlan,
    /// Environment of the most recent call, used to prewarm a new artifact's
    /// worker under the key its first call will most likely use.
    last_env: Mutex<Option<Vec<(String, String)>>>,
    prewarm: bool,
}

/// The sandbox level `RUST_NATIVE_SANDBOX` selects (platform default if unset
/// or invalid).
fn resolve_sandbox_level() -> SandboxLevel {
    match RUST_NATIVE_SANDBOX.as_str() {
        "" => SandboxLevel::default_for_platform(),
        s => s.parse().unwrap_or_else(|e| {
            tracing::error!("RUST_NATIVE_SANDBOX: {e}; using the platform default");
            SandboxLevel::default_for_platform()
        }),
    }
}

/// Backend startup check for `RUST_NATIVE_MODE=worker`: refuses to run user
/// code with no sandbox unless `RUST_NATIVE_ALLOW_UNSANDBOXED=true`.
pub fn startup_check() -> anyhow::Result<()> {
    sandbox::check_policy(resolve_sandbox_level(), *RUST_NATIVE_ALLOW_UNSANDBOXED)
}

fn running_as_root() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

impl NativeRuntime {
    pub fn new() -> Arc<Self> {
        let sandbox = resolve_sandbox_level();
        if let Err(e) = sandbox::check_policy(sandbox, *RUST_NATIVE_ALLOW_UNSANDBOXED) {
            // `startup_check` stops the backend before it gets here.
            tracing::error!("rust-native: {e:#}");
        }
        let limits = CallLimits {
            max_result_bytes: *FUNCTION_MAX_RESULT_SIZE + 64 * 1024,
            max_task_request_bytes: *RUST_MAX_TASK_REQUEST_BYTES,
            max_outstanding_tasks: *RUST_MAX_OUTSTANDING_TASKS,
            ..CallLimits::default()
        };
        let uid_gid = match (RUST_NATIVE_UID.parse::<u32>(), RUST_NATIVE_GID.parse::<u32>()) {
            (Ok(u), Ok(g)) => Some((u, g)),
            _ => None,
        };
        if uid_gid.is_none() && (!RUST_NATIVE_UID.is_empty() || !RUST_NATIVE_GID.is_empty()) {
            tracing::warn!(
                "rust-native: RUST_NATIVE_UID and RUST_NATIVE_GID must both be numeric ids; \
                 ignoring them, workers do NOT drop privileges"
            );
        }
        if uid_gid.is_none() && running_as_root() && sandbox != SandboxLevel::Off {
            tracing::warn!(
                "rust-native: the backend runs as root and RUST_NATIVE_UID/RUST_NATIVE_GID are \
                 not set, so native workers run as root (inside the sandbox). Set both to an \
                 unprivileged user that can read RUST_NATIVE_DIR/bin, or run the backend unprivileged"
            );
        }
        let cgroup_set = !RUST_NATIVE_CGROUP.is_empty();
        let prot = sandbox::protections(sandbox, &sandbox::probe(), uid_gid.is_some(), cgroup_set);
        if sandbox == SandboxLevel::Off {
            tracing::warn!(
                "rust-native: sandbox level off: native code has full access to the host as the \
                 backend user (RUST_NATIVE_ALLOW_UNSANDBOXED is set). Development only"
            );
        } else if sandbox == SandboxLevel::BestEffort
            && (prot.missing.contains(&"landlock") || prot.missing.contains(&"seccomp"))
        {
            tracing::warn!(
                "rust-native: sandbox level best-effort is DEGRADED on this host: active [{}], \
                 NOT enforced [{}]. Use RUST_NATIVE_SANDBOX=strict to refuse to run workers \
                 without them",
                prot.active.join(", "),
                prot.missing.join(", ")
            );
        } else {
            tracing::info!(
                "rust-native: protections active [{}], not enforced [{}]",
                prot.active.join(", "),
                prot.missing.join(", ")
            );
        }
        let spawner = NativeSpawner::new(
            SpawnerConfig {
                sandbox,
                rss_slack_bytes: *RUST_NATIVE_RSS_SLACK_BYTES,
                max_frame_bytes: proto::max_frame_len(
                    limits.max_task_request_bytes,
                    limits.max_result_bytes,
                ),
                spawn_timeout: Duration::from_millis(*RUST_NATIVE_SPAWN_TIMEOUT_MS),
                extra_read: RUST_NATIVE_EXTRA_LIBS
                    .split(',')
                    .map(str::trim)
                    .filter(|s| s.starts_with('/'))
                    .map(PathBuf::from)
                    .collect(),
                launcher: Launcher::CurrentExe,
            },
            tokio::runtime::Handle::current(),
        );
        let pool = Pool::new(
            spawner,
            PoolConfig {
                min_idle: *RUST_NATIVE_MIN_IDLE,
                // Idle workers beyond the call concurrency can never be used.
                max_idle: (*RUST_NATIVE_MAX_IDLE).min((*RUST_MAX_CONCURRENT_ACTIONS).max(1)),
                spawn_threads: *RUST_NATIVE_SPAWN_THREADS,
                probe_every: *RUST_NATIVE_PROBE_EVERY,
                probe_slow_call: Duration::from_millis(*RUST_NATIVE_PROBE_SLOW_CALL_MS),
                idle_timeout: Duration::from_secs(*RUST_NATIVE_IDLE_TIMEOUT_SECS),
                max_calls_per_worker: *RUST_NATIVE_MAX_CALLS_PER_WORKER,
                recycle_rss_bytes: *RUST_NATIVE_RECYCLE_RSS_BYTES,
                idle_recycle_rss_bytes: *RUST_NATIVE_IDLE_RECYCLE_RSS_BYTES,
                ..PoolConfig::default()
            },
        );
        tracing::info!("rust-native: worker mode enabled, sandbox level {sandbox}");
        let store = ArtifactStore::new(artifact::native_dir_from_env());
        // Worker scratch dirs of a previous, dead backend process.
        handle::sweep_stale_scratch(&store.tmp_dir());
        let cgroup = Some(RUST_NATIVE_CGROUP.as_str())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
        let plan_template = SandboxPlan {
            memory_bytes: *RUST_NATIVE_MEMORY_BYTES,
            stack_bytes: *RUST_NATIVE_STACK_BYTES,
            allow_network: *RUST_NATIVE_ALLOW_NETWORK,
            tmp_dir: Some(store.tmp_dir()),
            uid_gid,
            cgroup: cgroup.clone(),
            env: Vec::new(),
        };
        let limits_json: Arc<[u8]> = limits.to_json().into();
        let rt = Arc::new(Self {
            store,
            pool,
            limiter: Semaphore::new((*RUST_MAX_CONCURRENT_ACTIONS).max(1)),
            refs: Mutex::new(HashMap::new()),
            next_call_id: AtomicU64::new(1),
            sandbox,
            memory_bytes: *RUST_NATIVE_MEMORY_BYTES,
            stack_bytes: *RUST_NATIVE_STACK_BYTES,
            allow_network: *RUST_NATIVE_ALLOW_NETWORK,
            uid_gid,
            cgroup,
            limits,
            limits_json,
            plan_template,
            last_env: Mutex::new(None),
            prewarm: *RUST_NATIVE_PREWARM,
        });
        let _ = GLOBAL.set(Arc::downgrade(&rt));
        rt
    }

    pub fn store(&self) -> &ArtifactStore {
        &self.store
    }

    /// Artifact for a module whose source key (`ModuleMetadata.sha256`) was
    /// resolved before. A hit needs no module source.
    pub fn cached(&self, metadata_sha256: &[u8]) -> Option<Arc<Artifact>> {
        if let Some(a) = self.store.cached_by_source_key(metadata_sha256) {
            return Some(a);
        }
        let sha = self.refs.lock().get(metadata_sha256).cloned()?;
        self.store.cached(&sha)
    }

    /// Records that the ref module with this source key resolves to `artifact`.
    pub fn remember_ref(&self, metadata_sha256: &[u8], artifact: &Artifact) {
        self.refs
            .lock()
            .insert(metadata_sha256.to_vec(), artifact.sha256.clone());
    }

    /// Artifacts still in use by the pools plus those the given source keys
    /// resolve to; everything else on disk can go (DESIGN.md §5.4).
    pub fn gc(&self, live_source_keys: &[Vec<u8>], extra: &[String]) {
        let mut live: HashSet<String> = extra.iter().cloned().collect();
        for k in live_source_keys {
            if let Some(a) = self.cached(k) {
                live.insert(a.sha256.clone());
            }
        }
        live.extend(self.pool.stats().into_iter().map(|s| s.artifact));
        let report = self.store.gc(&live);
        if !report.removed.is_empty() {
            tracing::info!(
                "rust-native: removed {} unused artifact(s), freed {} bytes",
                report.removed.len(),
                report.bytes_freed
            );
        }
    }

    /// Starts a worker for a newly resolved artifact in the background, so
    /// the first load of the library (hundreds of ms for a fresh dylib on
    /// macOS) is off the first call's path. Non-blocking; uses the
    /// environment of the last call (a redeploy usually keeps it), else an
    /// empty one, which still warms the library for any later spawn.
    pub fn prewarm(&self, artifact: &Arc<Artifact>) {
        if !self.prewarm {
            return;
        }
        let mut plan = self.plan_template.clone();
        if let Some(env) = self.last_env.lock().clone() {
            plan.env = env;
        }
        let env_hash = env_hash(&plan.env);
        self.pool.prewarm(artifact, env_hash, &plan);
    }

    /// Runs one call: takes the concurrency permit, leases a worker and pumps
    /// the call. `env` is the deployment environment of the worker process.
    pub async fn call<B: HostBackend>(
        &self,
        artifact: &Arc<Artifact>,
        export: String,
        args_json: Vec<u8>,
        env: Vec<(String, String)>,
        backend: B,
        timeout: Duration,
    ) -> NativeCallOutput {
        let t_call = Instant::now();
        let fail = |f: NativeFailure| NativeCallOutput {
            result: Err(f.into()),
            elapsed: Duration::ZERO,
            phases: metrics::CallPhases::default(),
        };
        let Ok(_permit) = self.limiter.acquire().await else {
            return fail(NativeFailure::StartFailed("the native runtime is shut down".into()));
        };
        let queue = t_call.elapsed();
        let t_pre = Instant::now();
        let mut plan = self.plan_template.clone();
        plan.env = env;
        let env_hash = env_hash(&plan.env);
        let pre = t_pre.elapsed();
        let t_acquire = Instant::now();
        let mut lease = match self.pool.acquire(artifact, env_hash, &plan).await {
            Ok(l) => l,
            Err(f) => return fail(f),
        };
        let checkout = t_acquire.elapsed();
        let t_spec = Instant::now();
        let mut spec = CallSpec::new(
            self.next_call_id.fetch_add(1, Ordering::Relaxed),
            export,
            args_json,
            timeout,
        );
        spec.limits = self.limits.clone();
        spec.limits_json = Some(self.limits_json.clone());
        spec.memory_bytes = self.memory_bytes;
        spec.stack_bytes = self.stack_bytes;
        let mut runtime = pre + t_spec.elapsed();
        // Remembered for `prewarm`; the plan is not needed past the acquire.
        {
            let mut last = self.last_env.lock();
            if last.as_deref() != Some(&plan.env[..]) {
                *last = Some(plan.env);
            }
        }
        let started = Instant::now();
        let report = CallPump::new(spec, backend).run(lease.worker_mut()).await;
        let elapsed = started.elapsed();
        let t_post = Instant::now();
        let result = match report.outcome {
            Err(f) => {
                let result = Err(f.clone().into());
                lease.release_failed(&f);
                result
            },
            Ok(out) => match parse_native_envelope(out.envelope_json) {
                Ok(r @ GuestResult::Error { kind: GuestErrorKind::Panic, .. }) => {
                    // A panicking guest may have left statics poisoned.
                    lease.release_recycle();
                    Ok(r)
                },
                Ok(r) => {
                    lease.release_clean_after(elapsed);
                    Ok(r)
                },
                Err(msg) => {
                    let f = NativeFailure::Protocol(msg);
                    let result = Err(f.clone().into());
                    lease.release_failed(&f);
                    result
                },
            },
        };
        runtime += t_post.elapsed();
        let mut phases = report.phases;
        phases.queue = queue;
        phases.checkout = checkout;
        phases.runtime = runtime;
        tracing::trace!(target: "rust_native", "{}", phases.trace_line());
        NativeCallOutput {
            result,
            elapsed,
            phases,
        }
    }

    /// `/api/rust_native/info` pool and sandbox sections.
    pub fn info(&self) -> JsonValue {
        let probe = sandbox::probe();
        let prot = sandbox::protections(
            self.sandbox,
            &probe,
            self.uid_gid.is_some(),
            self.cgroup.is_some(),
        );
        let pools: Vec<JsonValue> = self
            .pool
            .stats()
            .into_iter()
            .map(|s| {
                json!({
                    "artifact": s.artifact, "live": s.live, "idle": s.idle, "busy": s.busy,
                    "spawned": s.spawned, "crashes": s.crashes, "lastCrash": s.last_crash,
                    "breakerOpen": s.breaker_open, "pids": s.idle_pids,
                })
            })
            .collect();
        json!({
            "sandbox": {
                "level": self.sandbox.as_str(),
                "seccomp": probe.seccomp,
                "landlockAbi": probe.landlock_abi,
                "netns": false,
                "cgroup": self.cgroup.is_some(),
                "dropUid": self.uid_gid.is_some(),
                "runningAsRoot": running_as_root(),
                "networkAllowed": self.allow_network,
                // Which protections are actually enforced for workers.
                "active": prot.active,
                "missing": prot.missing,
                "degraded": self.sandbox != SandboxLevel::Strict && (
                    self.sandbox == SandboxLevel::Off
                        || prot.missing.contains(&"landlock")
                        || prot.missing.contains(&"seccomp")),
            },
            "pools": pools,
        })
    }
}

/// Parses the result envelope. The success shape (`{"ok": <value>}`) is read
/// with one scan into a raw slice, with no key map; everything else goes
/// through the shared [`parse_envelope`]. The value itself is parsed once,
/// later, by `deserialize_udf_result`.
fn parse_native_envelope(bytes: Vec<u8>) -> Result<GuestResult, String> {
    #[derive(serde::Deserialize)]
    struct OkEnvelope {
        // `Option<Box<RawValue>>` would turn `"ok": null` into `None`.
        #[serde(default, deserialize_with = "some_raw")]
        ok: Option<Box<serde_json::value::RawValue>>,
    }
    fn some_raw<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> Result<Option<Box<serde_json::value::RawValue>>, D::Error> {
        serde::Deserialize::deserialize(d).map(Some)
    }
    let mut text = String::from_utf8(bytes)
        .map_err(|e| format!("result envelope is not UTF-8: {e}"))?;
    // The worker writes exactly `{"ok":<json>}`. Slice the value out without
    // scanning it: `deserialize_udf_result` parses it next and reports
    // invalid JSON, so a scan here would only repeat that pass (and copy a
    // multi-MB value into a `RawValue`).
    const OK_PREFIX: &str = r#"{"ok":"#;
    if text.len() > OK_PREFIX.len() && text.starts_with(OK_PREFIX) && text.ends_with('}') {
        text.truncate(text.len() - 1);
        text.drain(..OK_PREFIX.len());
        return Ok(GuestResult::OkText(text));
    }
    // Unknown keys (`error`) are ignored; envelopes without `ok` take the
    // slow path, which also produces the error messages.
    if let Ok(OkEnvelope { ok: Some(ok) }) = serde_json::from_str(&text) {
        return Ok(GuestResult::Ok(ok));
    }
    parse_envelope(&text)
}

/// Pool key component for a worker environment. Order-insensitive (the same
/// deployment env listed in a different order must share workers);
/// `DefaultHasher::new()` uses fixed keys, so it is stable within a process.
fn env_hash(env: &[(String, String)]) -> u64 {
    let mut sorted: Vec<&(String, String)> = env.iter().collect();
    sorted.sort();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    sorted.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// N-T4: the same environment in any order maps to the same pool.
    #[test]
    fn env_hash_ignores_order() {
        let env: Vec<(String, String)> =
            (0..6).map(|i| (format!("K{i}"), format!("v{i}"))).collect();
        let h = env_hash(&env);
        let mut rev = env.clone();
        rev.reverse();
        assert_eq!(env_hash(&rev), h);
        let mut rot = env.clone();
        rot.rotate_left(2);
        assert_eq!(env_hash(&rot), h);
        let mut changed = env.clone();
        changed[0].1 = "other".into();
        assert_ne!(env_hash(&changed), h);
        assert_ne!(env_hash(&env[1..]), h);
    }
}
