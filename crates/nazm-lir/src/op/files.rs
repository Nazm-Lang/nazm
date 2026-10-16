//! Files and handles (Gate 2, `docs/spec.md`, *Files and handles*): each `os_` built-in as the
//! runtime call it means. A handle is a word, a path a `Str`'s pointer and length, a buffer a
//! `Bytes`'s; every failure the system reports comes back as a status, so nothing here traps
//! but `os_stat` or `os_lstat` handed a buffer too short for its answer (`N0405`).

use nazm_mir as mir;
use nazm_sema::Intrinsic;

use super::func::Fx;
use super::ir::{Class, Cmp};

/// The room `os_error_text` gives the system's message: more than any message is.
const MESSAGE_ROOM: i64 = 256;
/// The room an address gets: `[` an IPv6 address `]:` a port is at most 47 bytes.
const ADDRESS_ROOM: i64 = 80;
/// What a temporary directory's path adds to its parent's (R1d): a `/`, `nazm-` and twelve.
const MKDTEMP_ROOM: i64 = 18;

impl Fx<'_, '_> {
    /// `dst = op(args)` for an `os_` built-in; `false` for any other.
    pub(crate) fn files(
        &mut self,
        dst: mir::LocalId,
        op: Intrinsic,
        args: &[mir::Operand],
    ) -> bool {
        let v = match op {
            Intrinsic::OsOpenRead | Intrinsic::OsDirOpen => {
                let p = self.arg_place(args, 0);
                let (pp, pn) = self.str_parts(p);
                let name = if op == Intrinsic::OsOpenRead {
                    "nz.os_open_read"
                } else {
                    "nz.os_dir_open"
                };
                self.rt_v(name, &[pp, pn], Class::I64)
            }
            Intrinsic::OsOpenWrite => {
                let p = self.arg_place(args, 0);
                let mode = self.arg(args, 1);
                let (pp, pn) = self.str_parts(p);
                self.rt_v("nz.os_open_write", &[pp, pn, mode], Class::I64)
            }
            Intrinsic::OsStdin => self.rt_v("nz.os_stdin", &[], Class::I64),
            Intrinsic::OsFamily => self.rt_v("nz.os_family", &[], Class::I64),
            Intrinsic::OsStatus | Intrinsic::OsSync | Intrinsic::OsClose => {
                let h = self.arg(args, 0);
                let name = match op {
                    Intrinsic::OsStatus => "nz.os_status",
                    Intrinsic::OsSync => "nz.os_sync",
                    _ => "nz.os_close",
                };
                self.rt_v(name, &[h], Class::I64)
            }
            Intrinsic::OsRead
            | Intrinsic::OsReadLine
            | Intrinsic::OsWrite
            | Intrinsic::OsDirNext => {
                let h = self.arg(args, 0);
                let b = self.arg_place(args, 1);
                let (bp, bn) = self.str_parts(b);
                let name = match op {
                    Intrinsic::OsRead => "nz.os_read",
                    Intrinsic::OsReadLine => "nz.os_read_line",
                    Intrinsic::OsWrite => "nz.os_write",
                    _ => "nz.os_dir_next",
                };
                self.rt_v(name, &[h, bp, bn], Class::I64)
            }
            Intrinsic::OsSeek => {
                let h = self.arg(args, 0);
                let offset = self.arg(args, 1);
                let whence = self.arg(args, 2);
                self.rt_v("nz.os_seek", &[h, offset, whence], Class::I64)
            }
            Intrinsic::OsStat => {
                let p = self.arg_place(args, 0);
                let b = self.arg_place(args, 1);
                let (pp, pn) = self.str_parts(p);
                let (bp, bn) = self.str_parts(b);
                let short = self.cmp_imm(Cmp::Lt, bn, Class::I64, 32);
                self.guard(short, "N0405", "this buffer is too short for the answer");
                self.rt_v("nz.os_stat", &[pp, pn, bp], Class::I64)
            }
            Intrinsic::OsMkdir | Intrinsic::OsRmdir | Intrinsic::OsUnlink => {
                let p = self.arg_place(args, 0);
                let (pp, pn) = self.str_parts(p);
                let which = self.iconst(match op {
                    Intrinsic::OsMkdir => 0,
                    Intrinsic::OsRmdir => 1,
                    _ => 2,
                });
                self.rt_v("nz.os_path_op", &[pp, pn, which], Class::I64)
            }
            Intrinsic::OsRename => {
                let a = self.arg_place(args, 0);
                let b = self.arg_place(args, 1);
                let (ap, an) = self.str_parts(a);
                let (bp, bn) = self.str_parts(b);
                self.rt_v("nz.os_rename", &[ap, an, bp, bn], Class::I64)
            }
            Intrinsic::OsErrorText => {
                let status = self.arg(args, 0);
                let room = self.iconst(MESSAGE_ROOM);
                let (owner, buf) = self.string_backing(room);
                let n = self.rt_v("nz.os_error_text", &[status, buf, room], Class::I64);
                self.write_str(dst, buf, n, owner);
                return true;
            }
            // Networking (Gate 2): an address is a `Str`'s pointer and length too.
            Intrinsic::OsTcpListen | Intrinsic::OsUdpBind => {
                let a = self.arg_place(args, 0);
                let (ap, an) = self.str_parts(a);
                let name = if op == Intrinsic::OsTcpListen {
                    "nz.os_tcp_listen"
                } else {
                    "nz.os_udp_bind"
                };
                self.rt_v(name, &[ap, an], Class::I64)
            }
            Intrinsic::OsTcpConnect => {
                let a = self.arg_place(args, 0);
                let ms = self.arg(args, 1);
                let (ap, an) = self.str_parts(a);
                self.rt_v("nz.os_tcp_connect", &[ap, an, ms], Class::I64)
            }
            Intrinsic::OsTcpAccept => {
                let h = self.arg(args, 0);
                self.rt_v("nz.os_tcp_accept", &[h], Class::I64)
            }
            Intrinsic::OsSetTimeout | Intrinsic::OsShutdown => {
                let h = self.arg(args, 0);
                let k = self.arg(args, 1);
                let name = if op == Intrinsic::OsSetTimeout {
                    "nz.os_set_timeout"
                } else {
                    "nz.os_shutdown"
                };
                self.rt_v(name, &[h, k], Class::I64)
            }
            Intrinsic::OsUdpSendTo => {
                let h = self.arg(args, 0);
                let d = self.arg_place(args, 1);
                let a = self.arg_place(args, 2);
                let (dp, dn) = self.str_parts(d);
                let (ap, an) = self.str_parts(a);
                self.rt_v("nz.os_udp_send_to", &[h, dp, dn, ap, an], Class::I64)
            }
            Intrinsic::OsUdpRecvFrom => {
                let h = self.arg(args, 0);
                let b = self.arg_place(args, 1);
                let f = self.arg_place(args, 2);
                let (bp, bn) = self.str_parts(b);
                let (fp, fn_) = self.str_parts(f);
                self.rt_v("nz.os_udp_recv_from", &[h, bp, bn, fp, fn_], Class::I64)
            }
            Intrinsic::OsResolve => {
                let name = self.arg_place(args, 0);
                let port = self.arg(args, 1);
                let out = self.arg_place(args, 2);
                let (np, nn) = self.str_parts(name);
                let (op_, on) = self.str_parts(out);
                self.rt_v("nz.os_resolve", &[np, nn, port, op_, on], Class::I64)
            }
            // TLS (C1): an id is a word, every buffer a `Bytes`'s pointer and length.
            Intrinsic::OsTlsClientConfig => {
                let ca = self.arg_place(args, 0);
                let alpn = self.arg_place(args, 1);
                let (cp, cn) = self.str_parts(ca);
                let (ap, an) = self.str_parts(alpn);
                self.rt_v("nz.os_tls_client_config", &[cp, cn, ap, an], Class::I64)
            }
            Intrinsic::OsTlsServerConfig => {
                let cert = self.arg_place(args, 0);
                let key = self.arg_place(args, 1);
                let alpn = self.arg_place(args, 2);
                let (cp, cn) = self.str_parts(cert);
                let (kp, kn) = self.str_parts(key);
                let (ap, an) = self.str_parts(alpn);
                self.rt_v(
                    "nz.os_tls_server_config",
                    &[cp, cn, kp, kn, ap, an],
                    Class::I64,
                )
            }
            Intrinsic::OsTlsConnect => {
                let config = self.arg(args, 0);
                let host = self.arg_place(args, 1);
                let (hp, hn) = self.str_parts(host);
                self.rt_v("nz.os_tls_connect", &[config, hp, hn], Class::I64)
            }
            Intrinsic::OsTlsAccept | Intrinsic::OsTlsHandshake => {
                let id = self.arg(args, 0);
                let name = if op == Intrinsic::OsTlsAccept {
                    "nz.os_tls_accept"
                } else {
                    "nz.os_tls_handshake"
                };
                self.rt_v(name, &[id], Class::I64)
            }
            Intrinsic::OsTlsPush
            | Intrinsic::OsTlsPull
            | Intrinsic::OsTlsRead
            | Intrinsic::OsTlsWrite
            | Intrinsic::OsTlsClose => {
                let id = self.arg(args, 0);
                let b = self.arg_place(args, 1);
                let (bp, bn) = self.str_parts(b);
                let name = match op {
                    Intrinsic::OsTlsPush => "nz.os_tls_push",
                    Intrinsic::OsTlsPull => "nz.os_tls_pull",
                    Intrinsic::OsTlsRead => "nz.os_tls_read",
                    Intrinsic::OsTlsWrite => "nz.os_tls_write",
                    _ => "nz.os_tls_close",
                };
                self.rt_v(name, &[id, bp, bn], Class::I64)
            }
            Intrinsic::OsTlsInfo => {
                let id = self.arg(args, 0);
                let what = self.arg(args, 1);
                let out = self.arg_place(args, 2);
                let (op_, on) = self.str_parts(out);
                self.rt_v("nz.os_tls_info", &[id, what, op_, on], Class::I64)
            }
            Intrinsic::OsLocalAddr | Intrinsic::OsPeerAddr => {
                let h = self.arg(args, 0);
                let peer = self.iconst(i64::from(op == Intrinsic::OsPeerAddr));
                let room = self.iconst(ADDRESS_ROOM);
                let (owner, buf) = self.string_backing(room);
                let n = self.rt_v("nz.os_net_addr", &[h, peer, buf, room], Class::I64);
                self.write_str(dst, buf, n, owner);
                return true;
            }
            // Process and environment (Gate 2). A value or an entry is copied out of the
            // environment into a string the program owns.
            Intrinsic::OsEnvGet | Intrinsic::OsEnvHas => {
                let name = self.arg_place(args, 0);
                let (np, nn) = self.str_parts(name);
                let lenp = self.scratch(8);
                let p = self.rt_v("nz.os_env_find", &[np, nn, lenp], Class::Ptr);
                let len = self.load(Class::I64, lenp, 0);
                if op == Intrinsic::OsEnvHas {
                    self.cmp_imm(Cmp::Ge, len, Class::I64, 0)
                } else {
                    let zero = self.iconst(0);
                    let absent = self.cmp_imm(Cmp::Lt, len, Class::I64, 0);
                    let n = self.select(absent, zero, len);
                    self.fresh_copy(dst, p, n);
                    return true;
                }
            }
            Intrinsic::OsEnvCount => self.rt_v("nz.os_env_count", &[], Class::I64),
            Intrinsic::OsRandomBytes => {
                let b = self.arg_place(args, 0);
                let (bp, bn) = self.str_parts(b);
                self.rt_v("nz.os_random_bytes", &[bp, bn], Class::I64)
            }
            Intrinsic::OsEnvAt => {
                let i = self.arg(args, 0);
                let count = self.rt_v("nz.os_env_count", &[], Class::I64);
                self.within(i, 1, count, "this index is outside the environment");
                let lenp = self.scratch(8);
                let p = self.rt_v("nz.os_env_at", &[i, lenp], Class::Ptr);
                let len = self.load(Class::I64, lenp, 0);
                self.fresh_copy(dst, p, len);
                return true;
            }
            Intrinsic::OsSpawn => {
                let prog = self.arg_place(args, 0);
                let list = self.arg(args, 1);
                let (pp, pn) = self.str_parts(prog);
                self.rt_v("nz.os_spawn", &[pp, pn, list], Class::I64)
            }
            Intrinsic::OsChildPipe => {
                let h = self.arg(args, 0);
                let which = self.arg(args, 1);
                self.rt_v("nz.os_child_pipe", &[h, which], Class::I64)
            }
            Intrinsic::OsWait | Intrinsic::OsKill => {
                let h = self.arg(args, 0);
                let name = if op == Intrinsic::OsWait {
                    "nz.os_wait"
                } else {
                    "nz.os_kill"
                };
                self.rt_v(name, &[h], Class::I64)
            }
            // The platform, further (R1d).
            Intrinsic::OsStdout | Intrinsic::OsStderr => {
                let which = self.iconst(if op == Intrinsic::OsStdout { 1 } else { 2 });
                self.rt_v("nz.os_std_stream", &[which], Class::I64)
            }
            Intrinsic::OsSpawnIn | Intrinsic::OsSpawnWith => {
                let prog = self.arg_place(args, 0);
                let list = self.arg(args, 1);
                let dir = self.arg_place(args, 2);
                let env = self.arg(args, 3);
                let flags = if op == Intrinsic::OsSpawnWith {
                    self.arg(args, 4)
                } else {
                    self.iconst(0)
                };
                let (pp, pn) = self.str_parts(prog);
                let (dp, dn) = self.str_parts(dir);
                self.rt_v(
                    "nz.os_spawn_in",
                    &[pp, pn, list, dp, dn, env, flags],
                    Class::I64,
                )
            }
            Intrinsic::OsPid => self.rt_v("nz.os_pid", &[], Class::I64),
            Intrinsic::OsChildPid
            | Intrinsic::OsTryWait
            | Intrinsic::OsChildRusage
            | Intrinsic::OsUnlock => {
                let h = self.arg(args, 0);
                let name = match op {
                    Intrinsic::OsChildPid => "nz.os_child_pid",
                    Intrinsic::OsTryWait => "nz.os_try_wait",
                    Intrinsic::OsChildRusage => "nz.os_child_rusage",
                    _ => "nz.os_file_unlock",
                };
                self.rt_v(name, &[h], Class::I64)
            }
            Intrinsic::OsKillGroup => {
                let group = self.arg(args, 0);
                let sig = self.arg(args, 1);
                self.rt_v("nz.os_kill_group", &[group, sig], Class::I64)
            }
            Intrinsic::OsWaitUntil => {
                let h = self.arg(args, 0);
                let deadline = self.arg(args, 1);
                self.rt_v("nz.os_wait_until", &[h, deadline], Class::I64)
            }
            Intrinsic::OsLstat => {
                let p = self.arg_place(args, 0);
                let b = self.arg_place(args, 1);
                let (pp, pn) = self.str_parts(p);
                let (bp, bn) = self.str_parts(b);
                let short = self.cmp_imm(Cmp::Lt, bn, Class::I64, 48);
                self.guard(short, "N0405", "this buffer is too short for the answer");
                self.rt_v("nz.os_lstat", &[pp, pn, bp], Class::I64)
            }
            Intrinsic::OsChmod => {
                let p = self.arg_place(args, 0);
                let mode = self.arg(args, 1);
                let (pp, pn) = self.str_parts(p);
                self.rt_v("nz.os_chmod", &[pp, pn, mode], Class::I64)
            }
            Intrinsic::OsRealpath => {
                let p = self.arg_place(args, 0);
                let b = self.arg_place(args, 1);
                let (pp, pn) = self.str_parts(p);
                let (bp, bn) = self.str_parts(b);
                self.rt_v("nz.os_realpath", &[pp, pn, bp, bn], Class::I64)
            }
            // A new string, `parent`'s length and eighteen bytes, holding the path — or nothing,
            // when no directory could be made.
            Intrinsic::OsMkdtemp => {
                let p = self.arg_place(args, 0);
                let (pp, pn) = self.str_parts(p);
                let extra = self.iconst(MKDTEMP_ROOM);
                let room = self.bin(super::ir::BinOp::Add, pn, extra);
                let (owner, buf) = self.string_backing(room);
                let n = self.rt_v("nz.os_mkdtemp", &[pp, pn, buf, room], Class::I64);
                let zero = self.iconst(0);
                let failed = self.cmp_imm(Cmp::Lt, n, Class::I64, 0);
                let len = self.select(failed, zero, n);
                self.write_str(dst, buf, len, owner);
                return true;
            }
            Intrinsic::OsLock | Intrinsic::OsTryLock => {
                let h = self.arg(args, 0);
                let exclusive = self.arg(args, 1);
                let wait = self.iconst(i64::from(op == Intrinsic::OsLock));
                self.rt_v("nz.os_file_lock", &[h, exclusive, wait], Class::I64)
            }
            _ => return false,
        };
        self.set(dst, v);
        true
    }
}
