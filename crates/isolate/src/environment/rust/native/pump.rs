//! Host side of one native call: `CallPump` (NATIVE.md §2.3 "Host pump").
//!
//! The pump owns the per-call [`HostBackend`], sends `Call`, services the
//! worker's `Submit`/`Poll`/`Sync`/`Log` frames, pushes backend events
//! eagerly, forwards stdout/stderr lines, enforces the deadline and
//! classifies a dead worker (§4.4). Dropping the `run` future kills the
//! worker process group.

use std::{
    os::fd::{
        AsRawFd,
        OwnedFd,
    },
    sync::Arc,
    time::{
        Duration,
        Instant,
    },
};

use async_trait::async_trait;
use serde_json::{
    json,
    Value as JsonValue,
};
use tokio::{
    io::{
        unix::AsyncFd,
        AsyncReadExt,
        AsyncWriteExt,
    },
    net::UnixStream,
};

use super::{
    metrics::{
        self,
        CallPhases,
        CallbackKind,
        ExitReason,
    },
    proto::{
        self,
        Frame,
        FrameDecoder,
        ProtoError,
    },
    NativeFailure,
};
use crate::environment::rust::core::{
    HostBackend,
    PollOutcome,
};

/// Bytes of stdout / stderr kept per call (same as wasm's `STDIO_CAPACITY`).
pub const STDIO_CAPACITY: usize = 16 * 1024 * 1024;
/// Bytes of `Log` frame payload forwarded per call.
pub const LOG_CAPACITY: usize = 16 * 1024 * 1024;
const READ_BUF_BYTES: usize = 256 * 1024;
/// Stop eagerly pulling backend events while this much is unsent.
const OUT_BACKLOG_BYTES: usize = 1024 * 1024;
const STDIO_TAIL_BYTES: usize = 8 * 1024;
const STATUS_RECORD_BYTES: usize = 64;
const STDIO_TRUNCATED: &str = "[output truncated: stdio limit exceeded]";
const LOG_TRUNCATED: &str = "[output truncated: log limit exceeded]";

// ---------------------------------------------------------------------------
// Worker handle abstractions
// ---------------------------------------------------------------------------

/// Exit information of a worker process.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    /// The cgroup reported an `oom_kill`.
    pub oom_killed: bool,
}

/// Process control of a worker. All methods take `&self` so the pump can keep
/// it next to the exclusively borrowed [`WorkerIo`].
#[async_trait]
pub trait WorkerProc: Send + Sync {
    /// `SIGKILL` the whole process group. Idempotent, never blocks.
    fn kill(&self);
    /// Waits for the process to exit; `None` if it is still alive after
    /// `grace`.
    async fn wait(&self, grace: Duration) -> Option<ExitInfo>;
}

/// A non-blocking-capable pipe read end (worker fd 1, 2 or 4 on the host).
pub struct PipeReader {
    fd: AsyncFd<OwnedFd>,
}

impl PipeReader {
    /// Takes ownership of `fd` and switches it to `O_NONBLOCK`.
    pub fn new(fd: OwnedFd) -> std::io::Result<Self> {
        let raw = fd.as_raw_fd();
        // SAFETY: fcntl on an fd we own.
        unsafe {
            let flags = libc::fcntl(raw, libc::F_GETFL);
            if flags < 0 || libc::fcntl(raw, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(Self {
            fd: AsyncFd::new(fd)?,
        })
    }

    fn read_raw(fd: &OwnedFd, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            // SAFETY: `buf` is a valid writable slice.
            let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n >= 0 {
                return Ok(n as usize);
            }
            let e = std::io::Error::last_os_error();
            if e.kind() != std::io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
    }

    /// Cancel-safe. `Ok(0)` is EOF.
    pub async fn read(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let mut guard = self.fd.readable().await?;
            match guard.try_io(|inner| Self::read_raw(inner.get_ref(), buf)) {
                Ok(r) => return r,
                Err(_would_block) => continue,
            }
        }
    }

    /// Reads whatever is in the pipe right now. `Ok(None)` if it would block.
    pub fn read_now(&self, buf: &mut [u8]) -> std::io::Result<Option<usize>> {
        match Self::read_raw(self.fd.get_ref(), buf) {
            Ok(n) => Ok(Some(n)),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// The host ends of a worker's fds.
pub struct WorkerIo {
    /// fd 3
    pub sock: UnixStream,
    /// fd 1
    pub stdout: Option<PipeReader>,
    /// fd 2
    pub stderr: Option<PipeReader>,
    /// fd 4
    pub status: Option<PipeReader>,
    /// Reusable read buffer.
    pub(super) inbuf: Vec<u8>,
}

impl WorkerIo {
    pub fn new(
        sock: UnixStream,
        stdout: Option<PipeReader>,
        stderr: Option<PipeReader>,
        status: Option<PipeReader>,
    ) -> Self {
        Self {
            sock,
            stdout,
            stderr,
            status,
            inbuf: Vec::new(),
        }
    }
}

/// What the pump needs from a worker. Implemented by the real worker handle
/// (WN-04/WN-06) and by the test fake.
pub trait PumpWorker: Send {
    fn split(&mut self) -> (&mut WorkerIo, &dyn WorkerProc);
}

// ---------------------------------------------------------------------------
// Call description and result
// ---------------------------------------------------------------------------

/// `limits` of the `Call` frame (camelCase JSON, NATIVE.md §2.2).
#[derive(Clone, Debug)]
pub struct CallLimits {
    pub max_result_bytes: usize,
    pub max_task_request_bytes: usize,
    pub max_outstanding_tasks: usize,
    pub max_log_message_bytes: usize,
}

impl Default for CallLimits {
    fn default() -> Self {
        // Same as `CoreLimits::default()`.
        Self {
            max_result_bytes: 16 * 1024 * 1024 + 64 * 1024,
            max_task_request_bytes: 64 * 1024 * 1024,
            max_outstanding_tasks: 1000,
            max_log_message_bytes: 32 * 1024,
        }
    }
}

impl CallLimits {
    pub fn to_json(&self) -> Vec<u8> {
        json!({
            "maxResultBytes": self.max_result_bytes,
            "maxTaskRequestBytes": self.max_task_request_bytes,
            "maxOutstandingTasks": self.max_outstanding_tasks,
            "maxLogMessageBytes": self.max_log_message_bytes,
        })
        .to_string()
        .into_bytes()
    }
}

#[derive(Clone, Debug)]
pub struct CallSpec {
    pub call_id: u64,
    pub export: String,
    pub args_json: Vec<u8>,
    pub limits: CallLimits,
    /// `limits.to_json()`, precomputed once per runtime so the hot path does
    /// not rebuild it per call. `None`: computed in the pump.
    pub limits_json: Option<Arc<[u8]>>,
    /// User action timeout; the pump kills the worker when it expires.
    pub timeout: Duration,
    /// For classification messages.
    pub memory_bytes: u64,
    pub stack_bytes: u64,
    pub stdio_capacity: usize,
    pub log_capacity: usize,
}

impl CallSpec {
    pub fn new(call_id: u64, export: impl Into<String>, args_json: Vec<u8>, timeout: Duration) -> Self {
        Self {
            call_id,
            export: export.into(),
            args_json,
            limits: CallLimits::default(),
            limits_json: None,
            timeout,
            memory_bytes: 512 * 1024 * 1024,
            stack_bytes: 8 * 1024 * 1024,
            stdio_capacity: STDIO_CAPACITY,
            log_capacity: LOG_CAPACITY,
        }
    }
}

#[derive(Debug)]
pub struct CallOutput {
    pub envelope_json: Vec<u8>,
    /// `guest_us` as reported by the worker.
    pub guest_us: u64,
}

#[derive(Debug)]
pub struct PumpReport {
    pub outcome: Result<CallOutput, NativeFailure>,
    /// `send`, `guest`, `pump` and `total` are filled; `queue` and `checkout`
    /// belong to the caller.
    pub phases: CallPhases,
}

// ---------------------------------------------------------------------------
// Status pipe and crash classification (NATIVE.md §4.4)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatusNote {
    Oom { limit: u64, requested: u64 },
    SigSys { nr: u32 },
    StackOverflow,
    InitFailed(String),
    Unknown(u8),
}

/// Parses fixed-size 64-byte records; a trailing partial record is ignored.
pub fn parse_status(bytes: &[u8]) -> Vec<StatusNote> {
    bytes
        .chunks_exact(STATUS_RECORD_BYTES)
        .map(|rec| {
            let detail = &rec[1..];
            match rec[0] {
                1 => StatusNote::Oom {
                    limit: u64::from_le_bytes(detail[0..8].try_into().unwrap()),
                    requested: u64::from_le_bytes(detail[8..16].try_into().unwrap()),
                },
                2 => StatusNote::SigSys {
                    nr: u32::from_le_bytes(detail[0..4].try_into().unwrap()),
                },
                3 => StatusNote::StackOverflow,
                4 => {
                    let end = detail[..62].iter().position(|b| *b == 0).unwrap_or(62);
                    StatusNote::InitFailed(String::from_utf8_lossy(&detail[..end]).into_owned())
                },
                other => StatusNote::Unknown(other),
            }
        })
        .collect()
}

/// Everything the classifier looks at.
pub struct Observation<'a> {
    pub notes: &'a [StatusNote],
    pub exit: Option<&'a ExitInfo>,
    pub stderr_tail: &'a str,
    pub memory_bytes: u64,
    pub stack_bytes: u64,
}

pub fn signal_name(sig: i32) -> String {
    match sig {
        libc::SIGSEGV => "SIGSEGV".into(),
        libc::SIGBUS => "SIGBUS".into(),
        libc::SIGILL => "SIGILL".into(),
        libc::SIGABRT => "SIGABRT".into(),
        libc::SIGFPE => "SIGFPE".into(),
        libc::SIGKILL => "SIGKILL".into(),
        libc::SIGTERM => "SIGTERM".into(),
        libc::SIGSYS => "SIGSYS".into(),
        libc::SIGPIPE => "SIGPIPE".into(),
        n => format!("signal {n}"),
    }
}

fn syscall_name(nr: u32) -> Option<&'static str> {
    // Linux x86_64 numbers of the calls most likely to hit the filter.
    Some(match nr {
        41 => "socket",
        42 => "connect",
        43 => "accept",
        49 => "bind",
        50 => "listen",
        56 => "clone",
        57 => "fork",
        58 => "vfork",
        59 => "execve",
        101 => "ptrace",
        165 => "mount",
        _ => return None,
    })
}

/// Classifies a worker that went away without a `Result` (§4.4).
pub fn classify(obs: &Observation<'_>) -> NativeFailure {
    for note in obs.notes {
        match note {
            StatusNote::Oom { limit, .. } => return NativeFailure::OutOfMemory { limit: *limit },
            StatusNote::StackOverflow => {
                return NativeFailure::StackOverflow {
                    stack: obs.stack_bytes,
                };
            },
            StatusNote::SigSys { nr } => {
                let what = match syscall_name(*nr) {
                    Some(n) => format!("{n}, {nr}"),
                    None => nr.to_string(),
                };
                return NativeFailure::Crashed(format!(
                    "Rust action was stopped by the sandbox (disallowed system call {what})"
                ));
            },
            StatusNote::InitFailed(msg) => return NativeFailure::StartFailed(msg.clone()),
            StatusNote::Unknown(_) => {},
        }
    }
    let exit = obs.exit.cloned().unwrap_or_default();
    let tail = obs.stderr_tail;
    let abrt_or_segv = matches!(exit.signal, Some(libc::SIGABRT) | Some(libc::SIGSEGV));
    if exit.oom_killed
        || exit.code == Some(137)
        || (exit.signal == Some(libc::SIGABRT) && tail.contains("memory allocation of"))
    {
        return NativeFailure::OutOfMemory {
            limit: obs.memory_bytes,
        };
    }
    if abrt_or_segv && tail.contains("has overflowed its stack") {
        return NativeFailure::StackOverflow {
            stack: obs.stack_bytes,
        };
    }
    match (exit.signal, exit.code) {
        (Some(sig), _) => NativeFailure::Crashed(format!(
            "Rust action crashed the worker process (signal {})",
            signal_name(sig)
        )),
        (None, Some(code)) => NativeFailure::Crashed(format!(
            "Rust action exited the worker process (exit status {code})"
        )),
        (None, None) => NativeFailure::Crashed(
            "Rust action crashed the worker process (unknown exit status)".to_string(),
        ),
    }
}

fn exit_reason(f: &NativeFailure) -> ExitReason {
    match f {
        NativeFailure::Timeout => ExitReason::Timeout,
        NativeFailure::OutOfMemory { .. } => ExitReason::Oom,
        NativeFailure::StackOverflow { .. } => ExitReason::Stack,
        NativeFailure::Crashed(m) if m.contains("sandbox") => ExitReason::Sigsys,
        NativeFailure::Crashed(m) if m.contains("signal") => ExitReason::Signal,
        NativeFailure::Crashed(_) => ExitReason::Exit,
        NativeFailure::Protocol(_) | NativeFailure::ResultTooLarge { .. } => ExitReason::Protocol,
        NativeFailure::StartFailed(_) => ExitReason::Exit,
    }
}

// ---------------------------------------------------------------------------
// stdout / stderr forwarding
// ---------------------------------------------------------------------------

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

struct SinkState {
    level: i32,
    pending: Vec<u8>,
    total: usize,
    truncated: bool,
    reported: bool,
    tail: Vec<u8>,
}

impl SinkState {
    fn new(level: i32) -> Self {
        Self {
            level,
            pending: Vec::new(),
            total: 0,
            truncated: false,
            reported: false,
            tail: Vec::new(),
        }
    }

    fn feed<B: HostBackend>(&mut self, data: &[u8], cap: usize, max_line: usize, backend: &mut B) {
        self.tail.extend_from_slice(data);
        if self.tail.len() > STDIO_TAIL_BYTES {
            let cut = self.tail.len() - STDIO_TAIL_BYTES;
            self.tail.drain(..cut);
        }
        let take = data.len().min(cap.saturating_sub(self.total));
        if take < data.len() {
            self.truncated = true;
        }
        self.total += take;
        let mut from = self.pending.len();
        self.pending.extend_from_slice(&data[..take]);
        while let Some(i) = self.pending[from..].iter().position(|b| *b == b'\n') {
            let i = from + i;
            let line = String::from_utf8_lossy(&self.pending[..i]).into_owned();
            self.pending.drain(..=i);
            from = 0;
            backend.log(self.level, truncate_utf8(line, max_line));
        }
    }

    fn finish<B: HostBackend>(&mut self, max_line: usize, backend: &mut B) {
        if !self.pending.is_empty() {
            let line = String::from_utf8_lossy(&self.pending).into_owned();
            self.pending.clear();
            backend.log(self.level, truncate_utf8(line, max_line));
        }
        if self.truncated && !self.reported {
            self.reported = true;
            backend.log(self.level, STDIO_TRUNCATED.to_string());
        }
    }
}

struct Sink<'a> {
    rd: Option<&'a PipeReader>,
    buf: Vec<u8>,
    st: SinkState,
}

impl<'a> Sink<'a> {
    fn new(rd: Option<&'a PipeReader>, level: i32) -> Self {
        Self {
            rd,
            buf: if rd.is_some() { vec![0; 8192] } else { Vec::new() },
            st: SinkState::new(level),
        }
    }

    fn open(&self) -> bool {
        self.rd.is_some()
    }

    /// Cancel-safe. Data is in `buf[..n]`.
    async fn read(&mut self) -> std::io::Result<usize> {
        match self.rd {
            Some(rd) => rd.read(&mut self.buf).await,
            None => std::future::pending().await,
        }
    }

    fn on_read<B: HostBackend>(
        &mut self,
        r: std::io::Result<usize>,
        cap: usize,
        max_line: usize,
        backend: &mut B,
    ) {
        match r {
            Ok(n) if n > 0 => self.st.feed(&self.buf[..n], cap, max_line, backend),
            _ => self.rd = None,
        }
    }

    /// Everything currently in the pipe.
    fn drain<B: HostBackend>(&mut self, cap: usize, max_line: usize, backend: &mut B) {
        while let Some(rd) = self.rd {
            match rd.read_now(&mut self.buf) {
                Ok(Some(n)) if n > 0 => self.st.feed(&self.buf[..n], cap, max_line, backend),
                Ok(None) => break,
                _ => {
                    self.rd = None;
                },
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The pump
// ---------------------------------------------------------------------------

struct KillGuard<'a> {
    proc_: &'a dyn WorkerProc,
    armed: bool,
}

impl Drop for KillGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.proc_.kill();
            metrics::log_worker_exit(ExitReason::Cancelled);
        }
    }
}

/// Mutable pump state shared by the frame handlers.
struct State<B> {
    backend: B,
    /// Frames waiting to be written to the worker.
    out: Vec<u8>,
    off: usize,
    /// A blocking `backend.poll(true)` should be running.
    armed: bool,
    /// Seq of an unanswered worker `Poll`.
    waiting: Option<u32>,
    /// `Submit` frames seen = next worker-assigned task id.
    submits: u64,
    log_bytes: usize,
    log_cap: usize,
    log_reported: bool,
    max_log_message: usize,
    max_result: usize,
    call_id: u64,
}

enum Stop {
    Fail(NativeFailure),
    /// Socket EOF or error.
    Dead,
}

enum Handled {
    Continue,
    Result { guest_us: u64, envelope: Vec<u8> },
}

impl<B: HostBackend> State<B> {
    fn push(&mut self, frame: &Frame) {
        if self.off == self.out.len() {
            self.out.clear();
            self.off = 0;
        }
        // Encoding only fails for strings > u16::MAX, never for these frames.
        let before = self.out.len();
        if frame.encode_into(&mut self.out).is_ok() {
            metrics::log_ipc_bytes(false, self.out.len() - before);
        } else {
            self.out.truncate(before);
        }
    }

    fn push_event(&mut self, ev: String) {
        self.waiting = None;
        self.push(&Frame::Event {
            event_json: ev.into_bytes(),
        });
    }

    fn push_chunk(&mut self, event: Vec<u8>) {
        self.waiting = None;
        self.push(&Frame::Chunk { event });
    }

    /// Queues one polled event (a no-op for `Empty` / `NoTasks`).
    fn push_outcome(&mut self, outcome: PollOutcome) -> bool {
        match outcome {
            PollOutcome::Event(ev) => self.push_event(ev),
            PollOutcome::Chunk(ev) => self.push_chunk(ev),
            PollOutcome::Empty | PollOutcome::NoTasks => return false,
        }
        true
    }

    /// After an event went out, queues every other ready event too (up to the
    /// write backlog cap), so the worker finds them in one read instead of
    /// paying a `Poll` round trip each.
    async fn drain_ready(&mut self) {
        while self.out.len() - self.off < OUT_BACKLOG_BYTES {
            let outcome = self.backend.poll(false).await;
            if !self.push_outcome(outcome) {
                break;
            }
        }
    }

    async fn handle(&mut self, frame: Frame) -> Result<Handled, NativeFailure> {
        match frame {
            Frame::Submit { request_json } => {
                metrics::log_callback(CallbackKind::Submit);
                let req = serde_json::from_slice(&request_json).unwrap_or(JsonValue::Null);
                let expected = self.submits;
                self.submits += 1;
                match self.backend.submit(req) {
                    Ok(id) if id as u64 == expected && id >= 0 => self.armed = true,
                    Ok(id) => {
                        return Err(NativeFailure::Protocol(format!(
                            "task id desync: worker id {expected}, host id {id} (ActionBackend::submit \
                             must consume an id for every Submit)"
                        )));
                    },
                    Err(msg) => {
                        let ev = json!({
                            "type": "done",
                            "taskId": expected,
                            "error": {"message": format!("host rejected the task request: {msg}")},
                        });
                        self.push_event(ev.to_string());
                    },
                }
            },
            Frame::Poll { seq } => {
                let outcome = self.backend.poll(false).await;
                if self.push_outcome(outcome) {
                    self.drain_ready().await;
                } else {
                    // The blocking poll answers NoTasks (also when streams are open).
                    metrics::log_callback(CallbackKind::PollBlock);
                    self.waiting = Some(seq);
                    self.armed = true;
                }
            },
            Frame::Sync { op, args_json } => {
                metrics::log_callback(CallbackKind::Sync);
                let args = serde_json::from_slice(&args_json).unwrap_or(JsonValue::Null);
                let reply = self.backend.sync_syscall(&op, args);
                self.push(&Frame::SyncResult {
                    result_json: reply.to_string().into_bytes(),
                });
            },
            Frame::Log { level, message } => {
                metrics::log_callback(CallbackKind::Log);
                // Level 4 is the guest's panic marker, not a user log line.
                if level == 4 {
                    return Ok(Handled::Continue);
                }
                if self.log_bytes >= self.log_cap {
                    if !self.log_reported {
                        self.log_reported = true;
                        self.backend.log(3, LOG_TRUNCATED.to_string());
                    }
                } else {
                    self.log_bytes += message.len();
                    let text = String::from_utf8_lossy(&message).into_owned();
                    self.backend
                        .log(level, truncate_utf8(text, self.max_log_message));
                }
            },
            Frame::Result {
                call_id,
                guest_us,
                envelope_json,
            } => {
                if call_id != self.call_id {
                    return Err(NativeFailure::Protocol(format!(
                        "Result for call {call_id}, expected {}",
                        self.call_id
                    )));
                }
                if envelope_json.len() > self.max_result {
                    return Err(NativeFailure::ResultTooLarge {
                        len: envelope_json.len(),
                        max: self.max_result,
                    });
                }
                return Ok(Handled::Result {
                    guest_us,
                    envelope: envelope_json,
                });
            },
            other => {
                return Err(NativeFailure::Protocol(format!(
                    "unexpected frame kind {:#04x} from the worker",
                    other.kind()
                )));
            },
        }
        Ok(Handled::Continue)
    }
}

pub struct CallPump<B: HostBackend> {
    spec: CallSpec,
    backend: B,
}

impl<B: HostBackend> CallPump<B> {
    pub fn new(spec: CallSpec, backend: B) -> Self {
        Self { spec, backend }
    }

    /// Runs the call to completion on `worker`. On any failure the worker is
    /// already killed. Dropping the future kills the worker too.
    pub async fn run<W: PumpWorker>(self, worker: &mut W) -> PumpReport {
        let started = Instant::now();
        let (io, proc_) = worker.split();
        let mut guard = KillGuard { proc_, armed: true };
        let mut phases = CallPhases::default();
        let outcome = self.run_inner(io, proc_, &mut phases).await;
        guard.armed = false;
        phases.total = started.elapsed();
        if let Err(f) = &outcome {
            metrics::log_worker_exit(exit_reason(f));
        }
        PumpReport { outcome, phases }
    }

    async fn run_inner(
        self,
        io: &mut WorkerIo,
        proc_: &dyn WorkerProc,
        phases: &mut CallPhases,
    ) -> Result<CallOutput, NativeFailure> {
        let CallPump { mut spec, backend } = self;
        let started = Instant::now();
        let max_frame = proto::max_frame_len(
            spec.limits.max_task_request_bytes,
            spec.limits.max_result_bytes,
        );
        let mut st = State {
            backend,
            out: Vec::with_capacity(spec.args_json.len() + spec.export.len() + 512),
            off: 0,
            armed: false,
            waiting: None,
            submits: 0,
            log_bytes: 0,
            log_cap: spec.log_capacity,
            log_reported: false,
            max_log_message: spec.limits.max_log_message_bytes,
            max_result: spec.limits.max_result_bytes,
            call_id: spec.call_id,
        };
        // Moves (not clones) the args and export into the frame; neither is
        // used after encoding.
        let call = Frame::Call {
            call_id: spec.call_id,
            limits_json: match &spec.limits_json {
                Some(j) => j.to_vec(),
                None => spec.limits.to_json(),
            },
            export: std::mem::take(&mut spec.export),
            args_json: std::mem::take(&mut spec.args_json),
        };
        if let Err(e) = call.encode_into(&mut st.out) {
            proc_.kill();
            return Err(NativeFailure::Protocol(format!("cannot encode Call: {e}")));
        }
        metrics::log_ipc_bytes(false, st.out.len());

        let max_line = spec.limits.max_log_message_bytes;
        let cap = spec.stdio_capacity;
        let WorkerIo {
            sock,
            stdout,
            stderr,
            status,
            inbuf,
        } = io;
        let (mut rd, mut wr) = sock.split();
        let mut out_sink = Sink::new(stdout.as_ref(), 1);
        let mut err_sink = Sink::new(stderr.as_ref(), 3);
        let mut decoder = FrameDecoder::new(max_frame);
        if inbuf.capacity() < READ_BUF_BYTES {
            *inbuf = Vec::with_capacity(READ_BUF_BYTES);
        }
        let deadline = tokio::time::sleep(spec.timeout);
        tokio::pin!(deadline);

        let mut call_written_at: Option<Instant> = None;
        let mut pump_time = Duration::ZERO;
        let mut dead = false;

        let result: Result<(u64, Vec<u8>), Stop> = 'outer: loop {
            // Frames already buffered.
            loop {
                match decoder.next_frame() {
                    Ok(Some(frame)) => {
                        let t = Instant::now();
                        // Host ops run outside the `select!` below; the call
                        // deadline must still bound them.
                        let Ok(h) = tokio::time::timeout_at(deadline.deadline(), st.handle(frame)).await
                        else {
                            break 'outer Err(Stop::Fail(NativeFailure::Timeout));
                        };
                        pump_time += t.elapsed();
                        match h {
                            Ok(Handled::Continue) => {},
                            Ok(Handled::Result { guest_us, envelope }) => {
                                break 'outer Ok((guest_us, envelope));
                            },
                            Err(f) => break 'outer Err(Stop::Fail(f)),
                        }
                    },
                    Ok(None) => break,
                    Err(ProtoError::Oversize {
                        len,
                        kind: Some(proto::KIND_RESULT),
                        ..
                    }) => {
                        break 'outer Err(Stop::Fail(NativeFailure::ResultTooLarge {
                            len,
                            max: spec.limits.max_result_bytes,
                        }));
                    },
                    Err(e) => break 'outer Err(Stop::Fail(NativeFailure::Protocol(e.to_string()))),
                }
            }
            if dead {
                break Err(Stop::Dead);
            }

            let pending_out = st.out.len() - st.off;
            tokio::select! {
                biased;
                _ = &mut deadline => break Err(Stop::Fail(NativeFailure::Timeout)),
                r = async {
                    inbuf.clear();
                    rd.read_buf(&mut *inbuf).await
                } => {
                    match r {
                        Ok(n) if n > 0 => {
                            metrics::log_ipc_bytes(true, n);
                            decoder.push(&inbuf[..]);
                        },
                        _ => dead = true,
                    }
                },
                r = wr.write(&st.out[st.off..]), if pending_out > 0 => {
                    match r {
                        Ok(n) => {
                            st.off += n;
                            if st.off == st.out.len() {
                                st.out.clear();
                                st.off = 0;
                                if call_written_at.is_none() {
                                    let now = Instant::now();
                                    phases.send = now - started;
                                    call_written_at = Some(now);
                                }
                            }
                        },
                        // The worker is gone; the read side reports it.
                        Err(_) => {
                            st.out.clear();
                            st.off = 0;
                        },
                    }
                },
                ev = st.backend.poll(true), if st.armed && pending_out < OUT_BACKLOG_BYTES => {
                    match ev {
                        PollOutcome::Event(e) => {
                            st.push_event(e);
                            if tokio::time::timeout_at(deadline.deadline(), st.drain_ready()).await.is_err() {
                                break 'outer Err(Stop::Fail(NativeFailure::Timeout));
                            }
                        },
                        PollOutcome::Chunk(c) => {
                            st.push_chunk(c);
                            if tokio::time::timeout_at(deadline.deadline(), st.drain_ready()).await.is_err() {
                                break 'outer Err(Stop::Fail(NativeFailure::Timeout));
                            }
                        },
                        PollOutcome::NoTasks => {
                            st.armed = false;
                            if let Some(seq) = st.waiting.take() {
                                st.push(&Frame::NoTasks { seq });
                            }
                        },
                        // `poll(true)` never reports Empty; re-arm on the next frame.
                        PollOutcome::Empty => st.armed = false,
                    }
                },
                r = out_sink.read(), if out_sink.open() => {
                    out_sink.on_read(r, cap, max_line, &mut st.backend);
                },
                r = err_sink.read(), if err_sink.open() => {
                    err_sink.on_read(r, cap, max_line, &mut st.backend);
                },
            }
        };

        phases.pump = pump_time;
        if let Some(t) = call_written_at {
            phases.guest = t.elapsed();
        }

        match result {
            Ok((guest_us, envelope_json)) => {
                // Output written before the Result is in the pipes already.
                out_sink.drain(cap, max_line, &mut st.backend);
                err_sink.drain(cap, max_line, &mut st.backend);
                out_sink.st.finish(max_line, &mut st.backend);
                err_sink.st.finish(max_line, &mut st.backend);
                phases.guest_reported = Duration::from_micros(guest_us);
                Ok(CallOutput {
                    envelope_json,
                    guest_us,
                })
            },
            Err(Stop::Fail(NativeFailure::Timeout)) => {
                proc_.kill();
                out_sink.drain(cap, max_line, &mut st.backend);
                err_sink.drain(cap, max_line, &mut st.backend);
                out_sink.st.finish(max_line, &mut st.backend);
                err_sink.st.finish(max_line, &mut st.backend);
                Err(NativeFailure::Timeout)
            },
            Err(Stop::Dead) => {
                // Socket EOF or error: find out why the worker went away.
                out_sink.drain(cap, max_line, &mut st.backend);
                err_sink.drain(cap, max_line, &mut st.backend);
                let notes = match status.as_ref() {
                    Some(s) => read_status(s).await,
                    None => Vec::new(),
                };
                let exit = match proc_.wait(Duration::from_secs(1)).await {
                    Some(e) => Some(e),
                    None => {
                        proc_.kill();
                        proc_.wait(Duration::from_secs(2)).await
                    },
                };
                out_sink.drain(cap, max_line, &mut st.backend);
                err_sink.drain(cap, max_line, &mut st.backend);
                out_sink.st.finish(max_line, &mut st.backend);
                err_sink.st.finish(max_line, &mut st.backend);
                let tail = String::from_utf8_lossy(&err_sink.st.tail).into_owned();
                Err(classify(&Observation {
                    notes: &notes,
                    exit: exit.as_ref(),
                    stderr_tail: &tail,
                    memory_bytes: spec.memory_bytes,
                    stack_bytes: spec.stack_bytes,
                }))
            },
            Err(Stop::Fail(other)) => {
                // Protocol violation or oversize result: the stream is unusable.
                proc_.kill();
                err_sink.drain(cap, max_line, &mut st.backend);
                out_sink.drain(cap, max_line, &mut st.backend);
                out_sink.st.finish(max_line, &mut st.backend);
                err_sink.st.finish(max_line, &mut st.backend);
                Err(other)
            },
        }
    }
}

/// Reads the status pipe until EOF (the process is dead) or 200 ms.
async fn read_status(status: &PipeReader) -> Vec<StatusNote> {
    let mut data = Vec::new();
    let mut buf = [0u8; 256];
    let _ = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match status.read(&mut buf).await {
                Ok(n) if n > 0 => data.extend_from_slice(&buf[..n]),
                _ => break,
            }
        }
    })
    .await;
    parse_status(&data)
}


#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        io::Write as _,
        os::{
            fd::OwnedFd,
            unix::net::UnixStream as StdUnixStream,
        },
        sync::{
            atomic::{
                AtomicBool,
                Ordering,
            },
            Arc,
        },
        thread::JoinHandle,
    };

    use parking_lot::Mutex;
    use tokio::sync::mpsc;

    use super::*;

    // ---- fake backend ----------------------------------------------------

    #[derive(Default)]
    struct Shared {
        submitted: Vec<JsonValue>,
        logs: Vec<(i32, String)>,
    }

    /// Mirrors `ActionBackend`: every submit consumes an id; `{"type":
    /// "sleep","ms":n}` finishes after n ms; `{"type":"bad"}` is rejected.
    struct FakeBackend {
        shared: Arc<Mutex<Shared>>,
        next_id: u64,
        outstanding: usize,
        tx: mpsc::UnboundedSender<String>,
        rx: mpsc::UnboundedReceiver<String>,
        skip_id_on_reject: bool,
        /// `poll(false)` never completes (a host op that outlives the call).
        stuck_poll: bool,
    }

    impl FakeBackend {
        fn new() -> (Self, Arc<Mutex<Shared>>) {
            let shared = Arc::new(Mutex::new(Shared::default()));
            let (tx, rx) = mpsc::unbounded_channel();
            (
                Self {
                    shared: shared.clone(),
                    next_id: 0,
                    outstanding: 0,
                    tx,
                    rx,
                    skip_id_on_reject: false,
                    stuck_poll: false,
                },
                shared,
            )
        }
    }

    #[async_trait]
    impl HostBackend for FakeBackend {
        fn submit(&mut self, request: JsonValue) -> Result<i64, String> {
            self.shared.lock().submitted.push(request.clone());
            if request.get("type").and_then(|t| t.as_str()) == Some("bad") || request.is_null() {
                if !self.skip_id_on_reject {
                    self.next_id += 1;
                }
                return Err("bad request".into());
            }
            let id = self.next_id;
            self.next_id += 1;
            self.outstanding += 1;
            let ms = request.get("ms").and_then(|m| m.as_u64()).unwrap_or(0);
            let tx = self.tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(ms)).await;
                let _ = tx.send(json!({"type":"done","taskId":id,"ok":ms}).to_string());
            });
            Ok(id as i64)
        }

        async fn poll(&mut self, block: bool) -> PollOutcome {
            if self.stuck_poll {
                std::future::pending::<()>().await;
            }
            if let Ok(e) = self.rx.try_recv() {
                self.outstanding -= 1;
                return PollOutcome::Event(e);
            }
            if !block {
                return PollOutcome::Empty;
            }
            if self.outstanding == 0 {
                return PollOutcome::NoTasks;
            }
            let e = self.rx.recv().await.unwrap();
            self.outstanding -= 1;
            PollOutcome::Event(e)
        }

        fn sync_syscall(&mut self, op: &str, _args: JsonValue) -> JsonValue {
            json!({"ok": format!("sync:{op}")})
        }

        fn log(&mut self, level: i32, message: String) {
            self.shared.lock().logs.push((level, message));
        }

        fn outstanding_tasks(&self) -> usize {
            self.outstanding
        }
    }

    // ---- fake worker -----------------------------------------------------

    struct FakeProc {
        killed: AtomicBool,
        exit: Mutex<Option<ExitInfo>>,
        /// Closed on kill so a blocked fake worker thread wakes up.
        sock: StdUnixStream,
    }

    #[async_trait]
    impl WorkerProc for FakeProc {
        fn kill(&self) {
            self.killed.store(true, Ordering::SeqCst);
            let _ = self.sock.shutdown(std::net::Shutdown::Both);
        }

        async fn wait(&self, grace: Duration) -> Option<ExitInfo> {
            let deadline = Instant::now() + grace;
            loop {
                if self.killed.load(Ordering::SeqCst) {
                    return Some(ExitInfo {
                        signal: Some(libc::SIGKILL),
                        ..Default::default()
                    });
                }
                if let Some(e) = self.exit.lock().clone() {
                    return Some(e);
                }
                if Instant::now() > deadline {
                    return None;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        }
    }

    struct FakeWorker {
        io: WorkerIo,
        proc_: Arc<FakeProc>,
    }

    impl PumpWorker for FakeWorker {
        fn split(&mut self) -> (&mut WorkerIo, &dyn WorkerProc) {
            (&mut self.io, &*self.proc_)
        }
    }

    /// The worker's side, handed to the scripted thread.
    struct Side {
        sock: StdUnixStream,
        stdout: StdUnixStream,
        stderr: StdUnixStream,
        status: StdUnixStream,
        proc_: Arc<FakeProc>,
    }

    impl Drop for Side {
        fn drop(&mut self) {
            // The proc keeps a clone of the socket; close it for real.
            let _ = self.sock.shutdown(std::net::Shutdown::Both);
        }
    }

    impl Side {
        fn recv(&mut self) -> Frame {
            proto::read_frame(&mut self.sock, 1 << 26).unwrap().unwrap()
        }
        fn send(&mut self, f: Frame) {
            proto::write_frame(&mut self.sock, &f).unwrap();
        }
        fn result(&mut self, call_id: u64, envelope: &str) {
            self.send(Frame::Result {
                call_id,
                guest_us: 7,
                envelope_json: envelope.as_bytes().to_vec(),
            });
        }
        fn submit(&mut self, v: JsonValue) {
            self.send(Frame::Submit {
                request_json: v.to_string().into_bytes(),
            });
        }
        fn recv_call(&mut self) -> u64 {
            match self.recv() {
                Frame::Call { call_id, .. } => call_id,
                f => panic!("expected Call, got {f:?}"),
            }
        }
        fn recv_event(&mut self) -> JsonValue {
            match self.recv() {
                Frame::Event { event_json } => serde_json::from_slice(&event_json).unwrap(),
                f => panic!("expected Event, got {f:?}"),
            }
        }
    }

    fn fd_pair() -> (StdUnixStream, PipeReader) {
        let (w, r) = StdUnixStream::pair().unwrap();
        (w, PipeReader::new(OwnedFd::from(r)).unwrap())
    }

    fn fake_worker<F: FnOnce(Side) + Send + 'static>(
        script: F,
    ) -> (FakeWorker, JoinHandle<()>, Arc<FakeProc>) {
        let (host_sock, w_sock) = StdUnixStream::pair().unwrap();
        host_sock.set_nonblocking(true).unwrap();
        let host_sock = UnixStream::from_std(host_sock).unwrap();
        let (so, so_r) = fd_pair();
        let (se, se_r) = fd_pair();
        let (st, st_r) = fd_pair();
        let proc_ = Arc::new(FakeProc {
            killed: AtomicBool::new(false),
            exit: Mutex::new(None),
            sock: w_sock.try_clone().unwrap(),
        });
        let side = Side {
            sock: w_sock,
            stdout: so,
            stderr: se,
            status: st,
            proc_: proc_.clone(),
        };
        let h = std::thread::spawn(move || script(side));
        (
            FakeWorker {
                io: WorkerIo::new(host_sock, Some(so_r), Some(se_r), Some(st_r)),
                proc_: proc_.clone(),
            },
            h,
            proc_,
        )
    }

    fn spec(timeout_ms: u64) -> CallSpec {
        CallSpec::new(1, "m:f", b"[]".to_vec(), Duration::from_millis(timeout_ms))
    }

    async fn run_with<F: FnOnce(Side) + Send + 'static>(
        spec: CallSpec,
        script: F,
    ) -> (PumpReport, Arc<Mutex<Shared>>, bool) {
        let (backend, shared) = FakeBackend::new();
        let (mut w, h, proc_) = fake_worker(script);
        let report = CallPump::new(spec, backend).run(&mut w).await;
        let killed = proc_.killed.load(Ordering::SeqCst);
        // Let the script finish (kill closes its socket).
        proc_.kill();
        h.join().unwrap();
        (report, shared, killed)
    }

    // ---- tests -----------------------------------------------------------

    #[tokio::test]
    async fn happy_path_sends_call_and_returns_envelope() {
        let (r, _, killed) = run_with(spec(5000), |mut w| {
            let f = w.recv();
            let Frame::Call {
                call_id,
                limits_json,
                export,
                args_json,
            } = f
            else {
                panic!()
            };
            assert_eq!((call_id, export.as_str(), &args_json[..]), (1, "m:f", &b"[]"[..]));
            let limits: JsonValue = serde_json::from_slice(&limits_json).unwrap();
            assert_eq!(limits["maxOutstandingTasks"], 1000);
            w.result(1, r#"{"ok":1}"#);
        })
        .await;
        let out = r.outcome.unwrap();
        assert_eq!(out.envelope_json, br#"{"ok":1}"#);
        assert_eq!(out.guest_us, 7);
        assert!(r.phases.total >= r.phases.guest);
        assert!(!killed);
    }

    #[tokio::test]
    async fn events_are_pushed_eagerly_in_completion_order() {
        let (r, _, _) = run_with(spec(5000), |mut w| {
            w.recv_call();
            w.submit(json!({"type":"sleep","ms":60}));
            w.submit(json!({"type":"sleep","ms":5}));
            // No Poll is sent: events must arrive on their own.
            let first = w.recv_event();
            let second = w.recv_event();
            assert_eq!(first["taskId"], 1);
            assert_eq!(second["taskId"], 0);
            w.result(1, "{}");
        })
        .await;
        r.outcome.unwrap();
    }

    #[tokio::test]
    async fn poll_with_nothing_outstanding_gets_no_tasks_with_same_seq() {
        let (r, _, _) = run_with(spec(5000), |mut w| {
            w.recv_call();
            w.send(Frame::Poll { seq: 1 });
            assert_eq!(w.recv(), Frame::NoTasks { seq: 1 });
            w.submit(json!({"type":"sleep","ms":10}));
            w.send(Frame::Poll { seq: 2 });
            assert_eq!(w.recv_event()["taskId"], 0);
            w.send(Frame::Poll { seq: 3 });
            assert_eq!(w.recv(), Frame::NoTasks { seq: 3 });
            w.result(1, "{}");
        })
        .await;
        r.outcome.unwrap();
    }

    #[tokio::test]
    async fn answered_poll_does_not_produce_a_stale_no_tasks() {
        let (r, _, _) = run_with(spec(5000), |mut w| {
            w.recv_call();
            w.submit(json!({"type":"sleep","ms":20}));
            w.send(Frame::Poll { seq: 1 });
            // Answered by the event, so a later NoTasks must carry seq 2 and
            // nothing may arrive in between.
            assert_eq!(w.recv_event()["taskId"], 0);
            w.send(Frame::Poll { seq: 2 });
            assert_eq!(w.recv(), Frame::NoTasks { seq: 2 });
            w.result(1, "{}");
        })
        .await;
        r.outcome.unwrap();
    }

    #[tokio::test]
    async fn rejected_submit_becomes_done_error_with_the_same_id() {
        let (r, shared, _) = run_with(spec(5000), |mut w| {
            w.recv_call();
            w.submit(json!({"type":"bad"}));
            let ev = w.recv_event();
            assert_eq!(ev["type"], "done");
            assert_eq!(ev["taskId"], 0);
            assert_eq!(
                ev["error"]["message"],
                "host rejected the task request: bad request"
            );
            // Unparsable frame: still consumes id 1.
            w.send(Frame::Submit {
                request_json: b"not json".to_vec(),
            });
            assert_eq!(w.recv_event()["taskId"], 1);
            w.submit(json!({"type":"sleep","ms":1}));
            assert_eq!(w.recv_event()["taskId"], 2);
            w.result(1, "{}");
        })
        .await;
        r.outcome.unwrap();
        assert!(shared.lock().submitted[1].is_null());
    }

    #[tokio::test]
    async fn id_desync_is_a_protocol_error_and_kills_the_worker() {
        let (backend, _) = {
            let (mut b, s) = FakeBackend::new();
            b.skip_id_on_reject = true; // old ActionBackend behaviour
            (b, s)
        };
        let (mut w, h, proc_) = fake_worker(|mut w| {
            w.recv_call();
            w.submit(json!({"type":"bad"}));
            w.recv_event();
            w.submit(json!({"type":"sleep","ms":1}));
            let _ = proto::read_frame(&mut w.sock, 1 << 20);
        });
        let r = CallPump::new(spec(5000), backend).run(&mut w).await;
        assert!(matches!(r.outcome, Err(NativeFailure::Protocol(m)) if m.contains("desync")));
        assert!(proc_.killed.load(Ordering::SeqCst));
        h.join().unwrap();
    }

    #[tokio::test]
    async fn sync_round_trip() {
        let (r, _, _) = run_with(spec(5000), |mut w| {
            w.recv_call();
            w.send(Frame::Sync {
                op: "now".into(),
                args_json: b"{}".to_vec(),
            });
            let Frame::SyncResult { result_json } = w.recv() else {
                panic!()
            };
            assert_eq!(result_json, br#"{"ok":"sync:now"}"#);
            w.result(1, "{}");
        })
        .await;
        r.outcome.unwrap();
    }

    #[tokio::test]
    async fn log_flood_hits_the_cap() {
        let mut s = spec(5000);
        s.log_capacity = 1000;
        let (r, shared, _) = run_with(s, |mut w| {
            w.recv_call();
            for _ in 0..100 {
                w.send(Frame::Log {
                    level: 1,
                    message: vec![b'x'; 100],
                });
            }
            w.send(Frame::Log {
                level: 2,
                message: b"warn".to_vec(),
            });
            w.result(1, "{}");
        })
        .await;
        r.outcome.unwrap();
        let logs = shared.lock().logs.clone();
        let normal = logs.iter().filter(|(l, m)| *l == 1 && m.len() == 100).count();
        assert_eq!(normal, 10);
        assert_eq!(logs.iter().filter(|(_, m)| m == LOG_TRUNCATED).count(), 1);
        assert!(!logs.iter().any(|(_, m)| m == "warn"));
    }

    #[tokio::test]
    async fn log_message_is_truncated_to_the_limit() {
        let mut s = spec(5000);
        s.limits.max_log_message_bytes = 8;
        let (r, shared, _) = run_with(s, |mut w| {
            w.recv_call();
            w.send(Frame::Log {
                level: 0,
                message: "0123456789".as_bytes().to_vec(),
            });
            w.result(1, "{}");
        })
        .await;
        r.outcome.unwrap();
        assert_eq!(shared.lock().logs, vec![(0, "01234567".to_string())]);
    }

    #[tokio::test]
    async fn stdout_and_stderr_are_split_into_lines() {
        let (r, shared, _) = run_with(spec(5000), |mut w| {
            w.recv_call();
            w.stdout.write_all(b"a\nb\npart").unwrap();
            w.stderr.write_all(b"e1\ne2").unwrap();
            // Result may be read before the pipes: they must be drained.
            w.result(1, "{}");
        })
        .await;
        r.outcome.unwrap();
        let logs = shared.lock().logs.clone();
        let of = |lvl: i32| -> Vec<String> {
            logs.iter()
                .filter(|(l, _)| *l == lvl)
                .map(|(_, m)| m.clone())
                .collect()
        };
        assert_eq!(of(1), vec!["a", "b", "part"]);
        assert_eq!(of(3), vec!["e1", "e2"]);
    }

    #[tokio::test]
    async fn stdout_is_capped() {
        let mut s = spec(5000);
        s.stdio_capacity = 100;
        let (r, shared, _) = run_with(s, |mut w| {
            w.recv_call();
            for _ in 0..50 {
                w.stdout.write_all(b"0123456789\n").unwrap();
            }
            w.result(1, "{}");
        })
        .await;
        r.outcome.unwrap();
        let logs = shared.lock().logs.clone();
        let lines = logs.iter().filter(|(_, m)| m == "0123456789").count();
        assert_eq!(lines, 9); // 100 / 11 bytes per line
        assert_eq!(logs.iter().filter(|(_, m)| m == STDIO_TRUNCATED).count(), 1);
    }

    #[tokio::test]
    async fn timeout_kills_the_worker() {
        let (r, _, killed) = run_with(spec(100), |mut w| {
            w.recv_call();
            // Hang until killed (socket shutdown wakes this up).
            let _ = proto::read_frame(&mut w.sock, 1 << 20);
        })
        .await;
        assert!(matches!(r.outcome, Err(NativeFailure::Timeout)));
        assert!(killed);
    }

    /// N-T2/N-C3: a host op that never finishes must not run past the
    /// call deadline (it is awaited outside the pump's `select!`).
    #[tokio::test]
    async fn timeout_bounds_a_stuck_host_op() {
        let (mut backend, _) = FakeBackend::new();
        backend.stuck_poll = true;
        let (mut w, h, proc_) = fake_worker(|mut w| {
            w.recv_call();
            w.send(Frame::Poll { seq: 1 });
            let _ = proto::read_frame(&mut w.sock, 1 << 20);
        });
        let r = tokio::time::timeout(
            Duration::from_secs(10),
            CallPump::new(spec(100), backend).run(&mut w),
        )
        .await
        .expect("the pump honours the deadline");
        assert!(matches!(r.outcome, Err(NativeFailure::Timeout)), "{:?}", r.outcome);
        assert!(proc_.killed.load(Ordering::SeqCst));
        h.join().unwrap();
    }

    #[tokio::test]
    async fn dropping_the_run_future_kills_the_worker() {
        let (backend, _) = FakeBackend::new();
        let (mut w, h, proc_) = fake_worker(|mut w| {
            w.recv_call();
            let _ = proto::read_frame(&mut w.sock, 1 << 20);
        });
        let res = tokio::time::timeout(
            Duration::from_millis(100),
            CallPump::new(spec(60_000), backend).run(&mut w),
        )
        .await;
        assert!(res.is_err());
        assert!(proc_.killed.load(Ordering::SeqCst));
        h.join().unwrap();
    }

    #[tokio::test]
    async fn dead_worker_is_classified_from_status_and_exit() {
        // Oom note.
        let (r, _, _) = run_with(spec(5000), |mut w| {
            w.recv_call();
            let mut rec = [0u8; 64];
            rec[0] = 1;
            rec[1..9].copy_from_slice(&(512u64 << 20).to_le_bytes());
            w.status.write_all(&rec).unwrap();
            *w.proc_.exit.lock() = Some(ExitInfo {
                code: Some(137),
                ..Default::default()
            });
            // dropping Side closes the socket
        })
        .await;
        assert!(
            matches!(r.outcome, Err(NativeFailure::OutOfMemory { limit }) if limit == 512 << 20),
            "{:?}",
            r.outcome
        );

        // Plain signal with stderr forwarded as level-3 logs.
        let (r, shared, _) = run_with(spec(5000), |mut w| {
            w.recv_call();
            w.stderr.write_all(b"boom\n").unwrap();
            *w.proc_.exit.lock() = Some(ExitInfo {
                signal: Some(libc::SIGSEGV),
                ..Default::default()
            });
        })
        .await;
        assert!(
            matches!(r.outcome, Err(NativeFailure::Crashed(m)) if m.contains("SIGSEGV"))
        );
        assert_eq!(shared.lock().logs, vec![(3, "boom".to_string())]);

        // Exit code without a Result.
        let (r, _, _) = run_with(spec(5000), |mut w| {
            w.recv_call();
            *w.proc_.exit.lock() = Some(ExitInfo {
                code: Some(3),
                ..Default::default()
            });
        })
        .await;
        assert!(
            matches!(r.outcome, Err(NativeFailure::Crashed(m)) if m.contains("exit status 3"))
        );
    }

    #[tokio::test]
    async fn oversize_result_frame_is_result_too_large() {
        let mut s = spec(5000);
        s.limits.max_result_bytes = 100;
        s.limits.max_task_request_bytes = 100;
        let (r, _, killed) = run_with(s, |mut w| {
            w.recv_call();
            // Frame limit is 100 + 64 KiB; send a 200 KiB envelope.
            // The pump may close the socket mid-write.
            let _ = proto::write_frame(
                &mut w.sock,
                &Frame::Result {
                    call_id: 1,
                    guest_us: 0,
                    envelope_json: vec![b' '; 200 * 1024],
                },
            );
        })
        .await;
        assert!(matches!(
            r.outcome,
            Err(NativeFailure::ResultTooLarge { max: 100, .. })
        ));
        assert!(killed);
    }

    #[tokio::test]
    async fn result_over_the_limit_but_inside_the_frame_slack_is_rejected() {
        let mut s = spec(5000);
        s.limits.max_result_bytes = 100;
        let (r, _, _) = run_with(s, |mut w| {
            w.recv_call();
            w.result(1, &" ".repeat(500));
        })
        .await;
        assert!(matches!(
            r.outcome,
            Err(NativeFailure::ResultTooLarge { len: 500, max: 100 })
        ));
    }

    #[tokio::test]
    async fn wrong_call_id_and_unexpected_frames_are_protocol_errors() {
        let (r, _, _) = run_with(spec(5000), |mut w| {
            w.recv_call();
            w.result(99, "{}");
        })
        .await;
        assert!(matches!(r.outcome, Err(NativeFailure::Protocol(_))));
        let (r, _, _) = run_with(spec(5000), |mut w| {
            w.recv_call();
            w.send(Frame::Shutdown);
        })
        .await;
        assert!(matches!(r.outcome, Err(NativeFailure::Protocol(_))));
    }

    #[test]
    fn classification_table() {
        let c = |notes: &[StatusNote], exit: ExitInfo, tail: &str| {
            classify(&Observation {
                notes,
                exit: Some(&exit),
                stderr_tail: tail,
                memory_bytes: 1 << 29,
                stack_bytes: 8 << 20,
            })
        };
        let sig = |s| ExitInfo {
            signal: Some(s),
            ..Default::default()
        };
        let code = |n| ExitInfo {
            code: Some(n),
            ..Default::default()
        };
        assert!(matches!(
            c(&[StatusNote::Oom { limit: 5, requested: 6 }], code(137), ""),
            NativeFailure::OutOfMemory { limit: 5 }
        ));
        assert!(matches!(
            c(&[], code(137), ""),
            NativeFailure::OutOfMemory { limit } if limit == 1 << 29
        ));
        assert!(matches!(
            c(&[], ExitInfo { oom_killed: true, ..Default::default() }, ""),
            NativeFailure::OutOfMemory { .. }
        ));
        assert!(matches!(
            c(&[], sig(libc::SIGABRT), "memory allocation of 99 bytes failed"),
            NativeFailure::OutOfMemory { .. }
        ));
        assert!(matches!(
            c(&[StatusNote::StackOverflow], sig(libc::SIGSEGV), ""),
            NativeFailure::StackOverflow { stack } if stack == 8 << 20
        ));
        assert!(matches!(
            c(&[], sig(libc::SIGSEGV), "thread 'x' has overflowed its stack"),
            NativeFailure::StackOverflow { .. }
        ));
        assert!(matches!(
            c(&[], sig(libc::SIGABRT), "thread 'x' has overflowed its stack"),
            NativeFailure::StackOverflow { .. }
        ));
        match c(&[StatusNote::SigSys { nr: 41 }], code(159), "") {
            NativeFailure::Crashed(m) => assert_eq!(
                m,
                "Rust action was stopped by the sandbox (disallowed system call socket, 41)"
            ),
            f => panic!("{f:?}"),
        }
        match c(&[StatusNote::SigSys { nr: 9999 }], code(159), "") {
            NativeFailure::Crashed(m) => assert!(m.ends_with("system call 9999)")),
            f => panic!("{f:?}"),
        }
        match c(&[], sig(libc::SIGBUS), "") {
            NativeFailure::Crashed(m) => {
                assert_eq!(m, "Rust action crashed the worker process (signal SIGBUS)")
            },
            f => panic!("{f:?}"),
        }
        match c(&[], code(2), "") {
            NativeFailure::Crashed(m) => {
                assert_eq!(m, "Rust action exited the worker process (exit status 2)")
            },
            f => panic!("{f:?}"),
        }
        assert!(matches!(
            c(&[StatusNote::InitFailed("dlopen: nope".into())], code(70), ""),
            NativeFailure::StartFailed(m) if m == "dlopen: nope"
        ));
    }

    #[test]
    fn status_records_parse() {
        let mut data = vec![0u8; 64 * 2 + 10];
        data[0] = 2;
        data[1..5].copy_from_slice(&59u32.to_le_bytes());
        data[64] = 4;
        data[65..70].copy_from_slice(b"hello");
        assert_eq!(
            parse_status(&data),
            vec![
                StatusNote::SigSys { nr: 59 },
                StatusNote::InitFailed("hello".into())
            ]
        );
        let _ = VecDeque::<u8>::new();
    }
}
