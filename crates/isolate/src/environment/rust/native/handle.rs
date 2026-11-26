//! Host-side worker process: spawn, handshake, handle (NATIVE.md §2.1, §4.4;
//! DESIGN.md §1.2).
//!
//! [`NativeSpawner`] implements [`Spawner`] for the [`Pool`](super::pool::Pool).
//! It runs on the pool's `rust-native-spawner` thread (not a tokio thread), so
//! it captures a tokio [`Handle`] at construction and enters it only to
//! register the host ends of the fds with the reactor.
//!
//! fds of the child: 0 = `/dev/null`, 1 and 2 = pipes, 3 = socketpair (frames),
//! 4 = status pipe. L2-03: there is no `pre_exec` (it forced a `fork()` of the
//! whole backend; plain `Command` uses `posix_spawn`). The child is put into its
//! own process group by `Command::process_group(0)` (pid == pgid, so
//! `kill(-pid)` takes the tree down); the worker itself moves the inherited fds
//! onto 3 and 4 and applies rlimits, uid drop, Landlock, seccomp
//! (`sandbox::enter`) before it opens the user dylib. The only parent-side
//! step is the cgroup: right after `spawn` the parent writes the pid into
//! `cgroup.procs` and sends one go byte on the frame socket; the worker blocks
//! on that byte before loading anything and the parent does not accept `Ready`
//! earlier than that.
//!
//! [`NativeWorker`] is both the [`PumpWorker`] (the `CallPump` borrows its
//! [`WorkerIo`] and process control) and the [`PoolWorker`]. The pump itself
//! reads the status pipe and classifies a dead worker (`pump::classify`); this
//! module reads it only for spawn failures.

use std::{
    ffi::OsString,
    os::{
        fd::{
            AsRawFd,
            FromRawFd,
            OwnedFd,
        },
        unix::{
            net::UnixStream as StdUnixStream,
            process::ExitStatusExt,
        },
    },
    path::{
        Path,
        PathBuf,
    },
    process::{
        Child,
        Command,
        ExitStatus,
        Stdio,
    },
    sync::Arc,
    time::{
        Duration,
        Instant,
    },
};

use anyhow::Context as _;
use std::os::unix::process::CommandExt as _;
use async_trait::async_trait;
use parking_lot::Mutex;
use tokio::{
    net::UnixStream,
    runtime::Handle,
};

use super::{
    pool::PoolWorker,
    proto::{
        self,
        Frame,
    },
    pump::{
        classify,
        parse_status,
        ExitInfo,
        Observation,
        PipeReader,
        PumpWorker,
        WorkerIo,
        WorkerProc,
    },
    sandbox::{
        check_support,
        SandboxLevel,
    },
    worker::{
        WorkerArgs,
        EXIT_OOM,
    },
    Artifact,
    NativeFailure,
    SandboxPlan,
    Spawner,
};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// How the worker process is started.
#[derive(Clone, Debug)]
pub enum Launcher {
    /// `current_exe() __rust_native_worker …` (D2: the backend is the worker).
    CurrentExe,
    /// A binary that is not the backend (tests): `exe <prefix_args…>` is run
    /// and the real worker arguments (starting with `WORKER_ARG`) are passed
    /// in the environment variable `args_env`, separated by `\x1f`. The
    /// program is expected to call `worker::worker_main_with`.
    Shim {
        exe: PathBuf,
        prefix_args: Vec<String>,
        args_env: String,
    },
}

/// Separator of worker arguments in a [`Launcher::Shim`] environment variable.
pub const SHIM_ARG_SEP: char = '\x1f';

/// Spawner settings that are not per call (the per-call part is [`SandboxPlan`]).
#[derive(Clone, Debug)]
pub struct SpawnerConfig {
    /// `RUST_NATIVE_SANDBOX`
    pub sandbox: SandboxLevel,
    /// `RUST_NATIVE_RSS_SLACK_BYTES`
    pub rss_slack_bytes: u64,
    /// `proto::max_frame_len(max_task_request_bytes, max_result_bytes)`
    pub max_frame_bytes: usize,
    /// `RUST_NATIVE_SPAWN_TIMEOUT_MS`
    pub spawn_timeout: Duration,
    /// `RUST_NATIVE_EXTRA_LIBS`
    pub extra_read: Vec<PathBuf>,
    pub launcher: Launcher,
}

impl Default for SpawnerConfig {
    fn default() -> Self {
        Self {
            sandbox: SandboxLevel::default_for_platform(),
            rss_slack_bytes: WorkerArgs::DEFAULT_RSS_SLACK,
            max_frame_bytes: WorkerArgs::DEFAULT_MAX_FRAME,
            spawn_timeout: Duration::from_millis(5000),
            extra_read: Vec::new(),
            launcher: Launcher::CurrentExe,
        }
    }
}

/// The contents of the worker's `Ready` frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadyInfo {
    pub pid: u32,
    pub resolved_exports: u32,
    pub init_us: u64,
    pub rss_bytes: u64,
}

// ---------------------------------------------------------------------------
// Process control
// ---------------------------------------------------------------------------

/// Converts a wait status into [`ExitInfo`]. `cgroup_oom` is a cgroup
/// `oom_kill` increment; exit 137 is the watchdog's `_exit(EXIT_OOM)`.
pub fn exit_info(status: ExitStatus, cgroup_oom: bool) -> ExitInfo {
    let code = status.code();
    ExitInfo {
        code,
        signal: status.signal(),
        oom_killed: cgroup_oom || code == Some(EXIT_OOM),
    }
}

/// `oom_kill` counter of `<cgroup>/memory.events`.
fn cgroup_oom_kills(cgroup: &Path) -> Option<u64> {
    let text = std::fs::read_to_string(cgroup.join("memory.events")).ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("oom_kill "))
        .and_then(|v| v.trim().parse().ok())
}

struct Proc {
    pid: u32,
    child: Mutex<Child>,
    /// Set once the child has been reaped.
    exit: Mutex<Option<ExitInfo>>,
    cgroup: Option<(PathBuf, u64)>,
}

impl Proc {
    fn record(&self, status: ExitStatus) -> ExitInfo {
        let oom = self
            .cgroup
            .as_ref()
            .and_then(|(p, base)| cgroup_oom_kills(p).map(|n| n > *base))
            .unwrap_or(false);
        let info = exit_info(status, oom);
        *self.exit.lock() = Some(info.clone());
        info
    }

    fn reaped(&self) -> Option<ExitInfo> {
        self.exit.lock().clone()
    }

    /// Non-blocking reap.
    fn poll(&self) -> Option<ExitInfo> {
        if let Some(e) = self.reaped() {
            return Some(e);
        }
        let mut child = self.child.lock();
        match child.try_wait() {
            Ok(Some(st)) => Some(self.record(st)),
            _ => None,
        }
    }

    fn kill_group(&self) {
        // Held while signalling so a concurrent reap cannot free the pid
        // between the check and `kill(2)`.
        let mut child = self.child.lock();
        if self.exit.lock().is_some() {
            return;
        }
        match child.try_wait() {
            Ok(Some(st)) => {
                self.record(st);
            },
            _ => {
                // SAFETY: the child is a session leader, so its pid is the
                // group id; it has not been reaped.
                unsafe {
                    libc::kill(-(self.pid as libc::pid_t), libc::SIGKILL);
                    libc::kill(self.pid as libc::pid_t, libc::SIGKILL);
                }
            },
        }
    }

    /// Kills the group and reaps the child (blocks briefly).
    fn terminate(&self) {
        self.kill_group();
        if self.exit.lock().is_some() {
            return;
        }
        let st = self.child.lock().wait();
        if let Ok(st) = st {
            self.record(st);
        }
    }
}

#[async_trait]
impl WorkerProc for Proc {
    fn kill(&self) {
        self.kill_group();
    }

    async fn wait(&self, grace: Duration) -> Option<ExitInfo> {
        let deadline = Instant::now() + grace;
        let mut delay = Duration::from_micros(200);
        loop {
            if let Some(e) = self.poll() {
                return Some(e);
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            tokio::time::sleep(delay.min(deadline - now)).await;
            delay = (delay * 2).min(Duration::from_millis(10));
        }
    }
}

/// Resident set size of `pid`, if it can be sampled.
#[cfg(target_os = "linux")]
pub fn process_rss_bytes(pid: u32) -> Option<u64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
    let pages: u64 = s.split_whitespace().nth(1)?.parse().ok()?;
    // SAFETY: sysconf has no preconditions.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    Some(pages * page.max(4096) as u64)
}

#[cfg(target_os = "macos")]
pub fn process_rss_bytes(pid: u32) -> Option<u64> {
    // SAFETY: `info` is a plain-old-data out parameter of the stated size.
    unsafe {
        let mut info: libc::proc_taskinfo = std::mem::zeroed();
        let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
        let n = libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTASKINFO,
            0,
            (&mut info as *mut libc::proc_taskinfo).cast(),
            size,
        );
        (n == size).then_some(info.pti_resident_size)
    }
}

/// Number of threads in `pid`, if it can be sampled.
#[cfg(target_os = "linux")]
pub fn process_thread_count(pid: u32) -> Option<u32> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    s.lines().find_map(|l| l.strip_prefix("Threads:"))?.trim().parse().ok()
}

#[cfg(target_os = "macos")]
pub fn process_thread_count(pid: u32) -> Option<u32> {
    // SAFETY: `info` is a plain-old-data out parameter of the stated size.
    unsafe {
        let mut info: libc::proc_taskinfo = std::mem::zeroed();
        let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
        let n = libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTASKINFO,
            0,
            (&mut info as *mut libc::proc_taskinfo).cast(),
            size,
        );
        (n == size).then_some(info.pti_threadnum as u32)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn process_thread_count(_pid: u32) -> Option<u32> {
    None
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn process_rss_bytes(pid: u32) -> Option<u64> {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let kb: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some(kb * 1024)
}

// ---------------------------------------------------------------------------
// The worker handle
// ---------------------------------------------------------------------------

/// A running, ready worker process.
pub struct NativeWorker {
    io: WorkerIo,
    proc_: Arc<Proc>,
    ready: ReadyInfo,
    /// Thread count right after `Ready`; more threads later mean the guest
    /// left one running (NATIVE.md 2.3: statics and threads persist).
    baseline_threads: Option<u32>,
    /// This worker's private scratch dir (`<tmp_dir>/w<backend pid>-<n>`),
    /// removed when the worker goes away.
    scratch: Option<PathBuf>,
}

impl NativeWorker {
    pub fn ready(&self) -> &ReadyInfo {
        &self.ready
    }

    /// Exit information if the process has been reaped already.
    pub fn exit_info(&self) -> Option<ExitInfo> {
        self.proc_.reaped()
    }

    /// Waits for the process to exit; used by tests and shutdown paths.
    pub async fn wait_exit(&self, grace: Duration) -> Option<ExitInfo> {
        self.proc_.wait(grace).await
    }
}

impl PumpWorker for NativeWorker {
    fn split(&mut self) -> (&mut WorkerIo, &dyn WorkerProc) {
        (&mut self.io, &*self.proc_)
    }
}

impl PoolWorker for NativeWorker {
    fn is_alive(&mut self) -> bool {
        self.proc_.poll().is_none()
    }

    fn rss_bytes(&mut self) -> Option<u64> {
        if self.proc_.reaped().is_some() {
            return None;
        }
        process_rss_bytes(self.proc_.pid)
    }

    fn rss_sampler(&self) -> Option<Box<dyn Fn() -> Option<u64> + Send>> {
        if self.proc_.reaped().is_some() {
            return None;
        }
        let pid = self.proc_.pid;
        Some(Box::new(move || process_rss_bytes(pid)))
    }

    fn has_extra_threads(&mut self) -> bool {
        if self.proc_.reaped().is_some() {
            return false;
        }
        match (self.baseline_threads, process_thread_count(self.proc_.pid)) {
            (Some(base), Some(now)) => now > base,
            _ => false,
        }
    }

    fn terminate(&mut self) {
        self.proc_.terminate();
        self.wipe_scratch();
    }

    fn request_shutdown(&mut self) {
        let sent = Frame::Shutdown
            .encode()
            .ok()
            .is_some_and(|bytes| matches!(self.io.sock.try_write(&bytes), Ok(n) if n == bytes.len()));
        if !sent {
            self.proc_.terminate();
        }
    }

    fn pid(&self) -> Option<u32> {
        Some(self.proc_.pid)
    }
}

impl NativeWorker {
    /// Only after the process is gone (nothing can recreate files).
    fn wipe_scratch(&mut self) {
        if let Some(d) = self.scratch.take() {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

impl Drop for NativeWorker {
    fn drop(&mut self) {
        // `kill_on_drop`: no worker outlives its handle.
        self.proc_.terminate();
        self.wipe_scratch();
    }
}

static SCRATCH_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Creates a worker-private scratch dir under `base` (mode 0700, owned by the
/// worker's uid when it drops privileges). Functions of different workers,
/// and so of different users' code, never share files; a recycled worker's
/// files are removed with it.
fn create_scratch(base: &Path, uid_gid: Option<(u32, u32)>) -> anyhow::Result<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::create_dir_all(base).with_context(|| format!("cannot create {}", base.display()))?;
    let n = SCRATCH_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = base.join(format!("w{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .with_context(|| format!("cannot create {}", dir.display()))?;
    if let Some((uid, gid)) = uid_gid {
        std::os::unix::fs::chown(&dir, Some(uid), Some(gid))
            .with_context(|| format!("cannot chown {}", dir.display()))?;
    }
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    Ok(dir)
}

/// Removes scratch dirs left by backend processes that no longer exist
/// (crash or SIGKILL; `Drop` never ran).
pub fn sweep_stale_scratch(base: &Path) {
    let Ok(rd) = std::fs::read_dir(base) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(pid) = name
            .strip_prefix('w')
            .and_then(|r| r.split('-').next())
            .and_then(|p| p.parse::<libc::pid_t>().ok())
        else {
            continue;
        };
        // SAFETY: signal 0 only checks existence.
        let gone = unsafe { libc::kill(pid, 0) } != 0
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        if gone {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

// ---------------------------------------------------------------------------
// Spawner
// ---------------------------------------------------------------------------

pub struct NativeSpawner {
    cfg: SpawnerConfig,
    handle: Handle,
}

impl NativeSpawner {
    /// `handle` is the runtime that will drive the workers' sockets and pipes
    /// (the backend's main runtime). Use `Handle::current()` inside it.
    pub fn new(cfg: SpawnerConfig, handle: Handle) -> Self {
        Self { cfg, handle }
    }

    pub fn config(&self) -> &SpawnerConfig {
        &self.cfg
    }

    /// The worker arguments for `artifact` under `plan`.
    pub fn worker_args(&self, artifact: &Artifact, plan: &SandboxPlan) -> WorkerArgs {
        let default = WorkerArgs::parse(["--artifact".to_string(), String::new()]);
        let stack = if plan.stack_bytes == 0 {
            default.as_ref().map_or(8 << 20, |d| d.stack_bytes)
        } else {
            plan.stack_bytes as usize
        };
        WorkerArgs {
            artifact: artifact.path.clone(),
            stack_bytes: stack,
            memory_bytes: plan.memory_bytes,
            sandbox: self.cfg.sandbox,
            tmp: plan.tmp_dir.clone(),
            functions: artifact.functions.len() as u32,
            allow_network: plan.allow_network,
            rss_slack_bytes: self.cfg.rss_slack_bytes,
            max_frame_bytes: self.cfg.max_frame_bytes,
            sock_fd: super::worker::SOCK_FD,
            status_fd: super::worker::STATUS_FD,
            uid_gid: plan.uid_gid,
            wait_go: plan.cgroup.is_some(),
            extra_read: self.cfg.extra_read.clone(),
        }
    }

    fn command(&self, args: &WorkerArgs, plan: &SandboxPlan) -> anyhow::Result<Command> {
        let worker_args = args.to_args();
        let mut cmd = match &self.cfg.launcher {
            Launcher::CurrentExe => {
                let mut c = Command::new(std::env::current_exe().context("current_exe")?);
                c.args(&worker_args);
                c
            },
            Launcher::Shim {
                exe,
                prefix_args,
                args_env,
            } => {
                let mut c = Command::new(exe);
                c.args(prefix_args);
                c.env_clear();
                c.env(
                    OsString::from(args_env),
                    worker_args.join(&SHIM_ARG_SEP.to_string()),
                );
                c
            },
        };
        if matches!(self.cfg.launcher, Launcher::CurrentExe) {
            cmd.env_clear();
        }
        cmd.envs(plan.env.iter().map(|(k, v)| (k, v)));
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        Ok(cmd)
    }

    fn spawn_inner(
        &self,
        artifact: &Arc<Artifact>,
        plan: &SandboxPlan,
    ) -> anyhow::Result<NativeWorker> {
        let mut args = self.worker_args(artifact, plan);
        check_support(self.cfg.sandbox).context("cannot build the sandbox")?;
        let scratch = match &plan.tmp_dir {
            Some(base) => {
                let dir = create_scratch(base, plan.uid_gid)?;
                args.tmp = Some(dir.clone());
                Some(dir)
            },
            None => None,
        };
        // Removes the scratch dir on every early return below.
        struct Cleanup(Option<PathBuf>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                if let Some(d) = self.0.take() {
                    let _ = std::fs::remove_dir_all(d);
                }
            }
        }
        let mut cleanup = Cleanup(scratch);
        let (host_sock, child_sock) = socketpair()?;
        let (status_rd, status_wr) = pipe()?;
        // The worker relocates these onto 3 and 4 itself (no pre_exec dup2).
        args.sock_fd = child_sock.as_raw_fd();
        args.status_fd = status_wr.as_raw_fd();
        let mut cmd = self.command(&args, plan)?;
        // Own process group => pid == pgid, so `kill(-pid)` kills the tree.
        // `process_group` keeps std on the posix_spawn path (unlike
        // pre_exec / uid / gid, which force fork).
        cmd.process_group(0);
        let baseline = plan
            .cgroup
            .as_ref()
            .map(|p| (p.clone(), cgroup_oom_kills(p).unwrap_or(0)));
        // The two child-end fds must survive exec. Hold a lock from clearing
        // CLOEXEC until the spawn returned so concurrent spawns of ours do not
        // inherit each other's fds.
        let mut child = {
            static SPAWN_FDS: Mutex<()> = Mutex::new(());
            let _g = SPAWN_FDS.lock();
            clear_cloexec(child_sock.as_raw_fd())?;
            clear_cloexec(status_wr.as_raw_fd())?;
            cmd.spawn().context("cannot start the worker process")?
        };
        // The child has its own copies now.
        drop(child_sock);
        drop(status_wr);
        let pid = child.id();
        let stdout = child.stdout.take().map(OwnedFd::from);
        let stderr = child.stderr.take().map(OwnedFd::from);
        let proc_ = Arc::new(Proc {
            pid,
            child: Mutex::new(child),
            exit: Mutex::new(None),
            cgroup: baseline,
        });

        // cgroup membership is parent-side (nothing in the child can do it for
        // itself unprivileged). Ordering: the worker is blocked on the go byte
        // before sandbox/dylib, so it is in the cgroup before any user code.
        if let Some(cg) = &plan.cgroup
            && let Err(e) = place_in_cgroup(cg, pid, &host_sock) {
                proc_.terminate();
                return Err(e.context("cannot place the worker in its cgroup"));
            }

        let ready = match self.handshake(&host_sock) {
            Ok(r) => r,
            Err(e) => {
                proc_.terminate();
                let diag = startup_diagnosis(&proc_, &status_rd, stderr.as_ref(), &args);
                return Err(e.context(diag));
            },
        };

        // Register the host ends with the reactor.
        let _guard = self.handle.enter();
        host_sock.set_nonblocking(true)?;
        let sock = UnixStream::from_std(host_sock)?;
        let reader = |fd: Option<OwnedFd>| fd.map(PipeReader::new).transpose();
        let io = WorkerIo::new(
            sock,
            reader(stdout)?,
            reader(stderr)?,
            Some(PipeReader::new(status_rd)?),
        );
        let baseline_threads = process_thread_count(proc_.pid);
        Ok(NativeWorker { io, proc_, ready, baseline_threads, scratch: cleanup.0.take() })
    }

    /// Blocks until the `Ready` frame arrives, or fails.
    fn handshake(&self, sock: &StdUnixStream) -> anyhow::Result<ReadyInfo> {
        let timeout = self.cfg.spawn_timeout.max(Duration::from_millis(1));
        let mut sock = sock;
        sock.set_read_timeout(Some(timeout))?;
        match proto::read_frame(&mut sock, self.cfg.max_frame_bytes) {
            Ok(Some(Frame::Ready {
                proto,
                pid,
                resolved_exports,
                init_us,
                rss_bytes,
            })) => {
                anyhow::ensure!(
                    proto == proto::PROTO_VERSION,
                    "worker protocol version {proto} does not match {} (stale binary on disk?)",
                    proto::PROTO_VERSION
                );
                sock.set_read_timeout(None)?;
                Ok(ReadyInfo {
                    pid,
                    resolved_exports,
                    init_us,
                    rss_bytes,
                })
            },
            Ok(Some(other)) => {
                anyhow::bail!("expected Ready from the worker, got frame {:#04x}", other.kind())
            },
            Ok(None) => anyhow::bail!("the worker closed the socket before Ready"),
            Err(proto::ProtoError::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                anyhow::bail!("the worker did not become ready within {} ms", timeout.as_millis())
            },
            Err(e) => anyhow::bail!("bad Ready frame: {e}"),
        }
    }
}

impl Spawner for NativeSpawner {
    type Worker = NativeWorker;

    fn spawn(&self, artifact: &Arc<Artifact>, plan: &SandboxPlan) -> anyhow::Result<NativeWorker> {
        self.spawn_inner(artifact, plan)
    }
}

/// What the status pipe, exit status and stderr say about a worker that never
/// became ready (the same classifier as a dead worker during a call, §4.4).
fn startup_diagnosis(
    proc_: &Proc,
    status_rd: &OwnedFd,
    stderr: Option<&OwnedFd>,
    args: &WorkerArgs,
) -> String {
    let notes = parse_status(&read_available(status_rd, 4096));
    let tail = stderr
        .map(|fd| String::from_utf8_lossy(&read_available(fd, 2048)).trim().to_string())
        .unwrap_or_default();
    let exit = proc_.reaped();
    let failure = classify(&Observation {
        notes: &notes,
        exit: exit.as_ref(),
        stderr_tail: &tail,
        memory_bytes: args.memory_bytes,
        stack_bytes: args.stack_bytes as u64,
    });
    let what = match failure {
        NativeFailure::StartFailed(m) | NativeFailure::Crashed(m) | NativeFailure::Protocol(m) => m,
        other => format!("{other:?}"),
    };
    if tail.is_empty() {
        what
    } else {
        format!("{what}; worker stderr: {tail}")
    }
}

// ---------------------------------------------------------------------------
// fd helpers
// ---------------------------------------------------------------------------

fn clear_cloexec(fd: libc::c_int) -> std::io::Result<()> {
    // SAFETY: fcntl on an fd we own.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Writes `pid` to `<cgroup>/cgroup.procs`, then sends the go byte the worker
/// waits for in `sandbox::enter`.
fn place_in_cgroup(cgroup: &Path, pid: u32, sock: &StdUnixStream) -> anyhow::Result<()> {
    use std::io::Write as _;
    std::fs::write(cgroup.join("cgroup.procs"), pid.to_string())
        .with_context(|| format!("write {}/cgroup.procs", cgroup.display()))?;
    let mut s = sock;
    s.write_all(b"G").context("send go byte")?;
    Ok(())
}

fn set_cloexec(fd: libc::c_int) -> std::io::Result<()> {
    // SAFETY: fcntl on an fd we own.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

const SOCK_BUF_BYTES: libc::c_int = 1 << 20;

fn set_sock_buf(fd: libc::c_int, opt: libc::c_int, bytes: libc::c_int) {
    // SAFETY: setsockopt on an fd we own with a c_int option value.
    unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            opt,
            (&bytes as *const libc::c_int).cast(),
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );
    }
}

/// `(host end, child end)`, both close-on-exec.
fn socketpair() -> anyhow::Result<(StdUnixStream, OwnedFd)> {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `fds` has room for two ints.
    let r = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) };
    anyhow::ensure!(r == 0, "socketpair: {}", std::io::Error::last_os_error());
    // SAFETY: fresh fds owned by us from here on.
    let (host, child) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    set_cloexec(host.as_raw_fd())?;
    set_cloexec(child.as_raw_fd())?;
    // macOS defaults to 8 KiB socket buffers, so a multi-MB frame (large args
    // or result) moves in hundreds of wake-ups. Best effort: failure keeps
    // the defaults.
    for fd in [host.as_raw_fd(), child.as_raw_fd()] {
        set_sock_buf(fd, libc::SO_SNDBUF, SOCK_BUF_BYTES);
        set_sock_buf(fd, libc::SO_RCVBUF, SOCK_BUF_BYTES);
    }
    Ok((StdUnixStream::from(host), child))
}

/// `(read end, write end)`, both close-on-exec.
fn pipe() -> anyhow::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `fds` has room for two ints.
    let r = unsafe { libc::pipe(fds.as_mut_ptr()) };
    anyhow::ensure!(r == 0, "pipe: {}", std::io::Error::last_os_error());
    // SAFETY: fresh fds owned by us from here on.
    let (rd, wr) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    set_cloexec(rd.as_raw_fd())?;
    set_cloexec(wr.as_raw_fd())?;
    Ok((rd, wr))
}

/// Reads up to `max` bytes that are available now (the fd is switched to
/// non-blocking). Returns what it got.
fn read_available(fd: &OwnedFd, max: usize) -> Vec<u8> {
    let raw = fd.as_raw_fd();
    // SAFETY: fcntl on an fd we own.
    unsafe {
        let flags = libc::fcntl(raw, libc::F_GETFL);
        if flags >= 0 {
            libc::fcntl(raw, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
    }
    let mut out = Vec::new();
    let mut buf = [0u8; 1024];
    while out.len() < max {
        // SAFETY: `buf` is a valid writable buffer.
        let n = unsafe { libc::read(raw, buf.as_mut_ptr().cast(), buf.len()) };
        if n <= 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::atomic::{
            AtomicU64,
            Ordering,
        },
    };

    use serde_json::{
        json,
        Value as JsonValue,
    };

    use super::{
        super::{
            artifact::{
                sha256_hex,
                LIB_EXTENSION,
            },
            pool::{
                Pool,
                PoolConfig,
            },
            pump::{
                CallPump,
                CallSpec,
            },
            WORKER_ARG,
        },
        *,
    };
    use crate::environment::rust::core::{
        HostBackend,
        PollOutcome,
    };

    const SHIM_ENV: &str = "RUST_NATIVE_TEST_WORKER_ARGS";
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Not a test: when this binary is started as a worker (the shim
    /// launcher sets `SHIM_ENV`), it becomes one and never returns. Under a
    /// normal `cargo test` run it passes immediately. This is how the host
    /// side is tested without the backend binary (libtest owns argv, so the
    /// worker arguments travel in the environment).
    #[test]
    fn worker_shim() {
        let Ok(args) = std::env::var(SHIM_ENV) else {
            return;
        };
        let argv: Vec<String> = args.split(SHIM_ARG_SEP).map(str::to_string).collect();
        assert_eq!(argv[0], WORKER_ARG);
        super::super::worker::worker_main_with(argv)
    }

    fn shim_launcher() -> Launcher {
        Launcher::Shim {
            exe: std::env::current_exe().unwrap(),
            prefix_args: [
                "--exact",
                "environment::rust::native::handle::tests::worker_shim",
                "--nocapture",
                "--test-threads=1",
            ]
            .map(String::from)
            .to_vec(),
            args_env: SHIM_ENV.into(),
        }
    }

    fn spawner_cfg() -> SpawnerConfig {
        SpawnerConfig {
            sandbox: SandboxLevel::Off,
            spawn_timeout: Duration::from_secs(20),
            launcher: shim_launcher(),
            ..Default::default()
        }
    }

    // ---- test guest -------------------------------------------------------

    const GUEST_SRC: &str = r#"
use std::sync::atomic::{AtomicUsize, Ordering};
static VT: AtomicUsize = AtomicUsize::new(0);
static COUNT: AtomicUsize = AtomicUsize::new(0);
#[repr(C)] pub struct ConvexBuf { ptr: *mut u8, len: usize, cap: usize }
#[repr(C)] pub struct Vt { size: usize, version: u32,
  submit: extern "C" fn(*const u8, usize) -> i64,
  poll: extern "C" fn(i32, *mut *mut u8, *mut usize) -> i32,
  sync: extern "C" fn(*const u8, usize, *const u8, usize, *mut *mut u8, *mut usize) -> i32,
  log: extern "C" fn(i32, *const u8, usize),
  oom: extern "C" fn(usize, usize) }
#[no_mangle] pub extern "C" fn convex_native_init(vt: *const Vt) -> u32 {
  VT.store(vt as usize, Ordering::SeqCst); unsafe { (*vt).version } }
#[no_mangle] pub extern "C" fn convex_alloc(len: usize) -> *mut u8 {
  let mut v = Vec::<u8>::with_capacity(len.max(1)); let p = v.as_mut_ptr(); std::mem::forget(v); p }
#[no_mangle] pub extern "C" fn convex_free_result(h: i64) { unsafe {
  let b = Box::from_raw(h as usize as *mut ConvexBuf); drop(Vec::from_raw_parts(b.ptr, b.len, b.cap)); } }
fn wrap(v: Vec<u8>) -> i64 { let mut v = v; let b = ConvexBuf { ptr: v.as_mut_ptr(), len: v.len(), cap: v.capacity() };
  std::mem::forget(v); Box::into_raw(Box::new(b)) as usize as i64 }
fn args(p: *mut u8, len: usize) -> Vec<u8> { unsafe { Vec::from_raw_parts(p, len, len.max(1)) } }
fn vt() -> *const Vt { VT.load(Ordering::SeqCst) as *const Vt }

#[export_name = "convex_run:m:echo"]
pub extern "C" fn echo(p: *mut u8, len: usize) -> i64 {
  let a = args(p, len);
  let n = COUNT.fetch_add(1, Ordering::SeqCst) + 1;
  let msg = b"echo called";
  unsafe { ((*vt()).log)(1, msg.as_ptr(), msg.len()); }
  println!("stdout line from guest");
  let mut out = format!("{{\"ok\":{{\"n\":{},\"pid\":{},\"args\":", n, std::process::id()).into_bytes();
  out.extend_from_slice(&a); out.extend_from_slice(b"}}"); wrap(out) }

#[export_name = "convex_run:m:env"]
pub extern "C" fn env(p: *mut u8, len: usize) -> i64 {
  drop(args(p, len));
  let v = std::env::var("GREETING").unwrap_or_default();
  let n = std::env::var("PATH").is_ok() as usize + std::env::var("HOME").is_ok() as usize;
  wrap(format!("{{\"ok\":{{\"greeting\":\"{}\",\"vars\":{}}}}}", v, n).into_bytes()) }

#[export_name = "convex_run:m:panics"]
pub extern "C" fn panics(p: *mut u8, len: usize) -> i64 {
  drop(args(p, len));
  let r = std::panic::catch_unwind(|| { if COUNT.load(Ordering::SeqCst) < 1000 { panic!("boom"); } });
  match r { Err(_) => wrap(b"{\"error\":{\"kind\":\"panic\",\"message\":\"boom\"}}".to_vec()),
            Ok(_) => wrap(b"{\"ok\":null}".to_vec()) } }

#[export_name = "convex_run:m:abort"]
pub extern "C" fn abort(p: *mut u8, len: usize) -> i64 {
  drop(args(p, len)); eprintln!("about to abort"); std::process::abort() }

#[export_name = "convex_run:m:exit3"]
pub extern "C" fn exit3(p: *mut u8, len: usize) -> i64 {
  drop(args(p, len)); std::process::exit(3) }

#[export_name = "convex_run:m:sleep"]
pub extern "C" fn sleep(p: *mut u8, len: usize) -> i64 {
  drop(args(p, len)); std::thread::sleep(std::time::Duration::from_secs(60)); wrap(b"{\"ok\":null}".to_vec()) }
"#;

    fn build_guest() -> PathBuf {
        static GUEST: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        GUEST
            .get_or_init(|| {
                let dir = std::env::temp_dir().join(format!("rn-handle-test-{}", std::process::id()));
                std::fs::create_dir_all(&dir).unwrap();
                let src = dir.join("guest.rs");
                std::fs::write(&src, GUEST_SRC).unwrap();
                let out = dir.join(format!("libguest.{LIB_EXTENSION}"));
                let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
                let st = Command::new(rustc)
                    .args(["--edition", "2021", "--crate-type", "cdylib", "--crate-name", "guest", "-o"])
                    .arg(&out)
                    .arg(&src)
                    .status()
                    .expect("rustc is required to build the test cdylib");
                assert!(st.success(), "building the test cdylib failed");
                out
            })
            .clone()
    }

    #[test]
    fn scratch_dirs_are_private_and_stale_ones_swept() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("rn-scratch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let a = create_scratch(&base, None).unwrap();
        let b = create_scratch(&base, None).unwrap();
        assert_ne!(a, b);
        assert_eq!(std::fs::metadata(&a).unwrap().permissions().mode() & 0o777, 0o700);
        // A dir of a dead backend pid is swept, ours is kept.
        let dead = base.join("w2147483646-0");
        std::fs::create_dir_all(&dead).unwrap();
        sweep_stale_scratch(&base);
        assert!(!dead.exists() && a.exists() && b.exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    fn artifact() -> Arc<Artifact> {
        let path = build_guest();
        let data = std::fs::read(&path).unwrap();
        Arc::new(Artifact {
            sha256: sha256_hex(&data),
            path,
            size: data.len() as u64,
            functions: BTreeMap::new(),
        })
    }

    fn plan() -> SandboxPlan {
        SandboxPlan {
            memory_bytes: 0,
            stack_bytes: 8 << 20,
            tmp_dir: Some(std::env::temp_dir().join(format!("rn-handle-tmp-{}", std::process::id()))),
            env: vec![("GREETING".into(), "hi".into())],
            ..Default::default()
        }
    }

    // ---- minimal host backend --------------------------------------------

    #[derive(Default)]
    struct TestBackend {
        logs: Arc<Mutex<Vec<(i32, String)>>>,
    }

    #[async_trait]
    impl HostBackend for TestBackend {
        fn submit(&mut self, _req: JsonValue) -> Result<i64, String> {
            Err("no tasks in this test".into())
        }
        async fn poll(&mut self, _block: bool) -> PollOutcome {
            PollOutcome::NoTasks
        }
        fn sync_syscall(&mut self, _op: &str, _args: JsonValue) -> JsonValue {
            json!({"ok": null})
        }
        fn log(&mut self, level: i32, message: String) {
            self.logs.lock().push((level, message));
        }
        fn outstanding_tasks(&self) -> usize {
            0
        }
    }

    type Logs = Arc<Mutex<Vec<(i32, String)>>>;

    fn backend() -> (TestBackend, Logs) {
        let b = TestBackend::default();
        let l = b.logs.clone();
        (b, l)
    }

    fn spawner() -> NativeSpawner {
        NativeSpawner::new(spawner_cfg(), Handle::current())
    }

    async fn spawn_worker() -> NativeWorker {
        let s = Arc::new(spawner());
        let a = artifact();
        tokio::task::spawn_blocking(move || s.spawn(&a, &plan()).expect("spawn"))
            .await
            .unwrap()
    }

    async fn call(
        w: &mut NativeWorker,
        export: &str,
        args: &str,
        timeout: Duration,
    ) -> (super::super::pump::PumpReport, Logs) {
        let (b, logs) = backend();
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let spec = CallSpec::new(id, export, args.as_bytes().to_vec(), timeout);
        (CallPump::new(spec, b).run(w).await, logs)
    }

    fn envelope(r: &super::super::pump::PumpReport) -> JsonValue {
        let out = r.outcome.as_ref().unwrap_or_else(|e| panic!("call failed: {e:?}"));
        serde_json::from_slice(&out.envelope_json).unwrap()
    }

    // ---- tests ------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_ready_call_result() {
        let mut w = spawn_worker().await;
        assert!(w.ready().pid > 0);
        assert_eq!(w.pid(), Some(w.ready().pid));
        assert!(w.is_alive());
        assert!(w.rss_bytes().unwrap() > 1 << 20);

        let (r, logs) = call(&mut w, "m:echo", r#"[1,2]"#, Duration::from_secs(20)).await;
        let env = envelope(&r);
        assert_eq!(env["ok"]["args"], json!([1, 2]));
        assert_eq!(env["ok"]["pid"], w.pid().unwrap());
        assert_eq!(env["ok"]["n"], 1);
        {
            let logs = logs.lock();
            assert!(logs.contains(&(1, "echo called".to_string())), "{logs:?}");
            assert!(
                logs.iter().any(|(l, m)| *l == 1 && m.ends_with("stdout line from guest")),
                "{logs:?}"
            );
        }
        // Second call on the same worker: statics persist (D3).
        let (r, _) = call(&mut w, "m:echo", "[]", Duration::from_secs(20)).await;
        assert_eq!(envelope(&r)["ok"]["n"], 2);
        assert!(w.is_alive());

        // The environment is exactly the plan's.
        let (r, _) = call(&mut w, "m:env", "[]", Duration::from_secs(20)).await;
        let env = envelope(&r);
        assert_eq!(env["ok"]["greeting"], "hi");
        // Nothing of the host environment (PATH, HOME) leaks in.
        assert_eq!(env["ok"]["vars"], 0);

        // Cooperative shutdown.
        let pid = w.pid().unwrap();
        w.request_shutdown();
        let exit = w.wait_exit(Duration::from_secs(10)).await.expect("worker exits");
        assert_eq!(exit.code, Some(0), "pid {pid}");
        assert!(!w.is_alive());
        assert_eq!(w.rss_bytes(), None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn guest_panic_envelope_keeps_the_worker_but_pool_recycles_it() {
        let mut w = spawn_worker().await;
        let (r, _) = call(&mut w, "m:panics", "[]", Duration::from_secs(20)).await;
        assert_eq!(envelope(&r)["error"]["kind"], "panic");
        // The SDK catches the panic: the process survives and still answers.
        assert!(w.is_alive());
        let (r, _) = call(&mut w, "m:echo", "[]", Duration::from_secs(20)).await;
        assert_eq!(envelope(&r)["ok"]["n"], 1);

        // Through the pool, the runtime releases a worker that panicked with
        // `release_recycle` (NATIVE.md §2.3: always recycled after a panic).
        let pool = Pool::new(
            spawner(),
            PoolConfig {
                min_idle: 0,
                ..PoolConfig::default()
            },
        );
        let a = artifact();
        let mut lease = pool.acquire(&a, 1, &plan()).await.unwrap();
        let first = lease.worker_mut().pid().unwrap();
        let (r, _) = call(lease.worker_mut(), "m:panics", "[]", Duration::from_secs(20)).await;
        assert_eq!(envelope(&r)["error"]["kind"], "panic");
        lease.release_recycle();
        let mut lease = pool.acquire(&a, 1, &plan()).await.unwrap();
        assert_ne!(lease.worker_mut().pid().unwrap(), first);
        lease.release_clean();
        pool.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn guest_abort_is_classified_as_a_crash() {
        let mut w = spawn_worker().await;
        let (r, logs) = call(&mut w, "m:abort", "[]", Duration::from_secs(20)).await;
        match r.outcome {
            Err(NativeFailure::Crashed(m)) => {
                assert_eq!(m, "Rust action crashed the worker process (signal SIGABRT)");
            },
            other => panic!("expected Crashed, got {other:?}"),
        }
        assert!(logs.lock().iter().any(|(l, m)| *l == 3 && m.contains("about to abort")));
        assert!(!w.is_alive());
        let exit = w.exit_info().unwrap();
        assert_eq!(exit.signal, Some(libc::SIGABRT));
        assert!(!exit.oom_killed);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn guest_exit_without_result_is_classified() {
        let mut w = spawn_worker().await;
        let (r, _) = call(&mut w, "m:exit3", "[]", Duration::from_secs(20)).await;
        match r.outcome {
            Err(NativeFailure::Crashed(m)) => {
                assert_eq!(m, "Rust action exited the worker process (exit status 3)");
            },
            other => panic!("expected Crashed, got {other:?}"),
        }
        assert_eq!(w.exit_info().unwrap().code, Some(3));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn timeout_kills_the_process_group() {
        let mut w = spawn_worker().await;
        let t = Instant::now();
        let (r, _) = call(&mut w, "m:sleep", "[]", Duration::from_millis(300)).await;
        assert!(matches!(r.outcome, Err(NativeFailure::Timeout)), "{:?}", r.outcome);
        assert!(t.elapsed() < Duration::from_secs(10));
        let exit = w.wait_exit(Duration::from_secs(5)).await.expect("reaped");
        assert_eq!(exit.signal, Some(libc::SIGKILL));
        assert!(!w.is_alive());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn explicit_kill_and_terminate_are_idempotent() {
        let mut w = spawn_worker().await;
        let pid = w.pid().unwrap();
        {
            let (_, proc_) = w.split();
            proc_.kill();
            proc_.kill();
            let exit = proc_.wait(Duration::from_secs(5)).await.expect("exits");
            assert_eq!(exit.signal, Some(libc::SIGKILL));
        }
        w.terminate();
        w.terminate();
        assert!(!w.is_alive());
        // SAFETY: signal 0 only checks existence.
        let gone = unsafe { libc::kill(pid as libc::pid_t, 0) } != 0;
        assert!(gone, "pid {pid} was reaped");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dropping_the_handle_kills_the_worker() {
        let w = spawn_worker().await;
        let pid = w.pid().unwrap();
        drop(w);
        // SAFETY: signal 0 only checks existence.
        assert!(unsafe { libc::kill(pid as libc::pid_t, 0) } != 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_failure_reports_the_dlopen_error() {
        let s = Arc::new(spawner());
        let bad = Arc::new(Artifact {
            sha256: "0".into(),
            path: PathBuf::from("/nonexistent/lib.so"),
            size: 0,
            functions: BTreeMap::new(),
        });
        let err = tokio::task::spawn_blocking(move || s.spawn(&bad, &plan()).err())
            .await
            .unwrap()
            .expect("spawn must fail");
        let msg = format!("{err:#}");
        assert!(msg.contains("dlopen") || msg.contains("nonexistent"), "{msg}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn missing_binary_and_ready_timeout_are_spawn_errors() {
        let mut cfg = spawner_cfg();
        cfg.launcher = Launcher::Shim {
            exe: PathBuf::from("/nonexistent/worker"),
            prefix_args: vec![],
            args_env: SHIM_ENV.into(),
        };
        let s = Arc::new(NativeSpawner::new(cfg, Handle::current()));
        let a = artifact();
        let s2 = s.clone();
        let a2 = a.clone();
        let r = tokio::task::spawn_blocking(move || s2.spawn(&a2, &plan()).err()).await.unwrap();
        assert!(format!("{:#}", r.expect("fails")).contains("cannot start"));

        // A process that never says Ready: `sleep` ignores the args.
        let mut cfg = spawner_cfg();
        cfg.spawn_timeout = Duration::from_millis(300);
        cfg.launcher = Launcher::Shim {
            exe: PathBuf::from("/bin/sleep"),
            prefix_args: vec!["30".into()],
            args_env: SHIM_ENV.into(),
        };
        let s = Arc::new(NativeSpawner::new(cfg, Handle::current()));
        let t = Instant::now();
        let r = tokio::task::spawn_blocking(move || s.spawn(&a, &plan()).err()).await.unwrap();
        assert!(format!("{:#}", r.expect("fails")).contains("did not become ready"));
        assert!(t.elapsed() < Duration::from_secs(10));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn pool_checkout_call_release_with_two_workers() {
        let pool = Pool::new(
            spawner(),
            PoolConfig {
                min_idle: 0,
                max_idle: 4,
                ..PoolConfig::default()
            },
        );
        let a = artifact();
        let mut l1 = pool.acquire(&a, 7, &plan()).await.unwrap();
        let mut l2 = pool.acquire(&a, 7, &plan()).await.unwrap();
        let (p1, p2) = (l1.worker_mut().pid().unwrap(), l2.worker_mut().pid().unwrap());
        assert_ne!(p1, p2);
        let (r1, r2) = {
            let (w1, w2) = (l1.worker_mut(), l2.worker_mut());
            tokio::join!(
                call(w1, "m:echo", "[1]", Duration::from_secs(20)),
                call(w2, "m:echo", "[2]", Duration::from_secs(20)),
            )
        };
        assert_eq!(envelope(&r1.0)["ok"]["args"], json!([1]));
        assert_eq!(envelope(&r2.0)["ok"]["args"], json!([2]));
        l1.release_clean();
        l2.release_clean();

        // Both come back as idle workers and are reused (one call each, with
        // the per-worker call counter advancing).
        let stats = pool.stats();
        assert_eq!(stats.iter().map(|s| s.idle).sum::<usize>(), 2, "{stats:?}");
        let mut seen = Vec::new();
        let mut leases = Vec::new();
        for _ in 0..2 {
            let mut l = pool.acquire(&a, 7, &plan()).await.unwrap();
            seen.push(l.worker_mut().pid().unwrap());
            let (r, _) = call(l.worker_mut(), "m:echo", "[]", Duration::from_secs(20)).await;
            assert_eq!(envelope(&r)["ok"]["n"], 2);
            leases.push(l);
        }
        seen.sort();
        let mut want = vec![p1, p2];
        want.sort();
        assert_eq!(seen, want);

        // A crash through the pool: the lease is released failed and the pool
        // spawns a replacement.
        let mut l = leases.pop().unwrap();
        let crashed = l.worker_mut().pid().unwrap();
        let (r, _) = call(l.worker_mut(), "m:abort", "[]", Duration::from_secs(20)).await;
        let failure = r.outcome.unwrap_err();
        assert!(matches!(failure, NativeFailure::Crashed(_)));
        l.release_failed(&failure);
        leases.pop().unwrap().release_clean();
        let mut l = pool.acquire(&a, 7, &plan()).await.unwrap();
        assert_ne!(l.worker_mut().pid().unwrap(), crashed);
        l.release_clean();
        let stats = pool.stats();
        assert_eq!(stats.iter().map(|s| s.crashes).sum::<u64>(), 1, "{stats:?}");
        pool.shutdown().await;
    }

    #[test]
    fn exit_info_maps_statuses() {
        let sig = ExitStatus::from_raw(libc::SIGSEGV);
        assert_eq!(
            exit_info(sig, false),
            ExitInfo { code: None, signal: Some(libc::SIGSEGV), oom_killed: false }
        );
        let code = ExitStatus::from_raw(3 << 8);
        assert_eq!(exit_info(code, false).code, Some(3));
        assert!(exit_info(code, true).oom_killed);
        assert!(exit_info(ExitStatus::from_raw(EXIT_OOM << 8), false).oom_killed);
    }

    #[test]
    fn cgroup_events_are_parsed() {
        let d = std::env::temp_dir().join(format!("rn-cg-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("memory.events"), "low 0\nhigh 0\nmax 2\noom 1\noom_kill 3\n").unwrap();
        assert_eq!(cgroup_oom_kills(&d), Some(3));
        assert_eq!(cgroup_oom_kills(Path::new("/nonexistent")), None);
        let _ = std::fs::remove_dir_all(d);
    }
}
