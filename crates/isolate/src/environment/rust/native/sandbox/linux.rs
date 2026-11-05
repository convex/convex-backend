//! Linux sandbox pieces (DESIGN §4.1): a hand-built seccomp-bpf program and
//! Landlock rule construction.
//!
//! The pure parts (syscall tables, BPF program builder, a test interpreter,
//! Landlock rule lists) compile on every OS so that both arches are unit
//! tested anywhere. Only the functions that issue syscalls are `cfg(linux)`.
//! No `seccompiler`/`landlock` crates: the programs are small and fixed.

use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Syscall tables (generated from libc 0.2.191 `SYS_*`, which mirror the
// kernel's per-arch tables). A name missing from an arch is absent there.
// ---------------------------------------------------------------------------
const NR_X86_64: &[(&str, u32)] = &[
    ("read", 0),
    ("write", 1),
    ("readv", 19),
    ("writev", 20),
    ("pread64", 17),
    ("pwrite64", 18),
    ("close", 3),
    ("fstat", 5),
    ("newfstatat", 262),
    ("statx", 332),
    ("stat", 4),
    ("lstat", 6),
    ("open", 2),
    ("lseek", 8),
    ("openat", 257),
    ("getdents", 78),
    ("getdents64", 217),
    ("readlink", 89),
    ("readlinkat", 267),
    ("getcwd", 79),
    ("unlink", 87),
    ("unlinkat", 263),
    ("rmdir", 84),
    ("mkdir", 83),
    ("mkdirat", 258),
    ("rename", 82),
    ("renameat", 264),
    ("renameat2", 316),
    ("ftruncate", 77),
    ("truncate", 76),
    ("fsync", 74),
    ("fdatasync", 75),
    ("fcntl", 72),
    ("ioctl", 16),
    ("dup", 32),
    ("dup2", 33),
    ("dup3", 292),
    ("pipe", 22),
    ("pipe2", 293),
    ("access", 21),
    ("faccessat", 269),
    ("faccessat2", 439),
    ("statfs", 137),
    ("fstatfs", 138),
    ("fchmod", 91),
    ("fchmodat", 268),
    ("umask", 95),
    ("chdir", 80),
    ("fchdir", 81),
    ("utimensat", 280),
    ("copy_file_range", 326),
    ("fallocate", 285),
    ("fadvise64", 221),
    ("symlinkat", 266),
    ("linkat", 265),
    ("mmap", 9),
    ("munmap", 11),
    ("mprotect", 10),
    ("mremap", 25),
    ("madvise", 28),
    ("brk", 12),
    ("mincore", 27),
    ("rt_sigaction", 13),
    ("rt_sigprocmask", 14),
    ("rt_sigreturn", 15),
    ("sigaltstack", 131),
    ("rt_sigtimedwait", 128),
    ("futex", 202),
    ("sched_yield", 24),
    ("sched_getaffinity", 204),
    ("set_robust_list", 273),
    ("rseq", 334),
    ("set_tid_address", 218),
    ("restart_syscall", 219),
    ("nanosleep", 35),
    ("clock_nanosleep", 230),
    ("clock_gettime", 228),
    ("clock_getres", 229),
    ("gettimeofday", 96),
    ("time", 201),
    ("getcpu", 309),
    ("flock", 73),
    ("getrandom", 318),
    ("exit", 60),
    ("exit_group", 231),
    ("getpid", 39),
    ("gettid", 186),
    ("tgkill", 234),
    ("getppid", 110),
    ("getuid", 102),
    ("geteuid", 107),
    ("getgid", 104),
    ("getegid", 108),
    ("poll", 7),
    ("ppoll", 271),
    ("select", 23),
    ("pselect6", 270),
    ("epoll_create", 213),
    ("epoll_create1", 291),
    ("epoll_ctl", 233),
    ("epoll_wait", 232),
    ("epoll_pwait", 281),
    ("eventfd2", 290),
    ("eventfd", 284),
    ("uname", 63),
    ("sysinfo", 99),
    ("prlimit64", 302),
    ("getrlimit", 97),
    ("getrusage", 98),
    ("membarrier", 324),
    ("arch_prctl", 158),
    ("clone", 56),
    ("clone3", 435),
    ("prctl", 157),
    ("socket", 41),
    ("connect", 42),
    ("bind", 49),
    ("listen", 50),
    ("accept", 43),
    ("accept4", 288),
    ("sendto", 44),
    ("recvfrom", 45),
    ("sendmsg", 46),
    ("recvmsg", 47),
    ("getsockname", 51),
    ("getpeername", 52),
    ("setsockopt", 54),
    ("getsockopt", 55),
    ("shutdown", 48),
    ("socketpair", 53),
    ("sendmmsg", 307),
    ("recvmmsg", 299),
    ("execve", 59),
    ("execveat", 322),
    ("fork", 57),
    ("vfork", 58),
    ("ptrace", 101),
    ("process_vm_readv", 310),
    ("process_vm_writev", 311),
    ("kexec_load", 246),
    ("kexec_file_load", 320),
    ("bpf", 321),
    ("mount", 165),
    ("umount2", 166),
    ("pivot_root", 155),
    ("chroot", 161),
    ("unshare", 272),
    ("setns", 308),
    ("keyctl", 250),
    ("add_key", 248),
    ("perf_event_open", 298),
    ("userfaultfd", 323),
    ("io_uring_setup", 425),
    ("io_uring_enter", 426),
    ("init_module", 175),
    ("finit_module", 313),
    ("delete_module", 176),
    ("reboot", 169),
    ("swapon", 167),
];

const NR_AARCH64: &[(&str, u32)] = &[
    ("read", 63),
    ("write", 64),
    ("readv", 65),
    ("writev", 66),
    ("pread64", 67),
    ("pwrite64", 68),
    ("close", 57),
    ("fstat", 80),
    ("newfstatat", 79),
    ("statx", 291),
    ("lseek", 62),
    ("openat", 56),
    ("renameat", 38),
    ("getdents64", 61),
    ("readlinkat", 78),
    ("getcwd", 17),
    ("unlinkat", 35),
    ("mkdirat", 34),
    ("renameat2", 276),
    ("ftruncate", 46),
    ("truncate", 45),
    ("fsync", 82),
    ("fdatasync", 83),
    ("fcntl", 25),
    ("ioctl", 29),
    ("dup", 23),
    ("dup3", 24),
    ("pipe2", 59),
    ("faccessat", 48),
    ("faccessat2", 439),
    ("statfs", 43),
    ("fstatfs", 44),
    ("fchmod", 52),
    ("fchmodat", 53),
    ("umask", 166),
    ("chdir", 49),
    ("fchdir", 50),
    ("utimensat", 88),
    ("copy_file_range", 285),
    ("fallocate", 47),
    ("fadvise64", 223),
    ("symlinkat", 36),
    ("linkat", 37),
    ("mmap", 222),
    ("munmap", 215),
    ("mprotect", 226),
    ("mremap", 216),
    ("madvise", 233),
    ("brk", 214),
    ("mincore", 232),
    ("rt_sigaction", 134),
    ("rt_sigprocmask", 135),
    ("rt_sigreturn", 139),
    ("sigaltstack", 132),
    ("rt_sigtimedwait", 137),
    ("futex", 98),
    ("sched_yield", 124),
    ("sched_getaffinity", 123),
    ("set_robust_list", 99),
    ("rseq", 293),
    ("set_tid_address", 96),
    ("restart_syscall", 128),
    ("nanosleep", 101),
    ("clock_nanosleep", 115),
    ("clock_gettime", 113),
    ("clock_getres", 114),
    ("gettimeofday", 169),
    ("getcpu", 168),
    ("flock", 32),
    ("getrandom", 278),
    ("exit", 93),
    ("exit_group", 94),
    ("getpid", 172),
    ("gettid", 178),
    ("tgkill", 131),
    ("getppid", 173),
    ("getuid", 174),
    ("geteuid", 175),
    ("getgid", 176),
    ("getegid", 177),
    ("ppoll", 73),
    ("pselect6", 72),
    ("epoll_create1", 20),
    ("epoll_ctl", 21),
    ("epoll_pwait", 22),
    ("eventfd2", 19),
    ("uname", 160),
    ("sysinfo", 179),
    ("prlimit64", 261),
    ("getrlimit", 163),
    ("getrusage", 165),
    ("membarrier", 283),
    ("clone", 220),
    ("clone3", 435),
    ("prctl", 167),
    ("socket", 198),
    ("connect", 203),
    ("bind", 200),
    ("listen", 201),
    ("accept", 202),
    ("accept4", 242),
    ("sendto", 206),
    ("recvfrom", 207),
    ("sendmsg", 211),
    ("recvmsg", 212),
    ("getsockname", 204),
    ("getpeername", 205),
    ("setsockopt", 208),
    ("getsockopt", 209),
    ("shutdown", 210),
    ("socketpair", 199),
    ("sendmmsg", 269),
    ("recvmmsg", 243),
    ("execve", 221),
    ("execveat", 281),
    ("ptrace", 117),
    ("process_vm_readv", 270),
    ("process_vm_writev", 271),
    ("kexec_load", 104),
    ("kexec_file_load", 294),
    ("bpf", 280),
    ("mount", 40),
    ("umount2", 39),
    ("pivot_root", 41),
    ("chroot", 51),
    ("unshare", 97),
    ("setns", 268),
    ("keyctl", 219),
    ("add_key", 217),
    ("perf_event_open", 241),
    ("userfaultfd", 282),
    ("io_uring_setup", 425),
    ("io_uring_enter", 426),
    ("init_module", 105),
    ("finit_module", 273),
    ("delete_module", 106),
    ("reboot", 142),
    ("swapon", 224),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
}

impl Arch {
    pub const ALL: [Arch; 2] = [Arch::X86_64, Arch::Aarch64];

    pub fn host() -> Option<Arch> {
        if cfg!(target_arch = "x86_64") {
            Some(Arch::X86_64)
        } else if cfg!(target_arch = "aarch64") {
            Some(Arch::Aarch64)
        } else {
            None
        }
    }

    pub fn audit_arch(self) -> u32 {
        match self {
            Arch::X86_64 => 0xC000_003E,
            Arch::Aarch64 => 0xC000_00B7,
        }
    }

    fn table(self) -> &'static [(&'static str, u32)] {
        match self {
            Arch::X86_64 => NR_X86_64,
            Arch::Aarch64 => NR_AARCH64,
        }
    }

    pub fn nr(self, name: &str) -> Option<u32> {
        self.table().iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
    }
}

// ---------------------------------------------------------------------------
// Classic BPF
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct SockFilter {
    pub code: u16,
    pub jt: u8,
    pub jf: u8,
    pub k: u32,
}

const LD_W_ABS: u16 = 0x20;
const JEQ_K: u16 = 0x15;
const JGE_K: u16 = 0x35;
const JSET_K: u16 = 0x45;
const RET_K: u16 = 0x06;

pub const RET_ALLOW: u32 = 0x7fff_0000;
pub const RET_TRAP: u32 = 0x0003_0000;
pub const RET_KILL_PROCESS: u32 = 0x8000_0000;
const RET_ERRNO: u32 = 0x0005_0000;
pub const fn ret_errno(e: u32) -> u32 {
    RET_ERRNO | (e & 0xffff)
}
pub const EPERM: u32 = 1;
pub const ENOSYS: u32 = 38;

// struct seccomp_data offsets.
const OFF_NR: u32 = 0;
const OFF_ARCH: u32 = 4;
const fn off_arg_lo(i: u32) -> u32 {
    16 + 8 * i
}
const fn off_arg_hi(i: u32) -> u32 {
    16 + 8 * i + 4
}

const CLONE_THREAD: u32 = 0x0001_0000;
const PR_SET_NAME: u32 = 15;
const X32_BIT: u32 = 0x4000_0000;

fn ld(off: u32) -> SockFilter {
    SockFilter { code: LD_W_ABS, jt: 0, jf: 0, k: off }
}
fn jeq(k: u32, jt: u8, jf: u8) -> SockFilter {
    SockFilter { code: JEQ_K, jt, jf, k }
}
fn jset(k: u32, jt: u8, jf: u8) -> SockFilter {
    SockFilter { code: JSET_K, jt, jf, k }
}
fn ret(k: u32) -> SockFilter {
    SockFilter { code: RET_K, jt: 0, jf: 0, k }
}

/// Syscalls allowed unconditionally (DESIGN §4.1 plus what Rust/glibc need).
const ALLOW: &[&str] = &[
    // I/O and files (paths are gated by Landlock)
    "read", "write", "readv", "writev", "pread64", "pwrite64", "close", "fstat", "newfstatat",
    "statx", "stat", "lstat", "open", "lseek", "openat", "getdents", "getdents64", "readlink",
    "readlinkat", "getcwd", "unlink", "unlinkat", "rmdir", "mkdir", "mkdirat", "rename",
    "renameat", "renameat2", "ftruncate", "truncate", "fsync", "fdatasync", "fcntl", "ioctl",
    "dup", "dup2", "dup3", "pipe", "pipe2", "access", "faccessat", "faccessat2", "statfs",
    "fstatfs", "fchmod", "fchmodat", "umask", "chdir", "fchdir", "utimensat", "copy_file_range",
    "fallocate", "fadvise64", "symlinkat", "linkat", "flock",
    // memory
    "mmap", "munmap", "mprotect", "mremap", "madvise", "brk", "mincore",
    // signals
    "rt_sigaction", "rt_sigprocmask", "rt_sigreturn", "sigaltstack", "rt_sigtimedwait",
    // threads and scheduling
    "futex", "sched_yield", "sched_getaffinity", "set_robust_list", "rseq", "set_tid_address",
    "restart_syscall",
    // time and randomness
    "nanosleep", "clock_nanosleep", "clock_gettime", "clock_getres", "gettimeofday", "time",
    "getcpu", "getrandom",
    // process (self only)
    "exit", "exit_group", "getpid", "gettid", "getppid", "getuid", "geteuid", "getgid",
    "getegid",
    // polling
    "poll", "ppoll", "select", "pselect6", "epoll_create", "epoll_create1", "epoll_ctl",
    "epoll_wait", "epoll_pwait", "eventfd2", "eventfd",
    // misc
    "uname", "sysinfo", "getrlimit", "getrusage", "membarrier", "arch_prctl",
];

/// Denied with `EPERM` unless the network is allowed, in which case allowed.
const NET: &[&str] = &[
    "socket", "connect", "bind", "listen", "accept", "accept4", "getsockname", "getpeername",
    "setsockopt", "getsockopt", "shutdown", "socketpair", "sendmmsg", "recvmmsg",
];

/// Allowed on the worker's own frame socket (fd 3) only, `EPERM` otherwise
/// unless the network is allowed.
const NET_FD: &[&str] = &["sendto", "recvfrom", "sendmsg", "recvmsg"];

#[derive(Clone, Debug)]
pub struct FilterConfig {
    pub arch: Arch,
    pub allow_network: bool,
    /// pid used for the `tgkill` self-only check.
    pub pid: u32,
    /// The frame socket (3).
    pub sock_fd: u32,
}

fn block(out: &mut Vec<SockFilter>, nr: u32, body: Vec<SockFilter>) {
    assert!(body.len() <= 255);
    out.push(jeq(nr, 0, body.len() as u8));
    out.extend(body);
}

/// Builds the seccomp-bpf program. Default action is `TRAP` (SIGSYS).
pub fn build_filter(cfg: &FilterConfig) -> Vec<SockFilter> {
    let a = cfg.arch;
    let mut p = vec![
        ld(OFF_ARCH),
        jeq(a.audit_arch(), 1, 0),
        ret(RET_KILL_PROCESS),
        ld(OFF_NR),
    ];
    if a == Arch::X86_64 {
        // x32 ABI syscalls (nr has bit 30) must not bypass the table.
        p.push(SockFilter { code: JGE_K, jt: 0, jf: 1, k: X32_BIT });
        p.push(ret(RET_TRAP));
    }
    for name in ALLOW {
        if let Some(nr) = a.nr(name) {
            block(&mut p, nr, vec![ret(RET_ALLOW)]);
        }
    }
    if let Some(nr) = a.nr("clone3") {
        block(&mut p, nr, vec![ret(ret_errno(ENOSYS))]);
    }
    if let Some(nr) = a.nr("clone") {
        // flags is arg0 on x86_64 and aarch64; require CLONE_THREAD.
        block(
            &mut p,
            nr,
            vec![ld(off_arg_lo(0)), jset(CLONE_THREAD, 0, 1), ret(RET_ALLOW), ret(RET_TRAP)],
        );
    }
    if let Some(nr) = a.nr("tgkill") {
        block(
            &mut p,
            nr,
            vec![ld(off_arg_lo(0)), jeq(cfg.pid, 0, 1), ret(RET_ALLOW), ret(RET_TRAP)],
        );
    }
    if let Some(nr) = a.nr("prctl") {
        block(
            &mut p,
            nr,
            vec![
                ld(off_arg_lo(0)),
                jeq(PR_SET_NAME, 0, 1),
                ret(RET_ALLOW),
                ret(ret_errno(EPERM)),
            ],
        );
    }
    if let Some(nr) = a.nr("prlimit64") {
        // get only: new_limit (arg2) must be NULL.
        block(
            &mut p,
            nr,
            vec![
                ld(off_arg_lo(2)),
                jeq(0, 0, 3),
                ld(off_arg_hi(2)),
                jeq(0, 0, 1),
                ret(RET_ALLOW),
                ret(ret_errno(EPERM)),
            ],
        );
    }
    for name in NET {
        if let Some(nr) = a.nr(name) {
            let act = if cfg.allow_network { RET_ALLOW } else { ret_errno(EPERM) };
            block(&mut p, nr, vec![ret(act)]);
        }
    }
    for name in NET_FD {
        if let Some(nr) = a.nr(name) {
            let body = if cfg.allow_network {
                vec![ret(RET_ALLOW)]
            } else {
                vec![
                    ld(off_arg_lo(0)),
                    jeq(cfg.sock_fd, 0, 1),
                    ret(RET_ALLOW),
                    ret(ret_errno(EPERM)),
                ]
            };
            block(&mut p, nr, body);
        }
    }
    // Everything else (execve, fork, ptrace, bpf, mount, io_uring, ...): trap.
    p.push(ret(RET_TRAP));
    p
}

/// Evaluates a program against `(arch, nr, args)`. Test support (and a
/// readable spec of what the kernel does with the filter).
pub fn eval_filter(prog: &[SockFilter], audit_arch: u32, nr: u32, args: [u64; 6]) -> u32 {
    let load = |off: u32| -> u32 {
        match off {
            0 => nr,
            4 => audit_arch,
            o if (16..64).contains(&o) => {
                let v = args[((o - 16) / 8) as usize];
                if (o - 16) % 8 == 0 { v as u32 } else { (v >> 32) as u32 }
            },
            _ => panic!("bad seccomp_data offset {off}"),
        }
    };
    let (mut acc, mut pc) = (0u32, 0usize);
    loop {
        let i = prog[pc];
        pc += 1;
        match i.code {
            LD_W_ABS => acc = load(i.k),
            JEQ_K => pc += if acc == i.k { i.jt } else { i.jf } as usize,
            JGE_K => pc += if acc >= i.k { i.jt } else { i.jf } as usize,
            JSET_K => pc += if acc & i.k != 0 { i.jt } else { i.jf } as usize,
            RET_K => return i.k,
            c => panic!("unsupported bpf code {c:#x}"),
        }
    }
}

/// Installs the filter on the calling thread (call before spawning others).
#[cfg(target_os = "linux")]
pub fn install_seccomp(cfg: &FilterConfig) -> std::io::Result<()> {
    let prog = build_filter(cfg);
    let fprog = libc::sock_fprog {
        len: prog.len() as u16,
        filter: prog.as_ptr() as *mut libc::sock_filter,
    };
    // SAFETY: plain syscalls; `fprog` points at `prog`, alive for the call.
    unsafe {
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SECCOMP_SET_MODE_FILTER = 1, flags = SECCOMP_FILTER_FLAG_TSYNC (1)
        // so every thread (watchdog included) gets the filter.
        let r = libc::syscall(libc::SYS_seccomp, 1 as libc::c_long, 1 as libc::c_long, &fprog);
        if r > 0 {
            // With TSYNC a positive return is the tid of a thread that could
            // not be synchronised (errno is not set).
            return Err(std::io::Error::other(format!(
                "seccomp TSYNC failed: thread {r} already has an incompatible filter"
            )));
        }
        if r != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

/// True when seccomp filter mode is supported by the kernel.
#[cfg(target_os = "linux")]
pub fn seccomp_available() -> bool {
    // SAFETY: PR_GET_SECCOMP takes no pointers; EINVAL means unsupported.
    unsafe { libc::prctl(libc::PR_GET_SECCOMP, 0, 0, 0, 0) >= 0 }
}

// ---------------------------------------------------------------------------
// Landlock
// ---------------------------------------------------------------------------

pub mod ll {
    pub const EXECUTE: u64 = 1 << 0;
    pub const WRITE_FILE: u64 = 1 << 1;
    pub const READ_FILE: u64 = 1 << 2;
    pub const READ_DIR: u64 = 1 << 3;
    pub const REMOVE_DIR: u64 = 1 << 4;
    pub const REMOVE_FILE: u64 = 1 << 5;
    pub const MAKE_CHAR: u64 = 1 << 6;
    pub const MAKE_DIR: u64 = 1 << 7;
    pub const MAKE_REG: u64 = 1 << 8;
    pub const MAKE_SOCK: u64 = 1 << 9;
    pub const MAKE_FIFO: u64 = 1 << 10;
    pub const MAKE_BLOCK: u64 = 1 << 11;
    pub const MAKE_SYM: u64 = 1 << 12;
    pub const REFER: u64 = 1 << 13;
    pub const TRUNCATE: u64 = 1 << 14;
    pub const NET_BIND_TCP: u64 = 1 << 0;
    pub const NET_CONNECT_TCP: u64 = 1 << 1;
    /// Newest ABI this code uses (fs through TRUNCATE, net TCP).
    pub const MAX_ABI: u32 = 4;
    /// Access bits valid on a non-directory.
    pub const FILE_OK: u64 = EXECUTE | WRITE_FILE | READ_FILE | TRUNCATE;
    pub const RO: u64 = READ_FILE | READ_DIR;
    pub const RX: u64 = RO | EXECUTE;

    /// Filesystem access bits handled (denied unless allowed) for an ABI.
    pub fn handled_fs(abi: u32) -> u64 {
        let mut m = (1u64 << 13) - 1;
        if abi >= 2 {
            m |= REFER;
        }
        if abi >= 3 {
            m |= TRUNCATE;
        }
        m
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathRule {
    pub path: PathBuf,
    /// Requested access; `u64::MAX` means everything handled (read-write).
    pub access: u64,
}

pub struct RuleInputs<'a> {
    pub artifact: &'a std::path::Path,
    pub exe: Option<&'a std::path::Path>,
    pub tmp: Option<&'a std::path::Path>,
    pub extra_read: &'a [PathBuf],
}

/// The path rules of DESIGN §4.1 step 7 (`/proc/<pid>` is added in
/// the worker, where the pid is known).
pub fn landlock_rules(i: &RuleInputs<'_>) -> Vec<PathRule> {
    let r = |p: &str, a: u64| PathRule { path: PathBuf::from(p), access: a };
    let mut v = vec![
        r("/usr", ll::RX),
        r("/lib", ll::RX),
        r("/lib64", ll::RX),
        r("/bin", ll::RX),
        r("/etc/ssl", ll::RO),
        r("/etc/ca-certificates", ll::RO),
        r("/etc/localtime", ll::RO),
        r("/etc/ld.so.cache", ll::RO),
        // glibc name resolution (only useful with --allow-network; read-only).
        r("/etc/resolv.conf", ll::RO),
        r("/etc/hosts", ll::RO),
        r("/etc/nsswitch.conf", ll::RO),
        r("/etc/host.conf", ll::RO),
        r("/etc/gai.conf", ll::RO),
        r("/usr/share/zoneinfo", ll::RO),
        r("/dev/urandom", ll::RO),
        r("/dev/random", ll::RO),
        r("/dev/null", ll::RO | ll::WRITE_FILE),
        r("/sys/devices/system/cpu", ll::RO),
        r("/sys/fs/cgroup", ll::RO),
    ];
    v.push(PathRule { path: i.artifact.to_path_buf(), access: ll::RX });
    if let Some(e) = i.exe {
        v.push(PathRule { path: e.to_path_buf(), access: ll::RX });
    }
    if let Some(t) = i.tmp {
        v.push(PathRule { path: t.to_path_buf(), access: u64::MAX });
    }
    for p in i.extra_read {
        v.push(PathRule { path: p.clone(), access: ll::RX });
    }
    v
}

#[cfg(target_os = "linux")]
mod sys {
    use std::{
        ffi::CString,
        os::{
            fd::{FromRawFd, OwnedFd},
            unix::ffi::OsStrExt,
        },
    };

    use super::{ll, PathRule};

    // Same numbers on x86_64 and aarch64.
    const SYS_CREATE_RULESET: libc::c_long = 444;
    const SYS_ADD_RULE: libc::c_long = 445;
    const SYS_RESTRICT_SELF: libc::c_long = 446;
    const CREATE_RULESET_VERSION: u32 = 1;
    const RULE_PATH_BENEATH: libc::c_long = 1;
    const RULE_NET_PORT: libc::c_long = 2;

    #[repr(C)]
    struct RulesetAttr {
        handled_access_fs: u64,
        handled_access_net: u64,
    }
    #[repr(C, packed)]
    struct PathBeneath {
        allowed_access: u64,
        parent_fd: i32,
    }
    #[repr(C)]
    struct NetPort {
        allowed_access: u64,
        port: u64,
    }

    /// Landlock ABI version, or `None` when unsupported/disabled.
    pub fn abi() -> Option<u32> {
        // SAFETY: documented probe form (null attr, size 0, VERSION flag).
        let r = unsafe {
            libc::syscall(
                SYS_CREATE_RULESET,
                std::ptr::null::<RulesetAttr>(),
                0usize,
                CREATE_RULESET_VERSION,
            )
        };
        (r > 0).then_some(r as u32)
    }

    /// Async-signal-safe: adds a read-only rule for `path` (NUL-terminated).
    ///
    /// # Safety
    /// `path` must be a valid NUL-terminated string, `ruleset` a ruleset fd.
    pub unsafe fn add_ro_path_raw(ruleset: i32, path: *const libc::c_char, access: u64) -> bool {
        // SAFETY: the caller guarantees `path` is NUL-terminated.
        let fd = unsafe { libc::open(path, libc::O_PATH | libc::O_CLOEXEC) };
        if fd < 0 {
            return false;
        }
        let attr = PathBeneath { allowed_access: access, parent_fd: fd };
        // SAFETY: `attr` outlives the call; `fd` is ours and closed right after.
        let r = unsafe { libc::syscall(SYS_ADD_RULE, ruleset, RULE_PATH_BENEATH, &attr, 0u32) };
        unsafe { libc::close(fd) };
        r == 0
    }

    /// Builds a ruleset (not yet enforced). Nonexistent paths are skipped.
    pub fn build_ruleset(
        abi: u32,
        rules: &[PathRule],
        allow_network: bool,
    ) -> std::io::Result<OwnedFd> {
        let abi = abi.min(ll::MAX_ABI);
        let handled = ll::handled_fs(abi);
        let net = if abi >= 4 { ll::NET_BIND_TCP | ll::NET_CONNECT_TCP } else { 0 };
        let attr = RulesetAttr { handled_access_fs: handled, handled_access_net: net };
        let size = if abi >= 4 { 16usize } else { 8usize };
        // SAFETY: attr outlives the call; size matches the ABI.
        let fd = unsafe { libc::syscall(SYS_CREATE_RULESET, &attr, size, 0u32) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: fd is a fresh descriptor we own.
        let owned = unsafe { OwnedFd::from_raw_fd(fd as i32) };
        use std::os::fd::AsRawFd;
        for rule in rules {
            let Ok(c) = CString::new(rule.path.as_os_str().as_bytes()) else { continue };
            // SAFETY: valid C string.
            let pfd = unsafe { libc::open(c.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
            if pfd < 0 {
                continue;
            }
            let mut st: libc::stat = unsafe { std::mem::zeroed() };
            // SAFETY: pfd valid; st is writable.
            let is_dir = unsafe { libc::fstat(pfd, &mut st) } == 0
                && (st.st_mode & libc::S_IFMT) == libc::S_IFDIR;
            let mut access = if rule.access == u64::MAX { handled } else { rule.access };
            access &= handled;
            if !is_dir {
                access &= ll::FILE_OK;
            }
            let attr = PathBeneath { allowed_access: access, parent_fd: pfd };
            // SAFETY: attr is valid for the call.
            let r = unsafe {
                libc::syscall(SYS_ADD_RULE, owned.as_raw_fd(), RULE_PATH_BENEATH, &attr, 0u32)
            };
            let err = std::io::Error::last_os_error();
            // SAFETY: pfd is ours.
            unsafe { libc::close(pfd) };
            if r != 0 {
                return Err(err);
            }
        }
        if net != 0 && allow_network {
            for port in [80u64, 443u64] {
                let attr = NetPort { allowed_access: ll::NET_CONNECT_TCP, port };
                // SAFETY: attr valid for the call.
                let r = unsafe {
                    libc::syscall(SYS_ADD_RULE, owned.as_raw_fd(), RULE_NET_PORT, &attr, 0u32)
                };
                if r != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
        }
        Ok(owned)
    }

    /// Async-signal-safe.
    pub fn restrict_self(ruleset: i32) -> std::io::Result<()> {
        // SAFETY: plain syscall.
        let r = unsafe { libc::syscall(SYS_RESTRICT_SELF, ruleset, 0u32) };
        if r == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
    }
}

#[cfg(target_os = "linux")]
pub use sys::{abi as landlock_abi, add_ro_path_raw, build_ruleset, restrict_self};

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(arch: Arch, net: bool) -> FilterConfig {
        FilterConfig { arch, allow_network: net, pid: 4242, sock_fd: 3 }
    }

    fn run(arch: Arch, net: bool, name: &str, args: [u64; 6]) -> u32 {
        let c = cfg(arch, net);
        let prog = build_filter(&c);
        eval_filter(&prog, arch.audit_arch(), arch.nr(name).unwrap(), args)
    }

    #[test]
    fn bpf_compiles_for_both_arches() {
        for arch in Arch::ALL {
            for net in [false, true] {
                let prog = build_filter(&cfg(arch, net));
                assert!(prog.len() < 4096, "{arch:?} {}", prog.len());
                // Statically validate jumps stay in range and end in RET.
                for (i, f) in prog.iter().enumerate() {
                    if f.code != RET_K && f.code != LD_W_ABS {
                        assert!(i + 1 + (f.jt.max(f.jf) as usize) < prog.len());
                    }
                }
                assert_eq!(prog.last().unwrap().code, RET_K);
            }
        }
    }

    #[test]
    fn names_resolve_and_are_unique() {
        for arch in Arch::ALL {
            let mut seen = std::collections::HashSet::new();
            for n in ALLOW.iter().chain(NET).chain(NET_FD) {
                if let Some(nr) = arch.nr(n) {
                    assert!(seen.insert(nr), "dup nr {nr} ({n}) on {arch:?}");
                }
            }
        }
        for n in ALLOW.iter().chain(NET).chain(NET_FD) {
            assert!(Arch::ALL.iter().any(|a| a.nr(n).is_some()), "{n}");
        }
        assert_eq!(Arch::X86_64.nr("read"), Some(0));
        assert_eq!(Arch::Aarch64.nr("read"), Some(63));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn tables_match_libc_for_host() {
        let a = Arch::host().unwrap();
        assert_eq!(a.nr("read"), Some(libc::SYS_read as u32));
        assert_eq!(a.nr("openat"), Some(libc::SYS_openat as u32));
        assert_eq!(a.nr("execve"), Some(libc::SYS_execve as u32));
        assert_eq!(a.nr("clone"), Some(libc::SYS_clone as u32));
    }

    #[test]
    fn arch_tables_are_consistent() {
        // Legacy x86_64-only syscalls (aarch64 has the *at / pipe2 / ppoll forms).
        const X86_ONLY: &[&str] = &[
            "stat", "lstat", "open", "getdents", "readlink", "unlink", "rmdir", "mkdir", "rename",
            "dup2", "pipe", "access", "poll", "select", "epoll_create", "epoll_wait", "eventfd",
            "arch_prctl", "time", "fork", "vfork",
        ];
        for n in ALLOW.iter().chain(NET).chain(NET_FD) {
            let (x, a) = (Arch::X86_64.nr(n), Arch::Aarch64.nr(n));
            if X86_ONLY.contains(n) {
                assert!(x.is_some() && a.is_none(), "{n}");
            } else {
                assert!(x.is_some() && a.is_some(), "{n} must exist on both arches");
            }
        }
        // Every x86_64-only name must have a replacement allowed on aarch64.
        for (legacy, modern) in [("open", "openat"), ("poll", "ppoll"), ("pipe", "pipe2")] {
            assert!(ALLOW.contains(&legacy) && ALLOW.contains(&modern));
            assert!(Arch::Aarch64.nr(legacy).is_none() && Arch::Aarch64.nr(modern).is_some());
        }
    }

    #[test]
    fn network_syscalls_denied_without_allow_network() {
        for arch in Arch::ALL {
            let off = [9u64; 6];
            for n in NET.iter().chain(NET_FD) {
                if arch.nr(n).is_some() {
                    assert_eq!(run(arch, false, n, off), ret_errno(EPERM), "{n} {arch:?}");
                    assert_eq!(run(arch, true, n, off), RET_ALLOW, "{n} {arch:?}");
                }
            }
            // fd 3 (the frame socket) keeps working for the message calls.
            for n in ["sendmsg", "recvmsg", "sendto", "recvfrom"] {
                assert_eq!(run(arch, false, n, [3, 0, 0, 0, 0, 0]), RET_ALLOW, "{n}");
            }
            // The runtime set a Rust cdylib needs.
            for n in [
                "futex", "mmap", "munmap", "mprotect", "madvise", "brk", "rt_sigreturn",
                "sigaltstack", "clock_gettime", "getrandom", "sched_yield", "exit", "exit_group",
                "close", "getcpu", "rseq", "set_robust_list", "membarrier", "sched_getaffinity",
            ] {
                assert_eq!(run(arch, false, n, [0; 6]), RET_ALLOW, "{n} {arch:?}");
            }
        }
    }

    #[test]
    fn decisions() {
        for arch in Arch::ALL {
            let z = [0u64; 6];
            assert_eq!(run(arch, false, "read", z), RET_ALLOW);
            assert_eq!(run(arch, false, "openat", z), RET_ALLOW);
            assert_eq!(run(arch, false, "execve", z), RET_TRAP);
            assert_eq!(run(arch, false, "ptrace", z), RET_TRAP);
            assert_eq!(run(arch, false, "io_uring_setup", z), RET_TRAP);
            assert_eq!(run(arch, false, "clone3", z), ret_errno(ENOSYS));
            // clone: thread only
            assert_eq!(run(arch, false, "clone", [CLONE_THREAD as u64, 0, 0, 0, 0, 0]), RET_ALLOW);
            assert_eq!(run(arch, false, "clone", z), RET_TRAP);
            // tgkill: self only
            assert_eq!(run(arch, false, "tgkill", [4242, 0, 6, 0, 0, 0]), RET_ALLOW);
            assert_eq!(run(arch, false, "tgkill", [1, 0, 9, 0, 0, 0]), RET_TRAP);
            // prctl
            assert_eq!(run(arch, false, "prctl", [15, 0, 0, 0, 0, 0]), RET_ALLOW);
            assert_eq!(run(arch, false, "prctl", [38, 0, 0, 0, 0, 0]), ret_errno(EPERM));
            // prlimit64: get only
            assert_eq!(run(arch, false, "prlimit64", z), RET_ALLOW);
            assert_eq!(
                run(arch, false, "prlimit64", [0, 0, 0x1_0000_0000, 0, 0, 0]),
                ret_errno(EPERM)
            );
            assert_eq!(run(arch, false, "prlimit64", [0, 0, 0x1000, 0, 0, 0]), ret_errno(EPERM));
            // network
            assert_eq!(run(arch, false, "socket", z), ret_errno(EPERM));
            assert_eq!(run(arch, true, "socket", z), RET_ALLOW);
            assert_eq!(run(arch, false, "recvfrom", [3, 0, 0, 0, 0, 0]), RET_ALLOW);
            assert_eq!(run(arch, false, "recvfrom", [9, 0, 0, 0, 0, 0]), ret_errno(EPERM));
            assert_eq!(run(arch, true, "recvfrom", [9, 0, 0, 0, 0, 0]), RET_ALLOW);
        }
    }

    #[test]
    fn wrong_arch_and_x32_are_rejected() {
        let p = build_filter(&cfg(Arch::X86_64, false));
        let read = Arch::X86_64.nr("read").unwrap();
        assert_eq!(
            eval_filter(&p, Arch::Aarch64.audit_arch(), read, [0; 6]),
            RET_KILL_PROCESS
        );
        assert_eq!(eval_filter(&p, Arch::X86_64.audit_arch(), read | X32_BIT, [0; 6]), RET_TRAP);
    }

    #[test]
    fn landlock_rules_cover_design() {
        let art = PathBuf::from("/data/bin/x.so");
        let tmp = PathBuf::from("/data/tmp/w1");
        let rules = landlock_rules(&RuleInputs {
            artifact: &art,
            exe: Some(std::path::Path::new("/convex/backend")),
            tmp: Some(&tmp),
            extra_read: &[],
        });
        assert!(rules.iter().any(|r| r.path.as_path() == std::path::Path::new("/usr") && r.access & ll::EXECUTE != 0));
        assert!(rules.iter().any(|r| r.path == tmp && r.access == u64::MAX));
        assert!(rules.iter().any(|r| r.path == art));
        assert!(!rules.iter().any(|r| r.path.starts_with("/convex/data")));
        assert_eq!(ll::handled_fs(1).count_ones(), 13);
        assert_eq!(ll::handled_fs(3).count_ones(), 15);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn probe_returns_something() {
        let _ = landlock_abi();
        let _ = seccomp_available();
    }
}
