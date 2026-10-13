//! The environment and child processes (Gate 2, `docs/spec.md`, *Process and environment*): the
//! `os_env_*` built-ins, and `os_spawn` and the child's handles over the table `eval::os` keeps.
//!
//! A child is a handle like any other, keyed by a descriptor it holds open on `/dev/null` — a child
//! has no descriptor of its own, and the native runtime keys it the same way — and its three pipes
//! are handles too, read and written with `os_read` and `os_write`.
//!
//! R1d (`docs/spec.md`, *The platform, further*): a child in a directory with an environment of its
//! own — the program found on this process's `PATH` here, as `posix_spawnp` finds it, and the
//! environment handed over ordered by name, as the native runtime orders it — a new process group, a
//! null standard input, bounded waits that look again every 10 ms at most, a signal to a group (by
//! the system's `kill`, the interpreter having no C), and this process's id. A child's peak memory
//! needs `wait4`, which a safe Rust program cannot call: the interpreter answers `-ENOSYS`.

use std::io::Read as _;
use std::os::fd::AsRawFd as _;
use std::os::unix::process::ExitStatusExt as _;
use std::sync::Mutex;

use nazm_sema::Intrinsic;

use super::os::{object, register, Object};
use super::value::{Str, Value};

const EINVAL: i64 = 22;
const ENOENT: i64 = 2;
const ENOTDIR: i64 = 20;
const EACCES: i64 = 13;
const EPERM: i64 = 1;
const ESRCH: i64 = 3;
const ENOSYS: i64 = if cfg!(target_os = "linux") { 38 } else { 78 };
const ETIMEDOUT: i64 = if cfg!(target_os = "linux") { 110 } else { 60 };

/// A child: the descriptor its handle is keyed by, the process, its three pipes' handles, and
/// its status once it has been waited for.
#[derive(Debug)]
pub(super) struct Child {
    _key: std::fs::File,
    process: Mutex<std::process::Child>,
    pipes: [i64; 3],
    status: Mutex<Option<i64>>,
    /// Its process id (R1d).
    id: i64,
}

/// A child's output or its errors.
#[derive(Debug)]
pub(super) enum Output {
    Out(std::process::ChildStdout),
    Err(std::process::ChildStderr),
}

fn status(e: &std::io::Error) -> i64 {
    -i64::from(e.raw_os_error().unwrap_or(22))
}

fn text(s: &Str) -> String {
    String::from_utf8_lossy(s.bytes()).into_owned()
}

/// The environment's entries, `NAME=value`, in the order the process holds them; an entry with
/// no `=` after its first byte is not one, as natively.
fn entries() -> Vec<Vec<u8>> {
    std::env::vars_os()
        .map(|(k, v)| {
            let mut e = std::os::unix::ffi::OsStrExt::as_bytes(k.as_os_str()).to_vec();
            e.push(b'=');
            e.extend_from_slice(std::os::unix::ffi::OsStrExt::as_bytes(v.as_os_str()));
            e
        })
        .collect()
}

/// `os_env_at(i)` with `i` outside the environment stops the program (`N0405`), as `arg(i)`
/// does.
pub(super) fn env_at_failure(
    op: Intrinsic,
    args: &[Value],
    at: nazm_span::Span,
) -> Option<super::RuntimeError> {
    let (Intrinsic::OsEnvAt, [Value::Int(i)]) = (op, args) else {
        return None;
    };
    let n = entries().len();
    if usize::try_from(*i).is_ok_and(|i| i < n) {
        return None;
    }
    Some(super::RuntimeError {
        diagnostic: Box::new(
            nazm_diag::Diagnostic::error(
                nazm_diag::codes::INDEX_OUT_OF_BOUNDS,
                at,
                "this index is outside the environment",
            )
            .help(format!("asked for entry {i} of {n}")),
        ),
    })
}

/// A read from a child's output.
pub(super) fn pipe_read(p: &Mutex<Output>, buf: &mut [u8]) -> std::io::Result<usize> {
    match p.lock() {
        Ok(mut p) => match &mut *p {
            Output::Out(o) => o.read(buf),
            Output::Err(e) => e.read(buf),
        },
        Err(_) => Ok(0),
    }
}

/// A process or environment built-in, or `None` for any other.
pub(super) fn process_value(op: Intrinsic, args: &[Value]) -> Option<Value> {
    let int = |n: i64| Some(Value::Int(n));
    let handle = |n: i64| Some(Value::OsHandle(n));
    let string = |b: Vec<u8>| Some(Value::Str(Str::heap(b)));
    match (op, args) {
        (Intrinsic::OsEnvGet, [Value::Str(name)]) => string(
            std::env::var_os(text(name))
                .map(|v| std::os::unix::ffi::OsStrExt::as_bytes(v.as_os_str()).to_vec())
                .unwrap_or_default(),
        ),
        (Intrinsic::OsEnvHas, [Value::Str(name)]) => {
            Some(Value::Bool(std::env::var_os(text(name)).is_some()))
        }
        (Intrinsic::OsEnvCount, []) => int(i64::try_from(entries().len()).unwrap_or(0)),
        (Intrinsic::OsEnvAt, [Value::Int(i)]) => string(
            entries()
                .into_iter()
                .nth(usize::try_from(*i).ok()?)
                .unwrap_or_default(),
        ),
        // Entropy (Gate 2, §21): the system's, from its random device.
        (Intrinsic::OsRandomBytes, [Value::Bytes(buf)]) => {
            let mut got = vec![0; buf.len];
            let ok = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut got));
            if ok.is_err() {
                return int(-EINVAL);
            }
            buf.write(0, &got);
            int(i64::try_from(buf.len).unwrap_or(-EINVAL))
        }
        (Intrinsic::OsSpawn, [Value::Str(program), Value::Strs(list)]) => {
            let argv: Vec<String> = list.lock().ok()?.iter().map(text).collect();
            handle(spawn(&text(program), &argv))
        }
        (
            Intrinsic::OsSpawnIn | Intrinsic::OsSpawnWith,
            [Value::Str(program), Value::Strs(list), Value::Str(dir), Value::Strs(env), rest @ ..],
        ) => {
            let flags = match rest {
                [Value::Int(f)] => *f,
                _ => 0,
            };
            let argv: Vec<String> = list.lock().ok()?.iter().map(text).collect();
            let entries: Vec<Vec<u8>> = env
                .lock()
                .ok()?
                .iter()
                .map(|e| e.bytes().to_vec())
                .collect();
            handle(spawn_in(
                program.bytes(),
                &argv,
                dir.bytes(),
                entries,
                flags,
            ))
        }
        (Intrinsic::OsPid, []) => int(i64::from(std::process::id())),
        (Intrinsic::OsChildPid, [Value::OsHandle(h)]) => int(match object(*h) {
            Err(s) => s,
            Ok(o) => match &*o {
                Object::Child(c) => c.id,
                _ => -EINVAL,
            },
        }),
        (Intrinsic::OsTryWait, [Value::OsHandle(h)]) => int(match object(*h) {
            Err(s) => s,
            Ok(o) => match &*o {
                Object::Child(c) => try_wait(c),
                _ => -EINVAL,
            },
        }),
        (Intrinsic::OsWaitUntil, [Value::OsHandle(h), Value::Int(deadline)]) => {
            int(match object(*h) {
                Err(s) => s,
                Ok(o) => match &*o {
                    Object::Child(c) => wait_until(c, *deadline),
                    _ => -EINVAL,
                },
            })
        }
        (Intrinsic::OsChildRusage, [Value::OsHandle(h)]) => int(match object(*h) {
            Err(s) => s,
            Ok(o) => match &*o {
                Object::Child(c) => {
                    if c.status.lock().ok().is_some_and(|s| s.is_some()) {
                        -ENOSYS
                    } else {
                        -EINVAL
                    }
                }
                _ => -EINVAL,
            },
        }),
        (Intrinsic::OsKillGroup, [Value::Int(group), Value::Int(sig)]) => {
            int(kill_group(*group, *sig))
        }
        (Intrinsic::OsChildPipe, [Value::OsHandle(h), Value::Int(which)]) => {
            handle(match object(*h) {
                Err(s) => s,
                Ok(o) => match (&*o, usize::try_from(*which)) {
                    (Object::Child(c), Ok(w)) => c.pipes.get(w).copied().unwrap_or(-EINVAL),
                    _ => -EINVAL,
                },
            })
        }
        (Intrinsic::OsWait, [Value::OsHandle(h)]) => int(match object(*h) {
            Err(s) => s,
            Ok(o) => match &*o {
                Object::Child(c) => wait(c),
                _ => -EINVAL,
            },
        }),
        (Intrinsic::OsKill, [Value::OsHandle(h)]) => int(match object(*h) {
            Err(s) => s,
            Ok(o) => match &*o {
                Object::Child(c) => {
                    if c.status.lock().ok().is_some_and(|s| s.is_some()) {
                        return int(0);
                    }
                    match c.process.lock() {
                        Ok(mut p) => p.kill().map_or_else(|e| status(&e), |()| 0),
                        Err(_) => -EINVAL,
                    }
                }
                _ => -EINVAL,
            },
        }),
        _ => None,
    }
}

fn spawn(program: &str, argv: &[String]) -> i64 {
    let mut child = match std::process::Command::new(program)
        .args(argv)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return status(&e),
    };
    let (Some(input), Some(output), Some(errors)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return -EINVAL;
    };
    let Ok(key) = std::fs::File::open("/dev/null") else {
        return -EINVAL;
    };
    let pin = register(i64::from(input.as_raw_fd()), Object::PipeIn(input));
    let pout = register(
        i64::from(output.as_raw_fd()),
        Object::PipeOut(Mutex::new(Output::Out(output))),
    );
    let perr = register(
        i64::from(errors.as_raw_fd()),
        Object::PipeOut(Mutex::new(Output::Err(errors))),
    );
    let fd = i64::from(key.as_raw_fd());
    let id = i64::from(child.id());
    register(
        fd,
        Object::Child(Child {
            _key: key,
            process: Mutex::new(child),
            pipes: [pin, pout, perr],
            status: Mutex::new(None),
            id,
        }),
    )
}

/// `entries` checked as a child's environment (R1d): each `NAME=value` with a `=` after its first
/// byte and no zero byte, no name twice; ordered by name, byte by byte. `None` for any that is not.
fn environment(mut entries: Vec<Vec<u8>>) -> Option<Vec<(Vec<u8>, Vec<u8>)>> {
    let mut pairs = Vec::with_capacity(entries.len());
    for e in entries.drain(..) {
        if e.contains(&0) {
            return None;
        }
        let at = e.iter().skip(1).position(|b| *b == b'=')? + 1;
        let (name, value) = e.split_at(at);
        pairs.push((name.to_vec(), value.get(1..).unwrap_or_default().to_vec()));
    }
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    if pairs.windows(2).any(|w| matches!(w, [a, b] if a.0 == b.0)) {
        return None;
    }
    Some(pairs)
}

/// Where `program` is, as `posix_spawnp` finds it (R1d): a relative name with a `/` beside the
/// canonical `dir`, as the native runtime makes it, any other the first executable file of that name
/// on this process's `PATH`.
fn located(program: &[u8], dir: &[u8]) -> Result<std::path::PathBuf, i64> {
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::PermissionsExt as _;
    let as_path = |b: &[u8]| std::path::PathBuf::from(std::ffi::OsStr::from_bytes(b));
    if program.is_empty() {
        return Err(-ENOENT);
    }
    if program.contains(&b'/') {
        let p = as_path(program);
        if p.is_relative() && !dir.is_empty() {
            let base = std::fs::canonicalize(as_path(dir))
                .map_err(|e| -i64::from(e.raw_os_error().unwrap_or(22)))?;
            let mut joined = std::os::unix::ffi::OsStrExt::as_bytes(base.as_os_str()).to_vec();
            joined.push(b'/');
            joined.extend_from_slice(program);
            return Ok(as_path(&joined));
        }
        return Ok(p);
    }
    let path = std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into());
    let mut denied = false;
    for entry in path.as_bytes().split(|b| *b == b':') {
        let base = if entry.is_empty() {
            b".".as_slice()
        } else {
            entry
        };
        let candidate = as_path(base).join(as_path(program));
        if let Ok(m) = std::fs::metadata(&candidate) {
            if m.is_file() && m.permissions().mode() & 0o111 != 0 {
                return Ok(candidate);
            }
            denied = true;
        }
    }
    Err(if denied { -EACCES } else { -ENOENT })
}

/// `os_spawn_in` and `os_spawn_with` (R1d): the child's handle, or a status.
fn spawn_in(program: &[u8], argv: &[String], dir: &[u8], env: Vec<Vec<u8>>, flags: i64) -> i64 {
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::process::CommandExt as _;
    if flags & !3 != 0 || dir.contains(&0) {
        return -EINVAL;
    }
    let Some(pairs) = environment(env) else {
        return -EINVAL;
    };
    // The directory is asked about first, as natively: a missing one is `NotFound` and a file
    // `NotADirectory` on every system.
    if !dir.is_empty() {
        match std::fs::metadata(std::ffi::OsStr::from_bytes(dir)) {
            Err(e) => return status(&e),
            Ok(m) if !m.is_dir() => return -ENOTDIR,
            Ok(_) => {}
        }
    }
    let resolved = match located(program, dir) {
        Ok(p) => p,
        Err(s) => return s,
    };
    let null_input = flags & 2 != 0;
    let mut c = std::process::Command::new(resolved);
    c.args(argv)
        .env_clear()
        .envs(pairs.iter().map(|(k, v)| {
            (
                std::ffi::OsStr::from_bytes(k),
                std::ffi::OsStr::from_bytes(v),
            )
        }))
        .stdin(if null_input {
            std::process::Stdio::null()
        } else {
            std::process::Stdio::piped()
        })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if !dir.is_empty() {
        c.current_dir(std::ffi::OsStr::from_bytes(dir));
    }
    if flags & 1 != 0 {
        c.process_group(0);
    }
    let mut child = match c.spawn() {
        Ok(c) => c,
        Err(e) => return status(&e),
    };
    let (Some(output), Some(errors)) = (child.stdout.take(), child.stderr.take()) else {
        return -EINVAL;
    };
    let pin = match child.stdin.take() {
        Some(input) => register(i64::from(input.as_raw_fd()), Object::PipeIn(input)),
        None => super::os::CLOSED,
    };
    let Ok(key) = std::fs::File::open("/dev/null") else {
        return -EINVAL;
    };
    let pout = register(
        i64::from(output.as_raw_fd()),
        Object::PipeOut(Mutex::new(Output::Out(output))),
    );
    let perr = register(
        i64::from(errors.as_raw_fd()),
        Object::PipeOut(Mutex::new(Output::Err(errors))),
    );
    let fd = i64::from(key.as_raw_fd());
    let id = i64::from(child.id());
    register(
        fd,
        Object::Child(Child {
            _key: key,
            process: Mutex::new(child),
            pipes: [pin, pout, perr],
            status: Mutex::new(None),
            id,
        }),
    )
}

/// An exit status as `os_wait` answers it: the code, or 256 and the signal that ended it.
fn decoded(st: std::process::ExitStatus) -> i64 {
    st.code()
        .map(i64::from)
        .or_else(|| st.signal().map(|g| 256 + i64::from(g)))
        .unwrap_or(-EINVAL)
}

/// `os_try_wait` (R1d): the child's end if it has come, `-EWOULDBLOCK` at once if it has not.
fn try_wait(c: &Child) -> i64 {
    if let Some(s) = c.status.lock().ok().and_then(|s| *s) {
        return s;
    }
    let Ok(mut p) = c.process.lock() else {
        return -EINVAL;
    };
    match p.try_wait() {
        Ok(Some(st)) => {
            let s = decoded(st);
            if let Ok(mut slot) = c.status.lock() {
                *slot = Some(s);
            }
            s
        }
        Ok(None) => -super::os::EAGAIN,
        Err(e) => status(&e),
    }
}

/// `os_wait_until` (R1d): the child's end if it comes before `time_now_ms()` reads `deadline`,
/// `-ETIMEDOUT` then; looking again at most every 10 ms, as natively.
fn wait_until(c: &Child, deadline: i64) -> i64 {
    loop {
        let s = try_wait(c);
        if s != -super::os::EAGAIN {
            return s;
        }
        let now = super::intrinsic::monotonic_ms();
        if now >= deadline {
            return -ETIMEDOUT;
        }
        let nap = (deadline - now).min(10);
        std::thread::sleep(std::time::Duration::from_millis(nap.unsigned_abs()));
    }
}

/// `os_kill_group` (R1d): by the system's `kill`, the interpreter having no C to call `kill(2)`
/// with; a refusal is `EPERM` when `kill` says so, `ESRCH` otherwise.
fn kill_group(group: i64, sig: i64) -> i64 {
    if group < 2 || group > i64::from(i32::MAX) || !(0..32).contains(&sig) {
        return -EINVAL;
    }
    // `--` before the negative id: procps `kill` reads `-123` after a signal as another signal
    // otherwise, and BSD `kill` accepts both spellings.
    let ran = std::process::Command::new("kill")
        .arg(format!("-{sig}"))
        .arg("--")
        .arg(format!("-{group}"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();
    match ran {
        Ok(o) if o.status.success() => 0,
        Ok(o) => {
            if String::from_utf8_lossy(&o.stderr).contains("ermitted") {
                -EPERM
            } else {
                -ESRCH
            }
        }
        Err(e) => status(&e),
    }
}

/// The child's exit: its code, or 256 and the signal that ended it; the same answer every time.
fn wait(c: &Child) -> i64 {
    if let Some(s) = c.status.lock().ok().and_then(|s| *s) {
        return s;
    }
    let Ok(mut p) = c.process.lock() else {
        return -EINVAL;
    };
    match p.wait() {
        Ok(st) => {
            let s = decoded(st);
            if let Ok(mut slot) = c.status.lock() {
                *slot = Some(s);
            }
            s
        }
        Err(e) => status(&e),
    }
}
