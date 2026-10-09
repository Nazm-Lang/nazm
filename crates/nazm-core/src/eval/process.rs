//! The environment and child processes (Gate 2, `docs/spec.md`, *Process and environment*): the
//! `os_env_*` built-ins, and `os_spawn` and the child's handles over the table `eval::os` keeps.
//!
//! A child is a handle like any other, keyed by a descriptor it holds open on `/dev/null` — a child
//! has no descriptor of its own, and the native runtime keys it the same way — and its three pipes
//! are handles too, read and written with `os_read` and `os_write`.

use std::io::Read as _;
use std::os::fd::AsRawFd as _;
use std::os::unix::process::ExitStatusExt as _;
use std::sync::Mutex;

use nazm_sema::Intrinsic;

use super::os::{object, register, Object};
use super::value::{Str, Value};

const EINVAL: i64 = 22;

/// A child: the descriptor its handle is keyed by, the process, its three pipes' handles, and
/// its status once it has been waited for.
#[derive(Debug)]
pub(super) struct Child {
    _key: std::fs::File,
    process: Mutex<std::process::Child>,
    pipes: [i64; 3],
    status: Mutex<Option<i64>>,
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
        (Intrinsic::OsSpawn, [Value::Str(program), Value::Strs(list)]) => {
            let argv: Vec<String> = list.lock().ok()?.iter().map(text).collect();
            handle(spawn(&text(program), &argv))
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
    register(
        fd,
        Object::Child(Child {
            _key: key,
            process: Mutex::new(child),
            pipes: [pin, pout, perr],
            status: Mutex::new(None),
        }),
    )
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
            let s = st
                .code()
                .map(i64::from)
                .or_else(|| st.signal().map(|g| 256 + i64::from(g)))
                .unwrap_or(-EINVAL);
            if let Ok(mut slot) = c.status.lock() {
                *slot = Some(s);
            }
            s
        }
        Err(e) => status(&e),
    }
}
