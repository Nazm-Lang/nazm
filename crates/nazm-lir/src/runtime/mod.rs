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

pub(crate) mod core;
pub(crate) mod entry;
pub(crate) mod os;

pub use self::core::emit_runtime;
pub use self::entry::emit_entry;

/// The runtime ABI's revision (§7.45). It names the runtime's contract — [`INVENTORY`]'s
/// signatures, [`GLOBALS`], this word-level table, the sequence header's layout (`len` at 0,
/// `cap` at 8, `data` at 16, the count at 24), a string's (pointer, length, owner) and a string
/// backing's count at 0 — and enters every object's cache key under both backends, so an object
/// built against another revision is never reused.
///
/// 1 was N41's: the word-level table alone. 2 is N43's, the first revision whose contract is
/// written down whole.
pub const RUNTIME_ABI_REVISION: u32 = 2;

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
    rt!("nz.str_join", "{ptr, i64, ptr}" ("ptr", "ptr", "i64") "", true, Join, "", Null, Any, "join a sequence of strings with a separator, allocating once", "borrows both; the result is owned by the caller"),
    rt!("nz.file_exists", "i1" ("ptr", "i64") "", true, Io, "io", Status, Any, "whether a path names a readable file", "borrows the path"),
    rt!("nz.read_file", "{ptr, i64, ptr}" ("ptr", "i64") "", true, Io, "io", Status, Any, "a file's contents as a string", "borrows the path; the result is owned by the caller, or null when unreadable"),
    rt!("nz.write_file", "i64" ("ptr", "i64", "ptr", "i64") "", true, Io, "io", Status, Any, "write a string to a file, truncating", "borrows both; returns the byte count or a negative status"),
    rt!("nz.arr_new", "ptr" () "", true, Sequences, "", Null, Any, "an empty internal list, not counted as a program sequence", "one reference, owned by the caller"),
    rt!("nz.arr_grow", "ptr" ("ptr", "i64") "", true, Sequences, "", Null, Owner, "room for one more element of this size", "borrows the sequence; returns its data pointer"),
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
    rt!("nz.failed_now", "i32" () "", false, Base, "", Never, Any, "whether this thread has failed, for a second backend", "none"),
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
        let entry = crate::emit_entry(&crate::symbol::NativeSymbol::foreign("nz_main"), 1, all());
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
        let emitter = include_str!("../emit.rs");
        // LLVM's intrinsics are declared there too, and are not the operating system.
        for d in declared.iter().filter(|d| !d.starts_with("llvm.")) {
            assert!(
                called.contains(d) || emitter.contains(&format!("@{d}(")),
                "the adapter declares `{d}`, which nothing calls"
            );
        }
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
}
