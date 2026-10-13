//! The C boundary: reserved symbols, foreign declarations and exports, and which types may cross.

use super::*;

/// The C symbols the native runtime and the platform entry declare themselves (N42): a
/// foreign declaration of one could disagree with the runtime's, so each is refused (`N0383`).
/// `a_reserved_c_symbol_is_every_one_the_runtime_calls` keeps this list the registry's.
pub const RESERVED_C_SYMBOLS: &[&str] = &[
    "exit",
    "fclose",
    "fopen",
    "fread",
    "free",
    "fseek",
    "ftell",
    "fwrite",
    "getenv",
    "main",
    "malloc",
    "memcmp",
    "memcpy",
    "printf",
    "pthread_cond_broadcast",
    "pthread_cond_destroy",
    // The stack guard and the task threads' stack size (N49).
    "getrlimit",
    "pthread_attr_init",
    "pthread_attr_setstacksize",
    "pthread_attr_destroy",
    "pthread_cond_init",
    "pthread_cond_wait",
    // Select's deadline and the clock (N54).
    "pthread_cond_timedwait",
    "clock_gettime",
    // The task pool: its settings, its tasks' guarded stacks, and their zeroing (N83).
    "atoll",
    "getpagesize",
    "posix_memalign",
    "mprotect",
    "memset",
    // Gate 2: the numbers part's parsing and the C library's mathematics, and the move bytes
    // and text make.
    "strtod",
    "strtof",
    "memmove",
    "atan2",
    "atan2f",
    "cos",
    "cosf",
    "exp",
    "expf",
    "fmax",
    "fmaxf",
    "fmin",
    "fminf",
    "fmod",
    "fmodf",
    "log",
    "log10",
    "log10f",
    "log2",
    "log2f",
    "logf",
    "pow",
    "powf",
    "round",
    "roundf",
    "sin",
    "sinf",
    "tan",
    "tanf",
    "nz_ctx_switch",
    "nz_ctx_prepare",
    "nz_task_main",
    // Where `errno` is (N85): the runtime's own question, and each target's answer to it.
    "nz_errno_location",
    "__error",
    "__errno_location",
    "pthread_create",
    "pthread_join",
    "pthread_mutex_destroy",
    "pthread_mutex_init",
    "pthread_mutex_lock",
    "pthread_mutex_unlock",
    "realloc",
    "snprintf",
    "strlen",
    "write",
    // Gate 2: files and handles — what the runtime calls, and what each target's `os` unit
    // defines and calls.
    "read",
    "close",
    "lseek",
    "fsync",
    "rmdir",
    "unlink",
    "rename",
    "dirfd",
    "closedir",
    "open",
    "mkdir",
    "stat",
    "opendir",
    "readdir",
    "strerror_r",
    "__xpg_strerror_r",
    "nz_os_open",
    "nz_os_mkdir",
    "nz_os_stat",
    "nz_os_opendir",
    "nz_os_readdir",
    "nz_os_family",
    "nz_os_error_text",
    // And the network (Gate 2): what the runtime calls, and what each target's `os` unit defines
    // and calls.
    "bind",
    "listen",
    "accept",
    "recvfrom",
    "getsockname",
    "getpeername",
    "shutdown",
    "inet_pton",
    "inet_ntop",
    "fcntl",
    "socket",
    "setsockopt",
    "getsockopt",
    "connect",
    "send",
    "sendto",
    "getaddrinfo",
    "freeaddrinfo",
    "kqueue",
    "kevent",
    "epoll_create1",
    "epoll_ctl",
    "epoll_wait",
    "nz_net_fill",
    "nz_net_format",
    "nz_net_prepare",
    "nz_net_open",
    "nz_net_connect",
    "nz_net_error",
    "nz_net_send",
    "nz_net_sendto",
    "nz_net_eagain",
    "nz_net_etimedout",
    "nz_net_resolve",
    "nz_net_poller",
    "nz_net_arm",
    "nz_net_poll",
    // And process and environment (Gate 2).
    "pipe",
    "posix_spawnp",
    "posix_spawn_file_actions_init",
    "posix_spawn_file_actions_adddup2",
    "posix_spawn_file_actions_destroy",
    "waitpid",
    "kill",
    "signal",
    "environ",
    "_NSGetEnviron",
    "nz_os_environ",
    "getentropy",
    // And the platform layer (Gate 3): every per-target function the runtime and the entry call,
    // and what Windows' units call.
    "nz_os_page_size",
    "nz_os_stack_alloc",
    "nz_os_stack_free",
    "nz_os_stack_size",
    "nz_os_clock",
    "nz_os_close",
    "nz_os_read",
    "nz_os_write",
    "nz_os_lseek",
    "nz_os_fsync",
    "nz_os_unlink",
    "nz_os_rmdir",
    "nz_os_rename",
    "nz_os_dirfd",
    "nz_os_closedir",
    "nz_os_entropy",
    "nz_os_spawn",
    "nz_os_reap",
    "nz_os_terminate",
    "nz_os_key",
    "nz_os_args",
    "nz_net_read",
    "nz_net_write",
    "nz_net_bind",
    "nz_net_listen",
    "nz_net_accept",
    "nz_net_recvfrom",
    "nz_net_shutdown",
    "nz_net_name",
];

/// The rules a foreign declaration must meet (N42, `docs/architecture.md` §7.44).
/// The ABI and the symbol of a foreign declaration or an export (N42, N55): `"C"`, a C
/// identifier, and none the runtime declares itself.
fn c_abi_and_symbol(d: &nazm_syntax::ForeignDecl, errors: &mut Vec<Diagnostic>) {
    if d.abi != "C" {
        errors.push(
            Diagnostic::error(
                codes::FOREIGN_ABI,
                d.abi_span,
                format!("`\"{}\"` is not an ABI this compiler calls", d.abi),
            )
            .help("the one foreign ABI is `\"C\"`: the C calling convention of the target"),
        );
    }
    let first_ok = d
        .symbol
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_');
    let rest_ok = d
        .symbol
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if !first_ok || !rest_ok || d.symbol.len() > 255 {
        errors.push(
            Diagnostic::error(
                codes::FOREIGN_SYMBOL,
                d.symbol_span,
                format!("`\"{}\"` is not a C symbol", d.symbol.escape_debug()),
            )
            .help(
                "a foreign symbol is a C identifier: ASCII letters, digits and `_`, not \
                 starting with a digit, at most 255 bytes",
            ),
        );
    }
    if RESERVED_C_SYMBOLS.contains(&d.symbol.as_str()) {
        errors.push(
            Diagnostic::error(
                codes::FOREIGN_CONFLICT,
                d.symbol_span,
                format!(
                    "`{}` is a symbol the native runtime already declares",
                    d.symbol
                ),
            )
            .help(
                "the runtime and the platform entry call these themselves with their own \
                 signatures; a foreign declaration of one could disagree with it",
            ),
        );
    }
}

pub(super) fn check_foreign(
    f: &Function,
    d: &nazm_syntax::ForeignDecl,
    params: &[Type],
    ret: Type,
    res: &Resolution,
    option: Option<EnumId>,
    errors: &mut Vec<Diagnostic>,
) {
    c_abi_and_symbol(d, errors);
    if let (Some(first), Some(last)) = (f.type_params.first(), f.type_params.last()) {
        errors.push(
            Diagnostic::error(
                codes::FOREIGN_SHAPE,
                first.span.join(last.span),
                "a foreign function cannot be generic",
            )
            .help("a C function has one signature; declare one per type"),
        );
    }
    if let Some(e) = &f.effects {
        errors.push(
            Diagnostic::error(
                codes::FOREIGN_SHAPE,
                e.span,
                "a foreign function's effects are not written",
            )
            .help("calling one is always the effect `foreign`, which needs a `ForeignCap`"),
        );
    }
    for (t, p) in params.iter().zip(&f.params) {
        if foreign_param(res, *t).is_none() {
            errors.push(foreign_type(p.ty.span, "take"));
        }
    }
    if foreign_result(res, option, ret).is_none() {
        errors.push(foreign_type(f.return_type.span, "return"));
    }
}

/// How a foreign function's parameter of type `t` crosses (N42, N55, N85), or `None` when it
/// cannot: the one table every consumer — the checker, Core IR lowering — reads.
#[must_use]
pub fn foreign_param(res: &Resolution, t: Type) -> Option<CrossIn> {
    match t {
        Type::Int | Type::Bool => Some(CrossIn::Scalar),
        Type::Str => Some(CrossIn::CString),
        Type::Record(id) => match res.records().get(id)?.repr {
            CRepr::Opaque => Some(CrossIn::Handle(id)),
            CRepr::Layout => Some(CrossIn::Struct),
            CRepr::Nazm => None,
        },
        // A C function pointer (N85): an export's C entry, its parameters and result scalars.
        Type::Fn(id) => {
            let sig = res.types().fn_signature(id)?;
            let scalar = |t: &Type| matches!(t, Type::Int | Type::Bool);
            (sig.params.iter().all(scalar) && scalar(&sig.ret)).then_some(CrossIn::Callback)
        }
        _ => None,
    }
}

/// How a foreign function's result of type `t` crosses back (N85), or `None` when it cannot.
#[must_use]
pub fn foreign_result(res: &Resolution, option: Option<EnumId>, t: Type) -> Option<CrossOut> {
    let handle = |t: Type| match t {
        Type::Record(id) if res.records().get(id)?.repr == CRepr::Opaque => Some(id),
        _ => None,
    };
    match t {
        Type::Int | Type::Bool => Some(CrossOut::Scalar),
        Type::Str => Some(CrossOut::CString { nullable: false }),
        Type::Record(_) => handle(t).map(|id| CrossOut::Handle {
            record: id,
            nullable: false,
        }),
        Type::Enum(_) => {
            let def = option?;
            if res.types().definition_of(t) != Some(TypeNode::Enum(def)) {
                return None;
            }
            match res.types().type_args(t).as_slice() {
                [Type::Str] => Some(CrossOut::CString { nullable: true }),
                [inner] => handle(*inner).map(|id| CrossOut::Handle {
                    record: id,
                    nullable: true,
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

/// A foreign parameter's crossing (N85).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossIn {
    /// `Int` or `Bool`, by value.
    Scalar,
    /// A `Str`, as a borrowed NUL-terminated copy (N55).
    CString,
    /// An opaque handle: its pointer.
    Handle(RecordId),
    /// A C-layout record, as a borrowed pointer to a copy.
    Struct,
    /// An export's C entry, as a C function pointer.
    Callback,
}

/// A foreign result's crossing (N85).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossOut {
    /// `Int` or `Bool`, by value.
    Scalar,
    /// A `const char *` C keeps, copied; null is `None` when `nullable`, a failure otherwise.
    CString { nullable: bool },
    /// A pointer, as a handle; null is `None` when `nullable`, a failure otherwise.
    Handle { record: RecordId, nullable: bool },
}

/// The effect parameter a function declares, if any (N51): the first, with every further one
/// and one a foreign function writes refused (`N0388`), and one on a function that declares no
/// effect set too — what such a function's body requires would name a parameter its callers
/// never bind.
pub(super) fn declared_effect_param<'f>(
    f: &'f Function,
    errors: &mut Vec<Diagnostic>,
) -> Option<&'f str> {
    let first = f.effect_params.first()?;
    for extra in f.effect_params.iter().skip(1) {
        errors.push(
            Diagnostic::error(
                codes::EFFECT_PARAMETER,
                extra.span,
                format!("`{}` declares a second effect parameter", f.name),
            )
            .secondary(first.span, "the first")
            .help("a function declares at most one: every set it writes may name that one"),
        );
    }
    if f.foreign.is_some() {
        errors.push(
            Diagnostic::error(
                codes::EFFECT_PARAMETER,
                first.span,
                "a foreign function cannot have an effect parameter",
            )
            .help("calling C is always the effect `foreign`"),
        );
        return None;
    }
    if f.effects.is_none() {
        errors.push(
            Diagnostic::error(
                codes::EFFECT_PARAMETER,
                first.span,
                format!(
                    "`{}` has an effect parameter, so it declares its effect set",
                    f.name
                ),
            )
            .help(format!(
                "write `! {{ {} }}` after the result type: a function with an effect parameter \
                 states what it does in terms of it",
                first.name
            )),
        );
    }
    Some(first.name.as_str())
}

/// The rules an export meets (N55, `docs/architecture.md` §7.57): the foreign declaration's ABI
/// and symbol rules, and — since C can hand Nazm no capability and has no cleanup Nazm knows of —
/// public, `! {}`, not generic, and only numbers and `Bool` across (every number since Gate 2).
pub(super) fn check_export(
    f: &Function,
    d: &nazm_syntax::ForeignDecl,
    params: &[Type],
    ret: Type,
    errors: &mut Vec<Diagnostic>,
) {
    c_abi_and_symbol(d, errors);
    let shape = |at: Span, what: String| {
        Diagnostic::error(codes::EXPORT_SHAPE, at, what).help(
            "an export is called by C, which can hand Nazm no capability and has no cleanup \
             Nazm knows of: it is `pub`, declares `! {}`, is not generic, and takes and \
             returns only numbers and `Bool`",
        )
    };
    if f.pub_span.is_none() {
        errors.push(shape(
            f.name_span,
            format!("`{}` is exported to C, so it is `pub`", f.name),
        ));
    }
    match &f.effects {
        Some(e) if e.names.is_empty() => {}
        Some(e) => errors.push(shape(
            e.span,
            format!("`{}` is exported to C, so it performs no effect", f.name),
        )),
        None => errors.push(shape(
            f.return_type.span,
            format!("`{}` is exported to C, so it declares `! {{}}`", f.name),
        )),
    }
    if let (Some(first), Some(last)) = (f.type_params.first(), f.type_params.last()) {
        errors.push(shape(
            first.span.join(last.span),
            format!("`{}` is exported to C, so it cannot be generic", f.name),
        ));
    }
    // Every number crosses as its C type since Gate 2 (§23): `int8_t` … `uint64_t`, `float`,
    // `double` — what a binding generator for any other language reads from the header. A
    // 128-bit integer does not (R1b, L13b): C has no standard type for one, and the platforms'
    // `__int128` disagree about how it is passed.
    let scalar = |t: Type| {
        matches!(t, Type::Int | Type::Bool) || matches!(t, Type::Num(k) if k.bits() <= 64)
    };
    for (t, p) in params.iter().zip(&f.params) {
        if !scalar(*t) {
            errors.push(shape(
                p.ty.span,
                format!("`{}` is exported to C, so it cannot take this type", f.name),
            ));
        }
    }
    if !scalar(ret) {
        errors.push(shape(
            f.return_type.span,
            format!(
                "`{}` is exported to C, so it cannot return this type",
                f.name
            ),
        ));
    }
}

fn foreign_type(at: Span, what: &str) -> Diagnostic {
    Diagnostic::error(
        codes::FOREIGN_TYPE,
        at,
        format!("a foreign function cannot {what} this type"),
    )
    .primary_label("not FFI-safe")
    .help(
        "an argument is `Int`, `Bool`, a `Str` (borrowed), an opaque handle, an `extern \"C\" \
         struct` (by borrowed pointer) or an export's function type; a result is `Int`, `Bool`, \
         a handle or a `Str` (copied), or an `Option` of either when C may return null. An \
         ordinary record or enum has no C layout, and a capability is never a word C can forge",
    )
}
