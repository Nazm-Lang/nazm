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

/// The registry's revision. It names the runtime's calling contract — this table, the
/// sequence header's layout (`len` at 0, `cap` at 8, `data` at 16, the count at 24) and a
/// string's (pointer, length, owner) — and enters every object's cache key, so an object built
/// against another revision is never reused.
pub const RUNTIME_ABI_REVISION: u32 = 1;

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

/// The entry with this symbol.
#[must_use]
pub fn import(symbol: &str) -> Option<&'static Import> {
    IMPORTS.iter().find(|i| i.symbol == symbol)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
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

    #[test]
    fn a_runtime_entry_the_registry_names_is_one_the_runtime_defines() {
        let all = crate::emit::Needs {
            tasks: true,
            channels: true,
            sequences: true,
            io: true,
            join: true,
            strings: true,
            shims: true,
        };
        let text = crate::emit_runtime(all);
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
}
