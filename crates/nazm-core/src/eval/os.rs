//! Files and handles (Gate 2): the `os_` built-ins, over the process's handle table.
//!
//! `docs/spec.md`, *Files and handles*, is the rule, and the native runtime's `nz.os_*` entries
//! are the other implementation of it. A handle is a word, `generation << 32 | descriptor`; the
//! table remembers each descriptor's current generation, so a handle whose generation has been
//! closed answers `Closed` and never reaches whatever the system later opened on the same
//! descriptor. The object an operation uses is taken out of the table under its lock and used
//! outside it, so a close while a read is in progress takes effect — the descriptor is released —
//! when the read lets go of it, as natively.
//!
//! A failed open answers its status as the handle, `-errno`, which is negative and so never a
//! live handle's word. Sockets (`eval::net`) are handles in the same table.

use std::collections::HashMap;
use std::fs::{File, OpenOptions, ReadDir};
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::sync::{Arc, Mutex, OnceLock};

use nazm_sema::Intrinsic;

use super::value::{BytesView, Str, Value};

/// The status of a handle that is closed: no system's error number, so `@std/ioerror` gives it
/// the kind `Closed` on every platform. `EBADF` is the system's own — a file written that was
/// opened to read. `EINVAL`, `EISDIR`, `ENOTDIR` and `ESPIPE`: a mode or `whence` out of range, a
/// directory used as a file, a file listed, standard input sought. The same numbers on every
/// supported platform.
pub(super) const CLOSED: i64 = -65_536;
const EBADF: i64 = 9;
const EINVAL: i64 = 22;
const EISDIR: i64 = 21;
const ENOTDIR: i64 = 20;
const ESPIPE: i64 = 29;
/// The status of a name the resolver did not answer: no system's error number either, so
/// `@std/ioerror` gives it the kind `NotFound` on every platform.
pub(super) const NAME_NOT_FOUND: i64 = -65_537;

/// What a live handle names.
#[derive(Debug)]
pub(super) enum Object {
    File(File),
    /// The directory's own descriptor, which the table is keyed by, and its listing.
    Dir {
        _descriptor: File,
        list: Mutex<ReadDir>,
    },
    Stdin,
    /// A connected TCP socket, a listening one, and a UDP one (Gate 2, `eval::net`, the one
    /// module of the interpreter that reaches a network).
    Tcp(super::net::Stream),
    Listener(super::net::Listener),
    Udp(super::net::Datagrams),
    /// A child process and its three pipes (Gate 2, `eval::process`).
    Child(super::process::Child),
    PipeIn(std::process::ChildStdin),
    PipeOut(Mutex<super::process::Output>),
}

/// One descriptor's slot: its current generation, its object while open, and the deadline in
/// milliseconds every wait on it has (`0`: none).
type Slot = (i64, Option<Arc<Object>>, i64);

#[derive(Debug, Default)]
struct Table {
    /// Each descriptor's slot.
    slots: HashMap<i64, Slot>,
    /// Standard input's handle, once a program has asked for it.
    stdin: Option<i64>,
}

fn table() -> &'static Mutex<Table> {
    static TABLE: OnceLock<Mutex<Table>> = OnceLock::new();
    TABLE.get_or_init(Mutex::default)
}

/// The host's family of error numbers: `0` for macOS, `1` for Linux.
const fn family() -> i64 {
    if cfg!(target_os = "linux") {
        1
    } else {
        0
    }
}

/// `-errno` for a failure the system reported, and `EINVAL` for one it did not number.
fn status(e: &std::io::Error) -> i64 {
    -i64::from(e.raw_os_error().unwrap_or(22))
}

/// A new handle for `object` on descriptor `fd`: the descriptor's next generation, with no
/// deadline.
pub(super) fn register(fd: i64, object: Object) -> i64 {
    let Ok(mut t) = table().lock() else {
        return CLOSED;
    };
    let slot = t.slots.entry(fd).or_insert((0, None, 0));
    slot.0 += 1;
    slot.1 = Some(Arc::new(object));
    slot.2 = 0;
    (slot.0 << 32) | fd
}

/// The object a live handle names, or its status.
pub(super) fn object(h: i64) -> Result<Arc<Object>, i64> {
    if h < 0 {
        return Err(h);
    }
    let (fd, generation) = (h & 0xFFFF_FFFF, h >> 32);
    let t = table().lock().map_err(|_| CLOSED)?;
    match t.slots.get(&fd) {
        Some((g, Some(o), _)) if *g == generation => Ok(Arc::clone(o)),
        _ => Err(CLOSED),
    }
}

/// The deadline every wait on a live handle has, if it has one.
pub(super) fn timeout_of(h: i64) -> Option<std::time::Duration> {
    let (fd, generation) = (h & 0xFFFF_FFFF, h >> 32);
    let t = table().lock().ok()?;
    match t.slots.get(&fd) {
        Some((g, Some(_), ms)) if *g == generation && *ms > 0 => {
            Some(std::time::Duration::from_millis(ms.unsigned_abs()))
        }
        _ => None,
    }
}

/// Give a live handle a deadline of `ms` milliseconds (`0`: none): `0`, or its status.
pub(super) fn set_timeout(h: i64, ms: i64) -> i64 {
    if h < 0 {
        return h;
    }
    if ms < 0 {
        return -EINVAL;
    }
    let (fd, generation) = (h & 0xFFFF_FFFF, h >> 32);
    let Ok(mut t) = table().lock() else {
        return CLOSED;
    };
    match t.slots.get_mut(&fd) {
        Some((g, Some(_), deadline)) if *g == generation => {
            *deadline = ms;
            0
        }
        _ => CLOSED,
    }
}

fn path(v: &Str) -> String {
    String::from_utf8_lossy(v.bytes()).into_owned()
}

fn opened(r: std::io::Result<File>) -> i64 {
    match r {
        Ok(f) => {
            let fd = i64::from(f.as_raw_fd());
            register(fd, Object::File(f))
        }
        Err(e) => status(&e),
    }
}

/// `n` as an `Int` count, or the status of a count no `Int` can hold.
fn counted(r: std::io::Result<usize>) -> i64 {
    match r {
        Ok(n) => i64::try_from(n).unwrap_or(-EINVAL),
        Err(e) => status(&e),
    }
}

fn done(r: std::io::Result<()>) -> i64 {
    r.map_or_else(|e| status(&e), |()| 0)
}

/// The `os_` built-in `op` on `args`, or `None` for any other built-in. Only `os_stat` handed a
/// buffer too short for its answer stops the program (`N0405`), with the sentence the compiled
/// program writes; every other failure is a status.
pub(super) fn os_intrinsic(
    op: Intrinsic,
    args: &[Value],
    at: nazm_span::Span,
) -> Option<Result<Value, super::RuntimeError>> {
    if let Some(r) = super::process::env_at_failure(op, args, at) {
        return Some(Err(r));
    }
    if let (Intrinsic::OsStat, [_, Value::Bytes(out)]) = (op, args) {
        if out.len < 32 {
            return Some(Err(super::RuntimeError {
                diagnostic: Box::new(
                    nazm_diag::Diagnostic::error(
                        nazm_diag::codes::INDEX_OUT_OF_BOUNDS,
                        at,
                        "this buffer is too short for the answer",
                    )
                    .help(format!(
                        "`os_stat` writes 32 bytes; this `Bytes` is {} long",
                        out.len
                    )),
                ),
            }));
        }
    }
    os_value(op, args).map(Ok)
}

#[allow(clippy::too_many_lines)]
fn os_value(op: Intrinsic, args: &[Value]) -> Option<Value> {
    let int = |n: i64| Some(Value::Int(n));
    let handle = |n: i64| Some(Value::OsHandle(n));
    match (op, args) {
        (Intrinsic::OsOpenRead, [Value::Str(p)]) => handle(opened(File::open(path(p)))),
        (Intrinsic::OsOpenWrite, [Value::Str(p), Value::Int(mode)]) => {
            let mut o = OpenOptions::new();
            match mode {
                0 => o.write(true).create(true).truncate(true),
                1 => o.append(true).create(true),
                2 => o.write(true).create_new(true),
                3 => o.read(true).write(true),
                _ => return handle(-EINVAL),
            };
            handle(opened(o.mode(0o666).open(path(p))))
        }
        (Intrinsic::OsDirOpen, [Value::Str(p)]) => {
            // The directory is opened as a descriptor too, so its handle names one as a native
            // `DIR *`'s does, and holding it keeps another open from taking its number.
            let name = path(p);
            match (File::open(&name), std::fs::read_dir(&name)) {
                (Ok(d), Ok(list)) if d.metadata().is_ok_and(|m| m.is_dir()) => {
                    let fd = i64::from(d.as_raw_fd());
                    handle(register(
                        fd,
                        Object::Dir {
                            _descriptor: d,
                            list: Mutex::new(list),
                        },
                    ))
                }
                (Ok(_), Ok(_)) => handle(-ENOTDIR),
                (Err(e), _) | (_, Err(e)) => handle(status(&e)),
            }
        }
        (Intrinsic::OsStdin, []) => {
            let mut t = table().lock().ok()?;
            if let Some(h) = t.stdin {
                return handle(h);
            }
            let slot = t.slots.entry(0).or_insert((0, None, 0));
            slot.0 += 1;
            slot.1 = Some(Arc::new(Object::Stdin));
            slot.2 = 0;
            let h = slot.0 << 32;
            t.stdin = Some(h);
            handle(h)
        }
        (Intrinsic::OsStatus, [Value::OsHandle(h)]) => int(object(*h).map_or_else(|s| s, |_| 0)),
        (Intrinsic::OsRead, [Value::OsHandle(h), Value::Bytes(buf)]) => int(match object(*h) {
            Err(s) => s,
            Ok(o) => {
                let mut got = vec![0; buf.len];
                let r = match &*o {
                    Object::File(f) => (&*f).read(&mut got),
                    Object::Stdin => std::io::stdin().lock().read(&mut got),
                    Object::Dir { .. } => return int(-EISDIR),
                    Object::Tcp(s) => return int(super::net::tcp_read(*h, s, buf)),
                    Object::PipeOut(p) => super::process::pipe_read(p, &mut got),
                    Object::Listener(_) | Object::Udp(_) | Object::Child(_) | Object::PipeIn(_) => {
                        return int(-EINVAL)
                    }
                };
                match r {
                    Ok(n) => {
                        write(buf, got.get(..n).unwrap_or_default());
                        i64::try_from(n).unwrap_or(-EINVAL)
                    }
                    Err(e) => status(&e),
                }
            }
        }),
        // A line of standard input (R1d, P3): through the same buffer `os_read` reads standard
        // input through, so what follows the line is still there for either.
        (Intrinsic::OsReadLine, [Value::OsHandle(h), Value::Bytes(buf)]) => int(match object(*h) {
            Err(s) => s,
            Ok(o) => match &*o {
                Object::Stdin => read_line(buf),
                Object::Dir { .. } => -EISDIR,
                Object::File(_)
                | Object::Tcp(_)
                | Object::Listener(_)
                | Object::Udp(_)
                | Object::Child(_)
                | Object::PipeIn(_)
                | Object::PipeOut(_) => -EINVAL,
            },
        }),
        (Intrinsic::OsWrite, [Value::OsHandle(h), Value::Bytes(data)]) => int(match object(*h) {
            Err(s) => s,
            Ok(o) => match &*o {
                Object::File(f) => counted((&*f).write(&data.read())),
                Object::Stdin => -EBADF,
                Object::Dir { .. } => -EISDIR,
                Object::Tcp(s) => super::net::tcp_write(*h, s, data),
                Object::PipeIn(p) => counted((&*p).write(&data.read())),
                Object::Listener(_) | Object::Udp(_) | Object::Child(_) | Object::PipeOut(_) => {
                    -EINVAL
                }
            },
        }),
        (Intrinsic::OsSeek, [Value::OsHandle(h), Value::Int(offset), Value::Int(whence)]) => {
            int(match object(*h) {
                Err(s) => s,
                Ok(o) => match &*o {
                    Object::File(f) => {
                        let from = match (whence, u64::try_from(*offset)) {
                            (0, Ok(n)) => SeekFrom::Start(n),
                            (0, Err(_)) => return int(-EINVAL),
                            (1, _) => SeekFrom::Current(*offset),
                            (2, _) => SeekFrom::End(*offset),
                            _ => return int(-EINVAL),
                        };
                        match (&*f).seek(from) {
                            Ok(n) => i64::try_from(n).unwrap_or(-EINVAL),
                            Err(e) => status(&e),
                        }
                    }
                    Object::Stdin
                    | Object::Tcp(_)
                    | Object::Listener(_)
                    | Object::Udp(_)
                    | Object::Child(_)
                    | Object::PipeIn(_)
                    | Object::PipeOut(_) => -ESPIPE,
                    Object::Dir { .. } => -EISDIR,
                },
            })
        }
        (Intrinsic::OsSync, [Value::OsHandle(h)]) => int(match object(*h) {
            Err(s) => s,
            Ok(o) => match &*o {
                Object::File(f) => done(f.sync_all()),
                Object::Stdin
                | Object::Tcp(_)
                | Object::Listener(_)
                | Object::Udp(_)
                | Object::Child(_)
                | Object::PipeIn(_)
                | Object::PipeOut(_) => -EINVAL,
                Object::Dir { .. } => -EISDIR,
            },
        }),
        (Intrinsic::OsClose, [Value::OsHandle(h)]) => {
            if *h < 0 {
                return int(*h);
            }
            let (fd, generation) = (h & 0xFFFF_FFFF, h >> 32);
            let Ok(mut t) = table().lock() else {
                return int(CLOSED);
            };
            match t.slots.get_mut(&fd) {
                Some((g, o @ Some(_), _)) if *g == generation => {
                    // Out of the table, and released when the last use in progress lets go. A
                    // connected socket is shut first, so a task waiting on it wakes — and, finding
                    // the handle closed, answers `Closed`, as natively.
                    let closing = o.take();
                    drop(t);
                    if let Some(Object::Tcp(s)) = closing.as_deref() {
                        super::net::shut(s);
                    }
                    drop(closing);
                    int(0)
                }
                _ => int(CLOSED),
            }
        }
        (Intrinsic::OsDirNext, [Value::OsHandle(h), Value::Bytes(name)]) => int(match object(*h) {
            Err(s) => s,
            Ok(o) => match &*o {
                Object::Dir { list, .. } => {
                    let Ok(mut list) = list.lock() else {
                        return int(CLOSED);
                    };
                    match list.next() {
                        None => 0,
                        Some(Err(e)) => status(&e),
                        Some(Ok(entry)) => {
                            let n = entry.file_name();
                            let bytes = std::os::unix::ffi::OsStrExt::as_bytes(n.as_os_str());
                            if bytes.len() > name.len {
                                -EINVAL
                            } else {
                                write(name, bytes);
                                i64::try_from(bytes.len()).unwrap_or(-EINVAL)
                            }
                        }
                    }
                }
                Object::File(_)
                | Object::Stdin
                | Object::Tcp(_)
                | Object::Listener(_)
                | Object::Udp(_)
                | Object::Child(_)
                | Object::PipeIn(_)
                | Object::PipeOut(_) => -ENOTDIR,
            },
        }),
        (Intrinsic::OsStat, [Value::Str(p), Value::Bytes(out)]) => {
            int(match std::fs::metadata(path(p)) {
                Err(e) => status(&e),
                Ok(m) => {
                    let kind = if m.is_file() {
                        0
                    } else if m.is_dir() {
                        1
                    } else {
                        2
                    };
                    let modified = m.mtime().saturating_mul(1_000_000_000) + m.mtime_nsec();
                    let mut b = Vec::with_capacity(32);
                    for w in [
                        i64::try_from(m.size()).unwrap_or(i64::MAX),
                        kind,
                        modified,
                        i64::from(m.mode() & 0o7777),
                    ] {
                        b.extend_from_slice(&w.to_le_bytes());
                    }
                    write(out, &b);
                    0
                }
            })
        }
        (Intrinsic::OsMkdir, [Value::Str(p)]) => int(done(std::fs::create_dir(path(p)))),
        (Intrinsic::OsRmdir, [Value::Str(p)]) => int(done(std::fs::remove_dir(path(p)))),
        (Intrinsic::OsUnlink, [Value::Str(p)]) => int(done(std::fs::remove_file(path(p)))),
        (Intrinsic::OsRename, [Value::Str(a), Value::Str(b)]) => {
            int(done(std::fs::rename(path(a), path(b))))
        }
        (Intrinsic::OsFamily, []) => int(family()),
        (Intrinsic::OsErrorText, [Value::Int(s)]) => {
            let text = match s.checked_neg().and_then(|e| i32::try_from(e).ok()) {
                _ if *s == CLOSED => "the handle is closed".to_owned(),
                _ if *s == NAME_NOT_FOUND => "the name was not found".to_owned(),
                Some(e) if e > 0 => {
                    let full = std::io::Error::from_raw_os_error(e).to_string();
                    let suffix = format!(" (os error {e})");
                    full.strip_suffix(&suffix).unwrap_or(&full).to_owned()
                }
                _ => String::new(),
            };
            Some(Value::Str(Str::heap(text.into_bytes())))
        }
        _ => super::net::net_value(op, args).or_else(|| super::process::process_value(op, args)),
    }
}

/// Standard input up to and including its next newline into `view`, stopping sooner at the
/// view's length or at the end (R1d, P3): the count, `0` at the end. What follows the newline
/// stays in the process's buffer. A failure is answered only when nothing was read; otherwise the
/// next read meets it again, as natively.
fn read_line(view: &BytesView) -> i64 {
    use std::io::BufRead as _;
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut got: Vec<u8> = Vec::new();
    while got.len() < view.len {
        let held = match input.fill_buf() {
            Ok(held) => held,
            Err(e) if got.is_empty() => return status(&e),
            Err(_) => break,
        };
        if held.is_empty() {
            break;
        }
        let room = view.len - got.len();
        let window = held.get(..room.min(held.len())).unwrap_or_default();
        let (take, ended) = match window.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (window.len(), false),
        };
        got.extend_from_slice(window.get(..take).unwrap_or_default());
        input.consume(take);
        if ended {
            break;
        }
    }
    write(view, &got);
    i64::try_from(got.len()).unwrap_or(-EINVAL)
}

/// `bytes` over the start of `view`.
fn write(view: &BytesView, bytes: &[u8]) {
    view.write(0, bytes);
}
