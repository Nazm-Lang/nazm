//! Sockets (Gate 2, `docs/spec.md`, *Networking*): the network `os_` built-ins over the handle
//! table `eval::os` keeps. Every interpreted task is a thread, so a wait here is a blocking call
//! with the handle's deadline; what the native runtime does with a reactor, the interpreter does
//! by blocking the task's own thread.
//!
//! The one file of the toolchain that reaches a network (`xtask`'s *network reach* gate), and only
//! as a program holding a `NetCap` asks: the compiler, the checker and every tool beside them
//! still cannot.

use std::io::{Read as _, Write as _};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs as _, UdpSocket};
use std::os::fd::AsRawFd as _;
use std::time::{Duration, Instant};

use nazm_sema::Intrinsic;

use super::os::{object, register, set_timeout, timeout_of, Object, CLOSED, NAME_NOT_FOUND};
use super::value::{BytesView, Str, Value};

const EINVAL: i64 = 22;

/// The socket types the handle table holds (`eval::os::Object`).
pub(super) type Stream = TcpStream;
pub(super) type Listener = TcpListener;
pub(super) type Datagrams = UdpSocket;

/// Shut a connected socket both ways, so a task waiting on it wakes.
pub(super) fn shut(s: &TcpStream) {
    let _ = s.shutdown(Shutdown::Both);
}

/// The host's `ETIMEDOUT`: what a deadline that runs out answers, as natively.
const fn etimedout() -> i64 {
    if cfg!(target_os = "linux") {
        110
    } else {
        60
    }
}

fn status(e: &std::io::Error) -> i64 {
    match e.raw_os_error() {
        // A read or write past `SO_RCVTIMEO` reports `EAGAIN`; the language says a deadline ran
        // out.
        Some(_) if e.kind() == std::io::ErrorKind::WouldBlock => -etimedout(),
        Some(n) => -i64::from(n),
        None if e.kind() == std::io::ErrorKind::TimedOut => -etimedout(),
        None => -EINVAL,
    }
}

/// `HOST:PORT` with a literal host, or `None`.
fn address(s: &Str) -> Option<SocketAddr> {
    std::str::from_utf8(s.bytes()).ok()?.parse().ok()
}

fn shown(a: std::io::Result<SocketAddr>) -> Value {
    Value::Str(Str::heap(
        a.map(|a| a.to_string()).unwrap_or_default().into_bytes(),
    ))
}

pub(super) fn net_value(op: Intrinsic, args: &[Value]) -> Option<Value> {
    let int = |n: i64| Some(Value::Int(n));
    let handle = |n: i64| Some(Value::OsHandle(n));
    match (op, args) {
        (Intrinsic::OsTcpListen, [Value::Str(a)]) => {
            let Some(at) = address(a) else {
                return handle(-EINVAL);
            };
            handle(match TcpListener::bind(at) {
                Ok(l) => register(i64::from(l.as_raw_fd()), Object::Listener(l)),
                Err(e) => status(&e),
            })
        }
        (Intrinsic::OsTcpAccept, [Value::OsHandle(h)]) => handle(match object(*h) {
            Err(s) => s,
            Ok(o) => match &*o {
                Object::Listener(l) => {
                    let deadline = timeout_of(*h).map(|d| Instant::now() + d);
                    if l.set_nonblocking(true).is_err() {
                        return handle(-EINVAL);
                    }
                    loop {
                        match l.accept() {
                            Ok((s, _)) => {
                                let _ = s.set_nonblocking(false);
                                break register(i64::from(s.as_raw_fd()), Object::Tcp(s));
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                if object(*h).is_err() {
                                    break CLOSED;
                                }
                                if deadline.is_some_and(|d| Instant::now() >= d) {
                                    break -etimedout();
                                }
                                std::thread::sleep(Duration::from_millis(1));
                            }
                            Err(e) => break status(&e),
                        }
                    }
                }
                _ => -EINVAL,
            },
        }),
        (Intrinsic::OsTcpConnect, [Value::Str(a), Value::Int(ms)]) => {
            let Some(at) = address(a) else {
                return handle(-EINVAL);
            };
            let r = if *ms > 0 {
                TcpStream::connect_timeout(&at, Duration::from_millis(ms.unsigned_abs()))
            } else {
                TcpStream::connect(at)
            };
            handle(match r {
                Ok(s) => register(i64::from(s.as_raw_fd()), Object::Tcp(s)),
                Err(e) => status(&e),
            })
        }
        (Intrinsic::OsUdpBind, [Value::Str(a)]) => {
            let Some(at) = address(a) else {
                return handle(-EINVAL);
            };
            handle(match UdpSocket::bind(at) {
                Ok(u) => register(i64::from(u.as_raw_fd()), Object::Udp(u)),
                Err(e) => status(&e),
            })
        }
        (Intrinsic::OsUdpSendTo, [Value::OsHandle(h), Value::Bytes(data), Value::Str(a)]) => {
            let Some(at) = address(a) else {
                return int(-EINVAL);
            };
            int(match object(*h) {
                Err(s) => s,
                Ok(o) => match &*o {
                    Object::Udp(u) => match u.send_to(&data.read(), at) {
                        Ok(n) => i64::try_from(n).unwrap_or(-EINVAL),
                        Err(e) => status(&e),
                    },
                    _ => -EINVAL,
                },
            })
        }
        (Intrinsic::OsUdpRecvFrom, [Value::OsHandle(h), Value::Bytes(buf), Value::Bytes(from)]) => {
            int(match object(*h) {
                Err(s) => s,
                Ok(o) => match &*o {
                    Object::Udp(u) => udp_recv(*h, u, buf, from),
                    _ => -EINVAL,
                },
            })
        }
        (Intrinsic::OsSetTimeout, [Value::OsHandle(h), Value::Int(ms)]) => {
            int(set_timeout(*h, *ms))
        }
        (Intrinsic::OsShutdown, [Value::OsHandle(h), Value::Int(how)]) => int(match object(*h) {
            Err(s) => s,
            Ok(o) => match (&*o, how) {
                (Object::Tcp(s), 0) => done(s.shutdown(Shutdown::Read)),
                (Object::Tcp(s), 1) => done(s.shutdown(Shutdown::Write)),
                (Object::Tcp(s), 2) => done(s.shutdown(Shutdown::Both)),
                _ => -EINVAL,
            },
        }),
        (Intrinsic::OsLocalAddr, [Value::OsHandle(h)]) => Some(match object(*h).as_deref() {
            Ok(Object::Tcp(s)) => shown(s.local_addr()),
            Ok(Object::Listener(l)) => shown(l.local_addr()),
            Ok(Object::Udp(u)) => shown(u.local_addr()),
            _ => Value::Str(Str::heap(Vec::new())),
        }),
        (Intrinsic::OsPeerAddr, [Value::OsHandle(h)]) => Some(match object(*h).as_deref() {
            Ok(Object::Tcp(s)) => shown(s.peer_addr()),
            _ => Value::Str(Str::heap(Vec::new())),
        }),
        (Intrinsic::OsResolve, [Value::Str(name), Value::Int(port), Value::Bytes(out)]) => {
            let Ok(host) = std::str::from_utf8(name.bytes()) else {
                return int(NAME_NOT_FOUND);
            };
            let Ok(port) = u16::try_from(*port) else {
                return int(-EINVAL);
            };
            int(match (host, port).to_socket_addrs() {
                Err(_) => NAME_NOT_FOUND,
                Ok(all) => {
                    let text = all
                        .map(|a| a.to_string())
                        .collect::<Vec<_>>()
                        .join("\n")
                        .into_bytes();
                    if text.len() > out.len {
                        -EINVAL
                    } else {
                        out.write(0, &text);
                        i64::try_from(text.len()).unwrap_or(-EINVAL)
                    }
                }
            })
        }
        _ => None,
    }
}

fn done(r: std::io::Result<()>) -> i64 {
    r.map_or_else(|e| status(&e), |()| 0)
}

/// A read from a connected socket, bounded by the handle's deadline.
pub(super) fn tcp_read(h: i64, s: &TcpStream, buf: &BytesView) -> i64 {
    let _ = s.set_read_timeout(timeout_of(h));
    let mut got = vec![0; buf.len];
    let r = (&*s).read(&mut got);
    if object(h).is_err() {
        return CLOSED;
    }
    match r {
        Ok(n) => {
            buf.write(0, got.get(..n).unwrap_or_default());
            i64::try_from(n).unwrap_or(-EINVAL)
        }
        Err(e) => status(&e),
    }
}

/// A write to a connected socket, bounded by the handle's deadline.
pub(super) fn tcp_write(h: i64, s: &TcpStream, data: &BytesView) -> i64 {
    let _ = s.set_write_timeout(timeout_of(h));
    let r = (&*s).write(&data.read());
    if object(h).is_err() {
        return CLOSED;
    }
    match r {
        Ok(n) => i64::try_from(n).unwrap_or(-EINVAL),
        Err(e) => status(&e),
    }
}

/// A datagram into `buf` (cut to its length) and its sender's address into `from`, waiting at
/// most the handle's deadline. The wait is taken in short slices so that a close from another
/// task is noticed, as natively it wakes the receiver.
fn udp_recv(h: i64, u: &UdpSocket, buf: &BytesView, from: &BytesView) -> i64 {
    let deadline = timeout_of(h).map(|d| Instant::now() + d);
    let slice = Duration::from_millis(20);
    let mut got = vec![0; buf.len];
    loop {
        let wait = deadline.map_or(slice, |d| {
            d.saturating_duration_since(Instant::now()).min(slice)
        });
        if wait.is_zero() {
            return -etimedout();
        }
        let _ = u.set_read_timeout(Some(wait));
        match u.recv_from(&mut got) {
            Ok((n, who)) => {
                if object(h).is_err() {
                    return CLOSED;
                }
                buf.write(0, got.get(..n).unwrap_or_default());
                let mut text = who.to_string().into_bytes();
                text.resize(from.len, 0);
                from.write(0, text.get(..from.len).unwrap_or_default());
                return i64::try_from(n).unwrap_or(-EINVAL);
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if object(h).is_err() {
                    return CLOSED;
                }
            }
            Err(e) => return status(&e),
        }
    }
}
