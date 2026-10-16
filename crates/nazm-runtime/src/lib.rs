//! The runtime's entry points, as one typed registry.
//!
//! `docs/architecture.md` §7.43: every runtime symbol a backend's generated code calls is here,
//! once — its symbol, its parameters and result in machine words and pointers only, and what it
//! does with ownership. A backend that calls the runtime builds each call from this table and
//! from nothing else, so a symbol spelled twice or a signature guessed at a call site is a
//! compile error rather than a link-time surprise. The LLVM text emitter predates the table and
//! declares these by hand; `a_runtime_entry_the_registry_names_is_one_the_runtime_defines`
//! keeps the two in agreement.
//!
//! No first-class aggregate crosses this boundary. Where the runtime returns a string or a
//! pair, the registry's entry writes it through a pointer (`nz.read_file_to`), because code two
//! compilers generated must agree about nothing but words.

pub mod board;
pub(crate) mod core;
pub(crate) mod entry;
pub(crate) mod files;
pub mod heap;
pub(crate) mod net;
pub(crate) mod os;
pub mod platform;
pub mod switch;
pub mod tls;
pub mod wasi;
pub mod wasm;

use std::fmt::Write as _;

pub use self::core::{
    emit_runtime, inline_reference_counting, plain_reference_counts, runtime_declarations,
    MEMORY_FORMAT, MEMORY_VARIABLE, PLAIN_IN_ONE_THREAD, RESULT_STATUS, RESULT_VARIABLE,
    SCHED_FORMAT, SCHED_VARIABLE,
};
pub use self::entry::{emit_entry, emit_entry_for, policy as scheduler_policy};
pub use self::os::declarations;

/// What this runtime has been verified against, recorded so a reader does not have to
/// infer portability from silence.
pub const TARGET_NOTE: &str = "verified on aarch64-apple-darwin with Apple clang; other \
                               targets are unverified rather than unsupported";

/// Which runtime helper sets a compilation reaches.
///
/// # Why this is a whole-program fact
///
/// The runtime is emitted **once**, so what it defines cannot be decided by one module.
/// A unit records what it reached; the driver unions those and hands the answer to
/// [`emit_runtime`]. That makes the runtime artefact a function of the program's feature
/// use rather than of any module — the honest shape, and the one a future cache would
/// have to key on.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Needs {
    pub tasks: bool,
    pub channels: bool,
    pub sequences: bool,
    pub io: bool,
    pub join: bool,
    pub strings: bool,
    /// The registry's pointer-taking entries and accessors (`crate::runtime`), which code a
    /// second compiler generated calls instead of the aggregate-returning ones (N41).
    pub shims: bool,
    /// `nz.stack_check`, which a function in a call cycle calls first (N49).
    pub stack: bool,
    /// The closure objects' allocation and count (N50).
    pub closures: bool,
    /// `errno` captured after each foreign call and read by `c_errno()` (N85), with the
    /// per-target `errno` unit beside the runtime.
    pub foreign: bool,
    /// Formatting and parsing floats and `UInt64`s (Gate 2).
    pub numbers: bool,
    /// Files and handles (Gate 2), with the per-target `os` and `errno` units beside the
    /// runtime.
    pub files: bool,
    /// Sockets and the reactor (Gate 2), over the files part's handle table and the task part's
    /// waits.
    pub net: bool,
    /// `Int128` and `UInt128`'s division, signed product, float conversions and text (R1b,
    /// L13b), which take the C toolchain's 128-bit helpers.
    pub wide: bool,
    /// TLS (v1 track C1): the session table and the `nz.os_tls_*` entries, with the per-target
    /// `tls` unit ([`tls::emit_tls`]) and the platform's TLS library linked beside the runtime.
    pub tls: bool,
    /// The program has one thread (the speed milestone): its `main` holds no `SpawnCap`,
    /// `NetCap` or `ForeignCap` and it is not a library, so nothing can start another — the
    /// failure state and the stack limit are then plain globals, not thread-local ones, whose
    /// every read on macOS is a call. A property of the program, not a service: not named.
    pub single: bool,
}

impl Needs {
    /// Everything either of them reaches.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self {
            tasks: self.tasks || other.tasks,
            channels: self.channels || other.channels,
            sequences: self.sequences || other.sequences,
            io: self.io || other.io,
            join: self.join || other.join,
            strings: self.strings || other.strings,
            shims: self.shims || other.shims,
            stack: self.stack || other.stack,
            closures: self.closures || other.closures,
            foreign: self.foreign || other.foreign,
            numbers: self.numbers || other.numbers,
            files: self.files || other.files,
            net: self.net || other.net,
            wide: self.wide || other.wide,
            tls: self.tls || other.tls,
            single: self.single || other.single,
        }
    }

    /// Each service reached, by its stable name, in a fixed order (N82): what a runtime
    /// artifact's manifest lists.
    #[must_use]
    pub fn names(self) -> Vec<&'static str> {
        [
            (self.tasks, "tasks"),
            (self.channels, "channels"),
            (self.sequences, "sequences"),
            (self.io, "io"),
            (self.join, "join"),
            (self.strings, "strings"),
            (self.shims, "shims"),
            (self.stack, "stack"),
            (self.closures, "closures"),
            (self.foreign, "foreign"),
            (self.numbers, "numbers"),
            (self.files, "files"),
            (self.net, "net"),
            (self.wide, "wide"),
            (self.tls, "tls"),
        ]
        .into_iter()
        .filter_map(|(on, n)| on.then_some(n))
        .collect()
    }

    /// The services named, or `None` for a name that is not one.
    #[must_use]
    pub fn by_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        let mut n = Self::default();
        for name in names {
            match name {
                "tasks" => n.tasks = true,
                "channels" => n.channels = true,
                "sequences" => n.sequences = true,
                "io" => n.io = true,
                "join" => n.join = true,
                "strings" => n.strings = true,
                "shims" => n.shims = true,
                "stack" => n.stack = true,
                "closures" => n.closures = true,
                "foreign" => n.foreign = true,
                "numbers" => n.numbers = true,
                "files" => n.files = true,
                "net" => n.net = true,
                "wide" => n.wide = true,
                "tls" => n.tls = true,
                _ => return None,
            }
        }
        Some(n)
    }

    /// The services `self` needs that `have` does not provide, by name (N82).
    #[must_use]
    pub fn missing_from(self, have: Self) -> Vec<&'static str> {
        let have = have.names();
        self.settled()
            .names()
            .into_iter()
            .filter(|n| !have.contains(n))
            .collect()
    }

    /// What the helper sets imply about each other.
    ///
    /// Both the task and the channel runtimes grow a sequence, so either one reaching
    /// means the sequence runtime is reached too. Derived in one place rather than at each
    /// use, so the two facts cannot drift apart.
    #[must_use]
    pub fn settled(self) -> Self {
        Self {
            // `str_join` builds a string and the I/O helpers return one, so either of
            // them reaching means the string helpers are reached too. A `Strs` needs them
            // as well, and says so at the site that retains or releases an element rather
            // than by implication from the sequence flag — an `Ints`-only program carries
            // no string runtime at all.
            strings: self.strings || self.join || self.io || self.files || self.net,
            // The files part copies its paths with the I/O part's `nz.cstr`.
            io: self.io || self.files || self.net,
            // A socket is a handle in the files part's table, and its waits are the tasks'.
            files: self.files || self.net,
            tasks: self.tasks || self.net,
            sequences: self.sequences || self.tasks || self.channels || self.net,
            ..self
        }
    }
}

/// The header every generated artefact carries.
pub fn preamble(out: &mut String, source: &str) {
    let _ = writeln!(out, "; Generated by nazm build. Do not edit.");
    let _ = writeln!(out, "; {TARGET_NOTE}");
    let _ = writeln!(out, "source_filename = \"{source}\"");
    out.push('\n');
}

/// LLVM's constant-string escaping: `\XX` for anything not printable ASCII.
pub fn escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b == b'"' || b == b'\\' || !(0x20..0x7f).contains(&b) {
            let _ = write!(out, "\\{b:02X}");
        } else {
            out.push(b as char);
        }
    }
    out
}

/// The runtime ABI's revision (§7.45). It names the runtime's contract — [`INVENTORY`]'s
/// signatures, [`GLOBALS`], this word-level table, the sequence header's layout (`len` at 0,
/// `cap` at 8, `data` at 16, the count at 24), a string's (pointer, length, owner) and a string
/// backing's count at 0 — and enters every object's cache key under both backends, so an object
/// built against another revision is never reused.
///
/// 1 was N41's: the word-level table alone. 2 is N43's, the first revision whose contract is
/// written down whole. 3 is N49's: generated code calls `nz.stack_check`, the runtime has its
/// thread-local limit, and task threads are started with an 8 MiB stack. 4 is N50's: closure
/// objects (`nz.closure_new`, `nz.closure_retain`, `nz.closure_release`) and their layout. 5 is
/// N53's: the runtime is its own component, generic channels (`nz.chanv_*`, a header extended by
/// the element's stride at 64 and its release at 72), and [`digest`] beside the revision in
/// every key, so a runtime whose text changed without a bump is still another runtime. 6 is
/// N54's: select (`nz.chanv_select`, `nz.chanv_try`, `nz.chanv_close`, the event count
/// `nz.sel_*`), the clock, and the scheduler's counters. 7 is N55's: `nz.cstr_borrow`, a `Str`
/// argument to C. 8 is N62's: `nz.mmio_read32`/`nz.mmio_write32`, defined by the hosted runtime and
/// the freestanding one alike. 9 is N66's: `nz.ints_sum`, the ordered checked sum. 10 is N68's: `nz.soa_grow` and the
/// field-by-field `Vec` layout it grows. 11 is N83's: the task pool — `nz.cwait`, `nz.ctimedwait`
/// and `nz.cwake` around every wait, a task record in the argument block's head when the pool
/// runs, and the per-target `switch` artefact every build reaching tasks or channels links. 12 is N85's:
/// `nz.cstr_len` for a `Str` C returns, `nz.errno_save` after every foreign call and
/// `nz.errno_last` for `c_errno()`, the thread-local `nz.errno_saved`, and the per-target `errno`
/// unit defining `nz_errno_location` that a build reaching them links. 13 is N86's: the device
/// registers by width (`nz.mmio_read8`, `nz.mmio_write8`, `nz.mmio_read16`, `nz.mmio_write16`),
/// and a board's runtime generated from its description. 14 is N106's: the pool is the default —
/// `NAZM_SCHEDULER` unset runs it, `threads` asks for a thread per task, any other value starts
/// no task — and `nz.pool_policy`, which the platform entry defines (1 for a program that calls C).
/// 15 is Gate 2's: floats and fixed-width integers cross runtime entries as `float`, `double`
/// and the narrow words, and the numbers part — `nz.format_float`, `nz.format_u64`,
/// `nz.float_parses`, `nz.float_parse` — joins the runtime, with the C library's mathematics.
/// 16 is Gate 2's files and handles: the `nz.os_*` entries, the handle table, and the per-target
/// `os` unit (`platform::emit_os`) that a build reaching them links beside the `errno` unit. 17 is
/// Gate 2's networking: the `nz.os_*` socket entries, a handle-table slot of 64 bytes with a wait
/// record and a deadline, the reactor thread, and the `os` unit's `nz_net_*`. 18 is Gate 2's
/// time: `nz.time_now_ns`, `nz.time_wall_ns` and `nz.time_sleep_ms`, which parks on the pool. 19
/// is Gate 2's process and environment: `nz.os_env_*`, `nz.os_spawn`, `nz.os_child_pipe`,
/// `nz.os_wait`, `nz.os_kill`, a pipe's write, and the `os` unit's `nz_os_environ`. 20 is Gate 2's
/// randomness: `nz.os_random_bytes`, from `getentropy`. 21 is Gate 3's platform layer: every
/// system call the runtime makes goes through a per-target function — the `os` unit's file,
/// socket, reactor and process functions (`nz_os_close` … `nz_os_spawn`, `nz_os_reap`,
/// `nz_os_terminate`, `nz_os_key`, `nz_net_read` … `nz_net_name`), the `stack` unit's task stacks,
/// stack budget and clocks (`nz_os_stack_*`, `nz_os_page_size`, `nz_os_clock`), and on Windows
/// the `init` unit and the entry's `nz_os_args` — so the runtime's own text names no system.
/// 22 is Gate 3's atomics: `nz.atomic_load`, `nz.atomic_store`, `nz.atomic_swap` and
/// `nz.atomic_cas`, sequentially consistent, defined by the hosted runtime and a board's alike;
/// an atomic's cell is the data of a string backing, so it is `Str`'s triple. 23 is Gate 3's
/// 64-bit device registers: `nz.mmio_read64` and `nz.mmio_write64`. 24 is Gate 3's failure hook:
/// a board's `nz.report` calls the entry's `nz_board_on_failure` — a weak reference, absent in a
/// library — with the failure's number before stopping the machine. 25 is Gate 3's Cortex-M
/// board: every board's entry calls `nz.uart_init` before `main`, and a Thumb board's runtime
/// defines the 64-bit atomics and division ARMv7-M has no instruction for. 26 is Gate 3's
/// watchdog: `nz.watchdog_feed` and its two counters, which a board's scheduler arms where its
/// manifest has a `[watchdog]`. 27 is the v1 stack fixes': `nz.stack_check_frame`, which a
/// function whose own frame is large calls on entry (Q1-A1-01), and a stack unit whose
/// `nz_os_stack_size` is the calling thread's stack left below its frame, not `RLIMIT_STACK`
/// (Q1-A1-04). 28 is the monotonic deadlines' (Q1-M-12): `nz.ctimedwait` takes a monotonic
/// time, `nz.pool_now` reads the monotonic clock, and `nz.timed_nap` turns a deadline into the
/// wall-clock nap a timed wait measures. 29 is R1a's release queue: `nz.vec_release` takes the
/// queue of the release in progress, and an element's release function takes it too, so a
/// release nested in another queues instead of recursing (`nz.vec_drain`, `nz.vec_enqueue`).
/// 30 is R1a's fixed-point floats: `nz.format_fixed`, C's `%.*f`, for `float_to_str_fixed` and
/// an interpolation's `.precision`.
/// 31 is R1b's stack size (L14): `nz.stack_bytes`, which the platform entry stores before `main`
/// and which a task's thread, a pool's task stacks and `nz.stack_check` read in place of their
/// defaults. 32 is R1b's 128-bit integers (L13b): the wide part — `nz.i128_divrem`,
/// `nz.i128_mul`, `nz.i128_to_f64`, `nz.i128_to_f32`, `nz.f64_to_i128` and `nz.format_i128`,
/// each taking its 128-bit values by address. 33 is R1d: the counted heap (`nz.heap_reserve`,
/// `nz.heap_malloc`, `nz.heap_realloc`, `nz.heap_free`, a heap part); lines of standard input
/// (`nz.os_read_line` and the `nz.os_in_*` buffer, which `nz.os_read` of standard input now
/// reads through); standard output and errors as handles (`nz.os_std_stream`, `nz.os_stdout_h`,
/// `nz.os_stderr_h`); process control (`nz.os_spawn_in`, `nz.os_pid`, `nz.os_child_pid`,
/// `nz.os_kill_group`, `nz.os_try_wait`, `nz.os_wait_until`, `nz.os_child_rusage`, the child
/// record's process id at 40 and peak memory at 48); the file-system part (`nz.os_lstat`,
/// `nz.os_chmod`, `nz.os_realpath`, `nz.os_mkdtemp`, `nz.os_file_lock`, `nz.os_file_unlock`); and
/// the platform layer's `nz_os_spawn_in`, `nz_os_reap(ref, block, rss)` by `wait4`,
/// `nz_os_child_id`, `nz_os_pid`, `nz_os_signal_group`, `nz_os_nap`, `nz_os_lstat`,
/// `nz_os_chmod`, `nz_os_realpath` and `nz_os_flock`. Also R1 (5b.3): the entry ends with
/// `main`'s result as the exit status unless `NAZM_RESULT_REPORT` asks for it printed.
/// 34 is 4.2's failures by unwinding (`docs/architecture.md` §7.115): on the hosted targets under
/// LLVM a Nazm function returns its result alone and a failure is a forced unwind — `nz.raise`,
/// `nz.unwind_stop` and the exception object `nz.exc`, which each unit defines once per image —
/// under the C personality `__gcc_personality_v0`, so an image links the system's unwinder; a plain
/// entry lands the unwind and reads the flag as before. No runtime entry's signature moves.
/// 35 is performance leadership's multicore (2026-10-15): the `stack` unit defines `nz.os_cpus`,
/// the processors online, and the pool starts that many workers unless `NAZM_WORKERS` says
/// otherwise — four where the system will not say, as before on every system.
/// 36 is M1's microcontroller boards: a board's runtime defines `nz.stack_limit`, the limit the
/// inlined stack check reads (no board image that recursed linked without it); a Cortex-M33's
/// runtime adds the run-time ABI's 64-bit integer and float conversions and the double widening
/// and comparisons, an RV32IMC's the masked 64-bit atomics, libgcc's 64-bit division and the C
/// memory functions with 32-bit sizes; and a board's entry copies data from flash, zeroes the
/// rest and calls the manifest's `init` (`nz_board_init`) before the interrupts and `main`.
/// 37 is v1 track C1's TLS: the `tls` part — the session table `nz.tls_slots` and the entries
/// `nz.os_tls_client_config`, `nz.os_tls_server_config`, `nz.os_tls_connect`, `nz.os_tls_accept`,
/// `nz.os_tls_handshake`, `nz.os_tls_push`, `nz.os_tls_pull`, `nz.os_tls_read`, `nz.os_tls_write`,
/// `nz.os_tls_close` and `nz.os_tls_info` — over the per-target `tls` unit's `nz_tls_*`
/// ([`tls::emit_tls`]: OpenSSL 3 on macOS and Linux, with the Security framework's trust on macOS),
/// linked with the platform's TLS library.
/// 38 is R4's atomics outside the strings (2026-10-16): `nz.atomic_cell` allocates an `Atomic`'s
/// cell in a backing of a string's shape whose length is -1, and `nz.str_release` frees such a
/// backing without counting it, so the memory report counts no string for `atomic_new`, as the
/// interpreter does not. 39 is R1.5 3.1's small-block heap (`heap::emit_small`): `nz.small_malloc`,
/// `nz.small_free` and `nz.small_realloc`, which every unit but the stack's and the TLS engine's
/// calls in place of C's allocator in a hosted executable that calls no C.
/// 40 is performance leadership 6.2's copied stacks (2026-10-16): a pool task has no stack of its
/// own — each worker has one guarded run stack (the task-stack size), its tasks run on it at the
/// same addresses, and `nz.task_place` copies the frames of the task in place into a buffer it owns
/// when another is to run there, and back before it runs again; a worker record is 384 bytes (run
/// stack at 320, its size at 328, the task in place at 336), and a task's record keeps its saved
/// frames at 8, their capacity at 16, their length at 152 and whether it has run at 160.
pub const RUNTIME_ABI_REVISION: u32 = 40;

/// Every part of the runtime this component can emit, for [`digest`].
/// Every service the runtime has: what [`digest`] covers, and what a runtime artifact
/// (`nazm runtime build`, N82) holds.
pub const EVERYTHING: Needs = Needs {
    tasks: true,
    channels: true,
    sequences: true,
    io: true,
    join: true,
    strings: true,
    shims: true,
    stack: true,
    closures: true,
    foreign: true,
    numbers: true,
    files: true,
    net: true,
    wide: true,
    tls: true,
    single: false,
};

/// The runtime's identity as text: blake3 over the whole runtime, the platform entry and the
/// typed inventory, hex-encoded (N53). It enters every object key beside
/// [`RUNTIME_ABI_REVISION`]: the revision is the contract a person bumps, the digest is what
/// was actually emitted, and a change to either is another runtime.
#[must_use]
pub fn digest() -> &'static str {
    static DIGEST: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    DIGEST.get_or_init(|| {
        let mut h = blake3::Hasher::new();
        h.update(&RUNTIME_ABI_REVISION.to_le_bytes());
        h.update(emit_runtime(EVERYTHING).as_bytes());
        h.update(emit_entry("@nz_main", 1, EVERYTHING).as_bytes());
        h.update(heap::emit_heap(1 << 30).as_bytes());
        h.update(heap::emit_small(false).as_bytes());
        for t in switch::SwitchTarget::ALL {
            h.update(switch::emit_switch(t).as_bytes());
            h.update(switch::emit_errno(t).as_bytes());
            h.update(platform::emit_os(t).as_bytes());
            h.update(platform::emit_stack(t).as_bytes());
            h.update(platform::emit_init(t).unwrap_or_default().as_bytes());
            h.update(tls::emit_tls(t).unwrap_or_default().as_bytes());
        }
        for i in INVENTORY {
            h.update(format!("{i:?}").as_bytes());
        }
        h.finalize().to_hex().to_string()
    })
}

/// One machine-level parameter or result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Word {
    /// A 64-bit integer.
    I64,
    /// A C `int`.
    I32,
    /// A pointer.
    Ptr,
    /// A truth value in the low bit of a byte; the other bits are not promised.
    Bit,
    /// IEEE 754 binary32 and binary64, as C's `float` and `double` (Gate 2).
    F32,
    F64,
}

/// Which part of the runtime defines an entry, so a build that calls it links that part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// Always present.
    Base,
    Strings,
    Sequences,
    Io,
    Join,
    Tasks,
    Channels,
    /// Closure objects (N50).
    Closures,
    /// `errno` across foreign calls (N85).
    Foreign,
    /// Formatting and parsing numbers (Gate 2).
    Numbers,
    /// Files and handles (Gate 2).
    Files,
    /// Sockets and the reactor (Gate 2).
    Net,
    /// `Int128` and `UInt128` (R1b, L13b).
    Wide,
    /// TLS sessions over buffers (v1 track C1).
    Tls,
    /// The heap units: the counted heap (R1d, D5), linked only by `nazm build --heap-limit`, and
    /// the small-block heap (R1.5 3.1), linked by a hosted executable that calls no C.
    Heap,
    /// The C library the runtime itself links against.
    Libc,
}

/// One runtime entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Import {
    pub symbol: &'static str,
    pub params: &'static [Word],
    pub ret: Option<Word>,
    /// What the entry does with the references it is handed or hands back.
    pub ownership: &'static str,
    pub part: Part,
}

macro_rules! import {
    ($sym:literal, [$($p:ident),*], $ret:expr, $part:ident, $own:literal) => {
        Import {
            symbol: $sym,
            params: &[$(Word::$p),*],
            ret: $ret,
            ownership: $own,
            part: Part::$part,
        }
    };
}

/// Every entry point, in a fixed order.
pub const IMPORTS: &[Import] = &[
    import!("nz.fail", [Ptr, I64], None, Base, "records the thread's first failure: a static message and its length; takes nothing"),
    import!("nz.failed_now", [], Some(Word::I32), Base, "whether this thread has failed; takes nothing"),
    import!("nz.report", [], None, Base, "writes this thread's failure and ends the process with status 2; does not return (an export's C entry, N55)"),
    import!("nz.stack_check", [], Some(Word::I32), Base, "nonzero when this thread's stack is nearly exhausted; takes nothing"),
    import!("nz.stack_check_frame", [I64], Some(Word::I32), Base, "nonzero when this thread's stack is nearly exhausted or a frame of this many bytes would cross its limit; takes nothing"),
    import!("nz.closure_new", [I64, Ptr, Ptr], Some(Word::Ptr), Closures, "a closure object of this many bytes with its code and release, holding one reference the caller owns, or null"),
    import!("nz.closure_retain", [Ptr], None, Closures, "one more reference to a closure object; null is a no-op"),
    import!("nz.closure_release", [Ptr], None, Closures, "one reference fewer; the last runs its release and frees it; null is a no-op"),
    import!("nz.format_int", [Ptr, I64], Some(Word::I64), Base, "writes an Int's decimal digits into 21 bytes the caller owns and returns how many"),
    import!("nz.str_alloc", [I64], Some(Word::Ptr), Strings, "a string backing with this many usable bytes after a 16-byte header, holding one reference the caller owns, or null"),
    import!("nz.str_retain", [Ptr], None, Strings, "one more reference to a backing; null is a no-op"),
    import!("nz.str_release", [Ptr], None, Strings, "one reference fewer; the last frees it; null is a no-op"),
    import!("nz.atomic_cell", [I64], Some(Word::Ptr), Strings, "an atomic's cell holding this value, in a backing a string's release frees uncounted, holding one reference the caller owns, or null"),
    import!("nz.small_malloc", [I64], Some(Word::Ptr), Heap, "C's `malloc` in a program that calls no C (R1.5 3.1): memory the caller owns, or null"),
    import!("nz.small_realloc", [Ptr, I64], Some(Word::Ptr), Heap, "C's `realloc` of a small-block heap's block: takes the memory and returns memory the caller owns, or null with it unchanged"),
    import!("nz.small_free", [Ptr], None, Heap, "C's `free` of a small-block heap's block; null is a no-op"),
    import!("nz.seq_new", [],Some(Word::Ptr), Sequences, "an empty sequence with one reference the caller owns, or null"),
    import!("nz.seq_release", [Ptr], None, Sequences, "releases a sequence whose elements own nothing; null is a no-op"),
    import!("nz.strs_release", [Ptr], None, Sequences, "releases a sequence of strings, and on the last reference each string"),
    import!("nz.vec_release", [Ptr, Ptr, I64, Ptr], None, Sequences, "releases a sequence; on the last reference calls the release function on each element at the stride given, then frees it — queued on the release in progress, if one is (R1a)"),
    import!("nz.arr_new", [], Some(Word::Ptr), Sequences, "an empty internal list, one reference, not counted as a program sequence; or null"),
    import!("nz.arr_grow", [Ptr, I64], Some(Word::Ptr), Sequences, "room for one more element of this size; the data pointer, or null with the sequence unchanged"),
    import!("nz.ints_sum", [Ptr, I64, Ptr], Some(Word::I64), Sequences, "borrows the sequence; the sum, or 0 with the flag set at the first running sum that overflows (N66)"),
    import!("nz.soa_grow", [Ptr, I64], Some(Word::Ptr), Sequences, "room for one more element of a field-by-field vector of this many fields; the data pointer, or null with the vector unchanged (N68)"),
    import!("nz.arr_retain", [Ptr], None, Sequences, "one more reference to a sequence header"),
    import!("nz.arr_release", [Ptr], Some(Word::Bit), Sequences, "one reference fewer to an internal list; frees it on the last"),
    import!("nz.read_file_to", [Ptr, Ptr, I64], None, Io, "writes the file's contents as a string the caller owns, or a null pointer if it could not be read"),
    import!("nz.write_file", [Ptr, I64, Ptr, I64], Some(Word::I64), Io, "borrows both strings; bytes written, or negative"),
    import!("nz.file_exists", [Ptr, I64], Some(Word::Bit), Io, "borrows the path"),
    import!("nz.str_join_to", [Ptr, Ptr, Ptr, I64], None, Join, "writes the joined string the caller owns, or a null pointer on exhaustion; borrows the sequence and separator"),
    import!("nz.spawn", [Ptr, Ptr, Ptr], Some(Word::I32), Tasks, "starts a thread running the trampoline on the block and records it in the list: 0, -1 out of memory, >0 no thread"),
    import!("nz.scope_join_to", [Ptr, Ptr], None, Tasks, "joins every task in the list and writes the first failure message and length, or null"),
    import!("nz.task_publish", [Ptr], None, Tasks, "copies this thread's failure, if any, into the task block"),
    import!("nz.chan_new", [I64], Some(Word::Ptr), Channels, "a channel with one reference the caller owns, or null"),
    import!("nz.chan_send", [Ptr, I64], Some(Word::Bit), Channels, "borrows the channel"),
    import!("nz.chan_recv", [Ptr, Ptr, Ptr, I64], Some(Word::Bit), Channels, "borrows the channel and the buffer; the failure text is the call site's"),
    import!("nz.chan_close", [Ptr], None, Channels, "borrows the channel"),
    import!("nz.chan_retain", [Ptr], None, Channels, "one more reference"),
    import!("nz.chan_release", [Ptr], None, Channels, "one reference fewer; the last frees it"),
    import!("nz.chanv_new", [I64, I64, Ptr], Some(Word::Ptr), Channels, "a generic channel of slots this size, whose last release calls the function on each queued slot; one reference, or null"),
    import!("nz.chanv_send", [Ptr, Ptr], Some(Word::Bit), Channels, "borrows the channel; copies the slot in, taking the reference it holds; false and nothing copied when closed"),
    import!("nz.chanv_recv", [Ptr, Ptr], Some(Word::Bit), Channels, "borrows the channel; copies the oldest slot out, handing over its reference; false once closed and drained"),
    import!("nz.chanv_release", [Ptr], None, Channels, "one reference fewer; the last releases each queued slot and frees the channel"),
    import!("nz.atomic_load", [Ptr], Some(Word::I64), Base, "borrows the cell; one sequentially consistent load (Gate 3)"),
    import!("nz.atomic_store", [Ptr, I64], None, Base, "borrows the cell; one sequentially consistent store"),
    import!("nz.atomic_swap", [Ptr, I64], Some(Word::I64), Base, "borrows the cell; the value before"),
    import!("nz.atomic_cas", [Ptr, I64, I64], Some(Word::Bit), Base, "borrows the cell; true when it held the expected value and now holds the new one"),
    import!("nz.mmio_read32", [I64], Some(Word::I64), Base, "reads a device register; takes nothing (N62)"),
    import!("nz.mmio_write32", [I64, I64], Some(Word::I64), Base, "writes a device register; takes nothing (N62)"),
    import!("nz.mmio_read8", [I64], Some(Word::I64), Base, "reads a byte-wide device register; takes nothing (N86)"),
    import!("nz.mmio_write8", [I64, I64], Some(Word::I64), Base, "writes a byte-wide device register; takes nothing (N86)"),
    import!("nz.mmio_read16", [I64], Some(Word::I64), Base, "reads a 16-bit device register; takes nothing (N86)"),
    import!("nz.mmio_write16", [I64, I64], Some(Word::I64), Base, "writes a 16-bit device register; takes nothing (N86)"),
    import!("nz.mmio_read64", [I64], Some(Word::I64), Base, "reads a 64-bit device register; takes nothing (Gate 3)"),
    import!("nz.mmio_write64", [I64, I64], Some(Word::I64), Base, "writes a 64-bit device register; takes nothing (Gate 3)"),
    import!("nz.watchdog_feed", [], Some(Word::I64), Base, "re-arms the board's watchdog; takes nothing (Gate 3)"),
    import!("nz.cstr_has_nul", [Ptr, I64], Some(Word::Bit), Strings, "borrows the bytes; whether they hold a NUL (N55)"),
    import!("nz.cstr_borrow", [Ptr, I64], Some(Word::Ptr), Strings, "borrows the bytes; a NUL-terminated copy the caller owns and frees, or null (N55)"),
    import!("nz.cstr_len", [Ptr], Some(Word::I64), Strings, "borrows the C string; its length up to the NUL (N85)"),
    import!("nz.errno_save", [], None, Foreign, "takes nothing; saves this thread's errno (N85)"),
    import!("nz.errno_last", [], Some(Word::I64), Foreign, "takes nothing; the errno last saved on this thread (N85)"),
    import!("nz.chanv_close", [Ptr], None, Channels, "borrows the channel; closes it and wakes every select (N54)"),
    import!("nz.chanv_select", [Ptr, Ptr, I64, I64], Some(Word::I64), Channels, "borrows the channels; copies one slot out to the second pointer, handing over its reference, when it returns an index; -3 - i closed, -1 none, -2 timed out, i64::MIN out of memory"),
    import!("nz.time_now_ms", [], Some(Word::I64), Channels, "takes nothing"),
    import!("nz.time_now_ns", [], Some(Word::I64), Channels, "takes nothing (Gate 2)"),
    import!("nz.time_wall_ns", [], Some(Word::I64), Channels, "takes nothing (Gate 2)"),
    import!("nz.time_sleep_ms", [I64], Some(Word::I64), Tasks, "parks the task on the pool for the milliseconds given; 0 (Gate 2)"),
    import!("nz.sched_note_spawn", [], None, Tasks, "counts a started task; takes nothing"),
    import!("nz.sched_note_join", [Ptr], None, Tasks, "borrows the task list; counts its tasks as joined"),
    import!("write", [I32, Ptr, I64], Some(Word::I64), Libc, "borrows the bytes"),
    import!("exit", [I32], None, Libc, "does not return"),
    import!("strlen", [Ptr], Some(Word::I64), Libc, "borrows the text"),
    import!("malloc", [I64], Some(Word::Ptr), Libc, "memory the caller owns, or null"),
    import!("free", [Ptr], None, Libc, "takes the memory"),
    import!("memcpy", [Ptr, Ptr, I64], Some(Word::Ptr), Libc, "borrows both"),
    import!("memcmp", [Ptr, Ptr, I64], Some(Word::I32), Libc, "borrows both"),
    import!("nz.os_open_read", [Ptr, I64], Some(Word::I64), Files, "borrows the path; a handle, or -errno (Gate 2)"),
    import!("nz.os_open_write", [Ptr, I64, I64], Some(Word::I64), Files, "borrows the path; a handle, or -errno (Gate 2)"),
    import!("nz.os_dir_open", [Ptr, I64], Some(Word::I64), Files, "borrows the path; a handle, or -errno (Gate 2)"),
    import!("nz.os_stdin", [], Some(Word::I64), Files, "standard input's handle (Gate 2)"),
    import!("nz.os_status", [I64], Some(Word::I64), Files, "0 for a live handle, its status otherwise (Gate 2)"),
    import!("nz.os_read", [I64, Ptr, I64], Some(Word::I64), Files, "fills the caller's buffer; a count or -errno (Gate 2)"),
    import!("nz.os_read_line", [I64, Ptr, I64], Some(Word::I64), Files, "a line of standard input into the caller's buffer, its newline included; a count, 0 at the end, or -errno (R1d, P3)"),
    import!("nz.os_write", [I64, Ptr, I64], Some(Word::I64), Files, "borrows the bytes; a count or -errno (Gate 2)"),
    import!("nz.os_seek", [I64, I64, I64], Some(Word::I64), Files, "a position or -errno (Gate 2)"),
    import!("nz.os_sync", [I64], Some(Word::I64), Files, "0 or -errno (Gate 2)"),
    import!("nz.os_close", [I64], Some(Word::I64), Files, "0 or -errno; a close of a handle in use takes effect when the use ends (Gate 2)"),
    import!("nz.os_dir_next", [I64, Ptr, I64], Some(Word::I64), Files, "the next name into the caller's buffer; its length, 0 at the end, or -errno (Gate 2)"),
    import!("nz.os_stat", [Ptr, I64, Ptr], Some(Word::I64), Files, "borrows the path; four words into the caller's 32 bytes; 0 or -errno (Gate 2)"),
    import!("nz.os_path_op", [Ptr, I64, I64], Some(Word::I64), Files, "borrows the path; makes a directory (0), removes one (1) or a file (2); 0 or -errno (Gate 2)"),
    import!("nz.os_rename", [Ptr, I64, Ptr, I64], Some(Word::I64), Files, "borrows both paths; 0 or -errno (Gate 2)"),
    import!("nz.os_family", [], Some(Word::I64), Files, "0 on macOS, 1 on Linux (Gate 2)"),
    import!("nz.os_error_text", [I64, Ptr, I64], Some(Word::I64), Files, "the system's message for a status into the caller's buffer; its length (Gate 2)"),
    import!("nz.os_tcp_listen", [Ptr, I64], Some(Word::I64), Net, "borrows the address; a handle, or -errno (Gate 2)"),
    import!("nz.os_tcp_accept", [I64], Some(Word::I64), Net, "a handle, or -errno; waits at most the listener's deadline (Gate 2)"),
    import!("nz.os_tcp_connect", [Ptr, I64, I64], Some(Word::I64), Net, "borrows the address; a handle, or -errno (Gate 2)"),
    import!("nz.os_udp_bind", [Ptr, I64], Some(Word::I64), Net, "borrows the address; a handle, or -errno (Gate 2)"),
    import!("nz.os_udp_send_to", [I64, Ptr, I64, Ptr, I64], Some(Word::I64), Net, "borrows the bytes and the address; a count, or -errno (Gate 2)"),
    import!("nz.os_udp_recv_from", [I64, Ptr, I64, Ptr, I64], Some(Word::I64), Net, "fills the caller's two buffers; a count, or -errno (Gate 2)"),
    import!("nz.os_set_timeout", [I64, I64], Some(Word::I64), Net, "0, or a status (Gate 2)"),
    import!("nz.os_shutdown", [I64, I64], Some(Word::I64), Net, "0, or -errno (Gate 2)"),
    import!("nz.os_net_addr", [I64, I64, Ptr, I64], Some(Word::I64), Net, "a socket's own or peer's address into the caller's buffer; its length (Gate 2)"),
    import!("nz.os_resolve", [Ptr, I64, I64, Ptr, I64], Some(Word::I64), Net, "borrows the name; the addresses into the caller's buffer; a length, or a status (Gate 2)"),
    import!("nz.os_tls_client_config", [Ptr, I64, Ptr, I64], Some(Word::I64), Tls, "borrows the anchors and the ALPN names; a configuration's id, or a status (C1)"),
    import!("nz.os_tls_server_config", [Ptr, I64, Ptr, I64, Ptr, I64], Some(Word::I64), Tls, "borrows the chain, the key and the ALPN names; a configuration's id, or a status (C1)"),
    import!("nz.os_tls_connect", [I64, Ptr, I64], Some(Word::I64), Tls, "borrows the host name; a session's id, or a status (C1)"),
    import!("nz.os_tls_accept", [I64], Some(Word::I64), Tls, "a session's id, or a status (C1)"),
    import!("nz.os_tls_handshake", [I64], Some(Word::I64), Tls, "1 once the handshake is done, or a status (C1)"),
    import!("nz.os_tls_push", [I64, Ptr, I64], Some(Word::I64), Tls, "borrows the ciphertext and takes a copy of all of it; its length, or a status (C1)"),
    import!("nz.os_tls_pull", [I64, Ptr, I64], Some(Word::I64), Tls, "ciphertext to send into the caller's buffer; its length (C1)"),
    import!("nz.os_tls_read", [I64, Ptr, I64], Some(Word::I64), Tls, "plaintext into the caller's buffer; its length, 0 at the peer's close, or a status (C1)"),
    import!("nz.os_tls_write", [I64, Ptr, I64], Some(Word::I64), Tls, "borrows the plaintext; how much was taken, or a status (C1)"),
    import!("nz.os_tls_close", [I64, Ptr, I64], Some(Word::I64), Tls, "the last ciphertext into the caller's buffer; its length, or a status; the id dies (C1)"),
    import!("nz.os_tls_info", [I64, I64, Ptr, I64], Some(Word::I64), Tls, "a fact about the session into the caller's buffer; its length, or a status (C1)"),
    import!("nz.os_env_find", [Ptr, I64, Ptr], Some(Word::Ptr), Files, "borrows the name; the value inside the environment and its length through the pointer, or an empty string and -1 (Gate 2)"),
    import!("nz.os_env_count", [], Some(Word::I64), Files, "how many entries the environment has (Gate 2)"),
    import!("nz.os_random_bytes", [Ptr, I64], Some(Word::I64), Files, "fills the caller's buffer from the system's entropy; its length, or -errno (Gate 2)"),
    import!("nz.os_env_at", [I64, Ptr], Some(Word::Ptr), Files, "the entry inside the environment, its length through the pointer (Gate 2)"),
    import!("nz.os_spawn", [Ptr, I64, Ptr], Some(Word::I64), Net, "borrows the program and the arguments; the child's handle, or -errno (Gate 2)"),
    import!("nz.os_child_pipe", [I64, I64], Some(Word::I64), Net, "a child's pipe's handle, or a status (Gate 2)"),
    import!("nz.os_wait", [I64], Some(Word::I64), Net, "a child's exit, or a status (Gate 2)"),
    import!("nz.os_kill", [I64], Some(Word::I64), Net, "0, or a status (Gate 2)"),
    import!("nz.os_std_stream", [I64], Some(Word::I64), Files, "standard output's (1) or errors' (2) handle (R1d)"),
    import!("nz.os_spawn_in", [Ptr, I64, Ptr, Ptr, I64, Ptr, I64], Some(Word::I64), Net, "borrows the program, the arguments, the directory and the environment; the child's handle, or -errno (R1d)"),
    import!("nz.os_pid", [], Some(Word::I64), Net, "this process's id (R1d)"),
    import!("nz.os_child_pid", [I64], Some(Word::I64), Net, "a child's process id, or a status (R1d)"),
    import!("nz.os_kill_group", [I64, I64], Some(Word::I64), Net, "0, or a status (R1d)"),
    import!("nz.os_try_wait", [I64], Some(Word::I64), Net, "a child's exit, or a status (R1d)"),
    import!("nz.os_wait_until", [I64, I64], Some(Word::I64), Net, "a child's exit, or a status (R1d)"),
    import!("nz.os_child_rusage", [I64], Some(Word::I64), Net, "a waited child's peak resident memory, or a status (R1d)"),
    import!("nz.os_lstat", [Ptr, I64, Ptr], Some(Word::I64), Files, "borrows the path; six words into the caller's 48 bytes; 0 or -errno (R1d)"),
    import!("nz.os_chmod", [Ptr, I64, I64], Some(Word::I64), Files, "borrows the path; 0 or -errno (R1d)"),
    import!("nz.os_realpath", [Ptr, I64, Ptr, I64], Some(Word::I64), Files, "borrows the path; the canonical path into the caller's buffer; a length, or a status (R1d)"),
    import!("nz.os_mkdtemp", [Ptr, I64, Ptr, I64], Some(Word::I64), Files, "borrows the parent; the new directory's path into the caller's buffer; a length, or a status (R1d)"),
    import!("nz.os_file_lock", [I64, Bit, I64], Some(Word::I64), Files, "0, or a status; waits when the third word is 1 (R1d)"),
    import!("nz.os_file_unlock", [I64], Some(Word::I64), Files, "0, or a status (R1d)"),
    import!("nz.format_float", [Ptr, F64, Bit], Some(Word::I64), Numbers, "writes a float's shortest round-trip text, as a Float32 when the flag is set, into 40 bytes the caller owns and returns how many (Gate 2)"),
    import!("nz.format_fixed", [Ptr, F64, I64], Some(Word::I64), Numbers, "writes a float with exactly so many decimals, C's `%.*f`, into 360 bytes the caller owns and returns how many (R1a, L4)"),
    import!("nz.format_u64", [Ptr, I64], Some(Word::I64), Numbers, "writes a UInt64's decimal digits into 21 bytes the caller owns and returns how many (Gate 2)"),
    import!("nz.float_parses", [Ptr, I64], Some(Word::Bit), Numbers, "borrows the bytes; whether they are a floating-point number as `float_parses` reads one (Gate 2)"),
    import!("nz.float_parse", [Ptr, I64], Some(Word::F64), Numbers, "borrows the bytes; the Float64 they write, or NaN (Gate 2)"),
    import!("nz.i128_divrem", [Ptr, Ptr, Ptr, I64], None, Wide, "writes the quotient or remainder of two 128-bit values it reads, signed or unsigned by the code, the divisor already checked (R1b, L13b)"),
    import!("nz.i128_mul", [Ptr, Ptr, Ptr, I64], Some(Word::I32), Wide, "writes the wrapped product of two 128-bit values it reads and returns nonzero when the exact one leaves the signed or unsigned range (R1b, L13b)"),
    import!("nz.i128_to_f64", [Ptr, I64], Some(Word::F64), Wide, "reads a 128-bit value; the Float64 nearest it, ties to even (R1b, L13b)"),
    import!("nz.i128_to_f32", [Ptr, I64], Some(Word::F32), Wide, "reads a 128-bit value; the Float32 nearest it, ties to even, rounded once (R1b, L13b)"),
    import!("nz.f64_to_i128", [Ptr, F64, I64], None, Wide, "writes a Float64 truncated to a 128-bit value, saturating, a NaN as 0 (R1b, L13b)"),
    import!("nz.format_i128", [Ptr, Ptr, I64], Some(Word::I64), Wide, "reads a 128-bit value and writes its decimal digits into 48 bytes the caller owns; returns how many (R1b, L13b)"),
    import!("nz.utf8_valid", [Ptr, I64], Some(Word::Bit), Strings, "borrows the bytes; whether they are UTF-8 (Gate 2)"),
    import!("nz.utf8_count", [Ptr, I64], Some(Word::I64), Strings, "borrows the bytes, which are UTF-8; how many scalar values they encode (Gate 2)"),
    import!("nz.utf8_decode", [Ptr, I64, I64], Some(Word::I64), Strings, "borrows the bytes, which are UTF-8; the scalar value starting at a boundary (Gate 2)"),
    import!("nz.utf8_encode", [Ptr, I64], Some(Word::I64), Strings, "writes a scalar value's UTF-8 into 4 bytes the caller owns and returns how many (Gate 2)"),
    import!("nz.mem_zero", [Ptr, I64], None, Strings, "zeroes bytes the caller owns (Gate 2)"),
    import!("nz.mem_move", [Ptr, Ptr, I64], None, Strings, "copies bytes that may overlap, borrowing the source (Gate 2)"),
    import!("fmod", [F64, F64], Some(Word::F64), Libc, "the remainder of truncated division"),
    import!("fmodf", [F32, F32], Some(Word::F32), Libc, "the remainder of truncated division"),
    import!("round", [F64], Some(Word::F64), Libc, "half away from zero"),
    import!("roundf", [F32], Some(Word::F32), Libc, "half away from zero"),
    import!("fmin", [F64, F64], Some(Word::F64), Libc, "a NaN loses"),
    import!("fminf", [F32, F32], Some(Word::F32), Libc, "a NaN loses"),
    import!("fmax", [F64, F64], Some(Word::F64), Libc, "a NaN loses"),
    import!("fmaxf", [F32, F32], Some(Word::F32), Libc, "a NaN loses"),
    import!("sin", [F64], Some(Word::F64), Libc, "the host's"),
    import!("sinf", [F32], Some(Word::F32), Libc, "the host's"),
    import!("cos", [F64], Some(Word::F64), Libc, "the host's"),
    import!("cosf", [F32], Some(Word::F32), Libc, "the host's"),
    import!("tan", [F64], Some(Word::F64), Libc, "the host's"),
    import!("tanf", [F32], Some(Word::F32), Libc, "the host's"),
    import!("exp", [F64], Some(Word::F64), Libc, "the host's"),
    import!("expf", [F32], Some(Word::F32), Libc, "the host's"),
    import!("log", [F64], Some(Word::F64), Libc, "the host's"),
    import!("logf", [F32], Some(Word::F32), Libc, "the host's"),
    import!("log2", [F64], Some(Word::F64), Libc, "the host's"),
    import!("log2f", [F32], Some(Word::F32), Libc, "the host's"),
    import!("log10", [F64], Some(Word::F64), Libc, "the host's"),
    import!("log10f", [F32], Some(Word::F32), Libc, "the host's"),
    import!("pow", [F64, F64], Some(Word::F64), Libc, "the host's"),
    import!("powf", [F32, F32], Some(Word::F32), Libc, "the host's"),
    import!("atan2", [F64, F64], Some(Word::F64), Libc, "the host's"),
    import!("atan2f", [F32, F32], Some(Word::F32), Libc, "the host's"),
];

/// The runtime's data symbols a backend may read.
pub const DATA: &[(&str, &str)] = &[
    (
        "nz.argc",
        "the argument count, an i64, set by the platform entry",
    ),
    (
        "nz.argv",
        "the argument vector, a pointer, set by the platform entry",
    ),
];

/// How a runtime entry can fail (§7.45, *The failure model*).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// It cannot.
    Never,
    /// It returns null (or writes a null pointer) when the host refuses memory; the call site
    /// records `N0406`.
    Null,
    /// It returns a status the call site turns into a failure (`N0404`, `N0406`, `N0407`) or,
    /// for `file_exists` and a channel's `false`, into an ordinary answer.
    Status,
    /// It records the thread's failure itself.
    Records,
    /// It ends the process.
    Ends,
}

/// What may call a runtime entry concurrently (§7.45, *Thread safety*).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Safety {
    /// Any thread, at any time: it touches only its arguments, atomic counts or
    /// thread-local state.
    Any,
    /// Any thread: it takes the channel's lock.
    Locked,
    /// One thread at a time per value: sequences never cross into a task (`N0321`).
    Owner,
    /// Only the entry, after every task has been joined.
    Entry,
}

/// One symbol the runtime defines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeSymbol {
    pub symbol: &'static str,
    /// The LLVM result type, and the parameter types in order: exactly the definition's.
    pub ret: &'static str,
    pub params: &'static [&'static str],
    /// Function attributes after the parameter list (`noreturn`), or empty.
    pub attrs: &'static str,
    /// Whether every generated unit is given a declaration of it (the LLVM backend calls
    /// it); otherwise it is reached only through [`IMPORTS`] or from inside the runtime.
    pub declared: bool,
    pub part: Part,
    /// What it is for.
    pub responsibility: &'static str,
    /// What it does with the references it is handed or hands back.
    pub ownership: &'static str,
    /// The effect of the built-in it implements, if any; the authority for it was checked
    /// when the program was, and the runtime takes none.
    pub effect: &'static str,
    pub failure: Failure,
    pub safety: Safety,
}

macro_rules! rt {
    ($sym:literal, $ret:literal ($($p:literal),*) $attrs:literal, $decl:literal, $part:ident, $eff:literal, $fail:ident, $safe:ident, $resp:literal, $own:literal) => {
        RuntimeSymbol {
            symbol: $sym,
            ret: $ret,
            params: &[$($p),*],
            attrs: $attrs,
            declared: $decl,
            part: Part::$part,
            responsibility: $resp,
            ownership: $own,
            effect: $eff,
            failure: Failure::$fail,
            safety: Safety::$safe,
        }
    };
}

/// Every function the runtime defines, once (N43, §7.45). The declared ones come first, in
/// the order every unit declares them.
pub const INVENTORY: &[RuntimeSymbol] = &[
    rt!("nz.fail", "void" ("ptr", "i64") "", true, Base, "", Records, Any, "record this thread's first failure", "takes a static message; owns nothing"),
    rt!("nz.report", "void" () "noreturn", true, Base, "", Ends, Entry, "write the thread's failure to stderr and exit 2", "none"),
    rt!("nz.stack_check", "i32" () "", true, Base, "", Never, Any, "whether this thread's stack is past its limit, the limit set on the thread's first check (N49)", "none"),
    rt!("nz.stack_check_frame", "i32" ("i64") "", true, Base, "", Never, Any, "whether this thread's stack is past its limit or a frame of that many bytes would cross it (Q1-A1-01)", "none"),
    rt!("nz.closure_new", "ptr" ("i64", "ptr", "ptr") "", true, Closures, "", Null, Any, "a closure object with its code and release function (N50)", "one reference, owned by the caller; the caller stores the captures"),
    rt!("nz.closure_retain", "void" ("ptr") "", true, Closures, "", Never, Any, "one more reference to a closure object", "adds a reference, atomically; null is a no-op"),
    rt!("nz.closure_release", "void" ("ptr") "", true, Closures, "", Never, Any, "one reference fewer; the last releases the captures and frees it", "takes a reference, atomically; null is a no-op"),
    rt!("nz.str_join", "{ptr, i64, ptr}" ("ptr", "ptr", "i64") "", true, Join, "", Null, Any, "join a sequence of strings with a separator, allocating once", "borrows both; the result is owned by the caller"),
    rt!("nz.file_exists", "i1" ("ptr", "i64") "", true, Io, "io", Status, Any, "whether a path names a readable file", "borrows the path"),
    rt!("nz.read_file", "{ptr, i64, ptr}" ("ptr", "i64") "", true, Io, "io", Status, Any, "a file's contents as a string", "borrows the path; the result is owned by the caller, or null when unreadable"),
    rt!("nz.write_file", "i64" ("ptr", "i64", "ptr", "i64") "", true, Io, "io", Status, Any, "write a string to a file, truncating", "borrows both; returns the byte count or a negative status"),
    rt!("nz.arr_new", "ptr" () "", true, Sequences, "", Null, Any, "an empty internal list, not counted as a program sequence", "one reference, owned by the caller"),
    rt!("nz.arr_grow", "ptr" ("ptr", "i64") "", true, Sequences, "", Null, Owner, "room for one more element of this size", "borrows the sequence; returns its data pointer"),
    rt!("nz.ints_sum", "i64" ("ptr", "i64", "ptr") "", true, Sequences, "", Never, Owner, "the ordered checked sum of an `Ints` onto a start, vectorised where every partial sum is provably in range (N66)", "borrows the sequence; writes the overflow flag"),
    rt!("nz.soa_grow", "ptr" ("ptr", "i64") "", true, Sequences, "", Null, Owner, "room for one more element of a `Vec` laid out one array per field (N68)", "borrows the vector; returns its data pointer"),
    rt!("nz.arr_retain", "void" ("ptr") "", true, Sequences, "", Never, Owner, "one more reference to a sequence header", "adds a reference"),
    rt!("nz.arr_release", "i1" ("ptr") "", true, Sequences, "", Never, Owner, "one reference fewer to an internal list; frees it on the last", "takes a reference; true when it was the last"),
    rt!("nz.seq_new", "ptr" () "", true, Sequences, "", Null, Any, "an empty program sequence, counted", "one reference, owned by the caller"),
    rt!("nz.seq_release", "void" ("ptr") "", true, Sequences, "", Never, Owner, "release a sequence whose elements own nothing", "takes a reference; null is a no-op"),
    rt!("nz.strs_release", "void" ("ptr") "", true, Sequences, "", Never, Owner, "release a sequence of strings, and each string on the last reference", "takes a reference; null is a no-op"),
    rt!("nz.str_alloc", "ptr" ("i64") "", true, Strings, "", Null, Any, "a string backing with this many bytes after its 16-byte header", "one reference, owned by the caller"),
    rt!("nz.str_retain", "void" ("ptr") "", true, Strings, "", Never, Any, "one more reference to a backing", "adds a reference, atomically; null is a no-op"),
    rt!("nz.str_release", "void" ("ptr") "", true, Strings, "", Never, Any, "one reference fewer; the last frees it", "takes a reference, atomically; null is a no-op"),
    rt!("nz.atomic_cell", "ptr" ("i64") "", true, Strings, "", Null, Any, "an atomic's cell holding a value, not counted as a string", "one reference, owned by the caller"),
    rt!("nz.spawn", "i32" ("ptr", "ptr", "ptr") "", true, Tasks, "spawn", Status, Any, "start a thread for a task and record it in its scope's list, record first", "takes the argument block; 0, -1 out of memory, >0 no thread"),
    rt!("nz.scope_join", "{ptr, i64}" ("ptr") "", true, Tasks, "", Never, Any, "join every task in a list and hand back the first failure", "frees the list and each task's block"),
    rt!("nz.chan_new", "ptr" ("i64") "", true, Channels, "", Null, Any, "a bounded channel", "one reference, owned by the caller"),
    rt!("nz.chan_send", "i1" ("ptr", "i64") "", true, Channels, "", Status, Locked, "send, blocking while full", "borrows the channel; false when closed"),
    rt!("nz.chan_recv", "i1" ("ptr", "ptr", "ptr", "i64") "", true, Channels, "", Records, Locked, "receive into a sequence, blocking while empty", "borrows the channel and the sequence; false once closed and drained; records the call site's message only when the sequence cannot grow"),
    rt!("nz.chan_close", "void" ("ptr") "", true, Channels, "", Never, Locked, "close a channel, waking every waiter", "borrows the channel"),
    rt!("nz.chan_retain", "void" ("ptr") "", true, Channels, "", Never, Any, "one more reference to a channel", "adds a reference, atomically"),
    rt!("nz.chan_release", "void" ("ptr") "", true, Channels, "", Never, Any, "one reference fewer; the last frees the channel", "takes a reference, atomically"),
    rt!("nz.chanv_new", "ptr" ("i64", "i64", "ptr") "", true, Channels, "", Null, Any, "a bounded generic channel (N53)", "one reference, owned by the caller"),
    rt!("nz.chanv_send", "i1" ("ptr", "ptr") "", true, Channels, "", Status, Locked, "send a slot, blocking while full", "borrows the channel; takes the slot's reference when it returns true"),
    rt!("nz.chanv_recv", "i1" ("ptr", "ptr") "", true, Channels, "", Status, Locked, "receive a slot, blocking while empty", "borrows the channel; hands the slot's reference to the caller when it returns true"),
    rt!("nz.chanv_release", "void" ("ptr") "", true, Channels, "", Never, Any, "one reference fewer; the last releases the queued slots and frees the channel", "takes a reference, atomically"),
    rt!("nz.atomic_load", "i64" ("ptr") "", true, Base, "", Never, Any, "a sequentially consistent load of an atomic's cell (Gate 3)", "borrows the cell"),
    rt!("nz.atomic_store", "void" ("ptr", "i64") "", true, Base, "", Never, Any, "a sequentially consistent store to an atomic's cell (Gate 3)", "borrows the cell"),
    rt!("nz.atomic_swap", "i64" ("ptr", "i64") "", true, Base, "", Never, Any, "a sequentially consistent exchange of an atomic's cell (Gate 3)", "borrows the cell; the value before"),
    rt!("nz.atomic_cas", "i1" ("ptr", "i64", "i64") "", true, Base, "", Status, Any, "a sequentially consistent compare-and-swap of an atomic's cell (Gate 3)", "borrows the cell; whether it stored"),
    rt!("nz.mmio_read32", "i64" ("i64") "", true, Base, "", Never, Owner, "a volatile 32-bit read of a device register (N62); reached only in a freestanding image", "none"),
    rt!("nz.mmio_write32", "i64" ("i64", "i64") "", true, Base, "", Never, Owner, "a volatile 32-bit write of a device register (N62); reached only in a freestanding image", "none"),
    rt!("nz.mmio_read8", "i64" ("i64") "", true, Base, "", Never, Owner, "a volatile 8-bit read of a device register (N86); reached only in a freestanding image", "none"),
    rt!("nz.mmio_write8", "i64" ("i64", "i64") "", true, Base, "", Never, Owner, "a volatile 8-bit write of a device register (N86); reached only in a freestanding image", "none"),
    rt!("nz.mmio_read16", "i64" ("i64") "", true, Base, "", Never, Owner, "a volatile 16-bit read of a device register (N86); reached only in a freestanding image", "none"),
    rt!("nz.mmio_write16", "i64" ("i64", "i64") "", true, Base, "", Never, Owner, "a volatile 16-bit write of a device register (N86); reached only in a freestanding image", "none"),
    rt!("nz.mmio_read64", "i64" ("i64") "", true, Base, "", Never, Owner, "a volatile 64-bit read of a device register (Gate 3); reached only in a freestanding image", "none"),
    rt!("nz.mmio_write64", "i64" ("i64", "i64") "", true, Base, "", Never, Owner, "a volatile 64-bit write of a device register (Gate 3); reached only in a freestanding image", "none"),
    rt!("nz.watchdog_feed", "i64" () "", true, Base, "", Never, Owner, "re-arms the board's watchdog to what its scheduler armed (Gate 3); reached only in a freestanding image whose board has one", "none"),
    rt!("nz.cstr_has_nul", "i1" ("ptr", "i64") "", true, Strings, "", Status, Any, "whether a Str argument to C holds a NUL byte (N55)", "borrows the bytes"),
    rt!("nz.cstr_borrow", "ptr" ("ptr", "i64") "", true, Strings, "", Null, Any, "a Str argument as a C string, for one foreign call (N55)", "borrows the bytes; the copy is the caller's to free"),
    rt!("nz.cstr_len", "i64" ("ptr") "", true, Strings, "", Never, Any, "the length of a C string a foreign function returned (N85)", "borrows the C string"),
    rt!("nz.errno_save", "void" () "", true, Foreign, "", Never, Owner, "save this thread's errno as a foreign call returns (N85)", "none"),
    rt!("nz.errno_last", "i64" () "", true, Foreign, "", Never, Owner, "the errno the last foreign call on this thread left (N85)", "none"),
    rt!("nz.chanv_close", "void" ("ptr") "", true, Channels, "", Never, Locked, "close a generic channel and wake every select (N54)", "borrows the channel"),
    rt!("nz.chanv_select", "i64" ("ptr", "ptr", "i64", "i64") "", true, Channels, "", Status, Locked, "wait for the lowest-indexed ready channel, or a deadline (N54)", "borrows the channels; hands one slot's reference to the caller when it returns an index"),
    rt!("nz.time_now_ms", "i64" () "", true, Channels, "", Never, Any, "a monotonic clock in milliseconds (N54)", "none"),
    rt!("nz.time_now_ns", "i64" () "", false, Channels, "", Never, Any, "the same monotonic clock in nanoseconds (Gate 2)", "none"),
    rt!("nz.time_wall_ns", "i64" () "", false, Channels, "", Never, Any, "the wall clock in nanoseconds since the Unix epoch (Gate 2)", "none"),
    rt!("nz.time_sleep_ms", "i64" ("i64") "", false, Tasks, "", Never, Any, "wait, parked on the pool, for milliseconds (Gate 2)", "none"),
    rt!("nz.sched_note_spawn", "void" () "", true, Tasks, "", Never, Any, "count a started task (N54)", "none"),
    rt!("nz.sched_note_join", "void" ("ptr") "", true, Tasks, "", Never, Any, "count a scope's tasks as joined (N54)", "borrows the task list"),
    rt!("nz.sel_sync_get", "ptr" () "", false, Channels, "", Null, Any, "the select lock and condition variable, made on first use", "the process owns it"),
    rt!("nz.sel_signal", "void" () "", false, Channels, "", Never, Any, "advance the select event count and wake any select", "none"),
    rt!("nz.chanv_try", "i64" ("ptr", "ptr") "", false, Channels, "", Never, Locked, "one look at a channel without waiting", "borrows the channel; hands the slot's reference over when it returns 1"),
    rt!("nz.failed_now", "i32" () "", false, Base, "", Never, Any, "whether this thread has failed, for a second backend", "none"),
    rt!("nz.pool_mode_get", "i64" () "", false, Tasks, "", Never, Any, "which scheduler this process runs, decided once from NAZM_SCHEDULER and the program's policy (N83, N106)", "none"),
    rt!("nz.pool_env", "i64" ("ptr", "i64") "", false, Tasks, "", Never, Any, "a positive count from an environment variable, or a default (N83)", "borrows the name"),
    rt!("nz.pool_sync", "ptr" () "", false, Tasks, "", Null, Any, "a mutex and a condition variable in one allocation (N83)", "the process owns it"),
    rt!("nz.pool_start", "i1" () "", false, Tasks, "", Status, Any, "start the pool's tables and workers (N83)", "the process owns them"),
    rt!("nz.pool_push", "void" ("ptr", "ptr") "", false, Tasks, "", Never, Locked, "queue a runnable task on its worker and wake it (N83)", "the worker's queue holds the task"),
    rt!("nz.pool_now", "i64" () "", false, Tasks, "", Never, Any, "the monotonic clock in nanoseconds, which every deadline is on (N83, Q1-M-12)", "none"),
    rt!("nz.pool_timers", "i1" ("ptr", "ptr") "", false, Tasks, "", Never, Locked, "queue the worker's parked tasks whose deadline passed; the soonest left (N83)", "none"),
    rt!("nz.task_enter", "void" ("ptr") "", false, Tasks, "", Never, Owner, "make a task this thread's, with its own failure state and stack limit (N83)", "borrows the task"),
    rt!("nz.task_leave", "void" ("ptr") "", false, Tasks, "", Never, Owner, "save a task's failure state and stack limit and clear the thread's (N83)", "borrows the task"),
    rt!("nz.worker_main", "ptr" ("ptr") "", false, Tasks, "", Never, Owner, "a pool worker: run its tasks first in, first out, on its run stack (N83, 6.2)", "the worker owns its run stack and its tasks' saved frames"),
    rt!("nz.task_start", "i32" ("ptr", "ptr") "", false, Tasks, "", Status, Any, "start a task on the pool: a record, no stack (N83, 6.2)", "takes the argument block; 0, or 1 when no record could be had"),
    rt!("nz.task_place", "i1" ("ptr", "ptr") "", false, Tasks, "", Status, Owner, "put a task's frames on its worker's run stack, saving those of the task in place (6.2)", "the worker's tasks own their saved frames; false when they could not be saved"),
    rt!("nz_task_main", "void" ("ptr") "", false, Tasks, "", Never, Owner, "a task's body on its own stack, then its end (N83)", "borrows the task"),
    rt!("nz.task_suspend", "void" ("ptr") "", false, Tasks, "", Never, Owner, "switch from a task back to its worker (N83)", "none"),
    rt!("nz.task_wake", "void" ("ptr") "", false, Tasks, "", Never, Any, "make a parked task runnable, or note the wake (N83)", "none"),
    rt!("nz.pool_bucket", "ptr" ("ptr") "", false, Tasks, "", Never, Any, "the waiter list a condition variable hashes to (N83)", "none"),
    rt!("nz.pool_unwait", "void" ("ptr") "", false, Tasks, "", Never, Any, "take a task off the waiters it is still on (N83)", "none"),
    rt!("nz.cwait", "i32" ("ptr", "ptr") "", false, Tasks, "", Never, Locked, "wait on a condition variable: park a pool task, block a thread (N83)", "the mutex is held again on return"),
    rt!("nz.ctimedwait", "i32" ("ptr", "ptr", "ptr") "", false, Tasks, "", Never, Locked, "as nz.cwait, until a monotonic time; a thread naps at most 100 ms on the wall clock and its caller looks again (N83, Q1-M-12)", "the mutex is held again on return"),
    rt!("nz.timed_nap", "void" ("ptr", "ptr") "", false, Tasks, "", Never, Any, "a monotonic deadline as the wall-clock time a timed wait measures, at most 100 ms ahead (Q1-M-12)", "writes the second timespec"),
    rt!("nz.pool_park", "void" ("ptr", "ptr", "ptr", "i64") "", false, Tasks, "", Never, Locked, "register a task as a waiter, park it and give its worker back (N83)", "releases and retakes the mutex"),
    rt!("nz.cwake", "void" ("ptr") "", false, Tasks, "", Never, Any, "wake every thread and pool task waiting on a condition variable (N83)", "none"),
    rt!("nz.task_yield", "void" () "", false, Tasks, "", Never, Owner, "let a pool task's worker run its other tasks first (N83)", "none"),
    rt!("nz.pool_report", "void" () "", false, Tasks, "", Never, Entry, "the pool's line of the schedule report, where it ran (N83)", "none"),
    rt!("nz.join_one", "i32" ("ptr") "", false, Tasks, "", Status, Any, "wait for one task to end, its thread joined or its record freed (N83)", "frees the pool task's record"),
    rt!("nz.format_int", "i64" ("ptr", "i64") "", false, Base, "", Never, Any, "an Int's decimal digits into a 21-byte buffer", "writes into the caller's buffer"),
    rt!("nz.vec_release", "void" ("ptr", "ptr", "i64", "ptr") "", false, Sequences, "", Never, Owner, "release a sequence, each element with its type's release at its stride, through a queue rather than recursion (R1a)", "takes a reference; null is a no-op; the queue is null outside a release"),
    rt!("nz.vec_drain", "void" ("ptr", "ptr", "i64", "ptr") "", false, Sequences, "", Never, Owner, "empty a dead sequence: each element's release, given the queue, then its storage (R1a)", "takes the sequence's last reference"),
    rt!("nz.vec_enqueue", "i1" ("ptr", "ptr", "ptr", "i64") "", false, Sequences, "", Never, Owner, "queue a dead sequence on the release in progress; false when the queue cannot grow (R1a)", "takes the sequence's last reference when it answers true"),
    rt!("nz.read_file_to", "void" ("ptr", "ptr", "i64") "", false, Io, "io", Status, Any, "`nz.read_file`, written through a pointer", "as `nz.read_file`"),
    rt!("nz.str_join_to", "void" ("ptr", "ptr", "ptr", "i64") "", false, Join, "", Null, Any, "`nz.str_join`, written through a pointer", "as `nz.str_join`"),
    rt!("nz.scope_join_to", "void" ("ptr", "ptr") "", false, Tasks, "", Never, Any, "`nz.scope_join`, written through a pointer", "as `nz.scope_join`"),
    rt!("nz.task_publish", "void" ("ptr") "", false, Tasks, "", Never, Any, "copy this thread's failure into its task block", "borrows the block"),
    rt!("nz.cstr", "ptr" ("ptr", "i64") "", false, Io, "", Null, Any, "a NUL-terminated copy of a path, for the C library", "the copy is owned by the caller, who frees it"),
    rt!("nz.os_acquire", "void" () "", false, Files, "", Never, Any, "take the handle table's spin lock (Gate 2)", "none"),
    rt!("nz.os_release", "void" () "", false, Files, "", Never, Any, "give the handle table's spin lock back", "none"),
    rt!("nz.os_errno", "i64" () "", false, Files, "", Never, Any, "-errno, read after a failed system call", "none"),
    rt!("nz.os_slot", "ptr" ("i64") "", false, Files, "", Null, Locked, "a descriptor's slot, the table grown to hold it", "the lock is held"),
    rt!("nz.os_register", "i64" ("i64", "i64", "ptr") "", false, Files, "", Status, Locked, "a new generation's handle for a descriptor", "the lock is held"),
    rt!("nz.os_live", "ptr" ("i64") "", false, Files, "", Null, Locked, "the live slot a handle names", "the lock is held"),
    rt!("nz.os_pin", "i64" ("i64") "", false, Files, "", Status, Any, "pin a live handle: its kind, or its status", "takes the lock"),
    rt!("nz.os_unpin", "void" ("i64") "", false, Files, "", Never, Any, "unpin a handle; the last unpin of one closed meanwhile closes it", "takes the lock"),
    rt!("nz.os_shut", "i64" ("i64", "i64", "ptr") "", false, Files, "", Status, Any, "close a descriptor or a directory", "owns what it closes"),
    rt!("nz.os_adopt", "i64" ("i32", "i64", "ptr") "", false, Files, "", Status, Any, "a handle for a descriptor just opened, closing it if the table cannot grow", "takes the descriptor"),
    rt!("nz.os_open_read", "i64" ("ptr", "i64") "", false, Files, "io", Status, Any, "open a file to read (Gate 2)", "borrows the path"),
    rt!("nz.os_open_write", "i64" ("ptr", "i64", "i64") "", false, Files, "io", Status, Any, "open a file to write, by mode (Gate 2)", "borrows the path"),
    rt!("nz.os_dir_open", "i64" ("ptr", "i64") "", false, Files, "io", Status, Any, "open a directory to list (Gate 2)", "borrows the path"),
    rt!("nz.os_stdin", "i64" () "", false, Files, "io", Status, Any, "standard input's handle (Gate 2)", "none"),
    rt!("nz.os_std_stream", "i64" ("i64") "", false, Files, "io", Status, Any, "standard output's or errors' handle (R1d)", "none"),
    rt!("nz.os_status", "i64" ("i64") "", false, Files, "", Status, Any, "whether a handle is live (Gate 2)", "none"),
    rt!("nz.os_read", "i64" ("i64", "ptr", "i64") "", false, Files, "io", Status, Any, "read into a buffer (Gate 2)", "writes the caller's buffer"),
    rt!("nz.heap_reserve", "void" ("i64") "", false, Heap, "", Ends, Any, "count bytes as live, or end the process past the heap limit (R1d, D5)", "none"),
    rt!("nz.heap_malloc", "ptr" ("i64") "", false, Heap, "", Null, Any, "C's `malloc`, counted against the heap limit (R1d, D5)", "memory the caller owns, or null"),
    rt!("nz.heap_free", "void" ("ptr") "", false, Heap, "", Never, Any, "C's `free` of a counted block (R1d, D5)", "takes the memory; null is a no-op"),
    rt!("nz.heap_realloc", "ptr" ("ptr", "i64") "", false, Heap, "", Null, Any, "C's `realloc` of a counted block, its growth counted first (R1d, D5)", "takes the memory and returns memory the caller owns, or null with it unchanged"),
    rt!("nz.small_malloc", "ptr" ("i64") "", false, Heap, "", Null, Any, "C's `malloc`, a block of up to 512 bytes from this thread's list of its size class (R1.5 3.1)", "memory the caller owns, or null"),
    rt!("nz.small_free", "void" ("ptr") "", false, Heap, "", Never, Any, "C's `free` of a small-block heap's block, kept on this thread's list while it is short (R1.5 3.1)", "takes the memory; null is a no-op"),
    rt!("nz.small_realloc", "ptr" ("ptr", "i64") "", false, Heap, "", Null, Any, "C's `realloc` of a small-block heap's block (R1.5 3.1)", "takes the memory and returns memory the caller owns, or null with it unchanged"),
    rt!("nz.os_read_line", "i64" ("i64", "ptr", "i64") "", false, Files, "io", Status, Any, "read a line of standard input into a buffer (R1d, P3)", "writes the caller's buffer"),
    rt!("nz.os_in_acquire", "void" () "", false, Files, "", Never, Any, "take standard input's spin lock (R1d, P3)", "none"),
    rt!("nz.os_in_release", "void" () "", false, Files, "", Never, Any, "give standard input's spin lock back (R1d, P3)", "none"),
    rt!("nz.os_in_read", "i64" ("ptr", "i64") "", false, Files, "io", Status, Any, "read standard input, its buffer first (R1d, P3)", "writes the caller's buffer; takes standard input's lock"),
    rt!("nz.os_in_line", "i64" ("ptr", "i64") "", false, Files, "io", Status, Any, "read standard input to its next newline, keeping what follows (R1d, P3)", "writes the caller's buffer; makes standard input's buffer, which the process owns; takes standard input's lock"),
    rt!("nz.os_write", "i64" ("i64", "ptr", "i64") "", false, Files, "io", Status, Any, "write bytes (Gate 2)", "borrows the bytes"),
    rt!("nz.os_seek", "i64" ("i64", "i64", "i64") "", false, Files, "io", Status, Any, "move a file's position (Gate 2)", "none"),
    rt!("nz.os_sync", "i64" ("i64") "", false, Files, "io", Status, Any, "a file's data on storage (Gate 2)", "none"),
    rt!("nz.os_close", "i64" ("i64") "", false, Files, "io", Status, Any, "close a handle (Gate 2)", "none"),
    rt!("nz.os_dir_next", "i64" ("i64", "ptr", "i64") "", false, Files, "io", Status, Any, "a directory's next entry (Gate 2)", "writes the caller's buffer"),
    rt!("nz.os_stat", "i64" ("ptr", "i64", "ptr") "", false, Files, "io", Status, Any, "a path's size, kind, time and permissions (Gate 2)", "borrows the path; writes the caller's 32 bytes"),
    rt!("nz.os_path_op", "i64" ("ptr", "i64", "i64") "", false, Files, "io", Status, Any, "make or remove a directory, or remove a file (Gate 2)", "borrows the path"),
    rt!("nz.os_rename", "i64" ("ptr", "i64", "ptr", "i64") "", false, Files, "io", Status, Any, "rename a path (Gate 2)", "borrows both paths"),
    rt!("nz.os_family", "i64" () "", false, Files, "", Never, Any, "the target's family of error numbers (Gate 2)", "none"),
    rt!("nz.os_error_text", "i64" ("i64", "ptr", "i64") "", false, Files, "", Never, Any, "the system's message for a status (Gate 2)", "writes the caller's buffer"),
    rt!("nz.net_recv", "i64" ("i64", "ptr", "i64") "", false, Net, "io", Status, Any, "a read from a connected socket, waiting at most its deadline (Gate 2)", "writes the caller's buffer"),
    rt!("nz.net_send", "i64" ("i64", "ptr", "i64") "", false, Net, "io", Status, Any, "a write to a connected socket, waiting at most its deadline (Gate 2)", "borrows the bytes"),
    rt!("nz.net_wake", "void" ("ptr") "", false, Net, "", Never, Any, "wake every task waiting on a wait record (Gate 2)", "takes the record's lock"),
    rt!("nz.net_now", "i64" () "", false, Net, "", Never, Any, "the monotonic clock in nanoseconds, which a socket's deadline is on (Gate 2, Q1-M-12)", "none"),
    rt!("nz.net_start", "void" () "", false, Net, "", Never, Any, "start the reactor, once (Gate 2)", "none"),
    rt!("nz.net_reactor", "ptr" ("ptr") "", false, Net, "", Never, Entry, "the reactor thread: wait for events and wake their handles (Gate 2)", "none"),
    rt!("nz.net_event", "void" ("i64") "", false, Net, "", Never, Any, "wake an event's handle if it is still that generation's (Gate 2)", "takes the table's lock, then the record's"),
    rt!("nz.net_waitrec", "ptr" ("i64") "", false, Net, "", Null, Any, "a live handle's wait record, made the first time (Gate 2)", "takes the table's lock"),
    rt!("nz.net_closing", "i1" ("i64") "", false, Net, "", Never, Any, "whether a handle is no longer live (Gate 2)", "takes the table's lock"),
    rt!("nz.net_deadline", "i64" ("i64") "", false, Net, "", Never, Any, "when a wait on a handle gives up (Gate 2)", "takes the table's lock"),
    rt!("nz.net_wait", "i64" ("i64", "i64", "i64") "", false, Net, "", Status, Any, "wait until a socket may be ready, its deadline passes or it closes (Gate 2)", "takes the record's lock"),
    rt!("nz.net_would_block", "i1" () "", false, Net, "", Never, Any, "whether the failure just reported would block (Gate 2)", "none"),
    rt!("nz.net_parse", "i64" ("ptr", "i64", "ptr") "", false, Net, "", Status, Any, "HOST:PORT into a socket address (Gate 2)", "borrows the text; writes the caller's 128 bytes"),
    rt!("nz.os_tcp_listen", "i64" ("ptr", "i64") "", false, Net, "io", Status, Any, "listen on an address (Gate 2)", "borrows the address"),
    rt!("nz.os_tcp_accept", "i64" ("i64") "", false, Net, "io", Status, Any, "accept a connection (Gate 2)", "none"),
    rt!("nz.os_tcp_connect", "i64" ("ptr", "i64", "i64") "", false, Net, "io", Status, Any, "connect to an address (Gate 2)", "borrows the address"),
    rt!("nz.os_udp_bind", "i64" ("ptr", "i64") "", false, Net, "io", Status, Any, "bind a datagram socket (Gate 2)", "borrows the address"),
    rt!("nz.os_udp_send_to", "i64" ("i64", "ptr", "i64", "ptr", "i64") "", false, Net, "io", Status, Any, "send a datagram (Gate 2)", "borrows the bytes and the address"),
    rt!("nz.os_udp_recv_from", "i64" ("i64", "ptr", "i64", "ptr", "i64") "", false, Net, "io", Status, Any, "receive a datagram and its sender (Gate 2)", "writes the caller's buffers"),
    rt!("nz.os_set_timeout", "i64" ("i64", "i64") "", false, Net, "io", Status, Any, "a handle's deadline (Gate 2)", "none"),
    rt!("nz.os_shutdown", "i64" ("i64", "i64") "", false, Net, "io", Status, Any, "shut a connection's reading or writing (Gate 2)", "none"),
    rt!("nz.os_net_addr", "i64" ("i64", "i64", "ptr", "i64") "", false, Net, "io", Never, Any, "a socket's own or peer's address (Gate 2)", "writes the caller's buffer"),
    rt!("nz.os_resolve", "i64" ("ptr", "i64", "i64", "ptr", "i64") "", false, Net, "io", Status, Any, "resolve a name (Gate 2)", "borrows the name; writes the caller's buffer"),
    rt!("nz.os_tls_client_config", "i64" ("ptr", "i64", "ptr", "i64") "", false, Tls, "io", Status, Any, "a client's TLS configuration: the platform's trust store or the named anchors (C1)", "borrows both; the engine keeps copies"),
    rt!("nz.os_tls_server_config", "i64" ("ptr", "i64", "ptr", "i64", "ptr", "i64") "", false, Tls, "io", Status, Any, "a server's TLS configuration: its chain and key, PEM (C1)", "borrows all three; the engine keeps copies"),
    rt!("nz.os_tls_connect", "i64" ("i64", "ptr", "i64") "", false, Tls, "io", Status, Locked, "a client session verifying a host (C1)", "borrows the host; holds the configuration's slot while it copies from it"),
    rt!("nz.os_tls_accept", "i64" ("i64") "", false, Tls, "io", Status, Locked, "a server session (C1)", "holds the configuration's slot while it copies from it"),
    rt!("nz.os_tls_handshake", "i64" ("i64") "", false, Tls, "io", Status, Locked, "one step of the handshake over what has been pushed (C1)", "holds the session's slot"),
    rt!("nz.os_tls_push", "i64" ("i64", "ptr", "i64") "", false, Tls, "io", Status, Locked, "ciphertext received, into the engine (C1)", "borrows the bytes; the engine copies them"),
    rt!("nz.os_tls_pull", "i64" ("i64", "ptr", "i64") "", false, Tls, "io", Never, Locked, "ciphertext the engine wants sent (C1)", "writes the caller's buffer"),
    rt!("nz.os_tls_read", "i64" ("i64", "ptr", "i64") "", false, Tls, "io", Status, Locked, "plaintext from what has been pushed (C1)", "writes the caller's buffer"),
    rt!("nz.os_tls_write", "i64" ("i64", "ptr", "i64") "", false, Tls, "io", Status, Locked, "plaintext into the engine, to be pulled as ciphertext (C1)", "borrows the bytes; the engine copies them"),
    rt!("nz.os_tls_close", "i64" ("i64", "ptr", "i64") "", false, Tls, "io", Status, Locked, "end a session or a configuration (C1)", "writes the caller's buffer; frees the engine's objects"),
    rt!("nz.os_tls_info", "i64" ("i64", "i64", "ptr", "i64") "", false, Tls, "io", Status, Locked, "the ALPN name, version, peer or last failure of a session (C1)", "writes the caller's buffer"),
    rt!("nz.tls_table", "ptr" () "", false, Tls, "", Null, Any, "the session table, made zeroed on first use (C1)", "none"),
    rt!("nz.tls_put", "i64" ("ptr", "i64") "", false, Tls, "", Status, Any, "a free slot for an engine object: its id, or -24 when the table is full (C1)", "takes the object into the slot"),
    rt!("nz.tls_slot", "ptr" ("i64") "", false, Tls, "", Null, Any, "an id's slot, or null for an id no slot has (C1)", "none"),
    rt!("nz.tls_hold", "ptr" ("ptr", "i64", "i64") "", false, Tls, "", Null, Locked, "hold a slot for a live id of a kind: its object, or null with nothing held (C1)", "takes the slot's hold"),
    rt!("nz.tls_let_go", "void" ("ptr") "", false, Tls, "", Never, Locked, "let a held slot go (C1)", "releases the slot's hold"),
    rt!("nz.tls_configure", "i64" ("i64", "ptr", "i64", "ptr", "i64", "ptr", "i64") "", false, Tls, "io", Status, Any, "a configuration from the engine, in a slot (C1)", "borrows the buffers"),
    rt!("nz.tls_open", "i64" ("i64", "i64", "ptr", "i64") "", false, Tls, "io", Status, Locked, "a session from a configuration, in a slot (C1)", "holds the configuration's slot while the engine copies from it"),
    rt!("nz.tls_call", "i64" ("i64", "i64", "ptr", "i64", "i64") "", false, Tls, "io", Status, Locked, "one engine call on a session, its slot held (C1)", "holds the session's slot"),
    rt!("nz.os_env_split", "i64" ("ptr") "", false, Files, "", Never, Any, "where an environment entry's name ends, or -1 (Gate 2)", "borrows the entry"),
    rt!("nz.os_env_nth", "ptr" ("i64", "ptr") "", false, Files, "", Never, Any, "the nth entry of the environment, or the count (Gate 2)", "none"),
    rt!("nz.os_env_count", "i64" () "", false, Files, "io", Never, Any, "how many entries the environment has (Gate 2)", "none"),
    rt!("nz.os_random_bytes", "i64" ("ptr", "i64") "", false, Files, "", Status, Any, "the system's entropy into a buffer (Gate 2)", "writes the caller's buffer"),
    rt!("nz.os_env_at", "ptr" ("i64", "ptr") "", false, Files, "io", Never, Any, "an entry of the environment (Gate 2)", "returns a pointer into the environment"),
    rt!("nz.os_env_find", "ptr" ("ptr", "i64", "ptr") "", false, Files, "io", Never, Any, "a variable's value (Gate 2)", "returns a pointer into the environment"),
    rt!("nz.net_pipe_write", "i64" ("i64", "ptr", "i64") "", false, Net, "io", Status, Any, "a write to a pipe, waiting at most its deadline (Gate 2)", "borrows the bytes"),
    rt!("nz.os_spawn", "i64" ("ptr", "i64", "ptr") "", false, Net, "io", Status, Any, "run a program with three pipes (Gate 2)", "borrows the program and the arguments"),
    rt!("nz.os_child", "ptr" ("i64") "", false, Net, "", Null, Any, "a child's record, pinned (Gate 2)", "pins the handle"),
    rt!("nz.os_child_pipe", "i64" ("i64", "i64") "", false, Net, "io", Status, Any, "a child's pipe (Gate 2)", "none"),
    rt!("nz.os_wait", "i64" ("i64") "", false, Net, "io", Status, Any, "wait for a child's exit (Gate 2)", "none"),
    rt!("nz.os_kill", "i64" ("i64") "", false, Net, "io", Status, Any, "kill a child (Gate 2)", "none"),
    rt!("nz.os_spawn_in", "i64" ("ptr", "i64", "ptr", "ptr", "i64", "ptr", "i64") "", false, Net, "io", Status, Any, "run a program in a directory with an environment, by flags (R1d)", "borrows the program, the arguments, the directory and the environment"),
    rt!("nz.os_has_byte", "i1" ("ptr", "i64", "i8") "", false, Net, "", Never, Any, "whether bytes hold a given byte (R1d)", "borrows the bytes"),
    rt!("nz.os_beside", "ptr" ("ptr", "ptr", "i64", "ptr") "", false, Net, "", Null, Any, "a relative program made absolute beside a canonical directory (R1d)", "borrows both; the path is the caller's"),
    rt!("nz.os_env_order", "i64" ("ptr", "ptr") "", false, Net, "", Never, Any, "two environment entries by name (R1d)", "borrows both"),
    rt!("nz.os_envp", "ptr" ("ptr", "ptr") "", false, Net, "", Null, Any, "a child's environment, checked and ordered by name (R1d)", "borrows the strings; the array and its strings are the caller's"),
    rt!("nz.os_env_unshift", "void" ("ptr", "i64", "i64") "", false, Net, "", Never, Any, "an insertion's shift undone (R1d)", "writes the array"),
    rt!("nz.os_envp_free", "void" ("ptr") "", false, Net, "", Never, Any, "free a child's environment array and its strings (R1d)", "owns what it frees"),
    rt!("nz.os_start", "i64" ("ptr", "i64", "ptr", "ptr", "ptr", "i64") "", false, Net, "io", Status, Any, "start a program, its pipes and its record (Gate 2, R1d)", "borrows the program, the arguments, the directory and the environment"),
    rt!("nz.os_try_wait", "i64" ("i64") "", false, Net, "io", Status, Any, "a child's exit if it has ended (R1d)", "none"),
    rt!("nz.os_wait_until", "i64" ("i64", "i64") "", false, Net, "io", Status, Any, "a child's exit if it ends before a deadline (R1d)", "none"),
    rt!("nz.os_mono_ms", "i64" () "", false, Net, "", Never, Any, "the monotonic clock in milliseconds (R1d)", "none"),
    rt!("nz.os_wait_for", "i64" ("i64", "i64", "i64") "", false, Net, "io", Status, Any, "wait for a child: until it ends, not at all, or until a deadline (R1d)", "pins the handle"),
    rt!("nz.os_child_pid", "i64" ("i64") "", false, Net, "io", Status, Any, "a child's process id (R1d)", "none"),
    rt!("nz.os_child_rusage", "i64" ("i64") "", false, Net, "io", Status, Any, "a waited child's peak resident memory (R1d)", "none"),
    rt!("nz.os_pid", "i64" () "", false, Net, "io", Never, Any, "this process's id (R1d)", "none"),
    rt!("nz.os_kill_group", "i64" ("i64", "i64") "", false, Net, "io", Status, Any, "a signal to a process group (R1d)", "none"),
    rt!("nz.os_lstat", "i64" ("ptr", "i64", "ptr") "", false, Files, "io", Status, Any, "a path's own size, kind, time, permissions, device and inode (R1d)", "borrows the path; writes the caller's 48 bytes"),
    rt!("nz.os_chmod", "i64" ("ptr", "i64", "i64") "", false, Files, "io", Status, Any, "a path's permission bits (R1d)", "borrows the path"),
    rt!("nz.os_realpath", "i64" ("ptr", "i64", "ptr", "i64") "", false, Files, "io", Status, Any, "a path's canonical form (R1d)", "borrows the path; writes the caller's buffer"),
    rt!("nz.os_mkdtemp", "i64" ("ptr", "i64", "ptr", "i64") "", false, Files, "io", Status, Any, "a new private directory (R1d)", "borrows the parent; writes the caller's buffer"),
    rt!("nz.os_file_lock", "i64" ("i64", "i1", "i64") "", false, Files, "io", Status, Any, "an advisory whole-file lock, waited for or tried (R1d)", "none"),
    rt!("nz.os_file_unlock", "i64" ("i64") "", false, Files, "io", Status, Any, "let a file's lock go (R1d)", "none"),
    rt!("nz.os_flock_handle", "i64" ("i64", "i32", "i1") "", false, Files, "io", Status, Any, "a lock operation on a pinned file (R1d)", "pins the handle"),
    rt!("nz.format_float", "i64" ("ptr", "double", "i1") "", false, Numbers, "", Never, Any, "a float's shortest round-trip text, by ECMAScript's layout (Gate 2)", "writes into the caller's buffer"),
    rt!("nz.format_fixed", "i64" ("ptr", "double", "i64") "", false, Numbers, "", Never, Any, "a float with exactly so many decimals, C's `%.*f` (R1a, L4)", "writes into the caller's buffer"),
    rt!("nz.fmt_layout", "i64" ("ptr", "ptr", "i64", "i64") "", false, Numbers, "", Never, Any, "lay out a float's digits and exponent by ECMAScript's rule (Gate 2)", "writes into the caller's buffer"),
    rt!("nz.format_u64", "i64" ("ptr", "i64") "", false, Numbers, "", Never, Any, "a UInt64's decimal digits into a 21-byte buffer (Gate 2)", "writes into the caller's buffer"),
    rt!("nz.float_parses", "i1" ("ptr", "i64") "", false, Numbers, "", Never, Any, "whether bytes are a floating-point number as `float_parses` reads one (Gate 2)", "borrows the bytes"),
    rt!("nz.digits_from", "i64" ("ptr", "i64", "i64") "", false, Numbers, "", Never, Any, "the index after a run of decimal digits (Gate 2)", "borrows the bytes"),
    rt!("nz.float_parse", "double" ("ptr", "i64") "", false, Numbers, "", Null, Any, "the Float64 bytes write, or NaN (Gate 2)", "borrows the bytes"),
    rt!("nz.i128_divrem", "void" ("ptr", "ptr", "ptr", "i64") "", false, Wide, "", Never, Any, "a 128-bit quotient or remainder, the divisor already checked (R1b, L13b)", "reads two values, writes the caller's"),
    rt!("nz.i128_mul", "i32" ("ptr", "ptr", "ptr", "i64") "", false, Wide, "", Never, Any, "a 128-bit product, wrapped, and whether it overflowed (R1b, L13b)", "reads two values, writes the caller's"),
    rt!("nz.i128_to_f64", "double" ("ptr", "i64") "", false, Wide, "", Never, Any, "a 128-bit value as the nearest Float64 (R1b, L13b)", "reads the value"),
    rt!("nz.i128_to_f32", "float" ("ptr", "i64") "", false, Wide, "", Never, Any, "a 128-bit value as the nearest Float32 (R1b, L13b)", "reads the value"),
    rt!("nz.f64_to_i128", "void" ("ptr", "double", "i64") "", false, Wide, "", Never, Any, "a Float64 truncated to 128 bits, saturating (R1b, L13b)", "writes the caller's"),
    rt!("nz.format_i128", "i64" ("ptr", "ptr", "i64") "", false, Wide, "", Never, Any, "a 128-bit value's decimal digits into a 48-byte buffer (R1b, L13b)", "reads the value, writes into the caller's buffer"),
    rt!("nz.utf8_valid", "i1" ("ptr", "i64") "", false, Strings, "", Never, Any, "whether bytes are UTF-8: shortest forms, no surrogates, at most U+10FFFF (Gate 2)", "borrows the bytes"),
    rt!("nz.utf8_count", "i64" ("ptr", "i64") "", false, Strings, "", Never, Any, "the scalar values UTF-8 bytes encode (Gate 2)", "borrows the bytes"),
    rt!("nz.utf8_decode", "i64" ("ptr", "i64", "i64") "", false, Strings, "", Never, Any, "the scalar value UTF-8 bytes hold at a boundary (Gate 2)", "borrows the bytes"),
    rt!("nz.utf8_encode", "i64" ("ptr", "i64") "", false, Strings, "", Never, Any, "a scalar value's UTF-8, into a 4-byte buffer (Gate 2)", "writes into the caller's buffer"),
    rt!("nz.mem_zero", "void" ("ptr", "i64") "", false, Strings, "", Never, Any, "zero a buffer's bytes (Gate 2)", "writes into the caller's buffer"),
    rt!("nz.mem_move", "void" ("ptr", "ptr", "i64") "", false, Strings, "", Never, Any, "copy bytes that may overlap (Gate 2)", "writes into the destination, borrows the source"),
];

/// One global the runtime defines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeGlobal {
    pub symbol: &'static str,
    pub ty: &'static str,
    pub thread_local: bool,
    /// Who writes it, and how that is synchronised.
    pub writers: &'static str,
}

/// Every global the runtime defines, in the order every unit declares them (§7.45).
pub const GLOBALS: &[RuntimeGlobal] = &[
    RuntimeGlobal { symbol: "nz.pool_mode", ty: "i64", thread_local: false, writers: "`nz.pool_mode_get`, once (N83)" },
    RuntimeGlobal { symbol: "nz.pool_policy", ty: "i64", thread_local: false, writers: "the platform entry, once, before `main`, for a program that calls C (N106)" },
    RuntimeGlobal { symbol: "nz.stack_bytes", ty: "i64", thread_local: false, writers: "the platform entry, once, before `main`, for `nazm build --stack-size` (R1b, L14)" },
    RuntimeGlobal { symbol: "nz.pool_workers", ty: "ptr", thread_local: false, writers: "`nz.pool_start`, once (N83)" },
    RuntimeGlobal { symbol: "nz.pool_count", ty: "i64", thread_local: false, writers: "`nz.pool_start`, once (N83)" },
    RuntimeGlobal { symbol: "nz.pool_next", ty: "i64", thread_local: false, writers: "`nz.task_start`, atomically (N83)" },
    RuntimeGlobal { symbol: "nz.pool_stack", ty: "i64", thread_local: false, writers: "`nz.pool_start`, once (N83)" },
    RuntimeGlobal { symbol: "nz.pool_page", ty: "i64", thread_local: false, writers: "`nz.pool_start`, once (N83)" },
    RuntimeGlobal { symbol: "nz.pool_live", ty: "i64", thread_local: false, writers: "`nz.task_start` and `nz_task_main`, atomically (N83)" },
    RuntimeGlobal { symbol: "nz.pool_peak", ty: "i64", thread_local: false, writers: "`nz.task_start`, atomically (N83)" },
    RuntimeGlobal { symbol: "nz.pool_wait", ty: "ptr", thread_local: false, writers: "`nz.pool_start`, once (N83)" },
    RuntimeGlobal { symbol: "nz.pool_buckets", ty: "ptr", thread_local: false, writers: "`nz.pool_start`, once (N83)" },
    RuntimeGlobal { symbol: "nz.pool_join", ty: "ptr", thread_local: false, writers: "`nz.pool_start`, once (N83)" },
    RuntimeGlobal { symbol: "nz.cur_task", ty: "ptr", thread_local: true, writers: "`nz.task_enter` and `nz.task_leave`, on its own worker only (N83)" },
    RuntimeGlobal { symbol: "nz.errno_saved", ty: "i64", thread_local: true, writers: "`nz.errno_save`, on its own thread only (N85)" },
    RuntimeGlobal { symbol: "nz.os_lock", ty: "i32", thread_local: false, writers: "`nz.os_acquire` and `nz.os_release`, atomically (Gate 2)" },
    RuntimeGlobal { symbol: "nz.os_slots", ty: "ptr", thread_local: false, writers: "`nz.os_slot`, under the handle table's lock (Gate 2)" },
    RuntimeGlobal { symbol: "nz.os_cap", ty: "i64", thread_local: false, writers: "`nz.os_slot`, under the handle table's lock (Gate 2)" },
    RuntimeGlobal { symbol: "nz.os_stdin_h", ty: "i64", thread_local: false, writers: "`nz.os_stdin`, once, under the handle table's lock (Gate 2)" },
    RuntimeGlobal { symbol: "nz.os_in_buf", ty: "ptr", thread_local: false, writers: "`nz.os_in_line`, once, under standard input's lock (R1d, P3)" },
    RuntimeGlobal { symbol: "nz.os_in_at", ty: "i64", thread_local: false, writers: "`nz.os_in_read` and `nz.os_in_line`, under standard input's lock (R1d, P3)" },
    RuntimeGlobal { symbol: "nz.os_in_end", ty: "i64", thread_local: false, writers: "`nz.os_in_line`, under standard input's lock (R1d, P3)" },
    RuntimeGlobal { symbol: "nz.os_in_lock", ty: "i32", thread_local: false, writers: "`nz.os_in_acquire` and `nz.os_in_release`, atomically (R1d, P3)" },
    RuntimeGlobal { symbol: "nz.os_stdout_h", ty: "i64", thread_local: false, writers: "`nz.os_std_stream`, once, under the handle table's lock (R1d)" },
    RuntimeGlobal { symbol: "nz.os_stderr_h", ty: "i64", thread_local: false, writers: "`nz.os_std_stream`, once, under the handle table's lock (R1d)" },
    RuntimeGlobal { symbol: "nz.net_state", ty: "i64", thread_local: false, writers: "`nz.net_start`, atomically, once (Gate 2)" },
    RuntimeGlobal { symbol: "nz.net_poller", ty: "i64", thread_local: false, writers: "`nz.net_start`, once, before the reactor runs (Gate 2)" },
    RuntimeGlobal { symbol: "nz.net_thread", ty: "i64", thread_local: false, writers: "`pthread_create`, once (Gate 2)" },
    RuntimeGlobal { symbol: "nz.tls_slots", ty: "ptr", thread_local: false, writers: "`nz.tls_table`, atomically, once; the table's slots `nz.tls_put`, `nz.tls_hold`, `nz.tls_let_go` and `nz.os_tls_close`, each by its kind and hold words (C1)" },
    RuntimeGlobal { symbol: "nz.tls_hint", ty: "i64", thread_local: false, writers: "`nz.tls_put`, atomically (C1)" },
    RuntimeGlobal { symbol: "nz.wd_reload", ty: "i64", thread_local: false, writers: "a board's scheduler, once, as it starts (Gate 3)" },
    RuntimeGlobal { symbol: "nz.wd_left", ty: "i64", thread_local: false, writers: "`nz.watchdog_feed`, and a board's timer tick, atomically (Gate 3)" },
    RuntimeGlobal {
        symbol: "nz.failed",
        ty: "i1",
        thread_local: true,
        writers: "`nz.fail`, on its own thread only",
    },
    RuntimeGlobal {
        symbol: "nz.errmsg",
        ty: "ptr",
        thread_local: true,
        writers: "`nz.fail`, on its own thread only",
    },
    RuntimeGlobal {
        symbol: "nz.errlen",
        ty: "i64",
        thread_local: true,
        writers: "`nz.fail`, on its own thread only",
    },
    RuntimeGlobal {
        symbol: "nz.stack_limit",
        ty: "i64",
        thread_local: true,
        writers: "`nz.stack_check`, on its own thread only, at the thread's first check (N49)",
    },
    RuntimeGlobal {
        symbol: "nz.argc",
        ty: "i64",
        thread_local: false,
        writers: "the entry, or the platform's `nz_os_args` it calls (Gate 3), once, before any task exists",
    },
    RuntimeGlobal {
        symbol: "nz.argv",
        ty: "ptr",
        thread_local: false,
        writers: "the entry, or the platform's `nz_os_args` it calls (Gate 3), once, before any task exists",
    },
    RuntimeGlobal {
        symbol: "nz.seq_allocated",
        ty: "i64",
        thread_local: false,
        writers: "atomic increments; read by the entry after every join",
    },
    RuntimeGlobal {
        symbol: "nz.seq_reclaimed",
        ty: "i64",
        thread_local: false,
        writers: "atomic increments; read by the entry after every join",
    },
    RuntimeGlobal {
        symbol: "nz.str_allocated",
        ty: "i64",
        thread_local: false,
        writers: "atomic increments; read by the entry after every join",
    },
    RuntimeGlobal {
        symbol: "nz.str_reclaimed",
        ty: "i64",
        thread_local: false,
        writers: "atomic increments; read by the entry after every join",
    },
    RuntimeGlobal {
        symbol: "nz.chan_allocated",
        ty: "i64",
        thread_local: false,
        writers: "atomic increments; read by the entry after every join",
    },
    RuntimeGlobal {
        symbol: "nz.chan_reclaimed",
        ty: "i64",
        thread_local: false,
        writers: "atomic increments; read by the entry after every join",
    },
    RuntimeGlobal {
        symbol: "nz.clo_allocated",
        ty: "i64",
        thread_local: false,
        writers: "atomic increments; read by the entry after every join",
    },
    RuntimeGlobal {
        symbol: "nz.clo_reclaimed",
        ty: "i64",
        thread_local: false,
        writers: "atomic increments; read by the entry after every join",
    },
    RuntimeGlobal {
        symbol: "nz.sched_spawned",
        ty: "i64",
        thread_local: false,
        writers: "an atomic increment after each task starts; read by the entry after every join",
    },
    RuntimeGlobal {
        symbol: "nz.sched_joined",
        ty: "i64",
        thread_local: false,
        writers: "an atomic add before each scope joins; read by the entry after every join",
    },
    RuntimeGlobal {
        symbol: "nz.sched_selects",
        ty: "i64",
        thread_local: false,
        writers: "an atomic increment per select; read by the entry after every join",
    },
    RuntimeGlobal {
        symbol: "nz.sched_timeouts",
        ty: "i64",
        thread_local: false,
        writers: "an atomic increment per select that timed out; read by the entry after every join",
    },
    RuntimeGlobal {
        symbol: "nz.sel_gen",
        ty: "i64",
        thread_local: false,
        writers: "the select event count: a seq_cst increment per typed send or close (N54)",
    },
    RuntimeGlobal {
        symbol: "nz.sel_waiters",
        ty: "i64",
        thread_local: false,
        writers: "how many selects are waiting: seq_cst increments and decrements",
    },
    RuntimeGlobal {
        symbol: "nz.sel_sync",
        ty: "ptr",
        thread_local: false,
        writers: "the select lock and condition variable, made once by compare-and-swap on first use and never freed",
    },
];

/// The inventoried entry with this symbol.
#[must_use]
pub fn defined(symbol: &str) -> Option<&'static RuntimeSymbol> {
    INVENTORY.iter().find(|i| i.symbol == symbol)
}

impl RuntimeSymbol {
    /// `ret @symbol(params) attrs`: how every unit declares it.
    #[must_use]
    pub fn declaration(&self) -> String {
        let attrs = if self.attrs.is_empty() {
            String::new()
        } else {
            format!(" {}", self.attrs)
        };
        format!(
            "declare {} @{}({}){attrs}",
            self.ret,
            self.symbol,
            self.params.join(", ")
        )
    }
}

/// The thread-local globals that are plain ones in a program of one thread (`Needs::single`):
/// its failure state, its stack limit and, where failures unwind, the exception object each unit
/// defines (`nz.exc`, 4.2). The rest stay thread-local; a program of one thread
/// never reaches them.
pub const SINGLE_THREAD_STATE: &[&str] = &[
    "nz.failed",
    "nz.errmsg",
    "nz.errlen",
    "nz.stack_limit",
    "nz.exc",
];

impl RuntimeGlobal {
    /// How every unit that does not define it declares it.
    #[must_use]
    pub fn declaration(&self) -> String {
        format!(
            "@{} = external {}global {}",
            self.symbol,
            if self.thread_local {
                "thread_local "
            } else {
                ""
            },
            self.ty
        )
    }
}

/// The entry with this symbol.
#[must_use]
pub fn import(symbol: &str) -> Option<&'static Import> {
    IMPORTS.iter().find(|i| i.symbol == symbol)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn every_symbol_is_registered_once() {
        for (i, a) in IMPORTS.iter().enumerate() {
            for b in IMPORTS.iter().skip(i + 1) {
                assert_ne!(a.symbol, b.symbol);
            }
        }
    }

    fn all() -> crate::Needs {
        crate::Needs {
            tasks: true,
            channels: true,
            sequences: true,
            io: true,
            join: true,
            strings: true,
            shims: true,
            stack: true,
            closures: true,
            foreign: true,
            numbers: true,
            files: true,
            net: true,
            wide: true,
            tls: true,
            single: false,
        }
    }

    /// `ret @name(types) attrs` of every `define` in the runtime text, spaces removed.
    fn definitions(text: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for line in text.lines() {
            let Some(rest) = line.strip_prefix("define hidden ") else {
                continue;
            };
            let at = rest.find('@').unwrap();
            let open = rest.find('(').unwrap();
            let close = rest.rfind(')').unwrap();
            let ret = rest[..at].trim();
            let name = &rest[at + 1..open];
            let params: Vec<&str> = rest[open + 1..close]
                .split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(|p| p.split(" %").next().unwrap())
                .collect();
            let attrs = rest[close + 1..].trim().trim_end_matches('{').trim();
            out.push((
                name.to_owned(),
                format!("{ret}({}){attrs}", params.join(",")).replace(' ', ""),
            ));
        }
        out
    }

    fn inventoried(f: &RuntimeSymbol) -> String {
        format!("{}({}){}", f.ret, f.params.join(","), f.attrs).replace(' ', "")
    }

    #[test]
    fn every_runtime_definition_is_inventoried_with_its_exact_signature() {
        // The counted heap's unit too (R1d, D5), which only a build with a limit links.
        let text = crate::emit_runtime(all())
            + &crate::heap::emit_heap(1 << 20)
            + &crate::heap::emit_small(false);
        let defs = definitions(&text);
        for (name, sig) in &defs {
            let f =
                defined(name).unwrap_or_else(|| panic!("`{name}` is defined and not inventoried"));
            assert_eq!(&inventoried(f), sig, "`{name}`'s inventoried signature");
        }
        for f in INVENTORY {
            assert!(
                defs.iter().any(|(n, _)| n == f.symbol),
                "`{}` is inventoried and not defined",
                f.symbol
            );
        }
        for (i, a) in INVENTORY.iter().enumerate() {
            assert!(INVENTORY.iter().skip(i + 1).all(|b| b.symbol != a.symbol));
        }
    }

    /// The body of `name`'s definition in `text`, from `define` to its closing brace.
    fn body<'a>(text: &'a str, name: &str) -> &'a str {
        let at = text.find(&format!(" @{name}(")).unwrap();
        let start = text[..at].rfind("define ").unwrap();
        let end = at + text[at..].find("\n}\n").unwrap() + 3;
        &text[start..end]
    }

    #[test]
    fn a_program_of_one_thread_counts_without_atomics_and_any_other_with_them() {
        let many = crate::emit_runtime(all());
        let one = crate::emit_runtime(Needs {
            single: true,
            ..all()
        });
        for name in crate::PLAIN_IN_ONE_THREAD {
            let (a, p) = (body(&many, name), body(&one, name));
            // Atomic everywhere a task may hold the value, and the free ordered after it.
            assert!(a.contains(" = atomicrmw "), "{name}: {a}");
            // Plain in a program of one thread: no count is atomic (the report's counters, globals
            // the program's tasks would share, are not reference counts and stay as they are).
            assert!(
                !p.contains("atomicrmw add ptr %") && !p.contains("atomicrmw sub ptr %"),
                "{name}: {p}"
            );
            assert!(!p.contains("fence "), "{name}: {p}");
            assert!(p.contains(".plain = "), "{name}: {p}");
        }
        // The old count is still what the test for the last reference reads.
        let rel = body(&one, "nz.str_release");
        assert!(
            rel.contains("%old = load i64, ptr %b\n  %old.plain = sub i64 %old, 1\n  store i64 %old.plain, ptr %b\n  %last = icmp eq i64 %old, 1"),
            "{rel}"
        );
        let ret = body(&one, "nz.str_retain");
        assert!(ret.contains("%old.plain = add i64 %old, 1"), "{ret}");
        // Nothing else changes: every other line is the same, in the same order, but for the
        // failure state's globals, plain there already.
        let strip = |t: &str| -> Vec<String> {
            let mut out = Vec::new();
            let mut skip = false;
            for l in t.lines() {
                if l.starts_with("define ") {
                    skip = crate::PLAIN_IN_ONE_THREAD
                        .iter()
                        .any(|n| l.contains(&format!(" @{n}(")));
                }
                if !skip {
                    out.push(l.replacen(" thread_local ", " ", 1));
                }
                if l == "}" {
                    skip = false;
                }
            }
            out
        };
        assert_eq!(strip(&many), strip(&one));
        // The copies a unit inlines follow the same rule.
        let unit = "declare void @nz.str_retain(ptr)\ndeclare void @nz.str_release(ptr)\n";
        let copied = crate::inline_reference_counting(unit, false);
        let plain = crate::inline_reference_counting(unit, true);
        assert!(copied.contains("atomicrmw add ptr %b"), "{copied}");
        assert!(copied.contains("atomicrmw sub ptr %b"), "{copied}");
        assert!(!plain.contains("atomicrmw add ptr %b"), "{plain}");
        assert!(!plain.contains("atomicrmw sub ptr %b"), "{plain}");
        assert!(!plain.contains("fence "), "{plain}");
        assert!(plain.contains("define available_externally hidden void @nz.str_release("));
    }

    #[test]
    fn every_runtime_global_is_inventoried() {
        let text = crate::emit_runtime(all());
        let mut found = Vec::new();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("@nz.") {
                let (name, def) = rest.split_once(" = ").unwrap();
                let tl = def.contains("thread_local");
                let ty = def.split_whitespace().nth(if tl { 3 } else { 2 }).unwrap();
                found.push((format!("nz.{name}"), tl, ty.to_owned()));
            }
        }
        assert_eq!(found.len(), GLOBALS.len(), "{found:?}");
        for g in GLOBALS {
            assert!(
                found.contains(&(g.symbol.to_owned(), g.thread_local, g.ty.to_owned())),
                "`{}` is not defined as inventoried: {found:?}",
                g.symbol
            );
        }
    }

    #[test]
    fn the_word_view_agrees_with_the_inventory() {
        let word = |w: Word| match w {
            Word::I64 => "i64",
            Word::I32 => "i32",
            Word::Ptr => "ptr",
            Word::Bit => "i1",
            Word::F32 => "float",
            Word::F64 => "double",
        };
        for i in IMPORTS.iter().filter(|i| i.part != Part::Libc) {
            let f = defined(i.symbol)
                .unwrap_or_else(|| panic!("`{}` is imported and not inventoried", i.symbol));
            assert_eq!(f.part, i.part, "`{}`'s part", i.symbol);
            let params: Vec<&str> = i.params.iter().map(|w| word(*w)).collect();
            assert_eq!(f.params, params.as_slice(), "`{}`'s parameters", i.symbol);
            assert_eq!(f.ret, i.ret.map_or("void", word), "`{}`'s result", i.symbol);
        }
    }

    #[test]
    fn a_runtime_calls_the_operating_system_only_through_the_adapter() {
        let mut adapter = String::new();
        os::declarations(&mut adapter);
        let declared: Vec<String> = adapter
            .lines()
            .filter_map(|l| {
                let at = l.find('@')?;
                let open = l[at..].find('(')? + at;
                Some(l[at + 1..open].to_owned())
            })
            .collect();
        let text = crate::emit_runtime(all());
        let entry = crate::emit_entry("@nz_main", 1, all());
        let mut called = Vec::new();
        // Calls into the platform's own units (`nz_os_*`, Gate 3): the adapter may declare them,
        // and they count as called; their definitions are each target's.
        let mut platform_called = Vec::new();
        for t in [&text, &entry] {
            for (i, _) in t.match_indices("call ") {
                let rest = &t[i..];
                let Some(at) = rest.find('@') else { continue };
                let line_end = rest.find('\n').unwrap_or(rest.len());
                if at > line_end {
                    continue;
                }
                let name: String = rest[at + 1..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
                    .collect();
                if name.starts_with("nz_os_") {
                    platform_called.push(name);
                } else if !name.starts_with("nz.") && !name.starts_with("nz_") {
                    called.push(name);
                }
            }
        }
        for c in &called {
            assert!(
                declared.contains(c),
                "the runtime calls `{c}`, which the operating-system adapter does not declare"
            );
        }
        // And the adapter declares nothing the runtime, the entry or generated code never calls.
        // Generated code's calls are LIR's (N105), named in its lowering, one crate over; read as
        // text, it is still the same files.
        let lowering = [
            include_str!("../../nazm-lir/src/op/func.rs"),
            include_str!("../../nazm-lir/src/op/prims.rs"),
            include_str!("../../nazm-lir/src/op/helpers.rs"),
        ]
        .concat();
        // LLVM's intrinsics are declared there too, and are not the operating system.
        for d in declared.iter().filter(|d| !d.starts_with("llvm.")) {
            assert!(
                called.contains(d)
                    || platform_called.contains(d)
                    || lowering.contains(&format!("\"{d}\"")),
                "the adapter declares `{d}`, which nothing calls"
            );
        }
    }

    #[test]
    fn the_runtime_is_revision_forty_and_reaches_the_system_only_through_its_platform_layer() {
        // 6.2's copied stacks: a bump is a decision, so a test says which one was made.
        assert_eq!(crate::RUNTIME_ABI_REVISION, 40);
        // The session table is the runtime's; the engine is each target's own unit, and a target
        // without one has none.
        assert!(
            crate::emit_runtime(crate::EVERYTHING).contains("define hidden i64 @nz.os_tls_read(")
        );
        for t in crate::switch::SwitchTarget::ALL {
            let bound = matches!(
                t,
                crate::switch::SwitchTarget::Aarch64Darwin
                    | crate::switch::SwitchTarget::X86_64Darwin
                    | crate::switch::SwitchTarget::Aarch64Linux
                    | crate::switch::SwitchTarget::X86_64Linux
            );
            assert_eq!(crate::tls::emit_tls(t).is_some(), bound, "{t:?}");
        }
        // The pool's default worker count is the processors online, from each target's stack unit.
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("call i64 @nz.os_cpus()"));
        for t in crate::switch::SwitchTarget::ALL {
            assert!(
                crate::platform::emit_stack(t).contains("define hidden i64 @nz.os_cpus()"),
                "{t:?}"
            );
        }
        // 128-bit integers (R1b, L13b), in the wide part only.
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.i128_divrem("));
        assert!(!crate::emit_runtime(crate::Needs::default()).contains("@nz.i128_divrem("));
        // Fixed-point floats (R1a, L4), in the numbers part.
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.format_fixed("));
        assert!(!crate::emit_runtime(crate::Needs::default()).contains("@nz.format_fixed("));
        // The release queue (R1a): a nested release queues on it rather than recursing.
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.vec_enqueue("));
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.os_random_bytes("));
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.os_spawn("));
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.time_sleep_ms("));
        // The network part (Gate 2), present only where a program reaches it; a program with files
        // and no sockets carries the three stubs the files part calls instead.
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.net_reactor("));
        let files_only = crate::Needs {
            files: true,
            ..crate::Needs::default()
        };
        assert!(!crate::emit_runtime(files_only).contains("@nz.net_reactor("));
        assert!(crate::emit_runtime(files_only).contains("@nz.net_wake("));
        // The files part (Gate 2), present only where a program reaches it.
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.os_read("));
        assert!(!crate::emit_runtime(crate::Needs::default()).contains("@nz.os_read("));
        // The numbers part (Gate 2), present only where a program reaches it.
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.format_float("));
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.float_parse("));
        assert!(!crate::emit_runtime(crate::Needs::default()).contains("@nz.format_float("));
        // The pool is the default, and a program's policy is the entry's (N106).
        assert!(crate::emit_runtime(crate::EVERYTHING)
            .contains("@nz.pool_policy = hidden global i64 2"));
        assert!(crate::emit_entry("@nz_main", 1, crate::EVERYTHING)
            .contains("store i64 1, ptr @nz.pool_policy"));
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.mmio_write8("));
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.errno_save("));
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.cstr_len("));
        // Each target's `errno` is its own C library's.
        assert!(
            crate::switch::emit_errno(crate::switch::SwitchTarget::Aarch64Darwin)
                .contains("@__error()")
        );
        assert!(
            crate::switch::emit_errno(crate::switch::SwitchTarget::X86_64Linux)
                .contains("@__errno_location()")
        );
        // Without C, no `errno` capture.
        assert!(!crate::emit_runtime(crate::Needs::default()).contains("errno"));
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.cwait("));
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.task_start("));
        // Every task switch is in the digest: another switch is another runtime.
        for t in crate::switch::SwitchTarget::ALL {
            assert!(crate::switch::emit_switch(t).contains("nz_ctx_switch"));
        }
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.soa_grow("));
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.ints_sum("));
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.cstr_borrow("));
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.chanv_select("));
        assert!(crate::emit_runtime(crate::EVERYTHING).contains("@nz.chanv_send("));
        let d = crate::digest();
        assert!(
            d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()),
            "{d}"
        );
        assert_eq!(d, crate::digest(), "a digest is a function of the text");
    }

    #[test]
    fn a_runtime_entry_the_registry_names_is_one_the_runtime_defines() {
        // The heap units are their own (R1d, R1.5 3.1), linked beside the runtime.
        let text = crate::emit_runtime(all()) + &crate::heap::emit_small(false);
        for i in IMPORTS {
            if i.part == Part::Libc {
                continue;
            }
            assert!(
                text.contains(&format!("@{}(", i.symbol)),
                "the runtime defines no `{}`",
                i.symbol
            );
        }
        for (d, _) in DATA {
            assert!(text.contains(&format!("@{d} = ")), "no `{d}`");
        }
    }

    #[test]
    fn a_task_thread_has_the_stack_the_check_budgets_for_on_every_host() {
        // Read from the text, so a host whose default happens to be 8 MiB (Linux) still sees
        // the size go missing (N49).
        let text = emit_runtime(all());
        // 8 MiB unless the program was built for another size (R1b, L14).
        assert!(
            text.contains("%sbtsize = select i1 %sbtset, i64 %sbt, i64 8388608")
                && text.contains("call i32 @pthread_attr_setstacksize(ptr %attr, i64 %sbtsize)"),
            "{text}"
        );
        assert!(
            text.contains("@pthread_create(ptr %blk, ptr %attr,"),
            "{text}"
        );
        // And the budget is the smaller of RLIMIT_STACK and that same 8 MiB.
        assert!(
            text.contains("%want = select i1 %sbcset, i64 %sbc, i64 8388608")
                && text.contains("icmp ult i64 %cap, %want"),
            "{text}"
        );
        // Nothing checks the stack in a program with no call cycle.
        let none = emit_runtime(crate::Needs {
            stack: false,
            ..all()
        });
        assert!(!none.contains("@nz.stack_check"), "{none}");
    }
}
