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
pub(crate) mod os;
pub mod switch;
pub mod wasm;

use std::fmt::Write as _;

pub use self::core::{
    emit_runtime, runtime_declarations, MEMORY_FORMAT, MEMORY_VARIABLE, SCHED_FORMAT,
    SCHED_VARIABLE,
};
pub use self::entry::emit_entry;
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
            sequences: self.sequences || self.tasks || self.channels,
            // `str_join` builds a string and the I/O helpers return one, so either of
            // them reaching means the string helpers are reached too. A `Strs` needs them
            // as well, and says so at the site that retains or releases an element rather
            // than by implication from the sequence flag — an `Ints`-only program carries
            // no string runtime at all.
            strings: self.strings || self.join || self.io,
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
/// runs, and the per-target `switch` artefact every build reaching tasks or channels links.
pub const RUNTIME_ABI_REVISION: u32 = 11;

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
        for t in switch::SwitchTarget::ALL {
            h.update(switch::emit_switch(t).as_bytes());
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
    import!("nz.closure_new", [I64, Ptr, Ptr], Some(Word::Ptr), Closures, "a closure object of this many bytes with its code and release, holding one reference the caller owns, or null"),
    import!("nz.closure_retain", [Ptr], None, Closures, "one more reference to a closure object; null is a no-op"),
    import!("nz.closure_release", [Ptr], None, Closures, "one reference fewer; the last runs its release and frees it; null is a no-op"),
    import!("nz.format_int", [Ptr, I64], Some(Word::I64), Base, "writes an Int's decimal digits into 21 bytes the caller owns and returns how many"),
    import!("nz.str_alloc", [I64], Some(Word::Ptr), Strings, "a string backing with this many usable bytes after a 16-byte header, holding one reference the caller owns, or null"),
    import!("nz.str_retain", [Ptr], None, Strings, "one more reference to a backing; null is a no-op"),
    import!("nz.str_release", [Ptr], None, Strings, "one reference fewer; the last frees it; null is a no-op"),
    import!("nz.seq_new", [], Some(Word::Ptr), Sequences, "an empty sequence with one reference the caller owns, or null"),
    import!("nz.seq_release", [Ptr], None, Sequences, "releases a sequence whose elements own nothing; null is a no-op"),
    import!("nz.strs_release", [Ptr], None, Sequences, "releases a sequence of strings, and on the last reference each string"),
    import!("nz.vec_release", [Ptr, Ptr, I64], None, Sequences, "releases a sequence; on the last reference calls the release function on each element at the stride given, then frees it"),
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
    import!("nz.mmio_read32", [I64], Some(Word::I64), Base, "reads a device register; takes nothing (N62)"),
    import!("nz.mmio_write32", [I64, I64], Some(Word::I64), Base, "writes a device register; takes nothing (N62)"),
    import!("nz.cstr_has_nul", [Ptr, I64], Some(Word::Bit), Strings, "borrows the bytes; whether they hold a NUL (N55)"),
    import!("nz.cstr_borrow", [Ptr, I64], Some(Word::Ptr), Strings, "borrows the bytes; a NUL-terminated copy the caller owns and frees, or null (N55)"),
    import!("nz.chanv_close", [Ptr], None, Channels, "borrows the channel; closes it and wakes every select (N54)"),
    import!("nz.chanv_select", [Ptr, Ptr, I64, I64], Some(Word::I64), Channels, "borrows the channels; copies one slot out to the second pointer, handing over its reference, when it returns an index; -3 - i closed, -1 none, -2 timed out, i64::MIN out of memory"),
    import!("nz.time_now_ms", [], Some(Word::I64), Channels, "takes nothing"),
    import!("nz.sched_note_spawn", [], None, Tasks, "counts a started task; takes nothing"),
    import!("nz.sched_note_join", [Ptr], None, Tasks, "borrows the task list; counts its tasks as joined"),
    import!("write", [I32, Ptr, I64], Some(Word::I64), Libc, "borrows the bytes"),
    import!("exit", [I32], None, Libc, "does not return"),
    import!("strlen", [Ptr], Some(Word::I64), Libc, "borrows the text"),
    import!("malloc", [I64], Some(Word::Ptr), Libc, "memory the caller owns, or null"),
    import!("free", [Ptr], None, Libc, "takes the memory"),
    import!("memcpy", [Ptr, Ptr, I64], Some(Word::Ptr), Libc, "borrows both"),
    import!("memcmp", [Ptr, Ptr, I64], Some(Word::I32), Libc, "borrows both"),
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
    rt!("nz.mmio_read32", "i64" ("i64") "", true, Base, "", Never, Owner, "a volatile 32-bit read of a device register (N62); reached only in a freestanding image", "none"),
    rt!("nz.mmio_write32", "i64" ("i64", "i64") "", true, Base, "", Never, Owner, "a volatile 32-bit write of a device register (N62); reached only in a freestanding image", "none"),
    rt!("nz.cstr_has_nul", "i1" ("ptr", "i64") "", true, Strings, "", Status, Any, "whether a Str argument to C holds a NUL byte (N55)", "borrows the bytes"),
    rt!("nz.cstr_borrow", "ptr" ("ptr", "i64") "", true, Strings, "", Null, Any, "a Str argument as a C string, for one foreign call (N55)", "borrows the bytes; the copy is the caller's to free"),
    rt!("nz.chanv_close", "void" ("ptr") "", true, Channels, "", Never, Locked, "close a generic channel and wake every select (N54)", "borrows the channel"),
    rt!("nz.chanv_select", "i64" ("ptr", "ptr", "i64", "i64") "", true, Channels, "", Status, Locked, "wait for the lowest-indexed ready channel, or a deadline (N54)", "borrows the channels; hands one slot's reference to the caller when it returns an index"),
    rt!("nz.time_now_ms", "i64" () "", true, Channels, "", Never, Any, "a monotonic clock in milliseconds (N54)", "none"),
    rt!("nz.sched_note_spawn", "void" () "", true, Tasks, "", Never, Any, "count a started task (N54)", "none"),
    rt!("nz.sched_note_join", "void" ("ptr") "", true, Tasks, "", Never, Any, "count a scope's tasks as joined (N54)", "borrows the task list"),
    rt!("nz.sel_sync_get", "ptr" () "", false, Channels, "", Null, Any, "the select lock and condition variable, made on first use", "the process owns it"),
    rt!("nz.sel_signal", "void" () "", false, Channels, "", Never, Any, "advance the select event count and wake any select", "none"),
    rt!("nz.chanv_try", "i64" ("ptr", "ptr") "", false, Channels, "", Never, Locked, "one look at a channel without waiting", "borrows the channel; hands the slot's reference over when it returns 1"),
    rt!("nz.failed_now", "i32" () "", false, Base, "", Never, Any, "whether this thread has failed, for a second backend", "none"),
    rt!("nz.pool_mode_get", "i64" () "", false, Tasks, "", Never, Any, "which scheduler this process runs, decided once from NAZM_SCHEDULER (N83)", "none"),
    rt!("nz.pool_env", "i64" ("ptr", "i64") "", false, Tasks, "", Never, Any, "a positive count from an environment variable, or a default (N83)", "borrows the name"),
    rt!("nz.pool_sync", "ptr" () "", false, Tasks, "", Null, Any, "a mutex and a condition variable in one allocation (N83)", "the process owns it"),
    rt!("nz.pool_start", "i1" () "", false, Tasks, "", Status, Any, "start the pool's tables and workers (N83)", "the process owns them"),
    rt!("nz.pool_push", "void" ("ptr", "ptr") "", false, Tasks, "", Never, Locked, "queue a runnable task on its worker and wake it (N83)", "the worker's queue holds the task"),
    rt!("nz.pool_now", "i64" () "", false, Tasks, "", Never, Any, "the wall clock in nanoseconds (N83)", "none"),
    rt!("nz.pool_timers", "i1" ("ptr", "ptr") "", false, Tasks, "", Never, Locked, "queue the worker's parked tasks whose deadline passed; the soonest left (N83)", "none"),
    rt!("nz.task_enter", "void" ("ptr") "", false, Tasks, "", Never, Owner, "make a task this thread's, with its own failure state and stack limit (N83)", "borrows the task"),
    rt!("nz.task_leave", "void" ("ptr") "", false, Tasks, "", Never, Owner, "save a task's failure state and stack limit and clear the thread's (N83)", "borrows the task"),
    rt!("nz.worker_main", "ptr" ("ptr") "", false, Tasks, "", Never, Owner, "a pool worker: run its tasks first in, first out (N83)", "the worker owns its tasks' stacks"),
    rt!("nz.task_start", "i32" ("ptr", "ptr") "", false, Tasks, "", Status, Any, "start a task on the pool with a guarded stack (N83)", "takes the argument block; 0, or 1 when no stack could be had"),
    rt!("nz_task_main", "void" ("ptr") "", false, Tasks, "", Never, Owner, "a task's body on its own stack, then its end (N83)", "borrows the task"),
    rt!("nz.task_suspend", "void" ("ptr") "", false, Tasks, "", Never, Owner, "switch from a task back to its worker (N83)", "none"),
    rt!("nz.task_wake", "void" ("ptr") "", false, Tasks, "", Never, Any, "make a parked task runnable, or note the wake (N83)", "none"),
    rt!("nz.pool_bucket", "ptr" ("ptr") "", false, Tasks, "", Never, Any, "the waiter list a condition variable hashes to (N83)", "none"),
    rt!("nz.pool_unwait", "void" ("ptr") "", false, Tasks, "", Never, Any, "take a task off the waiters it is still on (N83)", "none"),
    rt!("nz.cwait", "i32" ("ptr", "ptr") "", false, Tasks, "", Never, Locked, "wait on a condition variable: park a pool task, block a thread (N83)", "the mutex is held again on return"),
    rt!("nz.ctimedwait", "i32" ("ptr", "ptr", "ptr") "", false, Tasks, "", Never, Locked, "as nz.cwait, until a wall-clock time (N83)", "the mutex is held again on return"),
    rt!("nz.pool_park", "void" ("ptr", "ptr", "ptr", "i64") "", false, Tasks, "", Never, Locked, "register a task as a waiter, park it and give its worker back (N83)", "releases and retakes the mutex"),
    rt!("nz.cwake", "void" ("ptr") "", false, Tasks, "", Never, Any, "wake every thread and pool task waiting on a condition variable (N83)", "none"),
    rt!("nz.task_yield", "void" () "", false, Tasks, "", Never, Owner, "let a pool task's worker run its other tasks first (N83)", "none"),
    rt!("nz.pool_report", "void" () "", false, Tasks, "", Never, Entry, "the pool's line of the schedule report, where it ran (N83)", "none"),
    rt!("nz.join_one", "i32" ("ptr") "", false, Tasks, "", Status, Any, "wait for one task to end, its thread joined or its record freed (N83)", "frees the pool task's record"),
    rt!("nz.format_int", "i64" ("ptr", "i64") "", false, Base, "", Never, Any, "an Int's decimal digits into a 21-byte buffer", "writes into the caller's buffer"),
    rt!("nz.vec_release", "void" ("ptr", "ptr", "i64") "", false, Sequences, "", Never, Owner, "release a sequence, each element with its type's release at its stride", "takes a reference; null is a no-op"),
    rt!("nz.read_file_to", "void" ("ptr", "ptr", "i64") "", false, Io, "io", Status, Any, "`nz.read_file`, written through a pointer", "as `nz.read_file`"),
    rt!("nz.str_join_to", "void" ("ptr", "ptr", "ptr", "i64") "", false, Join, "", Null, Any, "`nz.str_join`, written through a pointer", "as `nz.str_join`"),
    rt!("nz.scope_join_to", "void" ("ptr", "ptr") "", false, Tasks, "", Never, Any, "`nz.scope_join`, written through a pointer", "as `nz.scope_join`"),
    rt!("nz.task_publish", "void" ("ptr") "", false, Tasks, "", Never, Any, "copy this thread's failure into its task block", "borrows the block"),
    rt!("nz.cstr", "ptr" ("ptr", "i64") "", false, Io, "", Null, Any, "a NUL-terminated copy of a path, for the C library", "the copy is owned by the caller, who frees it"),
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
        writers: "the entry, once, before any task exists",
    },
    RuntimeGlobal {
        symbol: "nz.argv",
        ty: "ptr",
        thread_local: false,
        writers: "the entry, once, before any task exists",
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
        let text = crate::emit_runtime(all());
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
                if !name.starts_with("nz.") && !name.starts_with("nz_") {
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
        // And the adapter declares nothing the runtime, the entry or the emitter never calls.
        // The emitter is the backend's, one crate over; read as text, it is still the same file.
        let emitter = include_str!("../../nazm-lir/src/emit.rs");
        // LLVM's intrinsics are declared there too, and are not the operating system.
        for d in declared.iter().filter(|d| !d.starts_with("llvm.")) {
            assert!(
                called.contains(d) || emitter.contains(&format!("@{d}(")),
                "the adapter declares `{d}`, which nothing calls"
            );
        }
    }

    #[test]
    fn the_runtime_is_revision_eleven_and_its_digest_covers_the_pool() {
        // N83's revision: a bump is a decision, so a test says which one was made.
        assert_eq!(crate::RUNTIME_ABI_REVISION, 11);
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
        let text = crate::emit_runtime(all());
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
        assert!(
            text.contains("call i32 @pthread_attr_setstacksize(ptr %attr, i64 8388608)"),
            "{text}"
        );
        assert!(
            text.contains("@pthread_create(ptr %blk, ptr %attr,"),
            "{text}"
        );
        // And the budget is the smaller of RLIMIT_STACK and that same 8 MiB.
        assert!(text.contains("icmp ult i64 %cap, 8388608"), "{text}");
        // Nothing checks the stack in a program with no call cycle.
        let none = emit_runtime(crate::Needs {
            stack: false,
            ..all()
        });
        assert!(!none.contains("@nz.stack_check"), "{none}");
    }
}
