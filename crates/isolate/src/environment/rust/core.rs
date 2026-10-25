//! The wasm-driving core of the `"use rust"` runtime: engine configuration,
//! the compiled-module LRU, the `convex` import linker and the call/envelope
//! protocol of `contracts/ABI.md`.
//!
//! Nothing here knows about Convex backend types. Everything the guest can
//! reach (tasks, sync syscalls, logging) goes through [`HostBackend`], which
//! lets the core be tested against a trivial in-memory backend.

use std::{
    borrow::Cow,
    collections::{
        BTreeMap,
        HashMap,
    },
    future::Future,
    num::NonZeroUsize,
    pin::Pin,
    sync::{
        atomic::{
            AtomicBool,
            Ordering,
        },
        Arc,
    },
    time::{
        Duration,
        Instant,
    },
};

use anyhow::Context as _;
use async_trait::async_trait;
use futures::future::Either;
use lru::LruCache;
use parking_lot::Mutex;
use serde_json::{
    value::RawValue,
    Value as JsonValue,
};
use sha2::{
    Digest,
    Sha256,
};
use wasmtime::{
    Caller,
    Config,
    Engine,
    Extern,
    InstancePre,
    Linker,
    Memory,
    Store,
    StoreLimits,
    StoreLimitsBuilder,
    UpdateDeadline,
};
use tokio::io::AsyncWrite;
use wasmtime_wasi::{
    cli::{
        IsTerminal,
        StdoutStream,
    },
    p1::{
        self,
        WasiP1Ctx,
    },
    p2::{
        OutputStream,
        Pollable,
        StreamError,
    },
    WasiCtxBuilder,
};

/// Linear memory cap per call (ABI.md: same as the Node action default).
pub const MEMORY_LIMIT_BYTES: usize = 512 * 1024 * 1024;
/// Epoch tick. Also the granularity at which a running guest yields to the
/// async executor and at which the user timeout is enforced.
pub const EPOCH_TICK: Duration = Duration::from_millis(10);
/// Capacity of the compiled-module LRU.
pub const MODULE_CACHE_CAPACITY: usize = 32;
/// Bytes of guest stdout / stderr kept per call; the rest is discarded.
const STDIO_CAPACITY: usize = 16 * 1024 * 1024;
/// Largest instance table (elements) a guest may create.
const TABLE_ELEMENTS_LIMIT: usize = 100_000;
/// Longest `convex` import operation name (`sync_syscall` op).
const MAX_OP_NAME_BYTES: usize = 4096;
const ABI_VERSION: i64 = 1;

/// Host-enforced limits on what a (hostile) guest can make the host do.
#[derive(Clone, Debug)]
pub struct CoreLimits {
    /// Linear memory cap (the only memory; multi-memory is disabled).
    pub memory_bytes: usize,
    /// Largest `task_submit` / `sync_syscall` payload read from guest memory.
    pub max_task_request_bytes: usize,
    /// Most unfinished tasks at once.
    pub max_outstanding_tasks: usize,
    /// Largest result envelope read from guest memory.
    pub max_result_bytes: usize,
    /// Longest log line / `log` import message kept (bytes).
    pub max_log_message_bytes: usize,
    /// Directory of the on-disk compiled-module cache (`<key>-<engine>.cwasm`).
    /// `None` disables it.
    pub disk_cache_dir: Option<std::path::PathBuf>,
}

impl Default for CoreLimits {
    fn default() -> Self {
        Self {
            memory_bytes: MEMORY_LIMIT_BYTES,
            max_task_request_bytes: 64 * 1024 * 1024,
            max_outstanding_tasks: 1000,
            max_result_bytes: 16 * 1024 * 1024 + 64 * 1024,
            max_log_message_bytes: 32 * 1024,
            disk_cache_dir: None,
        }
    }
}

/// The module itself is unusable (bad base64, invalid wasm, bad manifest,
/// unsupported imports, ...). A deterministic user error, not a system one.
#[derive(Debug)]
pub struct InvalidModule(pub String);

impl std::fmt::Display for InvalidModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for InvalidModule {}

fn invalid(e: impl std::fmt::Display) -> anyhow::Error {
    anyhow::Error::new(InvalidModule(e.to_string()))
}

pub type ModuleHash = [u8; 32];

/// What the guest sees of the outside world. One backend per call.
#[async_trait]
pub trait HostBackend: Send + 'static {
    /// `task_submit`. `Ok(id)` (>= 0) or `Err(reason)` when the request is
    /// malformed (the core logs `reason` and returns -1 to the guest).
    fn submit(&mut self, request: JsonValue) -> Result<i64, String>;
    /// `task_poll`. With `block`, waits for the next event.
    async fn poll(&mut self, block: bool) -> PollOutcome;
    /// `sync_syscall`; the returned value is the whole `{"ok": ..}` /
    /// `{"error": ..}` reply.
    fn sync_syscall(&mut self, op: &str, args: JsonValue) -> JsonValue;
    /// `log` import, and stdout/stderr lines (levels 1 and 3). 0 debug,
    /// 1 log/info, 2 warn, 3 error.
    fn log(&mut self, level: i32, message: String);
    /// Number of tasks submitted and not yet finished, for the outstanding
    /// task cap.
    fn outstanding_tasks(&self) -> usize;
}

pub enum PollOutcome {
    /// A serialized event (see ABI.md "Events").
    Event(String),
    /// `block = 0` and nothing is ready.
    Empty,
    /// `block = 1` and no tasks are outstanding.
    NoTasks,
}

/// Wasmtime store data.
/// Prefix of the log line the SDK's panic hook writes before aborting.
pub const PANIC_LOG_PREFIX: &str = "Uncaught panic: ";

pub struct Host<B: HostBackend> {
    wasi: WasiP1Ctx,
    limits: StoreLimits,
    core_limits: CoreLimits,
    stdout: StdioSink,
    stderr: StdioSink,
    backend: B,
    /// Last `Uncaught panic: <message> at <file:line:col>` log line, written
    /// by the SDK's panic hook just before the abort trap.
    panic: Option<String>,
}

impl<B: HostBackend> Host<B> {
    /// Forwards finished stdout / stderr lines to the backend. `finish` also
    /// forwards a trailing partial line.
    fn flush_stdio(&mut self, finish: bool) {
        let max = self.core_limits.max_log_message_bytes;
        for (level, sink) in [(1, &self.stdout), (3, &self.stderr)] {
            for line in sink.take_lines(finish, max) {
                self.backend.log(level, line);
            }
        }
    }
}

/// Captured guest stdout / stderr. Writes past the capacity are discarded
/// (reported as written) instead of trapping the guest.
#[derive(Clone)]
struct StdioSink {
    state: Arc<Mutex<StdioState>>,
    capacity: usize,
}

#[derive(Default)]
struct StdioState {
    bytes: Vec<u8>,
    overflowed: bool,
    overflow_reported: bool,
}

impl StdioSink {
    fn new(capacity: usize) -> Self {
        Self {
            state: Arc::default(),
            capacity,
        }
    }

    fn append(&self, buf: &[u8]) {
        let mut st = self.state.lock();
        let room = self.capacity.saturating_sub(st.bytes.len());
        if buf.len() > room {
            st.overflowed = true;
        }
        let n = buf.len().min(room);
        st.bytes.extend_from_slice(&buf[..n]);
    }

    /// Complete lines (or everything when `finish`), each truncated to
    /// `max_line` bytes.
    fn take_lines(&self, finish: bool, max_line: usize) -> Vec<String> {
        let mut st = self.state.lock();
        let end = if finish {
            st.bytes.len()
        } else {
            match st.bytes.iter().rposition(|b| *b == b'\n') {
                Some(i) => i + 1,
                None => 0,
            }
        };
        let taken: Vec<u8> = st.bytes.drain(..end).collect();
        let mut lines: Vec<String> = String::from_utf8_lossy(&taken)
            .split_terminator('\n')
            .map(|l| truncate_utf8(l.to_string(), max_line))
            .collect();
        if finish && st.overflowed && !st.overflow_reported {
            st.overflow_reported = true;
            lines.push("[output truncated: stdio limit exceeded]".to_string());
        }
        lines
    }
}

impl IsTerminal for StdioSink {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for StdioSink {
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(self.clone())
    }

    fn p2_stream(&self) -> Box<dyn OutputStream> {
        Box::new(self.clone())
    }
}

impl AsyncWrite for StdioSink {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        self.append(buf);
        std::task::Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

impl OutputStream for StdioSink {
    fn write(&mut self, bytes: bytes::Bytes) -> Result<(), StreamError> {
        self.append(&bytes);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), StreamError> {
        Ok(())
    }

    fn check_write(&mut self) -> Result<usize, StreamError> {
        Ok(1 << 20)
    }
}

#[async_trait]
impl Pollable for StdioSink {
    async fn ready(&mut self) {}
}

/// Truncates to at most `max` bytes on a char boundary.
fn truncate_utf8(mut s: String, max: usize) -> String {
    if s.len() > max {
        let mut cut = max;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
    }
    s
}

/// A compiled and linked module.
pub struct PreparedModule<B: HostBackend> {
    pub hash: ModuleHash,
    pre: InstancePre<Host<B>>,
    /// Convex function path (`rust/<module>:<name>`) to wasm export name.
    functions: BTreeMap<String, String>,
}

impl<B: HostBackend> PreparedModule<B> {
    /// The wasm export implementing the Convex function `rust/<module>:<name>`.
    pub fn export_for(&self, module: &str, function: &str) -> Option<&str> {
        let module = module.strip_suffix(".js").unwrap_or(module);
        self.functions
            .get(&format!("{module}:{function}"))
            .map(String::as_str)
    }

    pub fn function_paths(&self) -> impl Iterator<Item = &str> {
        self.functions.keys().map(String::as_str)
    }
}

pub struct CallOutput<B> {
    /// The parsed result envelope, or why the call failed.
    pub result: Result<GuestResult, CallFailure>,
    /// The backend handed back so the caller can finish tasks and logs.
    pub backend: B,
}

#[derive(Debug)]
pub enum GuestResult {
    /// The raw JSON of the `ok` field (already validated as JSON).
    Ok(Box<RawValue>),
    Error {
        message: String,
        data: Option<JsonValue>,
        kind: GuestErrorKind,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum GuestErrorKind {
    Convex,
    Panic,
    User,
}

#[derive(Debug)]
pub enum CallFailure {
    /// The guest ran past the user timeout.
    Timeout,
    /// Trap (unreachable, OOM, out-of-bounds guest pointer, ...).
    Trap(String),
    /// The guest panicked: `message` is `<panic message> at <file:line:col>`
    /// as reported by its panic hook; `trap` is the wasm trap that followed.
    Panic { message: String, trap: String },
    /// The guest broke the ABI (bad envelope, missing exports).
    Protocol(String),
    /// The result envelope is larger than the function result size limit.
    ResultTooLarge { len: usize, max: usize },
}

pub struct WasmCore<B: HostBackend> {
    engine: Engine,
    linker: Linker<Host<B>>,
    cache: Mutex<LruCache<ModuleHash, Arc<PreparedModule<B>>>>,
    /// Per-hash compile locks: concurrent `prepare`s of one module compile it
    /// once.
    compiling: Mutex<HashMap<ModuleHash, Arc<Mutex<()>>>>,
    limits: CoreLimits,
    /// Identifies this wasmtime build + engine config in cache file names.
    engine_tag: String,
    stop_ticker: Arc<AtomicBool>,
}

impl<B: HostBackend> Drop for WasmCore<B> {
    fn drop(&mut self) {
        self.stop_ticker.store(true, Ordering::Relaxed);
    }
}

impl<B: HostBackend> WasmCore<B> {
    pub fn new() -> anyhow::Result<Self> {
        Self::with_limits(CoreLimits::default())
    }

    pub fn with_limits(limits: CoreLimits) -> anyhow::Result<Self> {
        let mut cfg = Config::new();
        cfg.epoch_interruption(true);
        // The store limiter caps each memory separately, so allow only one.
        cfg.wasm_multi_memory(false);
        let engine = Engine::new(&cfg).map_err(werr)?;
        let engine_tag = {
            use std::hash::{
                Hash,
                Hasher,
            };
            // `DefaultHasher::new()` uses fixed keys, so the tag is stable
            // across processes of one build.
            let mut h = std::collections::hash_map::DefaultHasher::new();
            engine.precompile_compatibility_hash().hash(&mut h);
            format!("{:016x}", h.finish())
        };

        let mut linker: Linker<Host<B>> = Linker::new(&engine);
        p1::add_to_linker_async(&mut linker, |h: &mut Host<B>| &mut h.wasi).map_err(werr)?;
        add_convex_imports(&mut linker).map_err(werr)?;

        let stop_ticker = Arc::new(AtomicBool::new(false));
        {
            let engine = engine.clone();
            let stop = stop_ticker.clone();
            std::thread::Builder::new()
                .name("rust-action-epoch".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(EPOCH_TICK);
                        engine.increment_epoch();
                    }
                })
                .context("spawning epoch ticker")?;
        }
        Ok(Self {
            engine,
            linker,
            cache: Mutex::new(LruCache::new(
                NonZeroUsize::new(MODULE_CACHE_CAPACITY).expect("nonzero"),
            )),
            compiling: Mutex::new(HashMap::new()),
            engine_tag,
            limits,
            stop_ticker,
        })
    }

    /// Looks `wasm` up in the LRU (by sha256) or compiles it. Compilation is
    /// CPU bound; callers on an async runtime should run this on a blocking
    /// thread. Concurrent calls for the same module compile it once. A module
    /// that cannot be used fails with [`InvalidModule`].
    pub fn prepare(&self, wasm: &[u8]) -> anyhow::Result<Arc<PreparedModule<B>>> {
        let hash: ModuleHash = Sha256::digest(wasm).into();
        self.prepare_keyed(hash, || Ok(Cow::Borrowed(wasm)))
    }

    /// Like [`Self::prepare`] for a base64 module source. The cache is keyed
    /// by the source text, so a hit skips the decode too.
    pub fn prepare_base64(&self, source: &str) -> anyhow::Result<Arc<PreparedModule<B>>> {
        let hash: ModuleHash = Sha256::digest(source.as_bytes()).into();
        self.prepare_base64_keyed(hash, source)
    }

    /// Cache hit for `key` (e.g. `ModuleMetadata.sha256`), without touching
    /// the module source. Callers fetch the source only on a miss.
    pub fn lookup(&self, key: &ModuleHash) -> Option<Arc<PreparedModule<B>>> {
        self.cache.lock().get(key).cloned()
    }

    /// [`Self::prepare_base64`] under a caller-supplied content key.
    pub fn prepare_base64_keyed(
        &self,
        key: ModuleHash,
        source: &str,
    ) -> anyhow::Result<Arc<PreparedModule<B>>> {
        self.prepare_keyed(key, || {
            base64::decode(source.trim())
                .map(Cow::Owned)
                .map_err(|e| invalid(format!("module source is not valid base64: {e}")))
        })
    }

    fn prepare_keyed<'a>(
        &self,
        hash: ModuleHash,
        wasm: impl FnOnce() -> anyhow::Result<Cow<'a, [u8]>>,
    ) -> anyhow::Result<Arc<PreparedModule<B>>> {
        if let Some(hit) = self.cache.lock().get(&hash) {
            return Ok(hit.clone());
        }
        let lock = self.compiling.lock().entry(hash).or_default().clone();
        let result = {
            let _guard = lock.lock();
            // Someone may have compiled it while we waited.
            let hit = self.cache.lock().get(&hash).cloned();
            match hit {
                Some(hit) => Ok(hit),
                None => match self.load_from_disk(&hash) {
                    Some(module) => self.finish(hash, module),
                    None => wasm().and_then(|wasm| self.compile(hash, &wasm)),
                },
            }
        };
        self.compiling.lock().remove(&hash);
        result
    }

    fn disk_path(&self, hash: &ModuleHash) -> Option<std::path::PathBuf> {
        let dir = self.limits.disk_cache_dir.as_ref()?;
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        Some(dir.join(format!("{hex}-{}.cwasm", self.engine_tag)))
    }

    /// Loads a module we serialized earlier. Any failure is a cache miss.
    fn load_from_disk(&self, hash: &ModuleHash) -> Option<wasmtime::Module> {
        let path = self.disk_path(hash)?;
        if !path.exists() {
            return None;
        }
        // SAFETY: `Module::deserialize_file` trusts the file contents (a
        // forged artifact is arbitrary code execution) and requires the file
        // to stay unchanged while mapped. Both hold because (1) files in this
        // directory are written only by `store_to_disk` below, from modules
        // that already passed `check_manifest` and `Module::new` in this
        // process, with the name derived from the content hash and the exact
        // wasmtime build/config; the directory is under the backend's own
        // data dir with the same trust as the database; (2) files are
        // published by atomic rename of a fully written temp file and never
        // modified in place, so a replaced file leaves the old inode (and any
        // live mapping) intact. wasmtime additionally rejects artifacts from
        // another version/config.
        match unsafe { wasmtime::Module::deserialize_file(&self.engine, &path) } {
            Ok(m) => Some(m),
            Err(e) => {
                tracing::warn!("ignoring unusable wasm cache file {path:?}: {e:#}");
                None
            },
        }
    }

    fn store_to_disk(&self, hash: &ModuleHash, module: &wasmtime::Module) {
        let Some(path) = self.disk_path(hash) else {
            return;
        };
        let res = (|| -> anyhow::Result<()> {
            let bytes = module.serialize().map_err(werr)?;
            std::fs::create_dir_all(path.parent().context("no parent")?)?;
            let tmp = path.with_extension(format!("tmp{}-{:?}", std::process::id(), std::thread::current().id()));
            std::fs::write(&tmp, &bytes)?;
            std::fs::rename(&tmp, &path).inspect_err(|_| {
                let _ = std::fs::remove_file(&tmp);
            })?;
            Ok(())
        })();
        if let Err(e) = res {
            tracing::warn!("could not write wasm cache file {path:?}: {e:#}");
        }
    }

    fn compile(&self, hash: ModuleHash, wasm: &[u8]) -> anyhow::Result<Arc<PreparedModule<B>>> {
        check_manifest(wasm).map_err(|e| invalid(format!("{e:#}")))?;
        let module = wasmtime::Module::new(&self.engine, wasm)
            .map_err(|e| invalid(format!("invalid wasm module: {e:#}")))?;
        self.store_to_disk(&hash, &module);
        // Re-map the module from its cache file: outside Linux, wasmtime only
        // copy-on-write-initializes memory for file-backed modules, so the
        // in-memory one would copy every data segment on each instantiation.
        let module = self.load_from_disk(&hash).unwrap_or(module);
        self.finish(hash, module)
    }

    /// Reads the exports, links and caches a compiled module.
    fn finish(
        &self,
        hash: ModuleHash,
        module: wasmtime::Module,
    ) -> anyhow::Result<Arc<PreparedModule<B>>> {
        let mut functions = BTreeMap::new();
        for e in module.exports() {
            let Some(rest) = e.name().strip_prefix("convex_run:") else {
                continue;
            };
            let (module_path, name) = rest
                .rsplit_once(':')
                .ok_or_else(|| invalid(format!("bad export name {}", e.name())))?;
            // `app::a::b` (Rust `module_path!()`) -> `rust/a/b`; the first
            // segment is the crate name.
            let segs: Vec<&str> = module_path.split("::").skip(1).collect();
            functions.insert(
                format!("rust/{}:{}", segs.join("/"), name),
                e.name().to_string(),
            );
        }
        // Unknown imports show up here as link errors.
        let pre = self
            .linker
            .instantiate_pre(&module)
            .map_err(|e| invalid(format!("unsupported module imports: {e:#}")))?;
        let prepared = Arc::new(PreparedModule {
            hash,
            pre,
            functions,
        });
        self.cache.lock().put(hash, prepared.clone());
        Ok(prepared)
    }

    /// Runs one export in a fresh store and instance. `wall_clock` resolves
    /// when the user timeout has elapsed; it also covers a guest parked on
    /// `task_poll(block)`, where the epoch hook never fires. On timeout the
    /// call is abandoned but stdout / stderr are still forwarded.
    pub async fn call(
        &self,
        module: &PreparedModule<B>,
        export: &str,
        args_json: &str,
        env: Vec<(String, String)>,
        backend: B,
        timeout: Duration,
        wall_clock: impl Future<Output = ()>,
    ) -> CallOutput<B> {
        let stdout = StdioSink::new(STDIO_CAPACITY);
        let stderr = StdioSink::new(STDIO_CAPACITY);
        let mut wasi = WasiCtxBuilder::new();
        wasi.stdout(stdout.clone()).stderr(stderr.clone());
        for (k, v) in &env {
            wasi.env(k, v);
        }
        let mut store = Store::new(
            &self.engine,
            Host {
                wasi: wasi.build_p1(),
                limits: StoreLimitsBuilder::new()
                    .memory_size(self.limits.memory_bytes)
                    .memories(1)
                    .tables(4)
                    .table_elements(TABLE_ELEMENTS_LIMIT)
                    .instances(1)
                    .build(),
                core_limits: self.limits.clone(),
                stdout,
                stderr,
                backend,
                panic: None,
            },
        );
        store.limiter(|h| &mut h.limits);
        let timed_out = Arc::new(AtomicBool::new(false));
        {
            let timed_out = timed_out.clone();
            let start = Instant::now();
            store.set_epoch_deadline(1);
            store.epoch_deadline_callback(move |_| {
                if start.elapsed() >= timeout {
                    timed_out.store(true, Ordering::Relaxed);
                    Err(wasmtime::Error::msg("user timeout"))
                } else {
                    // Yield so a CPU-bound guest cannot starve the runtime.
                    Ok(UpdateDeadline::Yield(1))
                }
            });
        }

        let outcome = {
            let run = run_export(&mut store, &module.pre, export, args_json);
            futures::pin_mut!(run, wall_clock);
            match futures::future::select(run, wall_clock).await {
                Either::Left((outcome, _)) => outcome,
                Either::Right(((), _)) => {
                    timed_out.store(true, Ordering::Relaxed);
                    Err(RunError::Trap("user timeout".into()))
                },
            }
        };

        let mut host = store.into_data();
        host.flush_stdio(true);
        let panic = host.panic.take();
        let result = match outcome {
            Ok(text) => parse_envelope(&text).map_err(CallFailure::Protocol),
            Err(RunError::Trap(_)) if timed_out.load(Ordering::Relaxed) => {
                Err(CallFailure::Timeout)
            },
            Err(RunError::Trap(e)) => Err(match panic {
                Some(message) => CallFailure::Panic { message, trap: e },
                None => CallFailure::Trap(e),
            }),
            Err(RunError::Protocol(e)) => Err(CallFailure::Protocol(e)),
            Err(RunError::ResultTooLarge { len, max }) => {
                Err(CallFailure::ResultTooLarge { len, max })
            },
        };
        CallOutput {
            result,
            backend: host.backend,
        }
    }
}

enum RunError {
    Trap(String),
    Protocol(String),
    ResultTooLarge { len: usize, max: usize },
}

async fn run_export<B: HostBackend>(
    store: &mut Store<Host<B>>,
    pre: &InstancePre<Host<B>>,
    export: &str,
    args: &str,
) -> Result<String, RunError> {
    let trap = |e: wasmtime::Error| RunError::Trap(format!("{e:#}"));
    let proto = |e: wasmtime::Error| RunError::Protocol(format!("{e:#}"));
    let instance = pre.instantiate_async(&mut *store).await.map_err(trap)?;
    if let Some(init) = instance.get_func(&mut *store, "_initialize") {
        let init = init.typed::<(), ()>(&*store).map_err(proto)?;
        init.call_async(&mut *store, ()).await.map_err(trap)?;
    }
    let memory = instance
        .get_memory(&mut *store, "memory")
        .ok_or_else(|| RunError::Protocol("module has no `memory` export".into()))?;
    let alloc = instance
        .get_typed_func::<i32, i32>(&mut *store, "convex_alloc")
        .map_err(proto)?;
    let run = instance
        .get_typed_func::<(i32, i32), i64>(&mut *store, export)
        .map_err(proto)?;
    let len =
        i32::try_from(args.len()).map_err(|_| RunError::Protocol("arguments too large".into()))?;
    let ptr = alloc.call_async(&mut *store, len).await.map_err(trap)?;
    write_memory(&memory, &mut *store, ptr, args.as_bytes()).map_err(trap)?;
    let packed = run
        .call_async(&mut *store, (ptr, len))
        .await
        .map_err(trap)?;
    let (rptr, rlen) = unpack(packed);
    // Check the size before copying anything out of guest memory.
    let max = store.data().core_limits.max_result_bytes;
    if rlen as usize > max {
        return Err(RunError::ResultTooLarge {
            len: rlen as usize,
            max,
        });
    }
    let bytes = read_memory(&memory, &*store, rptr, rlen)
        .map_err(|e| RunError::Protocol(format!("result envelope: {e:#}")))?;
    String::from_utf8(bytes)
        .map_err(|e| RunError::Protocol(format!("result envelope is not UTF-8: {e}")))
}

/// Parses the packed `convex_run` return value (ABI.md "Result envelope").
pub fn parse_envelope(text: &str) -> Result<GuestResult, String> {
    let mut v: BTreeMap<String, Box<RawValue>> =
        serde_json::from_str(text).map_err(|e| format!("guest returned invalid JSON: {e}"))?;
    if let Some(ok) = v.remove("ok") {
        return Ok(GuestResult::Ok(ok));
    }
    let Some(err) = v.remove("error") else {
        return Err("result envelope has neither `ok` nor `error`".into());
    };
    let err: JsonValue = serde_json::from_str(err.get()).map_err(|e| e.to_string())?;
    let message = err
        .get("message")
        .and_then(JsonValue::as_str)
        .unwrap_or("unknown error")
        .to_string();
    let kind = match err.get("kind").and_then(JsonValue::as_str) {
        Some("convex") => GuestErrorKind::Convex,
        Some("panic") => GuestErrorKind::Panic,
        _ => GuestErrorKind::User,
    };
    // `ConvexError(null)` keeps its (null) data, like JS.
    let data = err
        .get("data")
        .filter(|d| !d.is_null() || kind == GuestErrorKind::Convex)
        .cloned();
    Ok(GuestResult::Error {
        message,
        data,
        kind,
    })
}

pub fn unpack(v: i64) -> (u32, u32) {
    let v = v as u64;
    ((v >> 32) as u32, (v & 0xffff_ffff) as u32)
}

pub fn pack(ptr: u32, len: u32) -> i64 {
    (((ptr as u64) << 32) | len as u64) as i64
}

fn werr(e: wasmtime::Error) -> anyhow::Error {
    anyhow::anyhow!("{e:#}")
}

fn read_memory(
    mem: &Memory,
    store: impl wasmtime::AsContext,
    ptr: u32,
    len: u32,
) -> wasmtime::Result<Vec<u8>> {
    let start = ptr as usize;
    let end = start
        .checked_add(len as usize)
        .ok_or_else(|| wasmtime::Error::msg("guest pointer out of bounds"))?;
    mem.data(&store)
        .get(start..end)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| wasmtime::Error::msg("guest pointer out of bounds"))
}

fn write_memory(
    mem: &Memory,
    store: impl wasmtime::AsContextMut,
    ptr: i32,
    bytes: &[u8],
) -> wasmtime::Result<()> {
    mem.write(store, ptr as u32 as usize, bytes)
        .map_err(|_| wasmtime::Error::msg("convex_alloc returned an out-of-bounds pointer"))
}

fn guest_memory<B: HostBackend>(caller: &mut Caller<'_, Host<B>>) -> wasmtime::Result<Memory> {
    match caller.get_export("memory") {
        Some(Extern::Memory(m)) => Ok(m),
        _ => Err(wasmtime::Error::msg("guest has no memory export")),
    }
}

fn read_guest<B: HostBackend>(
    caller: &mut Caller<'_, Host<B>>,
    ptr: i32,
    len: i32,
) -> wasmtime::Result<Vec<u8>> {
    let mem = guest_memory(caller)?;
    read_memory(&mem, &*caller, ptr as u32, len as u32)
}

/// `convex_alloc` handshake: allocate in the guest, copy, return packed
/// `(ptr << 32) | len`.
async fn write_guest<B: HostBackend>(
    caller: &mut Caller<'_, Host<B>>,
    bytes: &[u8],
) -> wasmtime::Result<i64> {
    let alloc = match caller.get_export("convex_alloc") {
        Some(Extern::Func(f)) => f.typed::<i32, i32>(&*caller)?,
        _ => return Err(wasmtime::Error::msg("guest has no convex_alloc export")),
    };
    let len = u32::try_from(bytes.len())
        .ok()
        .filter(|l| *l <= i32::MAX as u32)
        .ok_or_else(|| wasmtime::Error::msg("host reply too large for guest"))?;
    let ptr = alloc.call_async(&mut *caller, len as i32).await?;
    let mem = guest_memory(caller)?;
    write_memory(&mem, &mut *caller, ptr, bytes)?;
    Ok(pack(ptr as u32, len))
}

fn add_convex_imports<B: HostBackend>(linker: &mut Linker<Host<B>>) -> wasmtime::Result<()> {
    linker.func_wrap(
        "convex",
        "task_submit",
        |mut c: Caller<'_, Host<B>>, ptr: i32, len: i32| -> wasmtime::Result<i64> {
            let max = c.data().core_limits.max_task_request_bytes;
            // Reject before touching guest memory.
            if len as u32 as usize > max {
                c.data_mut().backend.log(
                    3,
                    format!("task_submit: request of {} bytes exceeds {max}", len as u32),
                );
                return Ok(-1);
            }
            let max_tasks = c.data().core_limits.max_outstanding_tasks;
            if c.data().backend.outstanding_tasks() >= max_tasks {
                c.data_mut().backend.log(
                    3,
                    format!("task_submit: more than {max_tasks} outstanding tasks"),
                );
                return Ok(-1);
            }
            let bytes = read_guest(&mut c, ptr, len)?;
            let req: JsonValue = match serde_json::from_slice(&bytes) {
                Ok(v) => v,
                Err(e) => {
                    c.data_mut()
                        .backend
                        .log(3, format!("task_submit: malformed request: {e}"));
                    return Ok(-1);
                },
            };
            match c.data_mut().backend.submit(req) {
                Ok(id) => Ok(id),
                Err(reason) => {
                    c.data_mut()
                        .backend
                        .log(3, format!("task_submit: {reason}"));
                    Ok(-1)
                },
            }
        },
    )?;

    linker.func_wrap_async(
        "convex",
        "task_poll",
        |mut c: Caller<'_, Host<B>>, (block,): (i32,)| {
            Box::new(async move {
                match c.data_mut().backend.poll(block != 0).await {
                    PollOutcome::Event(ev) => write_guest(&mut c, ev.as_bytes()).await,
                    PollOutcome::Empty => Ok(0),
                    PollOutcome::NoTasks => Ok(-1),
                }
            })
        },
    )?;

    linker.func_wrap_async(
        "convex",
        "sync_syscall",
        |mut c: Caller<'_, Host<B>>, (op_ptr, op_len, args_ptr, args_len): (i32, i32, i32, i32)| {
            Box::new(async move {
                let max = c.data().core_limits.max_task_request_bytes;
                if op_len as u32 as usize > MAX_OP_NAME_BYTES || args_len as u32 as usize > max {
                    let reply = serde_json::json!({
                        "error": {"message": "sync_syscall: request too large"}
                    });
                    return write_guest(&mut c, reply.to_string().as_bytes()).await;
                }
                let op = String::from_utf8_lossy(&read_guest(&mut c, op_ptr, op_len)?).into_owned();
                let args_bytes = read_guest(&mut c, args_ptr, args_len)?;
                let reply = match serde_json::from_slice(&args_bytes) {
                    Ok(args) => {
                        // `crypto/*` is served here; it reads guest buffers in place.
                        let served = if op.starts_with("crypto/") {
                            let mem = guest_memory(&mut c)?;
                            let data = mem.data_mut(&mut c);
                            // Large inputs would stall this tokio worker; hand the
                            // worker's queue to another thread while hashing.
                            // (`block_in_place` panics on a current-thread runtime.)
                            let big = super::host_crypto::input_len(&args) > (1 << 20)
                                && tokio::runtime::Handle::try_current().is_ok_and(|h| {
                                    h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread
                                });
                            if big {
                                tokio::task::block_in_place(|| {
                                    super::host_crypto::sync_op(&op, &args, data)
                                })
                            } else {
                                super::host_crypto::sync_op(&op, &args, data)
                            }
                        } else {
                            None
                        };
                        match served {
                            Some(reply) => reply,
                            None => c.data_mut().backend.sync_syscall(&op, args),
                        }
                    },
                    Err(e) => serde_json::json!({
                        "error": {"message": format!("sync op {op}: invalid args: {e}")}
                    }),
                };
                write_guest(&mut c, reply.to_string().as_bytes()).await
            })
        },
    )?;

    linker.func_wrap(
        "convex",
        "log",
        |mut c: Caller<'_, Host<B>>, level: i32, ptr: i32, len: i32| -> wasmtime::Result<()> {
            // Never copy more than one log line out of guest memory.
            let max = c.data().core_limits.max_log_message_bytes;
            let len = (len as u32 as usize).min(max) as i32;
            let bytes = read_guest(&mut c, ptr, len)?;
            let message = truncate_utf8(String::from_utf8_lossy(&bytes).into_owned(), max);
            // Keep stdout / stderr lines written so far ahead of this one.
            let host = c.data_mut();
            host.flush_stdio(false);
            // Level 4 is the panic marker (`convex_panic_marker` guests): not a
            // user log line.
            if level == 4 {
                return Ok(());
            }
            if level == 3
                && let Some(p) = message.strip_prefix(PANIC_LOG_PREFIX) {
                    host.panic = Some(p.to_string());
                }
            host.backend.log(level, message);
            Ok(())
        },
    )?;
    Ok(())
}

/// Parses the `convex:manifest` custom section (contracts/MANIFEST.md) and
/// checks `abiVersion`.
fn check_manifest(wasm: &[u8]) -> anyhow::Result<()> {
    let mut found = 0;
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        let payload = payload.context("invalid wasm module")?;
        if let wasmparser::Payload::CustomSection(s) = payload {
            if s.name() != "convex:manifest" {
                continue;
            }
            for line in std::str::from_utf8(s.data())?
                .split('\n')
                .filter(|l| !l.trim().is_empty())
            {
                let m: JsonValue =
                    serde_json::from_str(line).context("bad convex:manifest fragment")?;
                if m["abiVersion"].as_i64() != Some(ABI_VERSION) {
                    anyhow::bail!("unsupported abiVersion {}", m["abiVersion"]);
                }
                found += 1;
            }
        }
    }
    anyhow::ensure!(found > 0, "module has no convex:manifest entries");
    Ok(())
}
