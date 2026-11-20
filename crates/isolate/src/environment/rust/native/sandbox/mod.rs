//! Sandbox for native worker processes (WN-04; DESIGN §4.1/§4.2).
//!
//! L2-03: there is no `pre_exec` hook any more (it forced `fork()` of the whole
//! backend, 40-48 ms; plain `Command` uses `posix_spawn`, ~10 ms). The parent
//! only validates support ([`check_support`]) and passes the plan on the worker
//! command line; the worker applies everything itself in [`enter`], as the very
//! first thing it does and BEFORE it loads the user cdylib.
//!
//! # Security invariant (do not weaken)
//!
//! Untrusted code (the user dylib: its constructors, `convex_native_init`, any
//! `convex_run:*`) must never execute before the sandbox is fully applied.
//! This is enforced by types: [`crate::environment::rust::native::worker::LibGuest::open`]
//! takes an `&Applied`, which only [`enter`] can construct, and `enter`
//! returns `Err` (and the worker exits without touching the dylib) if any
//! required step fails. Ordering inside [`enter`]:
//!   1. relocate the frame socket / status pipe to fds 3 and 4, close every
//!      other inherited fd;
//!   2. `PR_SET_PDEATHSIG`, `PR_SET_NO_NEW_PRIVS` (Linux);
//!   3. rlimits;
//!   4. if the parent is placing us in a cgroup: block until the parent has
//!      written our pid to `cgroup.procs` and sent the go byte (the parent
//!      does this immediately after `spawn`, before it reads `Ready`);
//!   5. build the Landlock ruleset (needs the privileges we still have);
//!   6. drop uid/gid;
//!   7. `landlock_restrict_self`;
//!   8. install the seccomp filter (TSYNC; only the main thread exists yet).
//!
//! The process group is set by the parent via `Command::process_group(0)`
//! (`posix_spawn` `POSIX_SPAWN_SETPGROUP`): kill-tree uses `kill(-pid)`.

pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;

use std::{
    fmt,
    str::FromStr,
};

use anyhow::Context as _;

use super::worker::WorkerArgs;

/// `RUST_NATIVE_SANDBOX`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxLevel {
    /// Fail to spawn if Landlock or seccomp is unavailable.
    Strict,
    /// Apply what the probe finds.
    BestEffort,
    /// env_clear plus rlimits only.
    Off,
}

impl SandboxLevel {
    pub fn default_for_platform() -> Self {
        if cfg!(target_os = "linux") {
            SandboxLevel::BestEffort
        } else {
            SandboxLevel::Off
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SandboxLevel::Strict => "strict",
            SandboxLevel::BestEffort => "best-effort",
            SandboxLevel::Off => "off",
        }
    }
}

impl fmt::Display for SandboxLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SandboxLevel {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        Ok(match s {
            "strict" => SandboxLevel::Strict,
            "best-effort" => SandboxLevel::BestEffort,
            "off" => SandboxLevel::Off,
            other => anyhow::bail!("unknown sandbox level {other:?}"),
        })
    }
}

/// What this host can enforce (feeds `/api/rust_native/info`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SandboxProbe {
    pub landlock_abi: Option<u32>,
    pub seccomp: bool,
}

pub fn probe() -> SandboxProbe {
    #[cfg(target_os = "linux")]
    {
        SandboxProbe {
            landlock_abi: linux::landlock_abi(),
            seccomp: linux::seccomp_available(),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        SandboxProbe::default()
    }
}

const NOFILE: u64 = 256;
const FSIZE: u64 = 1 << 30;
/// Thread/process cap per uid, applied only with a uid drop and no cgroup
/// (`RLIMIT_NPROC` counts every process of the real uid, so it is meaningless
/// while the worker shares the backend's uid).
const NPROC: u64 = 4096;
/// CPU seconds per worker process, a backstop against a runaway guest when no
/// cgroup bounds it. Generous: a pooled worker serves up to
/// `RUST_NATIVE_MAX_CALLS_PER_WORKER` calls and CPU time sums over threads.
const CPU_SECS: u64 = 4 * 3600;

/// What the host can promise about a worker under `level`, for the startup
/// log and `/api/rust_native/info`. `active` lists enforced protections,
/// `missing` the ones that are not (empty when nothing is degraded).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Protections {
    pub active: Vec<&'static str>,
    pub missing: Vec<&'static str>,
}

pub fn protections(
    level: SandboxLevel,
    probe: &SandboxProbe,
    uid_drop: bool,
    cgroup: bool,
) -> Protections {
    let mut p = Protections::default();
    let mut put = |on: bool, name: &'static str| {
        if on { p.active.push(name) } else { p.missing.push(name) }
    };
    put(level != SandboxLevel::Off, "sandbox");
    put(level != SandboxLevel::Off && probe.landlock_abi.is_some(), "landlock");
    put(level != SandboxLevel::Off && probe.seccomp, "seccomp");
    put(uid_drop, "uid-drop");
    put(cgroup, "cgroup");
    p
}

/// Startup policy: running user code with no sandbox at all needs an explicit
/// opt-in (`RUST_NATIVE_ALLOW_UNSANDBOXED=true`). This is the default level
/// anywhere but Linux, so a backend there refuses to start native mode until
/// the operator acknowledges it.
pub fn check_policy(level: SandboxLevel, allow_unsandboxed: bool) -> anyhow::Result<()> {
    anyhow::ensure!(
        level != SandboxLevel::Off || allow_unsandboxed,
        "refusing to start rust-native workers with sandbox level `off`{}: user code would run with \
         the full filesystem, network and process access of the backend. Run on Linux (Landlock + \
         seccomp), or set RUST_NATIVE_ALLOW_UNSANDBOXED=true to accept this for a trusted local \
         development backend",
        if cfg!(target_os = "linux") { "" } else { " (the only level available on this platform)" }
    );
    Ok(())
}

/// Resource limits for a plan: `(resource, value)`, value for both soft and hard.
/// `pids_and_cpu` adds `RLIMIT_NPROC` and `RLIMIT_CPU` (Linux; used when no
/// cgroup bounds the worker).
pub fn rlimits_for(memory_bytes: u64, stack_bytes: u64, pids_and_cpu: bool) -> Vec<(libc::c_int, u64)> {
    let mut v = vec![
        (libc::RLIMIT_CORE as libc::c_int, 0),
        (libc::RLIMIT_NOFILE as libc::c_int, NOFILE),
        (libc::RLIMIT_FSIZE as libc::c_int, FSIZE),
    ];
    if cfg!(target_os = "linux") {
        // Hard backstop; thread stacks count against RLIMIT_DATA (not RLIMIT_AS,
        // which misfires on virtual reservations).
        let data = memory_bytes
            .saturating_add(stack_bytes.saturating_mul(64))
            .saturating_add(256 << 20);
        v.push((libc::RLIMIT_DATA as libc::c_int, data));
        if pids_and_cpu {
            v.push((libc::RLIMIT_NPROC as libc::c_int, NPROC));
            v.push((libc::RLIMIT_CPU as libc::c_int, CPU_SECS));
        }
    }
    let _ = pids_and_cpu;
    v
}


/// Parent-side validation, run before spawning: a `strict` plan must be
/// satisfiable on this host. Nothing is applied here.
pub fn check_support(level: SandboxLevel) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    if level == SandboxLevel::Strict {
        anyhow::ensure!(linux::landlock_abi().is_some(), "sandbox=strict but Landlock is unavailable");
        anyhow::ensure!(linux::seccomp_available(), "sandbox=strict but seccomp is unavailable");
    }
    #[cfg(not(target_os = "linux"))]
    anyhow::ensure!(
        level != SandboxLevel::Strict,
        "sandbox=strict is not supported on this platform"
    );
    let _ = level;
    Ok(())
}

/// Proof that [`enter`] completed. Only `enter` can build one; the dylib
/// loader requires it (see the module docs).
#[derive(Debug)]
#[allow(clippy::manual_non_exhaustive)] // same-crate construction is gated by `_private`, not visibility
pub struct Applied {
    /// What was enforced, for logs.
    pub description: String,
    _private: (),
}

impl Applied {
    /// For tests of code that is generic over the sandbox (fake guests).
    #[cfg(test)]
    pub fn for_test() -> Self {
        Applied { description: "test".into(), _private: () }
    }
}

fn check(r: libc::c_int) -> std::io::Result<()> {
    if r < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
}

/// Moves the inherited frame socket and status pipe onto fds 3 and 4 and
/// closes every other fd above 4 (std's `Command` cannot dup2 for us, and
/// without `pre_exec` the numbers the parent used are arbitrary).
fn relocate_fds(sock: libc::c_int, status: libc::c_int) -> std::io::Result<()> {
    // SAFETY: fcntl/dup2/close on fds this process inherited.
    unsafe {
        if sock != 3 || status != 4 {
            let n3 = libc::fcntl(sock, libc::F_DUPFD, 10);
            check(n3)?;
            let n4 = libc::fcntl(status, libc::F_DUPFD, 10);
            check(n4)?;
            check(libc::dup2(n3, 3))?;
            check(libc::dup2(n4, 4))?;
            libc::close(n3);
            libc::close(n4);
        }
    }
    close_from(5);
    Ok(())
}

fn close_from(first: libc::c_int) {
    #[cfg(target_os = "linux")]
    // SAFETY: close_range(2); falls through to the scan if unsupported.
    unsafe {
        if libc::syscall(libc::SYS_close_range, first as libc::c_uint, libc::c_uint::MAX, 0) == 0 {
            return;
        }
    }
    let fds: Vec<libc::c_int> = std::fs::read_dir("/dev/fd")
        .map(|d| {
            d.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<libc::c_int>().ok())
                .filter(|&fd| fd >= first)
                .collect()
        })
        .unwrap_or_default();
    for fd in fds {
        // SAFETY: closing fds we do not use (the read_dir fd itself is
        // already closed; EBADF is ignored).
        unsafe {
            libc::close(fd);
        }
    }
}

/// Applies the whole sandbox to the current (worker) process. Must be the
/// first thing the worker does and must complete before the user dylib is
/// opened. Any error is fatal for the worker.
pub fn enter(args: &WorkerArgs) -> anyhow::Result<Applied> {
    let level = args.sandbox;
    // 1. fds. The status pipe is only valid from here on.
    relocate_fds(args.sock_fd, args.status_fd).context("relocating the worker fds")?;
    #[cfg(target_os = "linux")]
    // SAFETY: prctl with integer arguments.
    unsafe {
        // 2.
        check(libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong, 0, 0, 0))
            .context("PR_SET_PDEATHSIG")?;
        check(libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1 as libc::c_ulong, 0, 0, 0))
            .context("PR_SET_NO_NEW_PRIVS")?;
    }
    // 3.
    // Without a cgroup nothing else bounds threads or CPU time; NPROC is per
    // uid, so it only helps once the worker has its own (see step 6).
    let own_limits = !args.wait_go && args.uid_gid.is_some();
    for (res, val) in rlimits_for(args.memory_bytes, args.stack_bytes as u64, own_limits) {
        let l = libc::rlimit { rlim_cur: val as libc::rlim_t, rlim_max: val as libc::rlim_t };
        // SAFETY: valid rlimit struct.
        check(unsafe { libc::setrlimit(res as _, &l) }).with_context(|| format!("setrlimit {res}"))?;
    }
    // 4. The parent writes our pid into cgroup.procs, then sends one byte.
    if args.wait_go {
        let mut b = [0u8; 1];
        loop {
            // SAFETY: reading one byte into a local buffer from the frame socket.
            let n = unsafe { libc::read(3, b.as_mut_ptr().cast(), 1) };
            if n == 1 {
                break;
            }
            if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            anyhow::bail!("the host did not confirm the cgroup placement");
        }
    }
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut notes = vec![level.as_str().to_string()];
    #[cfg(target_os = "linux")]
    if level != SandboxLevel::Off {
        use std::os::fd::AsRawFd;
        // 5.
        let ruleset = match linux::landlock_abi() {
            Some(abi) => {
                let exe = std::env::current_exe().ok();
                let rules = linux::landlock_rules(&linux::RuleInputs {
                    artifact: &args.artifact,
                    exe: exe.as_deref(),
                    tmp: args.tmp.as_deref(),
                    extra_read: &args.extra_read,
                });
                let fd = linux::build_ruleset(abi, &rules, args.allow_network)
                    .context("landlock ruleset")?;
                notes.push(format!("landlock-abi-{}", abi.min(linux::ll::MAX_ABI)));
                Some(fd)
            },
            None if level == SandboxLevel::Strict => {
                anyhow::bail!("sandbox=strict but Landlock is unavailable")
            },
            None => None,
        };
        // 6.
        if let Some((uid, gid)) = args.uid_gid {
            // SAFETY: plain syscalls.
            unsafe {
                check(libc::setgroups(0, std::ptr::null())).context("setgroups")?;
                check(libc::setgid(gid)).context("setgid")?;
                check(libc::setuid(uid)).context("setuid")?;
            }
            notes.push("uid-drop".into());
        }
        // 7.
        if let Some(fd) = &ruleset {
            let mut buf = [0u8; 32];
            let path = proc_path(std::process::id(), &mut buf);
            // SAFETY: `path` is NUL-terminated (proc_path) and `fd` is the ruleset fd.
            unsafe { linux::add_ro_path_raw(fd.as_raw_fd(), path.as_ptr().cast(), linux::ll::RO) };
            linux::restrict_self(fd.as_raw_fd()).context("landlock_restrict_self")?;
        }
        // 8.
        match linux::Arch::host() {
            None => {
                anyhow::ensure!(level != SandboxLevel::Strict, "no seccomp table for this arch");
                notes.push("no-seccomp-table".into());
            },
            Some(arch) => match linux::install_seccomp(&linux::FilterConfig {
                arch,
                allow_network: args.allow_network,
                pid: std::process::id(),
                sock_fd: 3,
            }) {
                Ok(()) => notes.push("seccomp".into()),
                Err(e) if level == SandboxLevel::Strict => return Err(e).context("seccomp"),
                Err(e) => notes.push(format!("seccomp-failed: {e}")),
            },
        }
    } else if let Some((uid, gid)) = args.uid_gid {
        // SAFETY: plain syscalls.
        unsafe {
            check(libc::setgroups(0, std::ptr::null())).context("setgroups")?;
            check(libc::setgid(gid)).context("setgid")?;
            check(libc::setuid(uid)).context("setuid")?;
        }
    }
    #[cfg(not(target_os = "linux"))]
    anyhow::ensure!(level != SandboxLevel::Strict, "strict sandbox needs Linux");
    Ok(Applied { description: notes.join(","), _private: () })
}


/// Writes `/proc/<pid>\0` into `buf` and returns the NUL-terminated slice.
#[cfg(any(target_os = "linux", test))]
fn proc_path(pid: u32, buf: &mut [u8; 32]) -> &[u8] {
    let prefix = b"/proc/";
    buf[..prefix.len()].copy_from_slice(prefix);
    let mut digits = [0u8; 10];
    let (mut n, mut i) = (pid, 0);
    loop {
        digits[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
        if n == 0 {
            break;
        }
    }
    let mut o = prefix.len();
    while i > 0 {
        i -= 1;
        buf[o] = digits[i];
        o += 1;
    }
    buf[o] = 0;
    &buf[..=o]
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_parse_roundtrip() {
        for l in [SandboxLevel::Strict, SandboxLevel::BestEffort, SandboxLevel::Off] {
            assert_eq!(l.as_str().parse::<SandboxLevel>().unwrap(), l);
        }
        assert!("x".parse::<SandboxLevel>().is_err());
    }

    #[test]
    fn rlimits_plan() {
        let v = rlimits_for(512 << 20, 8 << 20, false);
        assert!(!v.iter().any(|(r, _)| *r == libc::RLIMIT_NPROC as libc::c_int));
        let v = rlimits_for(512 << 20, 8 << 20, true);
        if cfg!(target_os = "linux") {
            assert!(v.contains(&(libc::RLIMIT_NPROC as libc::c_int, NPROC)));
            assert!(v.contains(&(libc::RLIMIT_CPU as libc::c_int, CPU_SECS)));
        }
        assert!(v.contains(&(libc::RLIMIT_CORE as libc::c_int, 0)));
        assert!(v.contains(&(libc::RLIMIT_NOFILE as libc::c_int, 256)));
        if cfg!(target_os = "linux") {
            let want = (512u64 << 20) + (8u64 << 20) * 64 + (256 << 20);
            assert!(v.contains(&(libc::RLIMIT_DATA as libc::c_int, want)));
        }
    }

    /// N-S1: the unsandboxed level needs the explicit opt-in.
    #[test]
    fn unsandboxed_level_needs_opt_in() {
        assert!(check_policy(SandboxLevel::Off, false).is_err());
        assert!(check_policy(SandboxLevel::Off, true).is_ok());
        assert!(check_policy(SandboxLevel::BestEffort, false).is_ok());
        assert!(check_policy(SandboxLevel::Strict, false).is_ok());
        let e = check_policy(SandboxLevel::Off, false).unwrap_err().to_string();
        assert!(e.contains("RUST_NATIVE_ALLOW_UNSANDBOXED"), "{e}");
    }

    /// N-S2/N-T3: a degraded best-effort host reports what is missing;
    /// strict refuses (`check_support`) instead of degrading.
    #[test]
    fn protections_report_what_is_missing() {
        let none = SandboxProbe::default();
        let full = SandboxProbe { landlock_abi: Some(4), seccomp: true };
        let p = protections(SandboxLevel::BestEffort, &none, false, false);
        assert_eq!(p.active, vec!["sandbox"]);
        assert_eq!(p.missing, vec!["landlock", "seccomp", "uid-drop", "cgroup"]);
        let p = protections(SandboxLevel::BestEffort, &full, true, true);
        assert!(p.missing.is_empty(), "{p:?}");
        let p = protections(SandboxLevel::Off, &full, false, false);
        assert_eq!(p.active, Vec::<&str>::new());
        assert!(p.missing.contains(&"sandbox") && p.missing.contains(&"landlock"));
        // Strict never runs degraded: on a host without support it errors.
        if !cfg!(target_os = "linux") {
            assert!(check_support(SandboxLevel::Strict).is_err());
        }
    }

    #[test]
    fn proc_path_formats() {
        let mut b = [0u8; 32];
        assert_eq!(proc_path(0, &mut b), b"/proc/0\0");
        assert_eq!(proc_path(12345, &mut b), b"/proc/12345\0");
        assert_eq!(proc_path(u32::MAX, &mut b), b"/proc/4294967295\0");
    }

    #[test]
    fn probe_returns_struct() {
        let p = probe();
        if !cfg!(target_os = "linux") {
            assert_eq!(p, SandboxProbe::default());
        }
    }

    #[test]
    fn strict_requires_support_off_does_not() {
        assert!(check_support(SandboxLevel::Off).is_ok());
        assert!(check_support(SandboxLevel::BestEffort).is_ok());
        if !cfg!(target_os = "linux") {
            assert!(check_support(SandboxLevel::Strict).is_err());
        }
    }

    /// `enter` applies rlimits and the fd layout in a child that only
    /// re-runs this test (the limits are process-wide, so never in-process).
    #[test]
    fn enter_applies_rlimits_and_fds_in_a_child() {
        const MARK: &str = "RUST_NATIVE_TEST_ENTER_CHILD";
        if std::env::var(MARK).is_ok() {
            let mut a = WorkerArgs::parse(["--artifact".to_string(), "/x.so".into()]).unwrap();
            a.sandbox = SandboxLevel::Off;
            let applied = enter(&a).expect("enter");
            assert_eq!(applied.description, "off");
            let mut l: libc::rlimit = unsafe { std::mem::zeroed() };
            unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut l) };
            assert_eq!(l.rlim_cur, 256);
            unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut l) };
            assert_eq!(l.rlim_cur, 0);
            return;
        }
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "environment::rust::native::sandbox::tests::enter_applies_rlimits_and_fds_in_a_child", "--nocapture"])
            .env(MARK, "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "{} {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn relocate_moves_arbitrary_fds_onto_3_and_4() {
        const MARK: &str = "RUST_NATIVE_TEST_RELOCATE_CHILD";
        if std::env::var(MARK).is_ok() {
            // Parent left two pipes' write ends at unknown numbers; emulate.
            let mut p = [0 as libc::c_int; 2];
            let mut q = [0 as libc::c_int; 2];
            unsafe {
                libc::pipe(p.as_mut_ptr());
                libc::pipe(q.as_mut_ptr());
            }
            relocate_fds(p[1], q[1]).unwrap();
            unsafe {
                for fd in [3, 4] {
                    let fl = libc::fcntl(fd, libc::F_GETFL);
                    assert!(fl >= 0 && fl & libc::O_ACCMODE == libc::O_WRONLY, "fd {fd} is a write end");
                }
                let (r0, r1) = (p[0], q[0]);
                for fd in [r0, r1].into_iter().filter(|&fd| fd > 4) {
                    assert_eq!(libc::fcntl(fd, libc::F_GETFD), -1, "stray fd {fd} closed");
                }
            }
            return;
        }
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "environment::rust::native::sandbox::tests::relocate_moves_arbitrary_fds_onto_3_and_4", "--nocapture"])
            .env(MARK, "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "{} {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    }
}
