//! TLS (v1 track C1, `docs/user/spec.md`, *TLS*) in the interpreter: refused, as a value. The engine is
//! a C library, and the interpreter is safe Rust that calls no C (`unsafe_code = "forbid"`), so a
//! configuration answers `-ENOSYS` — `@std/tls` reports it as an `Io` error of kind `Other` — and
//! every id, having never been made, answers `Closed`. A TLS program runs compiled, on every
//! backend; nothing here pretends to encrypt.

use nazm_sema::Intrinsic;

use super::os::CLOSED;
use super::value::Value;

/// The host's `ENOSYS`, as `process.rs` answers for what safe Rust cannot ask.
const ENOSYS: i64 = if cfg!(target_os = "linux") { 38 } else { 78 };

pub(super) fn tls_value(op: Intrinsic, _args: &[Value]) -> Option<Value> {
    match op {
        Intrinsic::OsTlsClientConfig | Intrinsic::OsTlsServerConfig => Some(Value::Int(-ENOSYS)),
        Intrinsic::OsTlsConnect
        | Intrinsic::OsTlsAccept
        | Intrinsic::OsTlsHandshake
        | Intrinsic::OsTlsPush
        | Intrinsic::OsTlsPull
        | Intrinsic::OsTlsRead
        | Intrinsic::OsTlsWrite
        | Intrinsic::OsTlsClose
        | Intrinsic::OsTlsInfo => Some(Value::Int(CLOSED)),
        _ => None,
    }
}
