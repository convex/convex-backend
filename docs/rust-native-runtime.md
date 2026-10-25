# Native Rust worker for self-hosted Convex: design

Status: proposal, implemented behind an off-by-default knob. Audience: Convex maintainers reviewing the
accompanying pull request.

This builds on a "use rust" action runtime for the self-hosted backend, which runs Rust actions as WebAssembly
inside the backend (the first commit of this pull request). The wasm runtime is
portable and sandboxed, but it cannot use real threads, it cannot link C libraries that do not target wasm, and
it pays for the wasm boundary on every call. This document describes an optional second runtime that runs the
same actions as native machine code in a separate, sandboxed worker process.

## 1. Goals and non-goals

Goals

- Run unmodified `#[action]` Rust code as a native shared library, with the same JSON payloads, result envelope
  and error messages as the wasm runtime.
- Keep the backend safe from a misbehaving action: a crash, an out-of-memory condition or an infinite loop
  affects one call, not the backend process.
- Add very little to files that upstream owns, so the change rebases easily.
- Change nothing when the feature is off, which is the default.

Non-goals

- Multi-tenant or untrusted code. Native mode is for trusted, single-tenant, self-hosted deployments (see the
  trust model below). Convex Cloud would never enable it.
- Queries and mutations (as with wasm, only actions run Rust).
- Windows, or musl-based hosts.

## 2. Decisions

| Decision | Reason |
|---|---|
| The user crate stays a `cdylib` loaded with `dlopen` by a generic worker, in a separate process. | No wrapper crate and no linker registration tricks. The dlopen hazards (no unload, thread-local destructors) disappear because the process is recycled instead of unloaded. |
| The worker is the backend binary itself, re-executed with a hidden first argument. | No packaging change (the Docker image builds one binary). Host and worker are always the same build, so the IPC protocol has no cross-version skew. |
| One call at a time per worker, with a warm pool keyed by (artifact hash, environment hash). | `std::env` is exact, memory accounting is exact, stdout attribution is exact, and a crash has a blast radius of one call. |
| A Unix socketpair carries length-prefixed binary frames of raw JSON. stdout and stderr are separate pipes. A fourth pipe carries fatal status notes. | Portable to Linux and macOS (`SOCK_SEQPACKET` is not available on macOS). |
| Guest ABI is the wasm ABI with native pointers and a host function table. | The SDK executor, the host `ActionBackend`, the result envelope and error mapping are reused unchanged. |
| Timeouts are a host-side `SIGKILL`. Memory is limited by a counting allocator, an RSS watchdog and an rlimit or cgroup backstop. | Real preemption, which in-process native code cannot have. Error strings match the wasm runtime. |
| The artifact travels in the existing module-source channel (one "carrier" module with a header and base64 payload, other modules hold small references). | No change to the push protocol. It fits under the existing push size limit. |
| Off by default (`RUST_NATIVE_MODE=off`); the CLI defaults to wasm. | Trusted deployments only. |
| The wasm runtime is untouched. The runtime is chosen per push, for the whole crate. | Zero behaviour change while the mode is off. |

Rejected alternative: link the user crate into its own executable through a generated wrapper crate. It would
allow static musl binaries, but needs a registration mechanism, matching SDK versions and an IPC client inside
every user binary, with protocol skew between that binary and the backend. It remains an open question for
distroless images.

## 3. Architecture

```
backend process (tokio)                               worker process (same binary, argv[1] = __rust_native_worker)
+----------------------------------------+            +----------------------------------------------+
| RustActionRuntime::execute_action      |            | main thread: watchdog (RSS, parent, flush)   |
|  -> native runtime ---- acquire ---->  |  fd 3      | call thread (configurable stack)             |
|     worker pool [(sha, env hash)]      |<==========>|   loop { read Call -> exported function      |
|     call pump (one tokio task / call)  |  fd 1,2    |          host function table: submit/poll/   |
|       host backend = ActionBackend     |<--pipes----|          sync/log -> frames; write Result }   |
|       task executor (unchanged)        |  fd 4      | user cdylib (panic=unwind, its own std)      |
+----------------------------------------+<-----------+----------------------------------------------+
```

### 3.1 Where it plugs into the backend

The existing call path is: dispatch, `IsolateClient::execute_rust_action`, `RustActionRuntime::execute_action`,
metadata lookup, module source load, compile, argument preparation, environment load, task executor spawn,
`ActionBackend` creation, the call, result mapping. Native mode branches after the metadata lookup: if the
module hash has a cached native artifact, module source loading is skipped; otherwise the source is loaded,
and if it carries the native marker the artifact is decoded, written to disk and cached. Argument
preparation, environment, executor and backend creation, and result mapping are shared with wasm. The native
call returns the same output type as the wasm call, with three new failure variants (out of memory, stack
overflow, crashed).

### 3.2 Lifecycle

Spawn. Workers are spawned from one dedicated long-lived thread (on Linux the parent-death signal fires when
the forking thread exits, and tokio blocking threads exit when idle). The command is the backend's own
executable with worker arguments, a cleared environment holding only the deployment environment variables,
stdin from `/dev/null`, `kill_on_drop`, and `process_group(0)` so the whole group can be killed. There is no
`pre_exec` hook: the worker applies the hardening below itself, first thing after it starts and before it loads
the user library (`sandbox::enter`). `pre_exec` was dropped because it forces a `fork` path (40 to 48 ms
measured, against about 10 ms for `posix_spawn`).

Handshake. The worker sends `Ready` (protocol version, pid, resolved exports, init time, RSS). The host waits
up to a configurable timeout (default 5 s). No `Ready`, a protocol mismatch or an early exit is a spawn failure.

Warm pool. One pool per (artifact hash, environment hash). Acquire pops an idle worker or spawns one. On
release, a worker that finished cleanly and has served fewer than the recycle threshold (default 1000 calls)
goes back to idle; otherwise it is killed. That includes timeout, crash, caught panic, protocol error, and RSS
above a threshold. After a call the pool refills to a minimum idle count in the background. An idle reaper
kills workers idle past a timeout (default 300 s) and caps total idle workers (default 4).

Concurrency. The existing action concurrency semaphore bounds busy workers. Inside a worker exactly one call
runs at a time; user threads (for example rayon) share that call's budget.

Failures and backoff. A crash during a call fails that call and discards the worker; there is no breaker for
it, so one deterministic crash in one function cannot block others. Spawn failures (exec, dlopen, `Ready`
timeout, missing exports) feed a per-pool breaker: three failures in 60 s open it, calls then fail fast with
the last spawn error, and backoff doubles from 1 s to a cap of 300 s.

Shutdown. Dropping the runtime sends `Shutdown`, waits 500 ms, then kills. If the backend is killed: Linux
`PR_SET_PDEATHSIG`; on all platforms the worker exits when its parent pid changes or when the socket reaches
EOF while idle. If the `execute_action` future is dropped, the call pump drops and the worker is killed, so no
call outlives its request.

## 4. Request flow

Function names below are the ones in the code. The sketches show the successful path first and the failure
branches after it.

### 4.1 Push time

```
developer        CLI (npm-packages/convex)                          backend
   |  npx convex deploy / dev
   |---------------->|
   |                 | resolveRustRuntime: --rust-runtime, else convex.json, else wasm
   |                 | bundleRustNative
   |                 |--- GET /api/rust_native/info ------------------->| enabled? host triple, glibc
   |                 |<------------------------------------------------|
   |                 | selectNativeBuild -> nativeCargoCommand: cargo build of the cdylib
   |                 |   (panic=unwind; cargo-zigbuild or Docker when the target differs from the host)
   |                 | checkNativeSize: warn at 100 MB, error at 180 MB
   |                 | parseNativeManifest: reads the convex_mf:* symbols from the ELF / Mach-O file
   |                 | nativeModuleConfigs: carrierSource (marker line + JSON header + base64 library)
   |                 |   for the first module, refSource (sha256 + carrier path) for every other one
   |                 |--- push: modules rust/<module>.js -------------->|
   |                 |                                                  | ApplicationFunctionRunner::analyze
   |                 |                                                  |  rust_analyze::analyze_into
   |                 |                                                  |   analyze_rust_modules
   |                 |                                                  |    native::analyze_native_module
   |                 |                                                  |     analyze_carrier / parse_native
   |                 |                                                  |      parse_elf | parse_macho
   |                 |                                                  |  (no guest code runs)
   |                 |<-- accepted: AnalyzedModule per module ----------|  same structure wasm produces
   |                 |<-- rejected: JsError with the message below -----|
```

The analyzer runs these checks in order and stops at the first failure (step 8 applies to reference modules and
runs before the carrier they name is analyzed):

| Step | Check | Source of the limit |
|---|---|---|
| 1 | The knob is on (`RUST_NATIVE_MODE=worker`). | `common::knobs` |
| 2 | Carrier header: `format`, `nativeAbi` and `abi` all 1. | `analyze_carrier` |
| 3 | Decoded size is at most `RUST_NATIVE_MAX_ARTIFACT_BYTES` (default 180 MiB) and equals the header size; sha256 equals the header. | `analyze_carrier` |
| 4 | 64-bit little-endian ELF, `ET_DYN`, has `PT_DYNAMIC`, `e_machine` matches the backend architecture (Mach-O: dylib for the backend architecture). | `parse_elf`, `parse_macho` |
| 5 | Every `DT_NEEDED` is in an allowlist: `libc.so.6`, `libm.so.6`, `libgcc_s.so.1`, `libpthread.so.0`, `libdl.so.2`, `librt.so.1`, `ld-linux*.so`, plus `RUST_NATIVE_EXTRA_LIBS`. Mach-O has its own dylib and framework allowlist. | `parse_elf`, `parse_macho` |
| 6 | The highest `GLIBC_x.y` in `.gnu.version_r` is no newer than the backend's glibc. | `elf_glibc_max` |
| 7 | The library exports `convex_native_init`, `convex_alloc`, `convex_free_result` and every function export named in the header; the function map built from the `convex_mf:*` symbols equals the header's map. | `analyze_carrier` |
| 8 | A reference module names a carrier that is in the same push, with the same sha256. | `analyze_native_module` |

A rejection fails the push with the analyzer's message. Examples of the text (the CLI's own formatting around it is
not shown here):

- Knob off: `This backend does not run native Rust actions (RUST_NATIVE_MODE is off). Push with --rust-runtime wasm, or enable it.`
- glibc too new: `` `rust/<module>.js`: the Rust library needs glibc <max> but the backend has <have>; rebuild with --target <triple>.<glibc> ``
- Library dependency: `` `rust/<module>.js`: the native library depends on the shared library `<name>`, which is not allowed (allowed: ...; extend with RUST_NATIVE_EXTRA_LIBS) ``

Before pushing, the CLI asks the backend for `/api/rust_native/info`. With `--rust-runtime native` and the mode off
it stops with `Cannot use --rust-runtime native: this backend does not run native Rust actions (RUST_NATIVE_MODE is off).`;
with `auto` it falls back to wasm and prints a note.

### 4.2 Call time

```
IsolateClient::execute_action_or_rust
  -> execute_rust_action
  -> RustActionRuntime::execute_action
       resolve_engine
         NativeRuntime::cached(module hash) ........ hit: Engine::Native, source is not loaded
         miss: load the module source
               starts with //convex-rust-native ....... load_native: decode, write <RUST_NATIVE_DIR>/bin/<sha>.so,
                                                         cache; then NativeRuntime::prewarm
               knob off ............................... developer error (MODE_OFF_MESSAGE), nothing runs
       (shared with wasm: args, environment, task executor, ActionBackend)
       NativeRuntime::call(artifact, export, args, env, backend, timeout)
         limiter.acquire                                  existing action concurrency bound
         Pool::acquire(artifact, env hash, plan)
           idle worker for (sha, env hash)? ............ lease it
           else spawn, on the "rust-native-spawner" thread (NativeSpawner::spawn_inner):
             host    check_support; create_scratch; socketpair + status pipe
                     Command: backend's own exe, env_clear + deployment env, stdin /dev/null,
                     process_group(0), spawn
             worker  worker_main: install_signal_handlers
                     -> sandbox::enter  (fds to 3/4, PDEATHSIG, no_new_privs, rlimits, [cgroup go byte],
                                         Landlock, uid/gid drop, seccomp)
                     -> LibGuest::open  (dlopen; needs the Applied token that only enter returns)
                     -> convex_native_init(vtable), call thread started, Ready sent
             host    handshake: waits for Ready (RUST_NATIVE_SPAWN_TIMEOUT_MS)
         CallPump::new(spec, backend).run(worker)
           host -> Call { id, limits, export, args }
           worker -> Submit / Poll / Sync / Log           (for example runQuery and runMutation as
                                                            syscall tasks, fetch, log lines)
           host: ActionBackend::submit / poll / sync_syscall / log   (unchanged)
           host -> Event / NoTasks / SyncResult
           worker -> Result { envelope }
         lease.release_clean_after      worker back to idle (or killed past the recycle thresholds)
         lease.release_recycle          guest panic envelope: worker killed
         lease.release_failed           any failure below: worker killed
       map GuestResult / CallFailure to ActionOutcome (same messages as wasm)
```

Failure branches inside `CallPump::run`:

| Event | Detection | Result the caller sees |
|---|---|---|
| Timeout | The pump's deadline fires. `kill()` sends `SIGKILL` to the worker's process group. | `Function execution timed out (maximum duration: ...)` |
| Out of memory | Worker watchdog sees RSS above the limit plus slack and writes a status note; or the cgroup reports `oom_kill`; or exit status 137; or `SIGABRT` with an allocator message on stderr. | `Uncaught Error: Rust execution ran out of memory (maximum memory usage: ...)` |
| Stack overflow | Status note, or `SIGSEGV`/`SIGABRT` with `has overflowed its stack` on stderr. | `Uncaught Error: Rust action overflowed its stack (stack size: ...)` |
| Sandbox kill | `SIGSYS` handler writes a status note with the syscall number. | `Uncaught Error: Rust action was stopped by the sandbox (disallowed system call ...)` |
| Other crash | No `Result`; classified from the exit status. | `Uncaught Error: Rust action crashed the worker process (signal ...)` or `... exited the worker process (exit status N)` |
| Guest panic | A `Result` envelope with kind panic. | The usual panic error; the worker is recycled. |
| Frame protocol error or oversized frame | `proto` decoding in the pump. | Worker killed; `The "use rust" module broke the guest ABI: ...` |
| Spawn, `dlopen` or `Ready` failure | `Pool::acquire` returns `StartFailed`. | `Could not start the native Rust worker: ...`; counts toward the per-pool breaker |
| Request future dropped | The pump drops with it. | Worker killed. |

## 5. IPC protocol

Transport: fd 0 is `/dev/null`; fds 1 and 2 are pipes carrying user stdout/stderr; fd 3 is the socketpair;
fd 4 is a status pipe written with raw `write(2)` from signal handlers and the allocator (no allocation).

Frames: `u32 len (LE) | u8 kind | body`, with `len` bounded by the larger of the task-request and result limits
plus 64 KiB. An oversized frame is a protocol error and the worker is killed, except an oversized `Result`,
which becomes the same "result too large" error the wasm runtime produces; the length is checked before the
body is read.

| Kind | Direction | Name | Content |
|---|---|---|---|
| 0x01 | worker to host | `Ready` | protocol, pid, resolved exports, init microseconds, RSS |
| 0x02 | host to worker | `Call` | call id, limits JSON, export name, args JSON |
| 0x03 | worker to host | `Submit` | task request JSON (fire and forget; ids implicit) |
| 0x04 | host to worker | `Event` | task event JSON, byte-identical to the wasm poll text |
| 0x05 | worker to host | `Poll` | sequence number; sent only when the worker would block |
| 0x06 | host to worker | `NoTasks` | answers the `Poll` with the same sequence number |
| 0x07 | worker to host | `Sync` | operation name, args JSON; worker blocks for the reply |
| 0x08 | host to worker | `SyncResult` | result JSON |
| 0x09 | worker to host | `Log` | level, UTF-8 |
| 0x0A | worker to host | `Result` | call id, guest microseconds, result envelope JSON |
| 0x0B | host to worker | `Shutdown` | none |

Status pipe records are 64 bytes: a code (out of memory, disallowed syscall, stack overflow, init failed) and
detail. Host and worker are the same binary, so the numbering is internal and carries no compatibility promise;
the protocol version in `Ready` only guards against a stale binary on disk.

Differences from the wasm ABI:

- `task_submit`: the worker assigns ids itself (0, 1, 2, ...) and applies the size and outstanding-task limits
  locally, returning `-1` exactly as wasm does. Rejections that only the host can make (unknown task type,
  executor gone) come back later as a task `done` event with an error whose text matches the SDK's message for
  `-1`. To keep ids aligned, the host's `ActionBackend::submit` now consumes a task id before parsing the
  request, so every `Submit` frame consumes exactly one host id (gaps are harmless; ids are map keys).
- `task_poll(0)` drains the socket without blocking. `task_poll(1)` returns a queued event if there is one,
  otherwise flushes writes, sends `Poll`, and blocks until an `Event` or the matching `NoTasks`.
- Events are pushed eagerly while the guest runs, so a later poll usually finds its event already buffered.
- stdout and stderr are real pipes. The host splits lines and logs them at the matching levels with the same cap
  as wasm. Ordering between pipe output and `Log` frames is not guaranteed.
- The process environment equals the deployment environment, so `std::env` behaves normally.
- A process serves many calls, so `static` and `thread_local` values persist across calls (a knob sets the
  recycle threshold to 1 for strict wasm parity). A worker is always discarded after a panic or timeout.

Native guest ABI (the library exports): `convex_native_init(vtable) -> abi version`, `convex_alloc`,
`convex_free_result(handle)`, one `convex_run:<module>:<name>(args_ptr, args_len) -> handle` per function (the
handle points to a boxed buffer holding the result envelope), `convex_mf:<module>:<name>` static byte arrays
holding the manifest, and the optional `convex_native_set_memory_limit(usize)` which the worker calls after
init.

The host runs one pump task per call. It selects over: a deadline (kill the worker, report timeout), frames
from the socket (submit, poll, sync, log, result, EOF classified below), task events from the backend (pushed to
the worker), and stdout/stderr lines. The backend poll it awaits is cancel-safe.

## 6. Sandbox and limits

Trust model. Native code is ordinary machine code. The sandbox reduces what a bug or malicious dependency can
reach, but the supported deployment is a backend whose operator controls every crate that gets pushed. It is
the same exposure class as the existing self-hosted `"use node"` executor, with a stronger sandbox, and it is
not a multi-tenant isolation boundary.

### Linux (glibc, for example the `ubuntu:noble` backend image)

The worker applies these in `sandbox::enter`, in order, before `dlopen`: move the frame socket and status pipe to
fds 3 and 4 and close every other fd; parent-death signal; `no_new_privs`; rlimits (core 0, nofile 256, fsize
1 GiB, and `RLIMIT_DATA` as a backstop sized from the memory limit and stacks; not `RLIMIT_AS`); when a cgroup is
configured, wait for the host to place the worker in it; build the Landlock ruleset; optionally drop uid/gid
(then `RLIMIT_NPROC` 4096 and `RLIMIT_CPU` 4 h are also set, if no cgroup is used); `landlock_restrict_self`;
install the seccomp-bpf filter. There are no network or IPC namespaces.

Landlock: read/execute on system library paths, read on TLS and zoneinfo paths and a few devices, read/write
only on a private temp dir, no access to the backend's data directory. With `RUST_NATIVE_ALLOW_NETWORK` and
Landlock ABI 4 or later, TCP connect is limited to ports 80 and 443 and bind is denied; UDP and other protocols
are not restricted by Landlock.

cgroup: the host only writes the worker's pid to the configured `cgroup.procs`. It does not set `memory.max`,
`pids.max` or group OOM kill; the operator prepares a delegated cgroup v2 subtree with those limits.

The seccomp filter is installed before the user library is loaded, so library constructors (`.init_array`) run
inside it; the filter allows what `dlopen` needs (`openat`, `mmap`, `mprotect`). `LibGuest::open` takes a
`sandbox::Applied` value that only `enter` returns, so the order is enforced by the compiler. The filter allows
I/O, memory, signal, futex, time, polling and thread-creating `clone` (`CLONE_THREAD` only); returns `ENOSYS`
for `clone3` so glibc falls back; returns `EPERM` for socket calls unless networking is allowed; and kills with
`SIGSYS`, after writing a status note, on `execve`, `fork`, `ptrace`, `mount`, `bpf`, `io_uring`, module loading,
and everything else.

Status: the Linux code compiles (cross build for `x86_64-unknown-linux-gnu`, clippy clean); the sandbox has not
yet been run end to end on a real Linux kernel, so the isolation checks in section 11 are pending there. In
particular, that `dlopen` and Rust start-up succeed under the real filter and Landlock ruleset is not yet
verified.

Levels (`RUST_NATIVE_SANDBOX`): `strict` refuses to spawn if Landlock or seccomp is unavailable; `best-effort`
(default on Linux) applies what a startup probe finds and logs the level once; `off` is environment clearing
plus rlimits. Level `off` is refused unless `RUST_NATIVE_ALLOW_UNSANDBOXED=true`.

### macOS (development only)

Cleared environment, a separate process group, core/nofile rlimits, the counting allocator, an RSS watchdog
(`phys_footprint`), host-side `SIGKILL` and an orphan check. No seccomp, Landlock or cgroups. The only sandbox
level is `off`, and the backend refuses to start native mode on it unless `RUST_NATIVE_ALLOW_UNSANDBOXED=true`
is set (a trusted local development backend). The host never overwrites a mapped library; it writes each artifact to a
new path keyed by hash and renames it into place, which avoids the "Code Signature Invalid" kill seen with
other tools.

### Limits and the errors users see

| Limit | Enforcement | Error |
|---|---|---|
| Action timeout (same knob as other actions) | Host deadline, `SIGKILL` of the process group | identical to the wasm runtime: `Function execution timed out (maximum duration: ...)` |
| Memory (default 512 MiB) | Counting allocator, RSS watchdog (5 ms lag), rlimit or cgroup | `Uncaught Error: Rust execution ran out of memory (maximum memory usage: 512 MiB)` |
| Result size, task request size, outstanding tasks | Worker-local checks plus host frame-length check | same as wasm |
| Log lines, stdout/stderr volume | Host, unchanged | unchanged |
| Concurrency | Existing semaphore | queueing, unchanged |

Crash classification (from status pipe, exit status, last stderr lines): out of memory; stack overflow
(`Rust action overflowed its stack`); sandbox kill (`stopped by the sandbox (disallowed system call ...)`);
other fatal signal (`crashed the worker process (signal ...)`); exit without result
(`exited the worker process (exit status N)`); timeout; panic (envelope, same text as wasm); protocol error;
spawn/dlopen/ready failure (`Could not start the native Rust worker: ...`, subject to the breaker).

## 7. Deploy and push flow

Capability discovery. `GET /api/rust_native/info` (admin key) returns whether native mode is enabled, the host
target (triple, os, arch, env, glibc), the sandbox that is actually active (seccomp, Landlock ABI, namespaces,
cgroup, uid drop) and live pool statistics. A 404 means the backend does not support it.

CLI build. `--rust-runtime wasm|native|auto` or `convex.json` `{"rust": {"runtime": ...}}`; default `wasm`;
`auto` picks native only if the backend reports it enabled and the target is buildable. If the backend target
equals the host target the CLI runs a plain `cargo build`; for Linux glibc targets from another host it uses
`cargo-zigbuild` with a glibc floor equal to the backend's (for example `aarch64-unknown-linux-gnu.2.39`), or a
pinned Docker build. The build forces `panic = "unwind"`, strips debuginfo and sets `--cfg convex_native`. The
CLI reads the function manifest straight from the built ELF or Mach-O dynamic symbols (`convex_mf:*`), so code
generation is shared with wasm.

Artifact encoding. The first module (sorted) is the carrier: a marker line, a JSON header (format, ABIs, triple,
glibc floor, sha256, size, function-to-export map, SDK version), then base64 of the library. Every other module
is a reference with the artifact sha256 and the carrier path. The environment stays `rust`. The CLI warns above
100 MB and errors above 180 MB, under the 200 MB push limit.

Push-time validation (no code is executed). The marker selects the native analyzer. It checks: the knob is on;
sha256 and size; ELF64 shared object (or Mach-O 64 dylib) for the backend's architecture; `DT_NEEDED` limited to
system libraries (extensible by knob); the highest required `GLIBC_x.y` is no newer than the host's; required
exports present; and that the manifest assembled from the `convex_mf:*` symbols produces the same function map
as the header. The result is the same analyzed-module structure wasm produces, so the dashboard, argument
validation and visibility are unchanged. A native-only push to a backend with the mode off is a push-time error,
never a silent switch.

Runtime artifact handling. A cache keyed by module metadata hash, plus one keyed by artifact hash to dedup
across modules. On a miss the carrier is decoded and written to `<RUST_NATIVE_DIR>/bin/<sha>.so` via a
temporary file, `fsync`, mode 0500 and `rename`; never overwritten. A startup check refuses a `noexec` mount.
Old artifacts are garbage collected when unreferenced and older than an hour (newest five kept).

Hot reload. A new push has a new hash and so a new pool; calls in flight finish on old workers (their mapped
file stays valid) and old pools idle out. The first call after a push pays the worker spawn cost.

## 8. Coexistence with wasm

Granularity is per push, for the whole crate. The source prefix selects the runtime and the choice is cached per
module hash (wasm sources are base64 and never begin with `//`). A push built with both artifacts can fall back
to wasm at runtime if native becomes unavailable (knob turned off, breaker open), with one warning per pool per
minute at most. Queries, mutations and components remain unsupported for Rust.

HTTP actions. `#[http_action]` functions run on both fork runtimes in a buffered form. The manifest entry has
`kind:"http"`; the analyzer (shared by wasm and native) registers it as an internal action, whatever visibility
the macro declared, and skips any other unknown kind with a warning. The CLI generates
`convex/_generated/rustHttp` with `rustHttpAction("<module>:<fn>")`: a V8 `httpAction` that reads the request,
calls the internal action with `{head, body (base64), buffered: true}` and builds the `Response` from the returned
`{status, headers, body}`. The native IPC protocol, worker and sandbox are unchanged: the call is an ordinary
action call. Limits: request body about 12 MiB (16 MiB argument limit), collected streaming response 20 MiB.
Streaming HTTP actions need the stock V8 host. This code is part of the wasm commit, not the native one.

## 9. Observability

Per-call timers (lookup, acquire by outcome, spawn, call, teardown), callback counters by kind, IPC byte
counters, worker gauges by state, worker exit counters by reason, spawn failures, breaker state and a worker
RSS histogram, registered with the backend's existing metrics macros. Guest `log`, stdout and stderr flow into
deployment function logs through the unchanged `ActionBackend::log` path; worker lifecycle goes to backend
`tracing` (spawn at info, crash at warn with classification and last stderr line, sandbox level once at
startup). Infrastructure lines never enter function logs.

## 10. Configuration

All are environment variables on the backend, defined in one block in `common/src/knobs.rs`:
`RUST_NATIVE_MODE` (`off`|`worker`), `_SANDBOX`, `_ALLOW_NETWORK`, `_DIR`, `_MEMORY_BYTES`, `_RSS_SLACK_BYTES`,
`_STACK_BYTES`, `_MIN_IDLE`, `_MAX_IDLE`, `_IDLE_TIMEOUT_SECS`, `_MAX_CALLS_PER_WORKER`, `_RECYCLE_RSS_BYTES`,
`_SPAWN_TIMEOUT_MS`, `_MAX_ARTIFACT_BYTES`, `_UID`, `_GID`, `_CGROUP`, `_EXTRA_LIBS`, `_FALLBACK_WASM`,
`_ALLOW_UNSANDBOXED` (default false; required for sandbox level `off`), `_PREWARM`, `_PROBE_EVERY`,
`_PROBE_SLOW_CALL_MS`, `_SPAWN_THREADS` and `_IDLE_RECYCLE_RSS_BYTES`. Any `RUST_NATIVE_MODE` value other than
`worker` means off.
Defaults are in the `RUST_NATIVE_*` block of `crates/common/src/knobs.rs`.

## 11. Testing

- Protocol: round trip of every frame, golden byte fixtures, length-limit rejection, one-byte-at-a-time
  incremental decoding.
- Pump: a fake worker thread on a socketpair; event order, eager push, poll/no-tasks sequence races, rejected
  submit consumes an id, log flood caps, stdout split and cap, timeout, cancel safety.
- Pool: a fake spawner; reuse, recycle thresholds, refill, reaper, breaker and backoff, shutdown.
- Worker loop: over a trait with a Rust fake, no `dlopen`; panics; logging from a second thread.
- Sandbox: BPF program compiles for x86_64 and aarch64; Landlock probe; sandbox plan construction (rlimits, fd relocation).
- Classification: a table of (status note, exit status, stderr tail) to failure kind.
- Push validation: synthetic ELF and Mach-O built in tests for each rejection case.
- End to end: the wasm spec suite re-run on native; failure modes (panic, abort, segfault, stack overflow, OOM,
  timeout, `process::exit`, output floods, oversized results) each asserting the exact message and that the
  next call succeeds; isolation (deployment env visible, backend secrets absent, data dir denied, sockets
  denied unless allowed, `exec` blocked); lifecycle (push during an in-flight call, backend killed with -9
  leaves no workers, parallel calls, idle reaping, breaker with a corrupt artifact).

## 12. Upstream footprint

Everything is new files except the following small edits to existing upstream files. Exact line counts for the
current patch are in the pull request body.

- `crates/local_backend/src/main.rs`: 3 lines at the top of `main` that hand control to the worker entry point
  when the first argument is `__rust_native_worker`. It must run before argument parsing and tracing so nothing
  writes to the worker's stderr.
- `crates/local_backend/src/router.rs` and `lib.rs`: one route (`/rust_native/info`) and one `mod` line.
- `crates/common/src/knobs.rs`: one block of knobs.
- `Cargo.toml`: `isolate` gains `libloading` (dlopen of the user library); `application` gains `object`
  (read-only ELF/Mach-O parsing) and `sha2` (already a workspace dependency); `local_backend` gains `isolate`
  (already a workspace crate). seccomp and Landlock are raw syscalls with a hand-built filter, so there is no
  `seccompiler` or `landlock` dependency.
- Edits to files that the wasm runtime already added (not upstream files): the runtime branch in
  `rust/mod.rs`, task-id allocation in `rust/backend.rs`, a few failure variants in `rust/core.rs`, and the
  native branch in the push-time analyzer.

## 13. Safety assessment

This section states what the design is meant to stop, which layer does it, and how much of that has been checked.
It separates three levels of evidence:

- Tested: a named test exercised the behaviour and passed on the machine it ran on (macOS arm64 unless stated).
- Implemented, not run on Linux: the code is written and compiles for Linux, and its pure logic may be unit
  tested off-kernel, but it has not been executed against a real Linux kernel.
- Not mitigated: nothing stops it on that platform, or it is a deliberate property of trusted mode.

Test names are the Rust test function names, or the `it(...)` titles in `e2e/native/test/`. The `e2e/` suites live in the
`convex-rs` SDK repository, not in this pull request.

### 13.1 Order of sandbox setup

There is no `pre_exec` hook. The parent spawns the worker with `Command` (environment cleared, stdin
`/dev/null`, `process_group(0)`), and the worker applies the sandbox itself as the first thing it does, in
`sandbox::enter`: move the frame socket and status pipe to fds 3 and 4 and close all other fds, `PR_SET_PDEATHSIG`,
`PR_SET_NO_NEW_PRIVS`, rlimits, wait for the parent's cgroup placement byte if a cgroup is configured, build the
Landlock ruleset, drop uid/gid, `landlock_restrict_self`, install the seccomp filter. `LibGuest::open` (the
`dlopen`) takes a `sandbox::Applied` value that only `enter` can construct, so the user library, including its
constructors, cannot be loaded before `enter` has succeeded. The test `sandbox_setup_runs_before_dlopen_and_gates_it`
checks the order and that a failed `enter` never reaches `open`, using fakes, so it runs on every platform. The
seccomp filter therefore has to allow what `dlopen` needs (`openat`, `mmap`, `mprotect`, `fstat`, reads); whether
`dlopen` and Rust start-up succeed under the real filter and Landlock ruleset on a Linux kernel has not been run.

### 13.2 Threats

| Threat | Layer that stops it | Linux | macOS |
|---|---|---|---|
| Crash, abort or memory corruption in user code | Separate process. The host classifies the exit (`pump::classify`) and discards the worker. The backend process is not affected. | Same code path as macOS; not run on Linux. | Tested: e2e `01-crash` (segfault, abort, `process::exit(3)`, stack overflow), unit `guest_abort_is_classified_as_a_crash`, `guest_exit_without_result_is_classified`. |
| Infinite loop | Host deadline; `SIGKILL` to the worker's process group. | Same code path as macOS; not run on Linux. | Tested: e2e `01-crash` ("`loop {}` hits the 5 s timeout"), unit `timeout_kills_the_process_group`. A child that leaves the process group would survive the group kill; nothing blocks `fork` on macOS. |
| Memory exhaustion | Counting allocator in the guest (SDK side), RSS watchdog in the worker (5 ms tick, limit plus slack), `RLIMIT_DATA` backstop, optional cgroup. | Watchdog: same code as macOS. `RLIMIT_DATA` and the cgroup path: implemented, not run on Linux. The cgroup subtree must be prepared by the operator; the host only moves the worker into it and reads `oom_kill`. | Tested: e2e `01-crash` (2 GiB allocation gives the out-of-memory message), unit `oom_vtable_entry_writes_status`. No hard backstop: a burst between watchdog ticks can exceed the limit. |
| Reading or writing the backend data directory or other files | Landlock: read/execute on system library paths, read on a short list of `/etc` and device files, read/write only in a private per-worker scratch directory. | Implemented, not run on Linux. The rule list is unit tested (`landlock_rules_cover_design`); the e2e check (`02-isolation`, "backend data dir is not readable") is Linux-only and was skipped. Needs kernel Landlock; with `best-effort` and no Landlock the layer is silently absent (logged, shown by `/api/rust_native/info`); `strict` refuses to spawn. | Not mitigated. The worker has the backend user's file access. The backend refuses to start native mode on macOS unless `RUST_NATIVE_ALLOW_UNSANDBOXED=true`. |
| Reading backend secrets from the environment | The worker's environment is cleared and holds only the deployment environment variables; fds above 4 are closed. | Same code as macOS; not run on Linux. | Tested: e2e `02-isolation` ("backend secrets are absent (names and values)", "host shell environment is not inherited"). |
| Reading backend secrets from files, `/proc`, process listings | Landlock (files, and `/proc` is limited to the worker's own pid); seccomp traps `ptrace`. | Implemented, not run on Linux. | Not mitigated. |
| Network exfiltration or lateral movement | seccomp returns `EPERM` for `socket`, `connect`, `bind` and related calls unless `RUST_NATIVE_ALLOW_NETWORK` is set. `fetch` from a guest goes through the host's fetch client, as for V8 actions. With networking allowed and Landlock ABI 4 or later, TCP connect is limited to ports 80 and 443; UDP and older ABIs are not limited by Landlock. | Implemented, not run on Linux. The filter is checked by a userspace BPF interpreter (`network_syscalls_denied_without_allow_network`, `decisions`); the e2e check (`02-isolation`, "`TcpStream::connect` is refused by default") is Linux-only and was skipped. | Not mitigated. `03-network` only checks that the allow flag does not break connecting. |
| Spawning processes | seccomp traps `execve`, `fork`, `vfork`, `clone` without `CLONE_THREAD`, and returns `ENOSYS` for `clone3`. | Implemented, not run on Linux. Filter logic: `decisions`, `wrong_arch_and_x32_are_rejected`. The e2e check ("`Command::new("sh")` is stopped by the sandbox") is Linux-only and was skipped. | Not mitigated. |
| Privilege escalation | `PR_SET_NO_NEW_PRIVS`; seccomp traps `setuid`-family calls, `ptrace`, `mount`, `bpf`, `io_uring`, module loading; optional drop to `RUST_NATIVE_UID`/`RUST_NATIVE_GID` with `setgroups(0)`. Capabilities are not dropped explicitly. | Implemented, not run on Linux. The stock image runs the backend as root; without a uid drop the worker is root, restricted only by Landlock and seccomp. The uid drop is not covered by any test. | Not mitigated. |
| Thread or process bombs | `fork` is blocked by seccomp. Threads are allowed. `RLIMIT_NPROC` (4096) and `RLIMIT_CPU` (4 h) apply only when the worker has its own uid and no cgroup. The host does not write `pids.max`. | Implemented, not run on Linux. Thread creation inside a call is otherwise unbounded unless the operator configures a cgroup. | Tested only for the aftermath: e2e `01-crash` ("a thread spinning after return: call succeeds, worker is recycled"). No limit on threads or processes. |
| State persisting between calls | By design a worker serves up to `RUST_NATIVE_MAX_CALLS_PER_WORKER` (default 1000) calls, so `static` and `thread_local` values persist. The worker is discarded after a panic, timeout, crash, protocol error or high RSS. A pool is keyed by (artifact hash, environment hash), so state never crosses artifacts. Setting the knob to 1 gives per-call isolation. The per-worker scratch directory is removed when the worker ends. | By design. Recycling is platform independent. | By design. Tested: unit `recycles_after_max_calls`, `recycles_on_rss_and_on_failure`, `scratch_dirs_are_private_and_stale_ones_swept`. |
| Library constructors (`.init_array`) running before the sandbox | The `Applied` token makes `dlopen` impossible before `sandbox::enter` completes (section 13.1). | Implemented, not run on Linux (order tested with fakes; the real filter has not been exercised). | Not mitigated: there is no sandbox to apply, so constructors run with the backend's privileges. |
| Backend killed, workers orphaned | `PR_SET_PDEATHSIG` (Linux); a watchdog in the worker exits when its parent pid changes; the worker exits on socket EOF; workers are spawned from one dedicated long-lived thread so the parent-death signal does not fire early. | `PDEATHSIG`: implemented, not run on Linux. Watchdog: same code as macOS. | Tested: e2e `04-lifecycle` ("`kill -9` leaves no workers"), through the parent-pid watchdog (up to 100 ms when idle). |
| Output floods, oversized results, oversized or malformed frames | Host-side caps on log volume and result size; frame length is bounded and checked before the body is read; a protocol error kills the worker. | Platform independent; not run on Linux. | Tested: e2e `01-crash` ("20 MiB `println!` flood"), `05-limits`; unit tests in `proto` (length-limit rejection, incremental decoding) and `pump`. |
| Malicious or malformed artifact at push | Push-time analyzer: knob, size, sha256, ELF/Mach-O structure, architecture, `DT_NEEDED` allowlist, glibc version, required exports, manifest consistency (section 4.1). No guest code runs. It does not and cannot judge what the machine code does; that is the trust model. The artifact is parsed in the backend process with the `object` crate. | Tested with synthetic ELF files built in the tests (15 tests in `rust_analyze/native.rs`, for example `glibc_too_new`, `elf_needed_allowlist`, `sha_and_size_checks`, `knob_off`). | The same analyzer tests, with synthetic Mach-O files (`valid_macho_carrier`, `macho_format_checks`, `macho_missing_export`). |

### 13.3 What has been verified, and how

Counts are the number of `#[test]` and `#[tokio::test]` attributes in the tree. Pass results are those recorded for
the prepared patch on macOS arm64; they were not re-run for this document.

| Area | Where | Count |
|---|---|---|
| Frame protocol | `crates/isolate/src/environment/rust/native/proto.rs` | 10 |
| Call pump with a fake worker | `native/pump.rs` | 20 |
| Pool with a fake spawner | `native/pool.rs` | 17 |
| Worker loop (fake guest, plus a real `dlopen` over a socketpair, a missing library, sandbox-before-dlopen order) | `native/worker.rs` | 18 |
| Real worker processes through a test shim (spawn, handshake, call, abort, exit, timeout kill, spawn failures) and scratch directories | `native/handle.rs` | 14, one of which is the shim entry point |
| Sandbox: rlimit plan, fd relocation in a child process, strict/off policy; BPF program for x86_64 and aarch64, syscall tables, Landlock rules | `native/sandbox/mod.rs`, `native/sandbox/linux.rs` | 9 and 9 |
| Artifact store, target detection, env hash | `native/artifact.rs`, `native/target.rs`, `native/mod.rs` | 5, 1, 1 |
| Push-time analyzer, native path | `crates/application/src/application_function_runner/rust_analyze/native.rs` | 15 |
| CLI (build planning, ELF/Mach-O manifest reading, carrier encoding) | `npm-packages/convex/src/cli/lib/rust/native.test.ts` | 17 |
| End to end | `e2e/native/test/01` to `09` (crash, isolation, network, lifecycle, limits, ctx, modes, sandbox levels, HTTP) | 48 `it(` cases; those that call `linuxOnly()` are skipped off Linux |
| Spec parity | `e2e/spec` run against the native runtime | 47 tests |

That makes 104 Rust unit tests in the native module of `isolate` and 15 in the analyzer. For the prepared patch the
pull request body records `cargo test -p isolate -p application --lib rust` at 128 and 32 passed (these filters also
include the wasm tests), CLI vitest 104 passed, and the end-to-end spec and parity suite 47/47 on macOS arm64. The
`e2e/native` suites pass on macOS arm64 except for cases that need a Linux kernel, which report SKIP rather than
pass.

Two unit tests are gated to Linux (`tables_match_libc_for_host`, `probe_returns_something`) and have not run.

### 13.4 What has not been verified

- The Linux sandbox has not been run on a real kernel. That covers Landlock acceptance of the ruleset (including
  whether the path list is sufficient for glibc, the dynamic loader and a Rust runtime), seccomp acceptance and
  whether the allow list lets a real cdylib load, start threads and run (a missing syscall would show up as a
  `SIGSYS` kill of valid code), `PR_SET_PDEATHSIG`, the effect of `RLIMIT_DATA` against the sizing formula (memory
  limit plus 64 stacks plus 256 MiB), and the `SIGSYS` status note path. The Linux-only e2e checks exist and are
  skipped on macOS. The code has been compiled for `x86_64-unknown-linux-gnu`, not executed.
- The cgroup placement path (`RUST_NATIVE_CGROUP`) and the uid/gid drop have no test, on any platform.
- No Landlock ABI below 4 has been tried. On those kernels the TCP port rules do not exist, and the code does not
  handle the newer rights.
- Docker's default seccomp profile may block Landlock or seccomp syscalls on older engines; this is listed under
  Risks and has not been tried.
- No end-to-end push of a real Linux cdylib to a Linux backend has been run. The ELF side of the analyzer has only
  seen synthetic files from its own tests, and cross-building with `cargo-zigbuild` is not part of the e2e runner.
- The e2e fixtures are cooperative: they crash, loop and probe the sandbox. Nobody has tried to escape the sandbox
  adversarially, and no external security review has been done.
- There is no fuzzing of the frame decoder or of the ELF and Mach-O parsing in the analyzer.
- The counting allocator and the guest side of the ABI live in the SDK repository and are outside this patch.
- Side channels (timing, CPU cache, `/proc` contents visible to the worker's own pid) were not considered.

### 13.5 The `unsafe` footprint

The patch adds 111 occurrences of the keyword `unsafe` under `crates/isolate/src/environment/rust/native/`, counted
with `grep -w` and excluding comment lines. 83 are in production code and 28 are in test modules. The
`application` and `local_backend` crates add none. The wasm patch adds one more (`wasmtime::Module::deserialize_file`
in `rust/core.rs`), which is not part of this runtime. Occurrences are not blocks: the count includes five
`unsafe extern "C" fn` type aliases.

| File | Production | What it is for |
|---|---|---|
| `native/worker.rs` | 40 | Calls into the loaded library through function pointers (`convex_native_init`, `convex_run:*`, `convex_free_result`, memory limit, trim), wrapped in `catch_unwind`; the `extern "C"` host function table that reads guest pointers and writes into guest-allocated memory; raw `recv`, `poll`, `write` and `_exit` on the frame socket and status pipe; `sigaction` handlers (`SIGSYS`, stack overflow detection); `getppid` and `sysconf` in the watchdog |
| `native/sandbox/linux.rs` | 16 | `prctl` to install the seccomp filter; the Landlock syscalls (create ruleset, add rule, restrict self) and the `open`/`fstat` calls that build them |
| `native/handle.rs` | 14 | `kill` of the process group, `socketpair` and `pipe` with `OwnedFd::from_raw_fd`, `fcntl` for close-on-exec and non-blocking, reading the status pipe, process RSS and thread count queries on macOS |
| `native/sandbox/mod.rs` | 9 | `prctl` (`PDEATHSIG`, `NO_NEW_PRIVS`), `setrlimit`, `setgroups`/`setgid`/`setuid`, fd relocation with `fcntl`/`dup2`/`close_range`, reading the cgroup go byte |
| `native/pump.rs` | 2 | `fcntl` and `read` on the stdout/stderr pipes |
| `native/mod.rs`, `native/target.rs` | 1 each | `geteuid`; `gnu_get_libc_version` |

Everything in the table is either a libc call on a file descriptor or process the module owns, or a call across the
`dlopen` boundary into code that is untrusted by definition. There is no `pre_exec` closure, so no `unsafe` code
runs between `fork` and `exec` in the backend. The remaining risk in this category is the guest ABI itself: the
host trusts pointers and lengths that the guest returns, inside a process that holds nothing but that call's data.

### 13.6 Conclusion

The native runtime is suitable for trusted, single-tenant, self-hosted deployments where the operator controls
every crate that is pushed, with the knob off by default. Process isolation, host-side timeouts, memory
accounting, crash classification, the environment boundary and push-time validation are implemented and tested on
macOS. The Linux filesystem, network, process and privilege restrictions are implemented but have not yet been run
on a real kernel, and the macOS path has no sandbox at all. This is not a multi-tenant isolation boundary and
should not be treated as one. It is not intended for Convex Cloud.

## 14. Risks

| Risk | Mitigation |
|---|---|
| Re-executing the large backend binary is slow or has high idle RSS. | Measure. If spawn p50 exceeds 30 ms or idle RSS exceeds 20 MB, switch to a separate small worker binary (needs packaging changes in upstream files). |
| Older Docker seccomp profiles block Landlock or seccomp syscalls. | Startup probe, the info endpoint reports the real level, `strict` refuses to spawn, documented `--security-opt` recipe. |
| Task-id alignment between worker and host. | Host consumes an id before parsing; pump tests assert ids. |
| Statics persist across calls. | Documented; recycle-per-call knob; recycle after panic or timeout. |
| C dependencies cross-compile badly with zig. | Prefer rustls/ring; Docker build fallback; build-failure hints. |
| A user `#[global_allocator]` conflicts with the SDK's counting allocator. | Clear error and docs; optional SDK feature disables counting (the RSS watchdog still applies). |
| `.init_array` constructors run at `dlopen`. | They run after the sandbox is applied (type-enforced); not yet verified on a real Linux kernel. On macOS there is no sandbox. |
| Build scripts and dependencies run on the server (inside the worker). | Trusted mode only, default off. |
| Rebase friction in `main.rs` and `router.rs`. | 3 + 1 + 1 lines, each in its own hunk. |
| Library built against a newer glibc, or with missing system libraries. | Push-time glibc and `DT_NEEDED` checks; `dlopen` errors surface through the breaker message. |

## 15. Open questions for maintainers

1. Is a separate static-musl executable variant wanted for distroless or older-glibc hosts?
2. Network default: fetch through the host (consistent) or real sockets behind `RUST_NATIVE_ALLOW_NETWORK`
   (which enables database drivers)? Note that with the knob on and Landlock ABI 4 or later, TCP connect is
   limited to ports 80 and 443, so a driver on another port (for example 5432) is blocked, and UDP is not
   restricted. Should the port list be configurable? Possibly a per-deployment setting.
3. Should `now` be answered inside the worker (saves one round trip per call) at the cost of clock parity under
   test runtimes?
4. Should the backend prewarm a pool when push analysis accepts a new artifact?
5. Is a 5 ms RSS watchdog plus `RLIMIT_DATA` enough without cgroups for Docker users, or should a delegated
   cgroup be documented as the way to get strict memory limits?
6. Should the artifact move to file storage (like external dependency packages) instead of base64 in module
   source? That touches upstream push code, so it is deferred.
7. Pool keying per environment hash means any environment edit recycles workers. Acceptable?
8. What support and licence wording do maintainers want for "executes native code with the backend's kernel
   privileges, minus the sandbox"?
