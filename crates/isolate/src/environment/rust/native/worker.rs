//! The rust-native worker process (WN-04). Contract: `contracts/NATIVE.md`
//! §2 (frames, status pipe, vtable) and `.plan/DESIGN.md` §3.3.
//!
//! The worker is the backend binary re-executed with `argv[1] ==
//! __rust_native_worker`. fd 3 is the frame socket, fd 4 the status pipe,
//! fd 1/2 are the user's stdout/stderr. The main thread is the watchdog
//! (RSS, parent liveness, log flushing); a dedicated call thread with a
//! `--stack-bytes` stack dlopens nothing itself but runs the call loop.
//!
//! The host vtable (`HostVtableV1`) is `extern "C"` without a context
//! pointer, so the live worker state is process-global (`SHARED`). The loop
//! itself is generic over [`GuestModule`], which lets tests drive it with a
//! fake guest and an in-process socketpair.

use std::{
    collections::{
        HashMap,
        VecDeque,
    },
    os::unix::io::RawFd,
    path::{
        Path,
        PathBuf,
    },
    sync::{
        atomic::{
            AtomicBool,
            AtomicI32,
            AtomicU32,
            AtomicU64,
            AtomicUsize,
            Ordering,
        },
        Arc,
        OnceLock,
    },
    time::{
        Duration,
        Instant,
    },
};

use anyhow::Context as _;
use parking_lot::{
    Mutex,
    RwLock,
};
use serde::Deserialize;

use super::{
    super::core::chunk_event_to_json,
    proto::{
        Frame,
        FrameDecoder,
        ProtoError,
        PROTO_VERSION,
    },
    sandbox::{
        self,
        SandboxLevel,
    },
    GuestModule,
    NATIVE_ABI_VERSION,
    WORKER_ARG,
};

pub const SOCK_FD: RawFd = 3;
pub const STATUS_FD: RawFd = 4;

/// Status pipe record codes (NATIVE.md §2.2).
pub const STATUS_OOM: u8 = 1;
pub const STATUS_SIGSYS: u8 = 2;
pub const STATUS_STACK_OVERFLOW: u8 = 3;
pub const STATUS_INIT_FAILED: u8 = 4;
pub const STATUS_RECORD_LEN: usize = 64;

/// Worker exit codes.
pub const EXIT_SIGSYS: i32 = 159;
pub const EXIT_OOM: i32 = 137;
pub const EXIT_FAILED: i32 = 70;
pub const EXIT_USAGE: i32 = 64;

const MAX_OP_NAME_BYTES: usize = 4096;
/// Flush the write buffer when it grows past this.
const OUT_FLUSH_BYTES: usize = 64 * 1024;
const WATCHDOG_TICK: Duration = Duration::from_millis(5);
/// Watchdog period while no call is running (nothing to time, nothing to
/// flush promptly); a call that starts is noticed within one idle tick, and
/// `RLIMIT_DATA` bounds memory in the meantime.
const WATCHDOG_IDLE_TICK: Duration = Duration::from_millis(100);

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

/// Command line of a worker: `__rust_native_worker --artifact P ...`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerArgs {
    pub artifact: PathBuf,
    pub stack_bytes: usize,
    pub memory_bytes: u64,
    pub sandbox: SandboxLevel,
    pub tmp: Option<PathBuf>,
    /// Number of `convex_run:*` exports the host expects (reported in
    /// `Ready`; symbols are resolved lazily by name).
    pub functions: u32,
    pub allow_network: bool,
    /// `RUST_NATIVE_RSS_SLACK_BYTES`.
    pub rss_slack_bytes: u64,
    pub max_frame_bytes: usize,
    /// Inherited fd numbers of the frame socket / status pipe (the sandbox
    /// moves them to 3 and 4; no `pre_exec` dup2 in the parent any more).
    pub sock_fd: RawFd,
    pub status_fd: RawFd,
    /// Privilege drop applied by the worker (Linux).
    pub uid_gid: Option<(u32, u32)>,
    /// The parent places this process in a cgroup and then sends one byte on
    /// the frame socket; the worker waits for it before loading anything.
    pub wait_go: bool,
    /// `RUST_NATIVE_EXTRA_LIBS` read/exec allowlist for Landlock.
    pub extra_read: Vec<PathBuf>,
}

impl WorkerArgs {
    pub const DEFAULT_RSS_SLACK: u64 = 64 << 20;
    pub const DEFAULT_MAX_FRAME: usize = 64 * 1024 * 1024 + 64 * 1024;

    /// Parses the arguments after the binary name. A leading `WORKER_ARG` is
    /// accepted and skipped.
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> anyhow::Result<Self> {
        let mut it = args.into_iter().peekable();
        if it.peek().map(String::as_str) == Some(WORKER_ARG) {
            it.next();
        }
        let mut artifact = None;
        let mut out = WorkerArgs {
            artifact: PathBuf::new(),
            stack_bytes: 8 << 20,
            memory_bytes: 512 << 20,
            sandbox: SandboxLevel::default_for_platform(),
            tmp: None,
            functions: 0,
            allow_network: false,
            rss_slack_bytes: Self::DEFAULT_RSS_SLACK,
            max_frame_bytes: Self::DEFAULT_MAX_FRAME,
            sock_fd: SOCK_FD,
            status_fd: STATUS_FD,
            uid_gid: None,
            wait_go: false,
            extra_read: Vec::new(),
        };
        while let Some(flag) = it.next() {
            let mut val = |name: &str| {
                it.next()
                    .with_context(|| format!("{name} needs a value"))
            };
            match flag.as_str() {
                "--artifact" => artifact = Some(PathBuf::from(val("--artifact")?)),
                "--stack-bytes" => out.stack_bytes = val("--stack-bytes")?.parse()?,
                "--memory-bytes" => out.memory_bytes = val("--memory-bytes")?.parse()?,
                "--sandbox" => out.sandbox = val("--sandbox")?.parse()?,
                "--tmp" => out.tmp = Some(PathBuf::from(val("--tmp")?)),
                "--functions" => out.functions = val("--functions")?.parse()?,
                "--rss-slack-bytes" => out.rss_slack_bytes = val("--rss-slack-bytes")?.parse()?,
                "--max-frame-bytes" => out.max_frame_bytes = val("--max-frame-bytes")?.parse()?,
                "--allow-network" => out.allow_network = true,
                "--sock-fd" => out.sock_fd = val("--sock-fd")?.parse()?,
                "--status-fd" => out.status_fd = val("--status-fd")?.parse()?,
                "--uid-gid" => {
                    let v = val("--uid-gid")?;
                    let (u, g) = v.split_once(':').context("--uid-gid needs uid:gid")?;
                    out.uid_gid = Some((u.parse()?, g.parse()?));
                },
                "--wait-go" => out.wait_go = true,
                "--extra-read" => out.extra_read.push(PathBuf::from(val("--extra-read")?)),
                other => anyhow::bail!("unknown worker argument {other:?}"),
            }
        }
        out.artifact = artifact.context("--artifact is required")?;
        anyhow::ensure!(out.stack_bytes >= 64 * 1024, "--stack-bytes is too small");
        Ok(out)
    }

    /// Inverse of [`WorkerArgs::parse`], including the leading `WORKER_ARG`.
    pub fn to_args(&self) -> Vec<String> {
        let mut v = vec![
            WORKER_ARG.to_string(),
            "--artifact".into(),
            self.artifact.display().to_string(),
            "--stack-bytes".into(),
            self.stack_bytes.to_string(),
            "--memory-bytes".into(),
            self.memory_bytes.to_string(),
            "--sandbox".into(),
            self.sandbox.as_str().into(),
            "--functions".into(),
            self.functions.to_string(),
            "--rss-slack-bytes".into(),
            self.rss_slack_bytes.to_string(),
            "--max-frame-bytes".into(),
            self.max_frame_bytes.to_string(),
        ];
        if let Some(t) = &self.tmp {
            v.push("--tmp".into());
            v.push(t.display().to_string());
        }
        if self.allow_network {
            v.push("--allow-network".into());
        }
        v.push("--sock-fd".into());
        v.push(self.sock_fd.to_string());
        v.push("--status-fd".into());
        v.push(self.status_fd.to_string());
        if let Some((u, g)) = self.uid_gid {
            v.push("--uid-gid".into());
            v.push(format!("{u}:{g}"));
        }
        if self.wait_go {
            v.push("--wait-go".into());
        }
        for p in &self.extra_read {
            v.push("--extra-read".into());
            v.push(p.display().to_string());
        }
        v
    }
}

// ---------------------------------------------------------------------------
// Status pipe and process exit helpers (async-signal-safe)
// ---------------------------------------------------------------------------

/// fd of the status pipe, or -1 (tests without one).
static STATUS_PIPE: AtomicI32 = AtomicI32::new(-1);

/// Builds a 64-byte status record: `code | detail (NUL padded)`.
pub fn status_record(code: u8, detail: &[u8]) -> [u8; STATUS_RECORD_LEN] {
    let mut rec = [0u8; STATUS_RECORD_LEN];
    rec[0] = code;
    let n = detail.len().min(STATUS_RECORD_LEN - 1);
    rec[1..1 + n].copy_from_slice(&detail[..n]);
    rec
}

/// Writes one record with raw `write(2)`; no allocation, no locks.
pub fn write_status(code: u8, detail: &[u8]) {
    let fd = STATUS_PIPE.load(Ordering::Relaxed);
    if fd < 0 {
        return;
    }
    let rec = status_record(code, detail);
    // SAFETY: plain write of a stack buffer. A short or failed write of a
    // 64-byte pipe record cannot be recovered from here.
    unsafe {
        libc::write(fd, rec.as_ptr().cast(), rec.len());
    }
}

fn exit_now(code: i32) -> ! {
    // SAFETY: `_exit` is async-signal-safe and never returns.
    unsafe { libc::_exit(code) }
}

// ---------------------------------------------------------------------------
// Socket
// ---------------------------------------------------------------------------

/// A raw stream socket fd. The owner (worker_main or a test) controls its
/// lifetime.
#[derive(Clone, Copy, Debug)]
pub struct Conn {
    fd: RawFd,
}

pub enum Recv {
    Data(usize),
    WouldBlock,
    Eof,
}

impl Conn {
    pub fn new(fd: RawFd) -> Self {
        Self { fd }
    }

    /// Waits until the socket is readable (or hung up) or `timeout` passes.
    fn wait_readable(&self, timeout: Duration) {
        let mut p = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
        // SAFETY: one valid pollfd.
        unsafe { libc::poll(&mut p, 1, timeout.as_millis().clamp(1, 1000) as libc::c_int) };
    }

    fn recv(&self, buf: &mut [u8], nonblock: bool) -> std::io::Result<Recv> {
        let flags = if nonblock { libc::MSG_DONTWAIT } else { 0 };
        loop {
            // SAFETY: valid buffer pointer and length.
            let n = unsafe { libc::recv(self.fd, buf.as_mut_ptr().cast(), buf.len(), flags) };
            if n > 0 {
                return Ok(Recv::Data(n as usize));
            }
            if n == 0 {
                return Ok(Recv::Eof);
            }
            let err = std::io::Error::last_os_error();
            match err.kind() {
                std::io::ErrorKind::Interrupted => continue,
                std::io::ErrorKind::WouldBlock => return Ok(Recv::WouldBlock),
                _ => {
                    // A reset peer is an EOF for our purposes.
                    if matches!(
                        err.raw_os_error(),
                        Some(libc::ECONNRESET) | Some(libc::EPIPE)
                    ) {
                        return Ok(Recv::Eof);
                    }
                    return Err(err);
                },
            }
        }
    }

    fn send_all(&self, mut data: &[u8]) -> std::io::Result<()> {
        while !data.is_empty() {
            // SAFETY: valid buffer pointer and length.
            let n = unsafe { libc::write(self.fd, data.as_ptr().cast(), data.len()) };
            if n < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            data = &data[n as usize..];
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Shared worker state
// ---------------------------------------------------------------------------

/// Limits delivered with each `Call` (`limits JSON`).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CallLimits {
    pub max_result_bytes: usize,
    pub max_task_request_bytes: usize,
    pub max_outstanding_tasks: usize,
    pub max_log_message_bytes: usize,
}

impl Default for CallLimits {
    fn default() -> Self {
        Self {
            max_result_bytes: 16 << 20,
            max_task_request_bytes: 64 << 20,
            max_outstanding_tasks: 1000,
            max_log_message_bytes: 32 << 10,
        }
    }
}

type AllocFn = unsafe extern "C" fn(usize) -> *mut u8;

struct Inp {
    dec: FrameDecoder,
    scratch: Vec<u8>,
}

/// What a blocking wait on the socket produced.
enum Got {
    Frame(Frame),
    WouldBlock,
    Eof,
}

/// Process-global worker state used by the vtable functions.
pub struct Shared {
    conn: Conn,
    out: Mutex<Vec<u8>>,
    inp: Mutex<Inp>,
    queue: Mutex<VecDeque<Vec<u8>>>,
    limits: Mutex<CallLimits>,
    submits: AtomicU64,
    outstanding: AtomicU64,
    seq: AtomicU32,
    alloc: AtomicUsize,
    dirty: AtomicBool,
    /// A call is running (selects the watchdog period).
    active: AtomicBool,
    /// The loaded guest parses binary chunk events (`convex_binary_events`).
    binary_events: AtomicBool,
}

static SHARED: RwLock<Option<Arc<Shared>>> = RwLock::new(None);

fn current() -> Arc<Shared> {
    match SHARED.read().as_ref() {
        Some(s) => s.clone(),
        None => {
            eprintln!("rust-native worker: vtable used before the worker state was installed");
            exit_now(EXIT_FAILED)
        },
    }
}

#[derive(Deserialize)]
struct EventHead<'a> {
    #[serde(rename = "type", borrow)]
    ty: Option<&'a str>,
}

enum Inbound {
    /// Consumed (an `Event` was queued).
    Handled,
    NoTasks(u32),
    SyncResult(Vec<u8>),
    Call(Frame),
}

impl Shared {
    pub fn new(conn: Conn, max_frame_bytes: usize) -> Arc<Self> {
        Arc::new(Self {
            conn,
            out: Mutex::new(Vec::new()),
            inp: Mutex::new(Inp {
                dec: FrameDecoder::new(max_frame_bytes),
                scratch: vec![0; 64 * 1024],
            }),
            queue: Mutex::new(VecDeque::new()),
            limits: Mutex::new(CallLimits::default()),
            submits: AtomicU64::new(0),
            outstanding: AtomicU64::new(0),
            seq: AtomicU32::new(0),
            alloc: AtomicUsize::new(0),
            dirty: AtomicBool::new(false),
            active: AtomicBool::new(false),
            binary_events: AtomicBool::new(false),
        })
    }

    /// Installs `self` as the state behind the vtable.
    pub fn install(self: &Arc<Self>) {
        *SHARED.write() = Some(self.clone());
    }

    pub fn set_alloc(&self, f: AllocFn) {
        self.alloc.store(f as usize, Ordering::SeqCst);
    }

    fn limits(&self) -> CallLimits {
        *self.limits.lock()
    }

    fn begin_call(&self, limits: CallLimits) {
        *self.limits.lock() = limits;
        self.active.store(true, Ordering::Release);
        self.submits.store(0, Ordering::SeqCst);
        self.outstanding.store(0, Ordering::SeqCst);
        self.queue.lock().clear();
    }

    // -- writing ----------------------------------------------------------

    /// Appends a frame to the write buffer (no syscall unless it is big).
    fn push(&self, frame: &Frame) -> Result<(), ProtoError> {
        let mut out = self.out.lock();
        frame.encode_into(&mut out)?;
        self.dirty.store(true, Ordering::Release);
        if out.len() >= OUT_FLUSH_BYTES {
            self.flush_locked(&mut out);
        }
        Ok(())
    }

    fn flush_locked(&self, out: &mut Vec<u8>) {
        if out.is_empty() {
            return;
        }
        let res = self.conn.send_all(out);
        out.clear();
        self.dirty.store(false, Ordering::Release);
        if res.is_err() {
            // The host is gone.
            exit_now(0);
        }
    }

    pub fn flush(&self) {
        let mut out = self.out.lock();
        self.flush_locked(&mut out);
    }

    /// Watchdog hook: flush only if something is pending and the lock is free.
    fn flush_if_dirty(&self) {
        if self.dirty.load(Ordering::Acquire)
            && let Some(mut out) = self.out.try_lock() {
                self.flush_locked(&mut out);
            }
    }

    fn send(&self, frame: &Frame) {
        if let Err(e) = self.push(frame) {
            eprintln!("rust-native worker: cannot encode frame: {e}");
            exit_now(EXIT_FAILED);
        }
    }

    // -- reading ----------------------------------------------------------

    fn next_frame(&self, inp: &mut Inp, block: bool) -> Result<Got, ProtoError> {
        loop {
            if let Some(f) = inp.dec.next_frame()? {
                return Ok(Got::Frame(f));
            }
            let Inp { dec, scratch } = inp;
            match self.conn.recv(scratch, !block)? {
                Recv::Data(n) => dec.push(&scratch[..n]),
                Recv::WouldBlock => return Ok(Got::WouldBlock),
                Recv::Eof => {
                    if dec.buffered() > 0 {
                        return Err(ProtoError::Truncated);
                    }
                    return Ok(Got::Eof);
                },
            }
        }
    }

    fn read_or_die(&self, inp: &mut Inp, block: bool) -> Got {
        match self.next_frame(inp, block) {
            Ok(Got::Eof) => exit_now(0),
            Ok(g) => g,
            Err(e) => {
                eprintln!("rust-native worker: protocol error: {e}");
                exit_now(EXIT_FAILED)
            },
        }
    }

    /// Routes a frame that arrived while a call is running.
    fn route(&self, frame: Frame) -> Inbound {
        match frame {
            Frame::Event { event_json } => {
                if let Ok(EventHead { ty: Some("done") }) = serde_json::from_slice(&event_json) {
                    let _ = self.outstanding.fetch_update(
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                        |v| Some(v.saturating_sub(1)),
                    );
                }
                self.queue.lock().push_back(event_json);
                Inbound::Handled
            },
            Frame::Chunk { event } => {
                // Raw for guests that know binary events, base64 JSON otherwise.
                let event = if self.binary_events.load(Ordering::Relaxed) {
                    event
                } else {
                    chunk_event_to_json(&event).into_bytes()
                };
                self.queue.lock().push_back(event);
                Inbound::Handled
            },
            Frame::NoTasks { seq } => Inbound::NoTasks(seq),
            Frame::SyncResult { result_json } => Inbound::SyncResult(result_json),
            Frame::Shutdown => exit_now(0),
            other => Inbound::Call(other),
        }
    }

    fn route_in_call(&self, frame: Frame) -> Inbound {
        match self.route(frame) {
            Inbound::Call(f) => {
                eprintln!("rust-native worker: unexpected frame {:#04x} during a call", f.kind());
                exit_now(EXIT_FAILED)
            },
            other => other,
        }
    }

    /// Non-blocking drain of the socket into the event queue.
    fn drain(&self, inp: &mut Inp) {
        loop {
            match self.read_or_die(inp, false) {
                Got::Frame(f) => {
                    // NoTasks/SyncResult cannot be pending here; ignore stale.
                    let _ = self.route_in_call(f);
                },
                Got::WouldBlock | Got::Eof => return,
            }
        }
    }

    // -- guest allocation -------------------------------------------------

    /// Copies `bytes` into guest memory allocated with `convex_alloc`.
    fn alloc_into_guest(&self, bytes: &[u8], out_ptr: *mut *mut u8, out_len: *mut usize) -> bool {
        let f = self.alloc.load(Ordering::SeqCst);
        if f == 0 {
            return false;
        }
        // SAFETY: `f` was stored from a valid `AllocFn` by `set_alloc`.
        let f: AllocFn = unsafe { std::mem::transmute::<usize, AllocFn>(f) };
        // SAFETY: calling the guest allocator; the result is checked.
        let p = unsafe { f(bytes.len()) };
        if p.is_null() {
            return false;
        }
        // SAFETY: `p` has room for `bytes.len()` bytes; out pointers come
        // from the guest and are valid for one write each.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
            *out_ptr = p;
            *out_len = bytes.len();
        }
        true
    }

    fn log_local(&self, level: i32, msg: String) {
        let max = self.limits().max_log_message_bytes;
        self.send(&Frame::Log { level, message: clip_utf8(msg.as_bytes(), max).to_vec() });
    }
}

/// Clips to `max` bytes without cutting a UTF-8 sequence.
fn clip_utf8(b: &[u8], max: usize) -> &[u8] {
    if b.len() <= max {
        return b;
    }
    let mut end = max;
    while end > 0 && (b[end] & 0xC0) == 0x80 {
        end -= 1;
    }
    &b[..end]
}

// ---------------------------------------------------------------------------
// Host vtable (NATIVE.md §2.4)
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct HostVtableV1 {
    pub size: usize,
    pub version: u32,
    pub task_submit: extern "C" fn(ptr: *const u8, len: usize) -> i64,
    pub task_poll: extern "C" fn(block: i32, out_ptr: *mut *mut u8, out_len: *mut usize) -> i32,
    pub sync_syscall: extern "C" fn(
        op: *const u8,
        op_len: usize,
        args: *const u8,
        args_len: usize,
        out_ptr: *mut *mut u8,
        out_len: *mut usize,
    ) -> i32,
    pub log: extern "C" fn(level: i32, ptr: *const u8, len: usize),
    pub oom: extern "C" fn(limit: usize, requested: usize),
}

pub static VTABLE: HostVtableV1 = HostVtableV1 {
    size: std::mem::size_of::<HostVtableV1>(),
    version: NATIVE_ABI_VERSION,
    task_submit: vt_task_submit,
    task_poll: vt_task_poll,
    sync_syscall: vt_sync_syscall,
    log: vt_log,
    oom: vt_oom,
};

unsafe fn slice<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if len == 0 || ptr.is_null() {
        &[]
    } else {
        // SAFETY: the guest promises `ptr..ptr+len` is readable.
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }
}

extern "C" fn vt_task_submit(ptr: *const u8, len: usize) -> i64 {
    let sh = current();
    let lim = sh.limits();
    if len > lim.max_task_request_bytes {
        sh.log_local(
            3,
            format!(
                "task_submit: request of {len} bytes exceeds {}",
                lim.max_task_request_bytes
            ),
        );
        return -1;
    }
    if sh.outstanding.load(Ordering::SeqCst) >= lim.max_outstanding_tasks as u64 {
        sh.log_local(
            3,
            format!(
                "task_submit: more than {} outstanding tasks",
                lim.max_outstanding_tasks
            ),
        );
        return -1;
    }
    // SAFETY: guest-provided buffer.
    let req = unsafe { slice(ptr, len) }.to_vec();
    if sh.push(&Frame::Submit { request_json: req }).is_err() {
        return -1;
    }
    sh.outstanding.fetch_add(1, Ordering::SeqCst);
    sh.submits.fetch_add(1, Ordering::SeqCst) as i64
}

/// `RUST_NATIVE_SPIN_US` (default 25, 0 disables): how long a blocking
/// `task_poll` polls the socket before it sends `Poll` and parks.
fn spin_budget() -> Duration {
    static SPIN: OnceLock<Duration> = OnceLock::new();
    *SPIN.get_or_init(|| {
        let us = std::env::var("RUST_NATIVE_SPIN_US")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(25);
        Duration::from_micros(us)
    })
}

extern "C" fn vt_task_poll(block: i32, out_ptr: *mut *mut u8, out_len: *mut usize) -> i32 {
    let sh = current();
    let mut inp = sh.inp.lock();
    sh.drain(&mut inp);
    sh.flush();
    let deliver = |ev: Vec<u8>| -> i32 {
        if sh.alloc_into_guest(&ev, out_ptr, out_len) {
            1
        } else {
            sh.log_local(3, "task_poll: guest allocation failed".into());
            -1
        }
    };
    if let Some(ev) = sh.queue.lock().pop_front() {
        return deliver(ev);
    }
    if block == 0 {
        return 0;
    }
    // Short spin before parking: an event for a task that is about to finish
    // usually lands within microseconds, and then no `Poll` round trip is
    // needed at all.
    let spin = spin_budget();
    if !spin.is_zero() && sh.outstanding.load(Ordering::SeqCst) > 0 {
        let t = Instant::now();
        while t.elapsed() < spin {
            sh.drain(&mut inp);
            if let Some(ev) = sh.queue.lock().pop_front() {
                return deliver(ev);
            }
            std::hint::spin_loop();
        }
    }
    let seq = sh.seq.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
    sh.send(&Frame::Poll { seq });
    sh.flush();
    loop {
        let Got::Frame(f) = sh.read_or_die(&mut inp, true) else {
            continue;
        };
        match sh.route_in_call(f) {
            Inbound::NoTasks(s) if s == seq => return -1,
            Inbound::NoTasks(_) | Inbound::SyncResult(_) => {},
            Inbound::Handled => {
                if let Some(ev) = sh.queue.lock().pop_front() {
                    return deliver(ev);
                }
            },
            Inbound::Call(_) => unreachable!(),
        }
    }
}

extern "C" fn vt_sync_syscall(
    op: *const u8,
    op_len: usize,
    args: *const u8,
    args_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    let sh = current();
    let lim = sh.limits();
    if op_len > MAX_OP_NAME_BYTES || args_len > lim.max_task_request_bytes {
        let reply = br#"{"error":{"message":"sync_syscall: request too large"}}"#;
        return sync_reply(&sh, reply, out_ptr, out_len);
    }
    // SAFETY: guest-provided buffers.
    let (op, args) = unsafe { (slice(op, op_len), slice(args, args_len)) };
    let op = String::from_utf8_lossy(op).into_owned();
    let mut inp = sh.inp.lock();
    sh.send(&Frame::Sync { op, args_json: args.to_vec() });
    sh.flush();
    let reply = loop {
        let Got::Frame(f) = sh.read_or_die(&mut inp, true) else {
            continue;
        };
        if let Inbound::SyncResult(r) = sh.route_in_call(f) {
            break r;
        }
    };
    sync_reply(&sh, &reply, out_ptr, out_len)
}

/// Hands a sync reply to the guest: 1 on success; on an allocation failure
/// the out buffer is left empty (null, 0) and -1 is returned, so the guest
/// never reads an unwritten buffer.
fn sync_reply(sh: &Shared, reply: &[u8], out_ptr: *mut *mut u8, out_len: *mut usize) -> i32 {
    if sh.alloc_into_guest(reply, out_ptr, out_len) {
        return 1;
    }
    // SAFETY: out pointers come from the guest and are valid for one write each.
    unsafe {
        *out_ptr = std::ptr::null_mut();
        *out_len = 0;
    }
    sh.log_local(3, "sync_syscall: guest allocation failed".into());
    -1
}

extern "C" fn vt_log(level: i32, ptr: *const u8, len: usize) {
    let sh = current();
    let max = sh.limits().max_log_message_bytes;
    // SAFETY: guest-provided buffer.
    let bytes = clip_utf8(unsafe { slice(ptr, len) }, max).to_vec();
    sh.send(&Frame::Log { level, message: bytes });
}

extern "C" fn vt_oom(limit: usize, requested: usize) {
    let mut d = [0u8; 16];
    d[..8].copy_from_slice(&(limit as u64).to_le_bytes());
    d[8..].copy_from_slice(&(requested as u64).to_le_bytes());
    write_status(STATUS_OOM, &d);
}

// ---------------------------------------------------------------------------
// The loop
// ---------------------------------------------------------------------------

/// Parameters of [`run_loop`] that are not process-global.
#[derive(Debug, Clone)]
pub struct LoopConfig {
    pub functions: u32,
}

#[derive(Debug)]
pub enum LoopError {
    /// `GuestModule::init` or the before-Ready step failed (-> `InitFailed`).
    Init(anyhow::Error),
    Proto(ProtoError),
}

impl std::fmt::Display for LoopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoopError::Init(e) => write!(f, "init failed: {e:#}"),
            LoopError::Proto(e) => write!(f, "protocol error: {e}"),
        }
    }
}

impl std::error::Error for LoopError {}

fn error_envelope(kind: &str, message: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"error": {"message": message, "kind": kind}}))
        .unwrap_or_else(|_| br#"{"error":{"message":"worker error"}}"#.to_vec())
}

fn panic_message(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "guest panicked".into()
    }
}

/// Idle time after a reply before a requested trim runs.
const TRIM_IDLE: Duration = Duration::from_millis(100);

/// Runs init, `before_ready` (seccomp), sends `Ready`, then serves `Call`s
/// until `Shutdown` or EOF. `sh` must already be installed (`Shared::install`)
/// because the guest may use the vtable inside `init`.
pub fn run_loop<G: GuestModule>(
    sh: &Arc<Shared>,
    guest: &mut G,
    cfg: &LoopConfig,
    before_ready: impl FnOnce() -> anyhow::Result<()>,
) -> Result<(), LoopError> {
    let t0 = Instant::now();
    guest.init().map_err(LoopError::Init)?;
    before_ready().map_err(LoopError::Init)?;
    sh.send(&Frame::Ready {
        proto: PROTO_VERSION,
        pid: std::process::id(),
        resolved_exports: cfg.functions,
        init_us: t0.elapsed().as_micros() as u64,
        rss_bytes: current_rss_bytes().unwrap_or(0),
    });
    sh.flush();

    let mut trim_at: Option<Instant> = None;
    loop {
        let frame = {
            let mut inp = sh.inp.lock();
            if let Some(at) = trim_at {
                // A trim is pending: poll for the next frame until the idle
                // deadline, then trim and go back to blocking reads.
                match sh.next_frame(&mut inp, false).map_err(LoopError::Proto)? {
                    Got::Frame(f) => {
                        trim_at = None;
                        f
                    },
                    Got::Eof => return Ok(()),
                    Got::WouldBlock => {
                        drop(inp);
                        let now = Instant::now();
                        if now >= at {
                            trim_at = None;
                            guest.trim();
                        } else {
                            // Wakes the moment the next frame arrives.
                            sh.conn.wait_readable(at - now);
                        }
                        continue;
                    },
                }
            } else {
                match sh.next_frame(&mut inp, true).map_err(LoopError::Proto)? {
                    Got::Frame(f) => f,
                    Got::Eof => return Ok(()),
                    Got::WouldBlock => continue,
                }
            }
        };
        match frame {
            Frame::Shutdown => return Ok(()),
            Frame::Call { call_id, limits_json, export, args_json } => {
                let limits = serde_json::from_slice::<CallLimits>(&limits_json).unwrap_or_default();
                sh.begin_call(limits);
                let started = Instant::now();
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    guest.call(&export, &args_json)
                }));
                let guest_us = started.elapsed().as_micros() as u64;
                let mut envelope = match res {
                    Ok(Ok(env)) => env,
                    Ok(Err(e)) => error_envelope("user", &format!("{e:#}")),
                    Err(p) => error_envelope("panic", &panic_message(&*p)),
                };
                if envelope.len() > limits.max_result_bytes.saturating_add(64 * 1024) {
                    envelope = error_envelope(
                        "user",
                        &format!(
                            "Function result is too large ({} bytes, maximum {} bytes)",
                            envelope.len(),
                            limits.max_result_bytes
                        ),
                    );
                }
                sh.send(&Frame::Result { call_id, guest_us, envelope_json: envelope });
                sh.flush();
                sh.active.store(false, Ordering::Release);
                // Off the latency path: only collect once idle (below).
                trim_at = guest.wants_trim().then(|| Instant::now() + TRIM_IDLE);
            },
            // Stale answers between calls.
            Frame::Event { .. } | Frame::NoTasks { .. } | Frame::SyncResult { .. } => {},
            other => {
                return Err(LoopError::Proto(ProtoError::Malformed {
                    kind: other.kind(),
                    reason: "unexpected frame while idle",
                }))
            },
        }
    }
}

// ---------------------------------------------------------------------------
// dlopen-backed guest
// ---------------------------------------------------------------------------

#[repr(C)]
struct ConvexBuf {
    ptr: *mut u8,
    len: usize,
    #[allow(dead_code)]
    cap: usize,
}

type InitFn = unsafe extern "C" fn(*const HostVtableV1) -> u32;
type FreeResultFn = unsafe extern "C" fn(i64);
type RunFn = unsafe extern "C" fn(*mut u8, usize) -> i64;
type SetMemFn = unsafe extern "C" fn(usize);
type TrimFn = unsafe extern "C" fn(i32) -> i32;

/// A cdylib opened with `dlopen(RTLD_NOW | RTLD_LOCAL)`.
pub struct LibGuest {
    lib: libloading::os::unix::Library,
    memory_limit: usize,
    alloc: Option<AllocFn>,
    free_result: Option<FreeResultFn>,
    trim: Option<TrimFn>,
    runs: HashMap<String, RunFn>,
}

impl LibGuest {
    /// SECURITY: requires an [`sandbox::Applied`], which only
    /// `sandbox::enter` can produce, so the user dylib (constructors run
    /// inside `dlopen`) can never be loaded before the sandbox is fully
    /// applied. Do not add a constructor that bypasses this.
    pub fn open(
        path: &Path,
        memory_limit: u64,
        _sandbox: &sandbox::Applied,
    ) -> anyhow::Result<Self> {
        // SAFETY: loading arbitrary user code is the point of the worker; the
        // sandbox (proved by `_sandbox`) bounds what it can do.
        let lib = unsafe {
            libloading::os::unix::Library::open(
                Some(path),
                libloading::os::unix::RTLD_NOW | libloading::os::unix::RTLD_LOCAL,
            )
        }
        .with_context(|| format!("dlopen {}", path.display()))?;
        Ok(Self {
            lib,
            memory_limit: usize::try_from(memory_limit).unwrap_or(usize::MAX),
            alloc: None,
            free_result: None,
            trim: None,
            runs: HashMap::new(),
        })
    }

    /// `convex_run:` is added unless the name already carries it.
    fn symbol_name(export: &str) -> String {
        if export.starts_with("convex_run:") {
            export.to_string()
        } else {
            format!("convex_run:{export}")
        }
    }

    fn resolve_run(&mut self, export: &str) -> anyhow::Result<RunFn> {
        let name = Self::symbol_name(export);
        if let Some(f) = self.runs.get(&name) {
            return Ok(*f);
        }
        let mut sym = name.clone().into_bytes();
        sym.push(0);
        // SAFETY: the symbol has the `convex_run` ABI (NATIVE.md §2.4).
        let f: RunFn = unsafe {
            *self
                .lib
                .get::<RunFn>(&sym)
                .with_context(|| format!("export {name} not found"))?
        };
        self.runs.insert(name, f);
        Ok(f)
    }
}

impl GuestModule for LibGuest {
    fn init(&mut self) -> anyhow::Result<()> {
        // SAFETY: symbols per NATIVE.md §2.4.
        unsafe {
            let init: InitFn = *self
                .lib
                .get::<InitFn>(b"convex_native_init\0")
                .context("missing export convex_native_init")?;
            let alloc: AllocFn = *self
                .lib
                .get::<AllocFn>(b"convex_alloc\0")
                .context("missing export convex_alloc")?;
            let free: FreeResultFn = *self
                .lib
                .get::<FreeResultFn>(b"convex_free_result\0")
                .context("missing export convex_free_result")?;
            self.alloc = Some(alloc);
            self.free_result = Some(free);
            if self.lib.get::<unsafe extern "C" fn() -> i32>(b"convex_binary_events\0").is_ok() {
                current().binary_events.store(true, Ordering::Relaxed);
            }
            current().set_alloc(alloc);
            let version = init(&VTABLE);
            anyhow::ensure!(
                version == NATIVE_ABI_VERSION,
                "native ABI version {version} is not supported (want {NATIVE_ABI_VERSION})"
            );
            // Optional idle hook; old guests without it are untouched.
            self.trim = self.lib.get::<TrimFn>(b"convex_native_trim\0").ok().map(|f| *f);
            // D-103: optional; missing export means unlimited.
            if self.memory_limit > 0
                && let Ok(set) = self.lib.get::<SetMemFn>(b"convex_native_set_memory_limit\0") {
                    let set: SetMemFn = *set;
                    set(self.memory_limit);
                }
        }
        Ok(())
    }

    fn wants_trim(&mut self) -> bool {
        // SAFETY: `convex_native_trim(1)` only reads a counter (NATIVE.md §2.4).
        self.trim.is_some_and(|f| {
            std::panic::catch_unwind(|| unsafe { f(1) }).is_ok_and(|r| r != 0)
        })
    }

    fn trim(&mut self) {
        if let Some(f) = self.trim {
            // SAFETY: `convex_native_trim(0)` collects the calling thread's heap.
            let _ = std::panic::catch_unwind(|| unsafe { f(0) });
        }
    }

    fn call(&mut self, export: &str, request: &[u8]) -> anyhow::Result<Vec<u8>> {
        let run = self.resolve_run(export)?;
        let alloc = self.alloc.context("call before init")?;
        let free = self.free_result.context("call before init")?;
        // SAFETY: ABI v1: the guest owns the args buffer from here on and the
        // returned handle is a boxed ConvexBuf freed by convex_free_result.
        unsafe {
            let p = alloc(request.len());
            anyhow::ensure!(!p.is_null(), "convex_alloc returned null");
            std::ptr::copy_nonoverlapping(request.as_ptr(), p, request.len());
            let handle = run(p, request.len());
            anyhow::ensure!(handle != 0, "guest returned a null result handle");
            let buf = &*(handle as usize as *const ConvexBuf);
            let out = slice(buf.ptr, buf.len).to_vec();
            free(handle);
            Ok(out)
        }
    }
}

// ---------------------------------------------------------------------------
// RSS
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
pub fn current_rss_bytes() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = s.split_whitespace().nth(1)?.parse().ok()?;
    // SAFETY: sysconf has no preconditions.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    Some(pages * page.max(4096) as u64)
}

#[cfg(target_os = "macos")]
pub fn current_rss_bytes() -> Option<u64> {
    // SAFETY: `info` is a plain-old-data out parameter of the stated size.
    unsafe {
        let mut info: libc::proc_taskinfo = std::mem::zeroed();
        let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
        let n = libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDTASKINFO,
            0,
            (&mut info as *mut libc::proc_taskinfo).cast(),
            size,
        );
        (n == size).then_some(info.pti_resident_size)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn current_rss_bytes() -> Option<u64> {
    None
}

// ---------------------------------------------------------------------------
// Signals
// ---------------------------------------------------------------------------

static CALL_STACK_LO: AtomicUsize = AtomicUsize::new(0);
static CALL_STACK_HI: AtomicUsize = AtomicUsize::new(0);
static PREV_SEGV: Mutex<Option<libc::sigaction>> = Mutex::new(None);
static PREV_BUS: Mutex<Option<libc::sigaction>> = Mutex::new(None);

extern "C" fn on_sigsys(_sig: libc::c_int, info: *mut libc::siginfo_t, _ctx: *mut libc::c_void) {
    // si_syscall is an int at byte offset 24 of siginfo_t on 64-bit Linux.
    let nr = if info.is_null() {
        0
    } else {
        // SAFETY: the kernel filled a full siginfo_t.
        unsafe { *(info.cast::<u8>().add(24).cast::<i32>()) as u32 }
    };
    write_status(STATUS_SIGSYS, &nr.to_le_bytes());
    exit_now(EXIT_SIGSYS)
}

extern "C" fn on_fault(sig: libc::c_int, info: *mut libc::siginfo_t, ctx: *mut libc::c_void) {
    let lo = CALL_STACK_LO.load(Ordering::Relaxed);
    if lo != 0 && !info.is_null() {
        // SAFETY: SA_SIGINFO handler; `info` is valid.
        let addr = unsafe { (*info).si_addr() } as usize;
        const BELOW: usize = 1 << 20;
        const ABOVE: usize = 64 * 1024;
        if addr >= lo.saturating_sub(BELOW) && addr < lo + ABOVE {
            write_status(STATUS_STACK_OVERFLOW, &[]);
        }
    }
    let prev = if sig == libc::SIGSEGV { &PREV_SEGV } else { &PREV_BUS };
    // try_lock: a signal may interrupt the installer; never block here.
    let old = prev.try_lock().and_then(|g| *g);
    match old {
        Some(old) if old.sa_sigaction != libc::SIG_DFL && old.sa_sigaction != libc::SIG_IGN => {
            if old.sa_flags & libc::SA_SIGINFO != 0 {
                // SAFETY: the previous handler was installed with SA_SIGINFO.
                let f: extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void) =
                    unsafe { std::mem::transmute(old.sa_sigaction) };
                f(sig, info, ctx);
            } else {
                // SAFETY: plain handler.
                let f: extern "C" fn(libc::c_int) = unsafe { std::mem::transmute(old.sa_sigaction) };
                f(sig);
            }
        },
        _ => {
            // SAFETY: restore the default action and re-raise on return.
            unsafe {
                libc::signal(sig, libc::SIG_DFL);
            }
        },
    }
}

fn install_sigaction(
    sig: libc::c_int,
    handler: extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void),
    prev: Option<&Mutex<Option<libc::sigaction>>>,
) {
    // SAFETY: standard sigaction setup with a zeroed struct.
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = handler as usize;
        sa.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
        libc::sigemptyset(&mut sa.sa_mask);
        let mut old: libc::sigaction = std::mem::zeroed();
        if libc::sigaction(sig, &sa, &mut old) == 0
            && let Some(p) = prev {
                *p.lock() = Some(old);
            }
    }
}

/// Installs SIGSYS and the stack-overflow-noting SIGSEGV/SIGBUS handlers and
/// ignores SIGPIPE.
fn install_signal_handlers() {
    // SAFETY: SIG_IGN for SIGPIPE; broken sockets then surface as EPIPE.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    install_sigaction(libc::SIGSYS, on_sigsys, None);
    install_sigaction(libc::SIGSEGV, on_fault, Some(&PREV_SEGV));
    install_sigaction(libc::SIGBUS, on_fault, Some(&PREV_BUS));
}

/// Records the bounds of the current (call) thread's stack.
fn record_call_stack() {
    #[cfg(target_os = "linux")]
    // SAFETY: pthread attr queries on the current thread.
    unsafe {
        let mut attr: libc::pthread_attr_t = std::mem::zeroed();
        if libc::pthread_getattr_np(libc::pthread_self(), &mut attr) == 0 {
            let mut addr: *mut libc::c_void = std::ptr::null_mut();
            let mut size: libc::size_t = 0;
            if libc::pthread_attr_getstack(&attr, &mut addr, &mut size) == 0 {
                CALL_STACK_LO.store(addr as usize, Ordering::Relaxed);
                CALL_STACK_HI.store(addr as usize + size, Ordering::Relaxed);
            }
            libc::pthread_attr_destroy(&mut attr);
        }
    }
    #[cfg(target_os = "macos")]
    // SAFETY: pthread queries on the current thread.
    unsafe {
        let t = libc::pthread_self();
        let top = libc::pthread_get_stackaddr_np(t) as usize;
        let size = libc::pthread_get_stacksize_np(t);
        CALL_STACK_LO.store(top - size, Ordering::Relaxed);
        CALL_STACK_HI.store(top, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Process entry
// ---------------------------------------------------------------------------

fn init_failed(detail: &str) -> ! {
    write_status(STATUS_INIT_FAILED, &detail.as_bytes()[..detail.len().min(62)]);
    eprintln!("rust-native worker: {detail}");
    exit_now(EXIT_FAILED)
}

/// Sandbox first, dylib second. `enter` and `open` are parameters so a test
/// can assert the order and that a failed `enter` never reaches `open`.
fn load_guest<G>(
    args: &WorkerArgs,
    prelude: impl FnOnce(),
    enter: impl FnOnce(&WorkerArgs) -> anyhow::Result<sandbox::Applied>,
    open: impl FnOnce(&Path, u64, &sandbox::Applied) -> anyhow::Result<G>,
) -> anyhow::Result<G> {
    prelude();
    let applied = enter(args).context("sandbox setup failed; refusing to load the artifact")?;
    open(&args.artifact, args.memory_bytes, &applied)
}

/// Worker process entry (argv[1] == `WORKER_ARG`). Never returns.
pub fn worker_main() -> ! {
    worker_main_with(std::env::args().skip(1))
}

/// [`worker_main`] with explicit arguments (after the binary name). Lets the
/// host-side tests run a worker from a test binary, whose own argv belongs to
/// libtest.
pub fn worker_main_with<I: IntoIterator<Item = String>>(argv: I) -> ! {
    let args = match WorkerArgs::parse(argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("rust-native worker: bad arguments: {e:#}");
            exit_now(EXIT_USAGE)
        },
    };
    // SECURITY ORDER (see sandbox/mod.rs): the sandbox is applied first,
    // everything the user dylib could influence (dlopen, init, calls) comes
    // after. A failed `enter` exits without ever opening the dylib.
    let handlers_first = || {
        STATUS_PIPE.store(STATUS_FD, Ordering::Relaxed);
        install_signal_handlers(); // SIGSYS handler must exist before seccomp
    };
    let guest = match load_guest(&args, handlers_first, sandbox::enter, LibGuest::open) {
        Ok(g) => g,
        Err(e) => init_failed(&format!("{e:#}")),
    };

    let shared = Shared::new(Conn::new(SOCK_FD), args.max_frame_bytes);
    shared.install();

    let cfg = LoopConfig { functions: args.functions };
    let sh = shared.clone();
    let spawned = std::thread::Builder::new()
        .name("rust-native-call".into())
        .stack_size(args.stack_bytes)
        .spawn(move || {
            record_call_stack();
            let mut guest = guest;
            // seccomp was already installed by `sandbox::enter`.
            let res = run_loop(&sh, &mut guest, &cfg, || Ok(()));
            match res {
                Ok(()) => exit_now(0),
                Err(LoopError::Init(e)) => init_failed(&format!("{e:#}")),
                Err(e) => {
                    eprintln!("rust-native worker: {e}");
                    exit_now(EXIT_FAILED)
                },
            }
        });
    if let Err(e) = spawned {
        init_failed(&format!("cannot start the call thread: {e}"));
    }
    watchdog(&shared, &args)
}

/// Main-thread loop: RSS limit, parent liveness, log flushing.
fn watchdog(sh: &Arc<Shared>, args: &WorkerArgs) -> ! {
    // SAFETY: getppid has no preconditions.
    let ppid = unsafe { libc::getppid() };
    let limit = args.memory_bytes.saturating_add(args.rss_slack_bytes);
    loop {
        std::thread::sleep(if sh.active.load(Ordering::Acquire) {
            WATCHDOG_TICK
        } else {
            WATCHDOG_IDLE_TICK
        });
        sh.flush_if_dirty();
        // SAFETY: as above.
        if unsafe { libc::getppid() } != ppid {
            exit_now(0);
        }
        if args.memory_bytes > 0
            && let Some(rss) = current_rss_bytes()
                && rss > limit {
                    let mut d = [0u8; 16];
                    d[..8].copy_from_slice(&args.memory_bytes.to_le_bytes());
                    d[8..].copy_from_slice(&rss.to_le_bytes());
                    write_status(STATUS_OOM, &d);
                    exit_now(EXIT_OOM);
                }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::{
        io::Read,
        os::unix::{
            io::IntoRawFd,
            net::UnixStream,
        },
    };

    use super::{
        super::proto::{
            read_frame,
            write_frame,
        },
        *,
    };

    /// The vtable is process-global, so tests that install state are serial.
    static SERIAL: Mutex<()> = Mutex::new(());

    const MAXF: usize = 1 << 20;

    unsafe extern "C" fn test_alloc(len: usize) -> *mut u8 {
        let mut v = Vec::<u8>::with_capacity(len.max(1));
        let p = v.as_mut_ptr();
        std::mem::forget(v);
        p
    }

    unsafe extern "C" fn failing_alloc(_len: usize) -> *mut u8 {
        std::ptr::null_mut()
    }

    unsafe fn guest_take(p: *mut u8, len: usize) -> Vec<u8> {
        // SAFETY: allocated by `test_alloc` with capacity len.max(1).
        unsafe { Vec::from_raw_parts(p, len, len.max(1)) }
    }

    struct Harness {
        host: UnixStream,
        sh: Arc<Shared>,
        worker_fd: RawFd,
        _guard: parking_lot::MutexGuard<'static, ()>,
    }

    impl Harness {
        fn new() -> Self {
            let guard = SERIAL.lock();
            let (host, worker) = UnixStream::pair().unwrap();
            host.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let worker_fd = worker.into_raw_fd();
            let sh = Shared::new(Conn::new(worker_fd), MAXF);
            sh.set_alloc(test_alloc);
            sh.install();
            Harness { host, sh, worker_fd, _guard: guard }
        }

        fn send(&mut self, f: Frame) {
            write_frame(&mut self.host, &f).unwrap();
        }

        fn recv(&mut self) -> Frame {
            read_frame(&mut self.host, MAXF).unwrap().expect("worker closed the socket")
        }

        fn call(&mut self, id: u64, export: &str, args: &str) {
            self.send(Frame::Call {
                call_id: id,
                limits_json: br#"{"maxResultBytes":1048576,"maxTaskRequestBytes":1024,"maxOutstandingTasks":2,"maxLogMessageBytes":16}"#.to_vec(),
                export: export.into(),
                args_json: args.as_bytes().to_vec(),
            });
        }
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            // SAFETY: we own the fd handed out by `into_raw_fd`.
            unsafe {
                libc::close(self.worker_fd);
            }
            *SHARED.write() = None;
        }
    }

    /// N-T1: a guest allocation failure must reach the guest as an error
    /// code with an empty buffer, never as a success over unwritten memory.
    #[test]
    fn sync_reply_reports_allocation_failure() {
        let h = Harness::new();
        let mut ptr = 0x1 as *mut u8;
        let mut len = 7usize;
        h.sh.set_alloc(failing_alloc);
        assert_eq!(sync_reply(&h.sh, b"{}", &mut ptr, &mut len), -1);
        assert!(ptr.is_null());
        assert_eq!(len, 0);
        h.sh.set_alloc(test_alloc);
        assert_eq!(sync_reply(&h.sh, b"{}", &mut ptr, &mut len), 1);
        // SAFETY: allocated by `test_alloc` for `len` bytes.
        assert_eq!(unsafe { guest_take(ptr, len) }, b"{}");
    }

    type Behavior = Box<dyn FnMut(&str, &[u8]) -> anyhow::Result<Vec<u8>> + Send>;

    struct Fake {
        inited: Arc<AtomicBool>,
        f: Behavior,
    }

    impl GuestModule for Fake {
        fn init(&mut self) -> anyhow::Result<()> {
            self.inited.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn call(&mut self, export: &str, request: &[u8]) -> anyhow::Result<Vec<u8>> {
            (self.f)(export, request)
        }
    }

    fn start(
        h: &Harness,
        f: Behavior,
    ) -> std::thread::JoinHandle<Result<(), LoopError>> {
        let sh = h.sh.clone();
        std::thread::spawn(move || {
            let mut g = Fake { inited: Arc::new(AtomicBool::new(false)), f };
            run_loop(&sh, &mut g, &LoopConfig { functions: 3 }, || Ok(()))
        })
    }

    fn expect_ready(h: &mut Harness) {
        match h.recv() {
            Frame::Ready { proto, resolved_exports, pid, .. } => {
                assert_eq!(proto, PROTO_VERSION);
                assert_eq!(resolved_exports, 3);
                assert_eq!(pid, std::process::id());
            },
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn call_result_roundtrip_and_shutdown() {
        let mut h = Harness::new();
        let t = start(
            &h,
            Box::new(|export, req| {
                assert_eq!(export, "m:f");
                Ok(format!(r#"{{"ok":{}}}"#, String::from_utf8_lossy(req)).into_bytes())
            }),
        );
        expect_ready(&mut h);
        h.call(7, "m:f", "[1,2]");
        match h.recv() {
            Frame::Result { call_id, envelope_json, .. } => {
                assert_eq!(call_id, 7);
                assert_eq!(envelope_json, br#"{"ok":[1,2]}"#);
            },
            other => panic!("{other:?}"),
        }
        h.send(Frame::Shutdown);
        t.join().unwrap().unwrap();
    }

    #[test]
    fn eof_ends_the_loop() {
        let mut h = Harness::new();
        let t = start(&h, Box::new(|_, _| Ok(vec![])));
        expect_ready(&mut h);
        h.host.shutdown(std::net::Shutdown::Both).unwrap();
        t.join().unwrap().unwrap();
    }

    #[test]
    fn panic_becomes_panic_envelope_and_error_becomes_user_envelope() {
        let mut h = Harness::new();
        let t = start(
            &h,
            Box::new(|export, _| match export {
                "boom" => panic!("kaboom at x.rs:1:2"),
                _ => anyhow::bail!("export nope not found"),
            }),
        );
        expect_ready(&mut h);
        h.call(1, "boom", "[]");
        let Frame::Result { envelope_json, .. } = h.recv() else { panic!() };
        let v: serde_json::Value = serde_json::from_slice(&envelope_json).unwrap();
        assert_eq!(v["error"]["kind"], "panic");
        assert_eq!(v["error"]["message"], "kaboom at x.rs:1:2");
        h.call(2, "nope", "[]");
        let Frame::Result { call_id, envelope_json, .. } = h.recv() else { panic!() };
        assert_eq!(call_id, 2);
        let v: serde_json::Value = serde_json::from_slice(&envelope_json).unwrap();
        assert_eq!(v["error"]["kind"], "user");
        assert!(v["error"]["message"].as_str().unwrap().contains("not found"));
        h.send(Frame::Shutdown);
        t.join().unwrap().unwrap();
    }

    #[test]
    fn log_from_second_thread_through_vtable() {
        let mut h = Harness::new();
        let t = start(
            &h,
            Box::new(|_, _| {
                let th = std::thread::spawn(|| {
                    let msg = b"hello from a user thread, long enough to clip";
                    (VTABLE.log)(2, msg.as_ptr(), msg.len());
                });
                (VTABLE.log)(1, b"main".as_ptr(), 4);
                th.join().unwrap();
                Ok(br#"{"ok":null}"#.to_vec())
            }),
        );
        expect_ready(&mut h);
        h.call(1, "m:f", "[]");
        let mut logs = vec![];
        loop {
            match h.recv() {
                Frame::Log { level, message } => logs.push((level, message)),
                Frame::Result { .. } => break,
                other => panic!("{other:?}"),
            }
        }
        logs.sort();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0], (1, b"main".to_vec()));
        // maxLogMessageBytes is 16 in the test limits.
        assert_eq!(logs[1], (2, b"hello from a use".to_vec()));
        h.send(Frame::Shutdown);
        t.join().unwrap().unwrap();
    }

    #[test]
    fn submit_ids_limits_and_poll_events() {
        let mut h = Harness::new();
        let t = start(
            &h,
            Box::new(|_, _| {
                let sub = |s: &str| (VTABLE.task_submit)(s.as_ptr(), s.len());
                assert_eq!(sub(r#"{"a":1}"#), 0);
                assert_eq!(sub(r#"{"a":2}"#), 1);
                // maxOutstandingTasks = 2
                assert_eq!(sub(r#"{"a":3}"#), -1);
                // maxTaskRequestBytes = 1024
                let big = "x".repeat(2000);
                assert_eq!(sub(&big), -1);
                let (mut p, mut l) = (std::ptr::null_mut(), 0usize);
                let mut got = vec![];
                loop {
                    match (VTABLE.task_poll)(1, &mut p, &mut l) {
                        1 => got.push(String::from_utf8(unsafe { guest_take(p, l) }).unwrap()),
                        -1 => break,
                        n => panic!("task_poll -> {n}"),
                    }
                }
                // After both tasks finished a new submit fits again.
                assert_eq!(sub(r#"{"a":4}"#), 2);
                Ok(serde_json::to_vec(&serde_json::json!({"ok": got})).unwrap())
            }),
        );
        expect_ready(&mut h);
        h.call(1, "m:f", "[]");
        // Frames from the worker, in order.
        let mut submits = 0;
        let mut polls = 0;
        loop {
            match h.recv() {
                Frame::Log { level: 3, .. } => {},
                Frame::Submit { .. } => {
                    submits += 1;
                    if submits == 2 {
                        h.send(Frame::Event {
                            event_json: br#"{"type":"done","taskId":0,"ok":1}"#.to_vec(),
                        });
                    }
                },
                Frame::Poll { seq } => {
                    polls += 1;
                    if polls == 1 {
                        // A stale NoTasks must be ignored.
                        h.send(Frame::NoTasks { seq: seq.wrapping_sub(5) });
                        h.send(Frame::Event {
                            event_json: br#"{"type":"done","taskId":1,"ok":2}"#.to_vec(),
                        });
                    } else if polls == 2 {
                        h.send(Frame::NoTasks { seq });
                    }
                },
                Frame::Result { envelope_json, .. } => {
                    let v: serde_json::Value = serde_json::from_slice(&envelope_json).unwrap();
                    let evs = v["ok"].as_array().unwrap();
                    assert_eq!(evs.len(), 2, "{v}");
                    assert!(evs[0].as_str().unwrap().contains("\"taskId\":0"));
                    assert!(evs[1].as_str().unwrap().contains("\"taskId\":1"));
                    break;
                },
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(submits, 3);
        h.send(Frame::Shutdown);
        t.join().unwrap().unwrap();
    }

    #[test]
    fn poll_nonblocking_empty_and_ready() {
        let mut h = Harness::new();
        let t = start(
            &h,
            Box::new(|_, _| {
                let (mut p, mut l) = (std::ptr::null_mut(), 0usize);
                assert_eq!((VTABLE.task_poll)(0, &mut p, &mut l), 0);
                Ok(br#"{"ok":true}"#.to_vec())
            }),
        );
        expect_ready(&mut h);
        h.call(1, "m:f", "[]");
        assert!(matches!(h.recv(), Frame::Result { .. }));
        h.send(Frame::Shutdown);
        t.join().unwrap().unwrap();
    }

    #[test]
    fn sync_syscall_round_trip_and_oversize() {
        let mut h = Harness::new();
        let t = start(
            &h,
            Box::new(|_, _| {
                let (mut p, mut l) = (std::ptr::null_mut(), 0usize);
                let (op, args) = (b"now", b"{}");
                let r = (VTABLE.sync_syscall)(op.as_ptr(), 3, args.as_ptr(), 2, &mut p, &mut l);
                assert_eq!(r, 1);
                let reply = String::from_utf8(unsafe { guest_take(p, l) }).unwrap();
                let big = vec![b'x'; 5000];
                let r = (VTABLE.sync_syscall)(op.as_ptr(), 3, big.as_ptr(), big.len(), &mut p, &mut l);
                assert_eq!(r, 1);
                let too_big = String::from_utf8(unsafe { guest_take(p, l) }).unwrap();
                Ok(serde_json::to_vec(&serde_json::json!({"ok": [reply, too_big]})).unwrap())
            }),
        );
        expect_ready(&mut h);
        h.call(1, "m:f", "[]");
        match h.recv() {
            Frame::Sync { op, args_json } => {
                assert_eq!(op, "now");
                assert_eq!(args_json, b"{}");
                h.send(Frame::SyncResult { result_json: br#"{"ok":42}"#.to_vec() });
            },
            other => panic!("{other:?}"),
        }
        let Frame::Result { envelope_json, .. } = h.recv() else { panic!() };
        let v: serde_json::Value = serde_json::from_slice(&envelope_json).unwrap();
        assert_eq!(v["ok"][0], r#"{"ok":42}"#);
        assert!(v["ok"][1].as_str().unwrap().contains("request too large"));
        h.send(Frame::Shutdown);
        t.join().unwrap().unwrap();
    }

    #[test]
    fn init_failure_is_reported_as_init_error() {
        struct Bad;
        impl GuestModule for Bad {
            fn init(&mut self) -> anyhow::Result<()> {
                anyhow::bail!("nope")
            }
            fn call(&mut self, _: &str, _: &[u8]) -> anyhow::Result<Vec<u8>> {
                unreachable!()
            }
        }
        let h = Harness::new();
        let r = run_loop(&h.sh, &mut Bad, &LoopConfig { functions: 0 }, || Ok(()));
        assert!(matches!(r, Err(LoopError::Init(_))));
        let r = run_loop(
            &h.sh,
            &mut Fake { inited: Arc::new(AtomicBool::new(false)), f: Box::new(|_, _| Ok(vec![])) },
            &LoopConfig { functions: 0 },
            || anyhow::bail!("seccomp"),
        );
        assert!(matches!(r, Err(LoopError::Init(_))));
    }

    #[test]
    fn large_result_within_slack_passes() {
        let mut h = Harness::new();
        let t = start(
            &h,
            Box::new(|_, _| {
                let mut v = br#"{"ok":""#.to_vec();
                v.extend(std::iter::repeat_n(b'a', 1_000_000));
                v.extend_from_slice(b"\"}");
                Ok(v)
            }),
        );
        expect_ready(&mut h);
        // maxResultBytes 1 MiB + 64 KiB slack: 1_000_010 bytes fits.
        h.call(1, "m:f", "[]");
        let Frame::Result { envelope_json, .. } = h.recv() else { panic!() };
        assert!(envelope_json.len() > 1_000_000);
        h.send(Frame::Shutdown);
        t.join().unwrap().unwrap();
    }

    #[test]
    fn status_records_have_the_wire_layout() {
        let r = status_record(STATUS_SIGSYS, &7u32.to_le_bytes());
        assert_eq!(r.len(), 64);
        assert_eq!(r[0], 2);
        assert_eq!(&r[1..5], &7u32.to_le_bytes());
        assert!(r[5..].iter().all(|b| *b == 0));
        let long = status_record(STATUS_INIT_FAILED, &[b'x'; 200]);
        assert_eq!(long[0], 4);
        assert!(long[1..].iter().all(|b| *b == b'x'));
    }

    #[test]
    fn oom_vtable_entry_writes_status() {
        let _g = SERIAL.lock();
        let mut fds = [0 as RawFd; 2];
        // SAFETY: fresh pipe.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let prev = STATUS_PIPE.swap(fds[1], Ordering::SeqCst);
        (VTABLE.oom)(1000, 2000);
        STATUS_PIPE.store(prev, Ordering::SeqCst);
        // SAFETY: we own both ends.
        let mut rd = unsafe { <std::fs::File as std::os::unix::io::FromRawFd>::from_raw_fd(fds[0]) };
        unsafe { libc::close(fds[1]) };
        let mut rec = Vec::new();
        rd.read_to_end(&mut rec).unwrap();
        assert_eq!(rec.len(), 64);
        assert_eq!(rec[0], STATUS_OOM);
        assert_eq!(u64::from_le_bytes(rec[1..9].try_into().unwrap()), 1000);
        assert_eq!(u64::from_le_bytes(rec[9..17].try_into().unwrap()), 2000);
    }

    #[test]
    fn worker_args_roundtrip() {
        let a = WorkerArgs {
            artifact: "/x/y.so".into(),
            stack_bytes: 1 << 20,
            memory_bytes: 123456,
            sandbox: SandboxLevel::Off,
            tmp: Some("/t".into()),
            functions: 4,
            allow_network: true,
            rss_slack_bytes: 5,
            max_frame_bytes: 99999,
            sock_fd: 17,
            status_fd: 18,
            uid_gid: Some((1000, 1001)),
            wait_go: true,
            extra_read: vec!["/lib/a".into(), "/lib/b".into()],
        };
        let b = WorkerArgs::parse(a.to_args()).unwrap();
        assert_eq!(a, b);
        assert!(WorkerArgs::parse(["--bogus".to_string()]).is_err());
        assert!(WorkerArgs::parse(Vec::<String>::new()).is_err());
    }

    #[test]
    fn clip_utf8_respects_boundaries() {
        assert_eq!(clip_utf8("héllo".as_bytes(), 2), b"h");
        assert_eq!(clip_utf8("héllo".as_bytes(), 3), "hé".as_bytes());
        assert_eq!(clip_utf8(b"abc", 10), b"abc");
    }

    #[test]
    fn rss_is_readable() {
        if cfg!(any(target_os = "linux", target_os = "macos")) {
            assert!(current_rss_bytes().unwrap() > 1 << 20);
        }
    }

    // -- real dlopen ------------------------------------------------------

    const CDYLIB_SRC: &str = r#"
use std::sync::atomic::{AtomicUsize, Ordering};
static LIMIT: AtomicUsize = AtomicUsize::new(0);
static VT: AtomicUsize = AtomicUsize::new(0);
#[repr(C)] pub struct ConvexBuf { ptr: *mut u8, len: usize, cap: usize }
#[repr(C)] pub struct Vt { size: usize, version: u32,
  submit: extern "C" fn(*const u8, usize) -> i64,
  poll: extern "C" fn(i32, *mut *mut u8, *mut usize) -> i32,
  sync: extern "C" fn(*const u8, usize, *const u8, usize, *mut *mut u8, *mut usize) -> i32,
  log: extern "C" fn(i32, *const u8, usize),
  oom: extern "C" fn(usize, usize) }
#[no_mangle] pub extern "C" fn convex_native_init(vt: *const Vt) -> u32 {
  VT.store(vt as usize, Ordering::SeqCst); unsafe { (*vt).version } }
#[no_mangle] pub extern "C" fn convex_native_set_memory_limit(l: usize) { LIMIT.store(l, Ordering::SeqCst); }
#[no_mangle] pub extern "C" fn convex_alloc(len: usize) -> *mut u8 {
  let mut v = Vec::<u8>::with_capacity(len.max(1)); let p = v.as_mut_ptr(); std::mem::forget(v); p }
#[no_mangle] pub extern "C" fn convex_free_result(h: i64) { unsafe {
  let b = Box::from_raw(h as usize as *mut ConvexBuf); drop(Vec::from_raw_parts(b.ptr, b.len, b.cap)); } }
fn wrap(v: Vec<u8>) -> i64 { let mut v = v; let b = ConvexBuf { ptr: v.as_mut_ptr(), len: v.len(), cap: v.capacity() };
  std::mem::forget(v); Box::into_raw(Box::new(b)) as usize as i64 }
#[export_name = "convex_run:m:echo"]
pub extern "C" fn echo(p: *mut u8, len: usize) -> i64 {
  let args = unsafe { Vec::from_raw_parts(p, len, len.max(1)) };
  let vt = VT.load(Ordering::SeqCst) as *const Vt; let msg = b"echo called";
  unsafe { ((*vt).log)(1, msg.as_ptr(), msg.len()); }
  let mut out = b"{\"ok\":".to_vec(); out.extend_from_slice(&args); out.push(b'}'); wrap(out) }
#[export_name = "convex_run:m:limit"]
pub extern "C" fn limit(p: *mut u8, len: usize) -> i64 {
  drop(unsafe { Vec::from_raw_parts(p, len, len.max(1)) });
  wrap(format!("{{\"ok\":{}}}", LIMIT.load(Ordering::SeqCst)).into_bytes()) }
"#;

    fn build_test_cdylib() -> Option<PathBuf> {
        let dir = std::env::temp_dir().join(format!("rn-worker-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        let src = dir.join("guest.rs");
        std::fs::write(&src, CDYLIB_SRC).ok()?;
        let ext = if cfg!(target_os = "macos") { "dylib" } else { "so" };
        let out = dir.join(format!("libguest.{ext}"));
        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
        let st = std::process::Command::new(rustc)
            .args(["--edition", "2021", "--crate-type", "cdylib", "--crate-name", "guest", "-o"])
            .arg(&out)
            .arg(&src)
            .status()
            .ok()?;
        st.success().then_some(out)
    }

    #[test]
    fn real_dlopen_over_socketpair() {
        let Some(so) = build_test_cdylib() else {
            eprintln!("skipping: rustc not available to build the test cdylib");
            return;
        };
        let mut h = Harness::new();
        let sh = h.sh.clone();
        let t = std::thread::spawn(move || {
            let mut g = LibGuest::open(&so, 123_456_789, &sandbox::Applied::for_test()).unwrap();
            run_loop(&sh, &mut g, &LoopConfig { functions: 3 }, || Ok(()))
        });
        expect_ready(&mut h);
        // The full symbol and the short form both resolve.
        h.call(1, "m:echo", r#"{"a":[1,2,3]}"#);
        assert!(matches!(h.recv(), Frame::Log { level: 1, ref message } if message == b"echo called"));
        let Frame::Result { call_id, envelope_json, .. } = h.recv() else { panic!() };
        assert_eq!(call_id, 1);
        assert_eq!(envelope_json, br#"{"ok":{"a":[1,2,3]}}"#);
        h.call(2, "convex_run:m:limit", "[]");
        let Frame::Result { envelope_json, .. } = h.recv() else { panic!() };
        assert_eq!(envelope_json, br#"{"ok":123456789}"#);
        // Unknown export: an error envelope, the worker stays up.
        h.call(3, "m:missing", "[]");
        let Frame::Result { envelope_json, .. } = h.recv() else { panic!() };
        assert!(String::from_utf8_lossy(&envelope_json).contains("not found"));
        h.send(Frame::Shutdown);
        t.join().unwrap().unwrap();
    }

    #[test]
    fn dlopen_of_missing_file_fails() {
        assert!(LibGuest::open(Path::new("/nonexistent/x.so"), 0, &sandbox::Applied::for_test()).is_err());
    }

    /// Security invariant: the sandbox is applied before the dylib is
    /// opened, and a failed setup means the dylib is never opened.
    #[test]
    fn sandbox_setup_runs_before_dlopen_and_gates_it() {
        let args = WorkerArgs::parse(["--artifact".to_string(), "/x.so".into()]).unwrap();
        let log = std::cell::RefCell::new(Vec::<&str>::new());
        let g = load_guest(
            &args,
            || log.borrow_mut().push("prelude"),
            |_| {
                log.borrow_mut().push("enter");
                Ok(sandbox::Applied::for_test())
            },
            |_, _, _| {
                log.borrow_mut().push("open");
                Ok(7)
            },
        )
        .unwrap();
        assert_eq!(g, 7);
        assert_eq!(*log.borrow(), ["prelude", "enter", "open"]);

        log.borrow_mut().clear();
        let r: anyhow::Result<u32> = load_guest(
            &args,
            || {},
            |_| {
                log.borrow_mut().push("enter");
                anyhow::bail!("landlock unavailable")
            },
            |_, _, _| {
                log.borrow_mut().push("open");
                Ok(1)
            },
        );
        let msg = format!("{:#}", r.unwrap_err());
        assert!(msg.contains("refusing to load") && msg.contains("landlock unavailable"), "{msg}");
        assert_eq!(*log.borrow(), ["enter"], "open must not run after a failed setup");
    }
}
