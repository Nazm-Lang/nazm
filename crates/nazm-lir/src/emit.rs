//! MIR to LLVM IR text.
//!
//! # What this reads
//!
//! Validated MIR (`docs/architecture.md` §7.42) and the layouts, symbols and linkage
//! [`crate::lower`] settled from it. Every language decision — which loop a `break` leaves, what
//! a `?` does, which values die on an edge, which scopes are joined before a `return`, where a
//! failure goes — is already a MIR statement or terminator. This file decides only how each
//! becomes instructions: a block becomes a label, a local a stack slot, a drop a call to the
//! runtime's release for that type, a failure edge a branch.
//!
//! # The contract this file exists to keep
//!
//! `docs/spec.md` says overflow **traps, identically in every build**. The obvious way to
//! write signed arithmetic in LLVM is `add nsw`, and it is exactly wrong: `nsw` does not
//! check for overflow, it *promises overflow cannot happen* and makes the result poison
//! if it does. An optimiser is then entitled to assume the promise and delete the branch
//! that was meant to catch it — so `nsw` would turn a specified trap into whatever
//! `-O2` felt like. Every fallible operation here goes through
//! `llvm.{sadd,ssub,smul}.with.overflow.i64` and an explicit branch, which is a value the
//! optimiser has to respect.
//!
//! `llvm.trap` is not used either. It raises `SIGILL`, and a process killed by a signal
//! is not the runtime-error contract Nazm promises: the spec says the program *stops with
//! a diagnostic*. So a failing operation records the diagnostic and leaves along its MIR
//! failure edge; the platform entry writes it and exits 2 — the same status `nazm run` uses
//! for a runtime failure.
//!
//! The four cases that are easy to get wrong, and what is emitted for each:
//!
//! | Case | Emitted |
//! | --- | --- |
//! | `a / 0`, `a % 0` | divisor compared to zero *before* any division instruction |
//! | `Int::MIN / -1` | both operands compared, and the failure reported as an overflow — the quotient has no representation |
//! | `Int::MIN % -1` | **`0`**, via a `phi`. Never reaches `srem`, which faults in hardware on exactly this pair |
//! | `-Int::MIN` | `ssub.with.overflow(0, x)`, the only negation without a representable result |
//!
//! `sdiv` and `srem` are reached only on operands already proved safe, so the guards are
//! not belt-and-braces — they are what stops the hardware fault that `docs/spec.md`
//! describes.
//!
//! # Locals are slots
//!
//! Every MIR local has a stack slot, allocated in the entry block and zeroed there if it can
//! own anything, so a drop of a local that holds nothing releases the representation's zero,
//! which every release in the runtime ignores. A move reads the slot and zeroes it. No `phi`
//! joins values across MIR blocks: MIR has none, and a value that crosses a block is in a slot.

use std::fmt::Write as _;

use nazm_mir as mir;
use nazm_span::Span;

use crate::names::SourceNames;

use crate::ir::{
    ArithOp, Builtin, EnumLayout, EnumTy, Lowering as _, Owns, RecordLayout, RecordTy, Ty, Unit,
    VariantLayout, VecLayout, VecTy,
};
use crate::lower::{Native, Sig};
use crate::symbol::NativeSymbol;

/// What this emitter has been verified against, recorded so a reader does not have to
/// infer portability from silence.
pub const TARGET_NOTE: &str = "verified on aarch64-apple-darwin with Apple clang; other \
                               targets are unverified rather than unsupported";

/// The environment variable that asks a compiled program to report what it reclaimed.
///
/// A diagnostic and not a language feature: it is read by `getenv` in the entry wrapper,
/// written to standard error, and invisible to every Nazm program. `docs/spec.md` states
/// *when* storage is reclaimed and deliberately not *how*, so a counter a program could
/// read would put the how into the language.
pub const MEMORY_VARIABLE: &str = "NAZM_MEMORY_REPORT";

/// Bytes between a string backing's address and its first byte of text.
///
/// `{ i64 rc, i64 length }`, so sixteen — which also keeps the text itself sixteen-byte
/// aligned, the alignment `memcpy` wants without being asked.
const STRING_HEADER: u32 = 16;

/// What that report says. One line, three numbers, read back by the tests.
pub const MEMORY_FORMAT: &str = "nazm-memory: sequences allocated=%lld reclaimed=%lld \
                                 live=%lld strings allocated=%lld reclaimed=%lld live=%lld \
                                 channels allocated=%lld reclaimed=%lld live=%lld";

/// One variant of an enum that has something to retain or release.
#[derive(Debug, Clone)]
struct OwningVariant {
    /// Its discriminant, which is also its payload's index in the aggregate minus one.
    tag: u32,
    /// The LLVM name of its payload struct.
    payload: String,
    /// Its source name, for a comment.
    source: String,
    /// The payload fields that own something: their index in the payload struct, and
    /// their type.
    owning: Vec<(u32, Ty)>,
}

/// The type definitions one unit carries, as a pair of borrowed tables.
///
/// `Copy`, so that a caller already writing into `self.out` can name a type in the same
/// expression: two fields of one struct are disjoint borrows, and `self.ty(…)` would not
/// have been. It is one struct rather than two parameters because the two tables are
/// always needed together — a record field may be an enum and a payload may be a record.
#[derive(Debug, Clone, Copy)]
struct Layouts<'a> {
    records: &'a [RecordLayout],
    enums: &'a [EnumLayout],
    /// The `Vec` types this unit's text mentions. N11.
    vecs: &'a [VecLayout],
}

/// How a type is written in LLVM text.
fn llvm_type(types: Layouts<'_>, t: Ty) -> String {
    match t {
        Ty::Record(id) => record_layout(types.records, id)
            .map_or_else(|| t.builtin_llvm().to_owned(), |l| l.name.to_string()),
        Ty::Enum(id) => enum_layout(types.enums, id)
            .map_or_else(|| t.builtin_llvm().to_owned(), |l| l.name.to_string()),
        other => other.builtin_llvm().to_owned(),
    }
}

/// The layout of a record this unit carries.
fn record_layout(records: &[RecordLayout], id: RecordTy) -> Option<&RecordLayout> {
    records.iter().find(|l| l.id == id)
}

/// The layout of an enum this unit carries.
fn enum_layout(enums: &[EnumLayout], id: EnumTy) -> Option<&EnumLayout> {
    enums.iter().find(|l| l.id == id)
}

/// The element type of a `Vec` this unit carries.
fn vec_layout(vecs: &[VecLayout], id: VecTy) -> Option<&VecLayout> {
    vecs.iter().find(|l| l.id == id)
}

/// Whether anything this type contains, at any depth, owns something to release.
///
/// A record's answer is over every field; an enum's is over every payload field of
/// **every** variant, because any of them may be the one a value holds. That asymmetry
/// with copy and release — which act on the active variant alone — is not an inconsistency:
/// *does this need cleanup at all* is a question about the type, and *what does this value
/// own* is a question about the value.
///
/// Iterative, with an explicit worklist: the containment graph is acyclic — the checker
/// refuses a cycle before lowering (`N0336`) — but its depth is a property of the program,
/// and a recursive walk would put that depth on the compiler's stack.
fn owns_anything(types: Layouts<'_>, t: Ty) -> bool {
    let mut seen: Vec<Ty> = Vec::new();
    let mut work = vec![t];
    while let Some(next) = work.pop() {
        if seen.contains(&next) {
            continue;
        }
        seen.push(next);
        let inner: Vec<Ty> = match next.owns() {
            None => continue,
            Some(Owns::Record(id)) => record_layout(types.records, id)
                .map(|l| l.fields.clone())
                .unwrap_or_default(),
            Some(Owns::Enum(id)) => enum_layout(types.enums, id)
                .map(|l| l.variants.iter().flat_map(|v| v.fields.clone()).collect())
                .unwrap_or_default(),
            Some(_) => return true,
        };
        work.extend(inner);
    }
    false
}

/// How many fixed fields a task's argument block has before its arguments.
///
/// The handle, the diagnostic's message pointer, and its length. Fixed, because the
/// runtime reads all three by offset.
const TASK_HEADER_FIELDS: usize = 3;

/// The diagnostic a failing operation reports, matching the interpreter's wording.
struct Failure {
    code: &'static str,
    message: String,
}

struct Emitter<'a> {
    out: String,
    /// Next SSA value number. LLVM requires unnamed values to be numbered consecutively
    /// within a function.
    next: u32,
    next_label: u32,
    /// The block instructions are currently being appended to — the predecessor a `phi`
    /// inside one statement's expansion must name.
    current: String,
    /// Error strings, interned so a program with the same failure twice carries it once.
    strings: Vec<(String, String)>,
    /// `Str` literals, interned the same way and for the same reason. Kept apart from
    /// the error strings because these are the program's data: they carry arbitrary
    /// bytes and are never NUL-terminated.
    blobs: Vec<(String, Vec<u8>)>,
    /// Where every span in this module came from.
    names: &'a SourceNames<'a>,
    /// How each record, enum and `Vec` this unit mentions is laid out.
    types: Layouts<'a>,
    /// The program whose functions are being emitted, and each function's signature.
    program: &'a mir::Program,
    sigs: &'a [Sig],
    /// The type of every slot in the function being emitted, by local: `None` for a task
    /// list, whose slot is a pointer the runtime owns.
    slot_types: Vec<Option<Ty>>,
    /// The label a failure in the statement being emitted leaves for: its MIR failure edge.
    unwind_to: String,
    /// The per-record retain and release helpers, generated on demand.
    ///
    /// One function per record type per direction, rather than the field walk inlined at
    /// every site that copies or drops one — and because a nested record's helper calls the
    /// inner record's, the recursion lives in the *emitted* code rather than in this
    /// compiler's stack.
    helpers: String,
    /// Which helpers have been generated: `(record, true)` for retain.
    generated: std::collections::BTreeSet<(RecordTy, bool)>,
    /// The same, for enums.
    generated_enums: std::collections::BTreeSet<(EnumTy, bool)>,
    /// The per-`Vec` release helpers generated, by element type. N11.
    generated_vecs: std::collections::BTreeSet<VecTy>,
    /// The equality helpers generated, by record or enum type. N13.
    generated_eq: std::collections::BTreeSet<Ty>,
    /// The argument-block types generated for `spawn` sites, by field list, in generation
    /// order, so two builds of one module produce one text.
    blocks: Vec<(Vec<Ty>, String)>,
    /// Their `type` lines, which must reach the **head** of the module: a named type
    /// referenced before it is defined is opaque to LLVM's textual parser.
    block_types: String,
    /// The task trampolines, accumulated while functions are emitted and appended after
    /// them. One per `spawn` site.
    tasks: String,
    /// Whether `scope` and `spawn` are reached, and with them the thread helpers.
    needs_tasks: bool,
    /// How many trampolines have been generated, which is what names the next one.
    task_count: usize,
    /// Whether a channel is reached.
    needs_channels: bool,
    /// Whether the I/O helpers and the argument globals are reached.
    needs_io: bool,
    /// Whether `str_join`'s helper is reached.
    needs_join: bool,
    /// Whether the sequence helpers are reached.
    needs_sequences: bool,
    /// Whether a string backing is allocated, retained or released.
    needs_strings: bool,
}

/// What emitting one built-in produced.
#[derive(Debug, Clone, Copy)]
enum Val {
    Of {
        value: u32,
        /// What each built-in states it produced. Nothing reads it since MIR carries every
        /// type; kept because it is how each arm below says what it returns.
        #[allow(dead_code)]
        ty: Ty,
    },
    /// The built-in does not return (only `exit_with`, which MIR makes a terminator, so
    /// this is never produced for a program MIR accepted).
    Transferred,
}

/// A MIR block's label. `bb` never collides with the `b` of the blocks one statement's
/// expansion opens.
fn label(b: mir::BlockId) -> String {
    format!("bb{}", b.0)
}

/// The predicate of a comparison. Signed: `Int` is signed.
const fn predicate(op: mir::CmpOp) -> &'static str {
    match op {
        mir::CmpOp::Eq => "eq",
        mir::CmpOp::NotEq => "ne",
        mir::CmpOp::Lt => "slt",
        mir::CmpOp::LtEq => "sle",
        mir::CmpOp::Gt => "sgt",
        mir::CmpOp::GtEq => "sge",
    }
}

/// The backend's arithmetic operator for MIR's.
const fn arith(op: mir::ArithOp) -> ArithOp {
    match op {
        mir::ArithOp::Add => ArithOp::Add,
        mir::ArithOp::Sub => ArithOp::Sub,
        mir::ArithOp::Mul => ArithOp::Mul,
        mir::ArithOp::Div => ArithOp::Div,
        mir::ArithOp::Rem => ArithOp::Rem,
    }
}

impl Emitter<'_> {
    fn fresh(&mut self) -> u32 {
        let n = self.next;
        self.next += 1;
        n
    }

    /// Block labels come from their own counter.
    ///
    /// They used to share `fresh()` with SSA temporaries, and because the labels are
    /// *named* (`b7:`, not `7:`) LLVM never counts them — so every label left a hole in
    /// the value sequence, and the function emitted `%5` where the implicit counter was
    /// still at 3. LLVM 19 and Apple clang 21 tolerate that; clang 17 rejects it outright
    /// with "instruction expected to be numbered '%3'". Sequential is the portable form,
    /// and the two counters cannot collide because one only ever prints after `%` and the
    /// other only ever after `b`.
    fn fresh_label(&mut self) -> u32 {
        let n = self.next_label;
        self.next_label += 1;
        n
    }

    /// Start a block and make it the current one.
    fn open(&mut self, label: &str) {
        let _ = writeln!(self.out, "{label}:");
        self.current = label.to_owned();
    }

    /// `line:column`, 1-based, from a byte offset. Mirrors the renderer's convention so a
    /// runtime error from a compiled program points where the same error from
    /// `nazm run` would.
    fn position(&self, span: Span) -> (usize, usize) {
        let at = self.names.line_col(span);
        (at.line as usize, at.column as usize)
    }

    /// Intern a `Str` literal, and return the symbol of a **constant `{ptr, i64, ptr}`** for
    /// it.
    ///
    /// A whole constant rather than a pointer the emitter then assembles: building the
    /// pair with `insertvalue` from `undef` would work, but a literal is known entirely
    /// at compile time and a load of a constant says so. Nothing is undefined even for
    /// an instant.
    ///
    /// An empty literal still gets a byte of storage, so its pointer is valid to pass to
    /// `memcmp`, which the specification's "equality compares bytes" rule needs for
    /// `"" == ""`.
    fn blob(&mut self, bytes: &[u8]) -> String {
        if let Some((sym, _)) = self.blobs.iter().find(|(_, b)| b == bytes) {
            return format!("{sym}.v");
        }
        let sym = format!("@.s{}", self.blobs.len());
        self.blobs.push((sym.clone(), bytes.to_vec()));
        format!("{sym}.v")
    }

    /// Build a `{ ptr, i64, ptr }` from a pointer, a length and an owner already in
    /// registers.
    ///
    /// `owner` is the literal text of an LLVM pointer operand — `null` for a string whose
    /// backing nothing has to keep alive, `%n` for one that does. Passing it as text
    /// rather than as a register is what lets the immortal cases say `null` without
    /// materialising a value that means nothing.
    fn make_str(&mut self, ptr: u32, len: u32, owner: &str) -> u32 {
        // `undef` here is the standard way to build an aggregate, and **all three** fields
        // are written before the value is read — this is not the poison-producing shortcut
        // the architecture rules forbid, which is about arithmetic whose overflow an
        // optimiser may then assume away.
        let a = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{a} = insertvalue {{ ptr, i64, ptr }} undef, ptr %{ptr}, 0"
        );
        let b = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{b} = insertvalue {{ ptr, i64, ptr }} %{a}, i64 %{len}, 1"
        );
        let whole = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{whole} = insertvalue {{ ptr, i64, ptr }} %{b}, ptr {owner}, 2"
        );
        whole
    }

    fn str_parts(&mut self, s: u32) -> (u32, u32) {
        let ptr = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{ptr} = extractvalue {{ ptr, i64, ptr }} %{s}, 0"
        );
        let len = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{len} = extractvalue {{ ptr, i64, ptr }} %{s}, 1"
        );
        (ptr, len)
    }

    /// The allocation that keeps this string's bytes valid, or null.
    fn str_owner(&mut self, s: u32) -> u32 {
        let owner = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{owner} = extractvalue {{ ptr, i64, ptr }} %{s}, 2"
        );
        owner
    }

    /// Fail unless `ok`, with the out-of-bounds diagnostic.
    fn require(&mut self, bad: u32, message: &str, span: Span) {
        self.guard(
            bad,
            &Failure {
                code: "N0405",
                message: message.to_owned(),
            },
            span,
        );
    }

    /// A pointer to element `i` of a sequence, bounds-checked.
    ///
    /// The check is here rather than inside a helper because the diagnostic has to name
    /// *this* call's source position, and a shared helper has only one.
    fn sequence_slot(&mut self, handle: u32, index: u32, elem: Ty, span: Span) -> u32 {
        let len = self.fresh();
        let _ = writeln!(self.out, "  %{len} = load i64, ptr %{handle}");
        let neg = self.fresh();
        let _ = writeln!(self.out, "  %{neg} = icmp slt i64 %{index}, 0");
        let past = self.fresh();
        let _ = writeln!(self.out, "  %{past} = icmp sge i64 %{index}, %{len}");
        let bad = self.fresh();
        let _ = writeln!(self.out, "  %{bad} = or i1 %{neg}, %{past}");
        self.require(bad, "this index is outside the sequence", span);
        let data_field = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{data_field} = getelementptr i8, ptr %{handle}, i64 16"
        );
        let data = self.fresh();
        let _ = writeln!(self.out, "  %{data} = load ptr, ptr %{data_field}");
        let slot = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{slot} = getelementptr {}, ptr %{data}, i64 %{index}",
            llvm_type(self.types, elem)
        );
        slot
    }

    /// A string backing of `bytes` usable bytes, as `(owner, data)`.
    ///
    /// The owner is the allocation; the data pointer is inside it. They are two values
    /// rather than one because that is exactly the distinction N8 exists to make: the
    /// bytes a `Str` names and the allocation whose lifetime protects them are not the
    /// same address, and a slice changes one without changing the other.
    fn string_backing(&mut self, bytes: u32, span: Span) -> (u32, u32) {
        self.needs_strings = true;
        let owner = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{owner} = call ptr @nz.str_alloc(i64 %{bytes})"
        );
        let null = self.fresh();
        let _ = writeln!(self.out, "  %{null} = icmp eq ptr %{owner}, null");
        self.guard(
            null,
            &Failure {
                code: "N0406",
                message: "out of memory".to_owned(),
            },
            span,
        );
        let data = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{data} = getelementptr i8, ptr %{owner}, i64 {STRING_HEADER}"
        );
        (owner, data)
    }

    /// Add a reference to whatever `value` owns.
    ///
    /// One function for every owning type, dispatched on [`Ty::owns`]: a type that owns
    /// nothing costs nothing, and adding a type to the language means answering `owns`
    /// rather than finding every site that retains.
    fn retain(&mut self, value: u32, ty: Ty) {
        match ty.owns() {
            None => {}
            // A `Vec` is a sequence header whatever it holds: taking a reference to one is
            // one increment, never a walk over its elements.
            Some(Owns::Ints | Owns::Strs | Owns::Vec(_)) => {
                let _ = writeln!(self.out, "  call void @nz.arr_retain(ptr %{value})");
                self.needs_sequences = true;
            }
            Some(Owns::Backing) => {
                let owner = self.str_owner(value);
                let _ = writeln!(self.out, "  call void @nz.str_retain(ptr %{owner})");
                self.needs_strings = true;
            }
            Some(Owns::Channel) => {
                let _ = writeln!(self.out, "  call void @nz.chan_retain(ptr %{value})");
                self.needs_channels = true;
            }
            // Field by field, recursively. A record owns nothing of its own — it has no
            // header and no count — so taking a reference to one is taking a reference to
            // each of the things its fields name. That is the composition N9 is a test of,
            // and it is written once, here, rather than at each place a record is copied.
            Some(Owns::Record(id)) => {
                if let Some(helper) = self.record_helper(id, true) {
                    let ty = llvm_type(self.types, Ty::Record(id));
                    let _ = writeln!(self.out, "  call void @{helper}({ty} %{value})");
                }
            }
            // The active variant's fields, and only those. The helper branches on the
            // discriminant, so *which* fields exist is decided at runtime — which is the
            // whole of what a sum type adds to the memory model, expressed as a `switch`
            // in the emitted program rather than as a case in this compiler.
            Some(Owns::Enum(id)) => {
                if let Some(helper) = self.enum_helper(id, true) {
                    let ty = llvm_type(self.types, Ty::Enum(id));
                    let _ = writeln!(self.out, "  call void @{helper}({ty} %{value})");
                }
            }
        }
    }

    /// The fields of a record that own something, each with its LLVM index and type.
    ///
    /// A record whose every field is an `Int` yields nothing, so it costs nothing —
    /// neither an instruction nor a frame slot.
    fn owning_fields(&self, id: RecordTy) -> Vec<(u32, Ty)> {
        record_layout(self.types.records, id).map_or_else(Vec::new, |l| {
            l.fields
                .iter()
                .enumerate()
                .filter(|(_, t)| owns_anything(self.types, **t))
                .map(|(i, t)| (u32::try_from(i).unwrap_or(u32::MAX), *t))
                .collect()
        })
    }

    /// One field out of a record value already in a register.
    fn extract(&mut self, value: u32, id: RecordTy, at: u32, ty: Ty) -> u32 {
        let record = llvm_type(self.types, Ty::Record(id));
        let field = llvm_type(self.types, ty);
        let out = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{out} = extractvalue {record} %{value}, {at}    ; {field}"
        );
        out
    }

    /// Drop a reference to whatever `value` owns. The storage goes when the last one does.
    fn release(&mut self, value: u32, ty: Ty) {
        match ty.owns() {
            None => {}
            Some(Owns::Ints) => {
                let _ = writeln!(self.out, "  call void @nz.seq_release(ptr %{value})");
                self.needs_sequences = true;
            }
            // A `Strs` releases its elements' backings before its own storage, which is
            // why it cannot share `@nz.seq_release`.
            Some(Owns::Strs) => {
                let _ = writeln!(self.out, "  call void @nz.strs_release(ptr %{value})");
                self.needs_sequences = true;
                self.needs_strings = true;
            }
            Some(Owns::Backing) => {
                let owner = self.str_owner(value);
                let _ = writeln!(self.out, "  call void @nz.str_release(ptr %{owner})");
                self.needs_strings = true;
            }
            Some(Owns::Channel) => {
                let _ = writeln!(self.out, "  call void @nz.chan_release(ptr %{value})");
                self.needs_channels = true;
            }
            // The mirror of the retain above, and the order between fields is not
            // observable: `docs/spec.md` says a record is destroyed by releasing each
            // field and deliberately does not say in which order, because no program can
            // tell. Layout order is what this does, which is deterministic and documented.
            Some(Owns::Record(id)) => {
                if let Some(helper) = self.record_helper(id, false) {
                    let ty = llvm_type(self.types, Ty::Record(id));
                    let _ = writeln!(self.out, "  call void @{helper}({ty} %{value})");
                }
            }
            Some(Owns::Enum(id)) => {
                if let Some(helper) = self.enum_helper(id, false) {
                    let ty = llvm_type(self.types, Ty::Enum(id));
                    let _ = writeln!(self.out, "  call void @{helper}({ty} %{value})");
                }
            }
            // The last reference releases every element by the element's own law, and
            // then the storage. An element type that owns nothing needs no walk, and the
            // header goes the way an `Ints` does.
            Some(Owns::Vec(id)) => {
                self.needs_sequences = true;
                match self.vec_helper(id) {
                    Some(helper) => {
                        let _ = writeln!(self.out, "  call void @{helper}(ptr %{value})");
                    }
                    None => {
                        let _ = writeln!(self.out, "  call void @nz.seq_release(ptr %{value})");
                    }
                }
            }
        }
    }

    /// The symbol of this `Vec` type's release helper, generating it if needed.
    ///
    /// `None` for an element type that owns nothing: releasing such a vector is releasing
    /// its header and buffer, which `@nz.seq_release` already does.
    ///
    /// The same algorithm `@nz.strs_release` is, over this element type: decrement, and
    /// on the last reference release each element **before** the buffer that holds it and
    /// the buffer before the header, then count the reclamation. One helper per element
    /// type rather than a loop at every site, and an element that is itself a `Vec`, a
    /// record or an enum is released by *its* helper — so the recursion is in the emitted
    /// program, not in this compiler.
    fn vec_helper(&mut self, id: VecTy) -> Option<String> {
        let layout = vec_layout(self.types.vecs, id)?.clone();
        if !owns_anything(self.types, layout.elem) {
            return None;
        }
        let name = format!("nz.vrel.{}", layout.name);
        if !self.generated_vecs.insert(id) {
            return Some(name);
        }
        self.needs_sequences = true;
        let elem = llvm_type(self.types, layout.elem);

        let saved_out = std::mem::take(&mut self.out);
        let saved_next = self.next;
        let saved_label = self.next_label;
        let saved_current = std::mem::take(&mut self.current);
        self.next = 0;
        self.next_label = 0;
        self.current = "entry".to_owned();

        let _ = writeln!(
            self.out,
            "\ndefine internal void @{name}(ptr %h) {{    ; {}",
            layout.source
        );
        for line in [
            "entry:",
            "  %none = icmp eq ptr %h, null",
            "  br i1 %none, label %done, label %drop",
            "drop:",
            "  %rcp = getelementptr i8, ptr %h, i64 24",
            "  %rc = load i64, ptr %rcp",
            "  %rc2 = sub i64 %rc, 1",
            "  store i64 %rc2, ptr %rcp",
            "  %last = icmp sle i64 %rc2, 0",
            "  br i1 %last, label %contents, label %done",
            "contents:",
            "  %n = load i64, ptr %h",
            "  %dp = getelementptr i8, ptr %h, i64 16",
            "  %data = load ptr, ptr %dp",
            "  br label %test",
            "test:",
            "  %i = phi i64 [ 0, %contents ], [ %next, %body ]",
            "  %more = icmp slt i64 %i, %n",
            "  br i1 %more, label %body, label %storage",
            "body:",
        ] {
            let _ = writeln!(self.out, "{line}");
        }
        self.current = "body".to_owned();
        let _ = writeln!(self.out, "  %e = getelementptr {elem}, ptr %data, i64 %i");
        let v = self.fresh();
        let _ = writeln!(self.out, "  %{v} = load {elem}, ptr %e");
        self.release(v, layout.elem);
        for line in [
            "  %next = add i64 %i, 1",
            "  br label %test",
            "storage:",
            "  call void @free(ptr %data)",
            "  call void @free(ptr %h)",
            "  %c = atomicrmw add ptr @nz.seq_reclaimed, i64 1 monotonic",
            "  br label %done",
            "done:",
            "  ret void",
            "}",
        ] {
            let _ = writeln!(self.out, "{line}");
        }

        let body = std::mem::replace(&mut self.out, saved_out);
        self.helpers.push_str(&body);
        self.next = saved_next;
        self.next_label = saved_label;
        self.current = saved_current;
        Some(name)
    }

    /// The symbol of this record's retain or release helper, generating it if needed.
    ///
    /// `None` for a record no field of which owns anything: there is nothing for the
    /// helper to do, so none is generated and no call is written.
    ///
    /// Generation swaps the output buffer, so the helper is written as a complete function
    /// beside the others rather than into the middle of the one being emitted. Value
    /// numbering restarts, which LLVM requires per function, and is restored afterwards.
    fn record_helper(&mut self, id: RecordTy, retain: bool) -> Option<String> {
        if !owns_anything(self.types, Ty::Record(id)) {
            return None;
        }
        let name = format!(
            "{}.{}",
            if retain { "nz.ret" } else { "nz.rel" },
            record_layout(self.types.records, id)?.name.as_str()
        );
        if !self.generated.insert((id, retain)) {
            return Some(name);
        }

        let record = llvm_type(self.types, Ty::Record(id));
        let saved_out = std::mem::take(&mut self.out);
        let saved_next = self.next;
        let saved_label = self.next_label;
        let saved_current = std::mem::take(&mut self.current);
        self.next = 0;
        self.next_label = 0;
        self.current = "entry".to_owned();

        let _ = writeln!(self.out, "\ndefine internal void @{name}({record} %r) {{");
        let _ = writeln!(self.out, "entry:");
        // Layout order, which is deterministic and which no program can observe:
        // `docs/spec.md` says a record is destroyed by releasing each field and
        // deliberately does not say in which order.
        for (at, ty) in self.owning_fields(id) {
            let field = llvm_type(self.types, ty);
            let v = self.fresh();
            let _ = writeln!(
                self.out,
                "  %{v} = extractvalue {record} %r, {at}    ; {field}"
            );
            if retain {
                self.retain(v, ty);
            } else {
                self.release(v, ty);
            }
        }
        let _ = writeln!(self.out, "  ret void");
        let _ = writeln!(self.out, "}}");

        let body = std::mem::replace(&mut self.out, saved_out);
        self.helpers.push_str(&body);
        self.next = saved_next;
        self.next_label = saved_label;
        self.current = saved_current;
        Some(name)
    }

    /// The symbol of this enum's retain or release helper, generating it if needed.
    ///
    /// `None` for an enum no payload field of which owns anything: there is nothing for
    /// the helper to do, so none is generated and no call is written.
    ///
    /// # Why this is one function per enum rather than code at each site
    ///
    /// Because the answer is a runtime fact. A record's copy is a fixed sequence of
    /// retains; an enum's is *a branch and then* a fixed sequence, and which sequence is
    /// not known where the copy is written. Putting the branch in a generated function
    /// puts it in the emitted program once per type rather than once per place a value of
    /// that type is copied — and it is what lets a nested enum's helper call an inner
    /// one's, so the recursion lives in the program rather than in this compiler.
    ///
    /// A variant whose payload owns nothing is left out of the `switch` and reaches the
    /// exit through its default, so a zero-payload arm costs nothing at all.
    fn enum_helper(&mut self, id: EnumTy, retain: bool) -> Option<String> {
        if !owns_anything(self.types, Ty::Enum(id)) {
            return None;
        }
        let name = format!(
            "{}.{}",
            if retain { "nz.ret" } else { "nz.rel" },
            enum_layout(self.types.enums, id)?.name.as_str()
        );
        if !self.generated_enums.insert((id, retain)) {
            return Some(name);
        }

        let whole = llvm_type(self.types, Ty::Enum(id));
        let arms = self.owning_variants(id);

        let saved_out = std::mem::take(&mut self.out);
        let saved_next = self.next;
        let saved_label = self.next_label;
        let saved_current = std::mem::take(&mut self.current);
        self.next = 0;
        self.next_label = 0;
        self.current = "entry".to_owned();

        let _ = writeln!(self.out, "\ndefine internal void @{name}({whole} %e) {{");
        let _ = writeln!(self.out, "entry:");
        let tag = self.fresh();
        let _ = writeln!(self.out, "  %{tag} = extractvalue {whole} %e, 0");
        let done = format!("b{}", self.fresh_label());
        let labelled: Vec<(OwningVariant, String)> = arms
            .into_iter()
            .map(|a| {
                let label = format!("b{}", self.fresh_label());
                (a, label)
            })
            .collect();
        // The default is the exit. An unknown discriminant cannot arise in a safe
        // program — every construction writes one this enum has — and treating it as
        // "nothing to release" is the answer that stays correct if one ever does.
        let table = labelled
            .iter()
            .map(|(a, label)| format!("i64 {}, label %{label}", a.tag))
            .collect::<Vec<_>>()
            .join(" ");
        let _ = writeln!(self.out, "  switch i64 %{tag}, label %{done} [ {table} ]");
        for (arm, label) in &labelled {
            self.open(label);
            let slot = self.fresh();
            // The variant's payload is one field of the enum, at its tag plus one: the
            // discriminant occupies index 0.
            let _ = writeln!(
                self.out,
                "  %{slot} = extractvalue {whole} %e, {}    ; {}",
                arm.tag + 1,
                arm.source
            );
            for (i, ty) in &arm.owning {
                let field = llvm_type(self.types, *ty);
                let v = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{v} = extractvalue {} %{slot}, {i}    ; {field}",
                    arm.payload
                );
                if retain {
                    self.retain(v, *ty);
                } else {
                    self.release(v, *ty);
                }
            }
            let _ = writeln!(self.out, "  br label %{done}");
        }
        self.open(&done);
        let _ = writeln!(self.out, "  ret void");
        let _ = writeln!(self.out, "}}");

        let body = std::mem::replace(&mut self.out, saved_out);
        self.helpers.push_str(&body);
        self.next = saved_next;
        self.next_label = saved_label;
        self.current = saved_current;
        Some(name)
    }

    /// `==` on two values of one type with equality, as an `i1`. N13.
    ///
    /// Borrowing: nothing is retained or released here, because comparing reads and the
    /// caller owns both operands. `Int` and `Bool` are an `icmp`; a `Str` is its bytes;
    /// a record or an enum is its helper. Checking has refused every other type
    /// (`N0304`), and lowering refuses it again rather than let an `icmp` on a handle's
    /// pointer answer a question the language has not asked.
    fn equality(&mut self, a: u32, b: u32, ty: Ty) -> u32 {
        match ty {
            Ty::Str => self.string_equality(a, b, false),
            Ty::Record(_) | Ty::Enum(_) => {
                let helper = self.equality_helper(ty);
                let t = llvm_type(self.types, ty);
                let out = self.fresh();
                let _ = writeln!(self.out, "  %{out} = call i1 @{helper}({t} %{a}, {t} %{b})");
                out
            }
            _ => {
                let t = llvm_type(self.types, ty);
                let out = self.fresh();
                let _ = writeln!(self.out, "  %{out} = icmp eq {t} %{a}, %{b}");
                out
            }
        }
    }

    /// The symbol of this record's or enum's equality helper, generating it if needed.
    /// N13.
    ///
    /// # Why a helper, and why this shape
    ///
    /// For the reason the retain and release helpers are one per type: a nested type's
    /// helper calls the inner one's, so the recursion over the type lives in the
    /// *emitted* program rather than in this compiler's stack, and a comparison written in
    /// forty places is forty calls rather than forty copies of a walk. Named from the
    /// type's [`NativeTypeName`], which is derived from its durable identity and its
    /// concrete arguments — never from an id of this session — and `internal`, so each unit
    /// that compares a type carries its own copy and no symbol is promised to anyone.
    ///
    /// A **record** compares its fields in layout order, which is canonical by field name,
    /// and returns `false` at the first that differs: comparing is pure, so the order is
    /// not observable, and canonical order makes the helper's text independent of how the
    /// declaration was written.
    ///
    /// An **enum** compares discriminants first, and different variants are unequal with
    /// nothing else read. Equal discriminants `switch` to that variant's payload, and only
    /// that slot is extracted: an inactive slot holds zeros no operation reads, and this
    /// is not the first. A zero-payload variant reaches `true` through the `switch`'s
    /// default, so it costs no block of its own.
    fn equality_helper(&mut self, ty: Ty) -> String {
        let name = match ty {
            Ty::Record(id) => {
                record_layout(self.types.records, id).map(|l| l.name.as_str().to_owned())
            }
            Ty::Enum(id) => enum_layout(self.types.enums, id).map(|l| l.name.as_str().to_owned()),
            _ => None,
        }
        .unwrap_or_default();
        let name = format!("nz.eq.{name}");
        if !self.generated_eq.insert(ty) {
            return name;
        }
        let whole = llvm_type(self.types, ty);

        let saved_out = std::mem::take(&mut self.out);
        let saved_next = self.next;
        let saved_label = self.next_label;
        let saved_current = std::mem::take(&mut self.current);
        self.next = 0;
        self.next_label = 0;
        self.current = "entry".to_owned();

        let _ = writeln!(
            self.out,
            "\ndefine internal i1 @{name}({whole} %a, {whole} %b) {{"
        );
        let _ = writeln!(self.out, "entry:");
        let differ = format!("b{}", self.fresh_label());
        match ty {
            Ty::Record(id) => {
                let fields = record_layout(self.types.records, id)
                    .map(|l| l.fields.clone())
                    .unwrap_or_default();
                for (at, fty) in fields.into_iter().enumerate() {
                    let at = u32::try_from(at).unwrap_or(u32::MAX);
                    let x = self.extract_from(ty, 'a', at, fty);
                    let y = self.extract_from(ty, 'b', at, fty);
                    self.equal_or_leave(x, y, fty, &differ);
                }
                let _ = writeln!(self.out, "  ret i1 true");
            }
            Ty::Enum(id) => {
                let variants = enum_layout(self.types.enums, id)
                    .map(|l| l.variants.clone())
                    .unwrap_or_default();
                let (ta, tb) = (self.fresh(), self.fresh());
                let _ = writeln!(self.out, "  %{ta} = extractvalue {whole} %a, 0");
                let _ = writeln!(self.out, "  %{tb} = extractvalue {whole} %b, 0");
                let same = self.fresh();
                let _ = writeln!(self.out, "  %{same} = icmp eq i64 %{ta}, %{tb}");
                let dispatch = format!("b{}", self.fresh_label());
                let equal = format!("b{}", self.fresh_label());
                let _ = writeln!(
                    self.out,
                    "  br i1 %{same}, label %{dispatch}, label %{differ}"
                );
                self.open(&dispatch);
                let arms: Vec<(u32, VariantLayout, String)> = variants
                    .into_iter()
                    .enumerate()
                    .filter(|(_, v)| !v.fields.is_empty())
                    .map(|(tag, v)| {
                        let label = format!("b{}", self.fresh_label());
                        (u32::try_from(tag).unwrap_or(u32::MAX), v, label)
                    })
                    .collect();
                let table = arms
                    .iter()
                    .map(|(tag, _, label)| format!("i64 {tag}, label %{label}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let _ = writeln!(self.out, "  switch i64 %{ta}, label %{equal} [ {table} ]");
                for (tag, variant, label) in arms {
                    self.open(&label);
                    let (pa, pb) = (self.fresh(), self.fresh());
                    let _ = writeln!(
                        self.out,
                        "  %{pa} = extractvalue {whole} %a, {}    ; {}",
                        tag + 1,
                        variant.source
                    );
                    let _ = writeln!(self.out, "  %{pb} = extractvalue {whole} %b, {}", tag + 1);
                    for (at, fty) in variant.fields.iter().enumerate() {
                        let field = llvm_type(self.types, *fty);
                        let (x, y) = (self.fresh(), self.fresh());
                        let _ = writeln!(
                            self.out,
                            "  %{x} = extractvalue {} %{pa}, {at}    ; {field}",
                            variant.name
                        );
                        let _ = writeln!(
                            self.out,
                            "  %{y} = extractvalue {} %{pb}, {at}",
                            variant.name
                        );
                        self.equal_or_leave(x, y, *fty, &differ);
                    }
                    let _ = writeln!(self.out, "  br label %{equal}");
                }
                self.open(&equal);
                let _ = writeln!(self.out, "  ret i1 true");
            }
            _ => {
                let _ = writeln!(self.out, "  ret i1 false");
            }
        }
        self.open(&differ);
        let _ = writeln!(self.out, "  ret i1 false");
        let _ = writeln!(self.out, "}}");

        let body = std::mem::replace(&mut self.out, saved_out);
        self.helpers.push_str(&body);
        self.next = saved_next;
        self.next_label = saved_label;
        self.current = saved_current;
        name
    }

    /// One field of a helper's parameter `%a` or `%b`, by layout position.
    fn extract_from(&mut self, ty: Ty, param: char, at: u32, field: Ty) -> u32 {
        let whole = llvm_type(self.types, ty);
        let shown = llvm_type(self.types, field);
        let out = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{out} = extractvalue {whole} %{param}, {at}    ; {shown}"
        );
        out
    }

    /// Compare one component and leave for `differ` if it is unequal; otherwise carry on
    /// in a fresh block.
    fn equal_or_leave(&mut self, x: u32, y: u32, ty: Ty, differ: &str) {
        let same = self.equality(x, y, ty);
        let next = format!("b{}", self.fresh_label());
        let _ = writeln!(self.out, "  br i1 %{same}, label %{next}, label %{differ}");
        self.open(&next);
    }

    /// Every variant of this enum whose payload owns something, with what it owns.
    ///
    /// Collected before the helper is written, because writing it borrows `self` mutably
    /// and reading the layout borrows it immutably.
    fn owning_variants(&self, id: EnumTy) -> Vec<OwningVariant> {
        enum_layout(self.types.enums, id).map_or_else(Vec::new, |l| {
            l.variants
                .iter()
                .enumerate()
                .filter_map(|(tag, v)| {
                    let owning: Vec<(u32, Ty)> = v
                        .fields
                        .iter()
                        .enumerate()
                        .filter(|(_, t)| owns_anything(self.types, **t))
                        .map(|(i, t)| (u32::try_from(i).unwrap_or(u32::MAX), *t))
                        .collect();
                    (!owning.is_empty()).then(|| OwningVariant {
                        tag: u32::try_from(tag).unwrap_or(u32::MAX),
                        payload: v.name.to_string(),
                        source: v.source.clone(),
                        owning,
                    })
                })
                .collect()
        })
    }

    /// Drop a reference to something the runtime allocated for itself.
    ///
    /// Same allocator, deliberately different counter: a scope's list of task ids is not a
    /// value any program can name, and a report about *the program's* sequences that
    /// counted one would be answering a different question from the one it was asked.
    fn release_internal(&mut self, value: u32) {
        // The result is named even though nothing reads it: an unnamed non-void result
        // still consumes a slot in LLVM's per-function numbering, and skipping one is how
        // a module stops parsing three hundred instructions later.
        let freed = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{freed} = call i1 @nz.arr_release(ptr %{value})"
        );
        self.needs_sequences = true;
    }

    /// The LLVM name of one variant's payload struct.
    fn payload_type(&self, enum_ty: Ty, tag: u32) -> String {
        let Ty::Enum(id) = enum_ty else {
            return enum_ty.builtin_llvm().to_owned();
        };
        enum_layout(self.types.enums, id)
            .and_then(|l| l.variants.get(tag as usize))
            .map_or_else(|| "<variant>".to_owned(), |v| v.name.to_string())
    }

    /// Replace the field `path` reaches, starting from `slot`.
    ///
    /// Reads the record out of the slot, extracts down the path, releases what was there,
    /// then rebuilds outward with `insertvalue` and stores the result back. The rebuild is
    /// what value semantics require: each level is a value, so replacing something inside
    /// one produces a new value for it, and so on out to the slot.
    ///
    /// The new value has already been taken by the caller, which is the ordering rule.
    fn set_field(&mut self, slot: usize, path: &[(u32, Ty)], new: u32, new_ty: Ty) {
        let root_ty = self.frame_ty(slot);
        // The aggregate at each level on the way in, so the way out can rebuild it.
        let mut levels: Vec<(u32, Ty, u32)> = Vec::with_capacity(path.len());
        let loaded = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{loaded} = load {}, ptr %slot{slot}",
            llvm_type(self.types, root_ty)
        );
        let mut current = loaded;
        let mut current_ty = root_ty;
        for (at, field_ty) in path {
            levels.push((current, current_ty, *at));
            // The last step is the one being replaced; the ones before it are records
            // being descended through.
            let next = self.fresh();
            let _ = writeln!(
                self.out,
                "  %{next} = extractvalue {} %{current}, {at}",
                llvm_type(self.types, current_ty)
            );
            current = next;
            current_ty = *field_ty;
        }
        // `current` is the old field. Released **after** the new value was taken.
        let old = current;
        let old_ty = current_ty;

        let mut built = new;
        let mut built_ty = new_ty;
        for (aggregate, aggregate_ty, at) in levels.into_iter().rev() {
            let out = self.fresh();
            let _ = writeln!(
                self.out,
                "  %{out} = insertvalue {} %{aggregate}, {} %{built}, {at}",
                llvm_type(self.types, aggregate_ty),
                llvm_type(self.types, built_ty)
            );
            built = out;
            built_ty = aggregate_ty;
        }
        let _ = writeln!(
            self.out,
            "  store {} %{built}, ptr %slot{slot}",
            llvm_type(self.types, built_ty)
        );
        self.release(old, old_ty);
    }

    /// The type of a slot, as MIR declared it.
    fn frame_ty(&self, slot: usize) -> Ty {
        self.slot_types
            .get(slot)
            .copied()
            .flatten()
            .unwrap_or(Ty::Int)
    }

    /// Leave for the statement's failure edge. What the failure owes — the joins of open
    /// scopes, the drops of what the function holds — is in the MIR blocks along that edge.
    fn unwind(&mut self) {
        let _ = writeln!(self.out, "  br label %{}", self.unwind_to);
    }

    /// If this thread has failed, leave along the failure edge; otherwise carry on in a fresh
    /// block. Emitted after every call to a generated function, after the failure check MIR
    /// places at a scope's closing brace, and inside `chan_recv`.
    fn check_failed(&mut self) {
        let flag = self.fresh();
        let cont = self.fresh_label();
        let _ = writeln!(self.out, "  %{flag} = load i1, ptr @nz.failed");
        let _ = writeln!(
            self.out,
            "  br i1 %{flag}, label %{}, label %b{cont}",
            self.unwind_to
        );
        self.open(&format!("b{cont}"));
    }

    /// The LLVM type of a local's slot.
    fn slot_llvm(&self, l: mir::LocalId) -> String {
        match self.slot_types.get(l.index()).copied().flatten() {
            Some(t) => llvm_type(self.types, t),
            None => "ptr".to_owned(),
        }
    }

    fn slot_ty(&self, l: mir::LocalId) -> Ty {
        self.slot_types
            .get(l.index())
            .copied()
            .flatten()
            .unwrap_or(Ty::Int)
    }

    /// The zero a slot of this local holds when it owns nothing.
    fn slot_zero(&self, l: mir::LocalId) -> &'static str {
        match self.slot_types.get(l.index()).copied().flatten() {
            Some(t) => t.unwind_value(),
            None => "null",
        }
    }

    fn load(&mut self, l: mir::LocalId) -> u32 {
        let ty = self.slot_llvm(l);
        let v = self.fresh();
        let _ = writeln!(self.out, "  %{v} = load {ty}, ptr %slot{}", l.0);
        v
    }

    fn store(&mut self, l: mir::LocalId, value: &str) {
        let ty = self.slot_llvm(l);
        let _ = writeln!(self.out, "  store {ty} {value}, ptr %slot{}", l.0);
    }

    fn store_zero(&mut self, l: mir::LocalId) {
        let zero = self.slot_zero(l);
        self.store(l, zero);
    }

    /// An operand's value in a register.
    fn operand(&mut self, o: &mir::Operand) -> (u32, Ty) {
        match o {
            mir::Operand::Local(l) => {
                let ty = self.slot_ty(*l);
                (self.load(*l), ty)
            }
            mir::Operand::Const(c) => self.constant(c),
        }
    }

    fn constant(&mut self, c: &mir::Const) -> (u32, Ty) {
        match c {
            // Materialised through an instruction rather than used inline so every value
            // has a number and one shape. `-O2` folds it; `-O0` does not, which is what makes
            // the arithmetic genuinely execute during testing.
            mir::Const::Int(v) => {
                let n = self.fresh();
                let _ = writeln!(self.out, "  %{n} = add i64 {v}, 0");
                (n, Ty::Int)
            }
            mir::Const::Bool(v) => {
                let n = self.fresh();
                let _ = writeln!(self.out, "  %{n} = or i1 {v}, false");
                (n, Ty::Bool)
            }
            // **Immortal.** The constant is in the object's read-only data and lives as long
            // as the process; its owner is null, and every retain and release of it is a
            // no-op that costs one comparison.
            mir::Const::Str(bytes) => {
                let sym = self.blob(bytes);
                let n = self.fresh();
                let _ = writeln!(self.out, "  %{n} = load {{ ptr, i64, ptr }}, ptr {sym}");
                (n, Ty::Str)
            }
        }
    }

    /// Where the field declared at `field` sits in its record's struct: the layout boundary.
    fn field_position(&self, f: mir::FieldRef) -> u32 {
        record_layout(self.types.records, rty(f.record))
            .and_then(|l| l.of_declaration.get(f.field as usize).copied())
            .unwrap_or(0)
    }

    /// The discriminant of the variant declared at `variant`.
    fn tag(&self, e: nazm_sema::EnumId, variant: u32) -> u32 {
        enum_layout(self.types.enums, ety(e))
            .and_then(|l| l.of_declaration.get(variant as usize).copied())
            .unwrap_or(0)
    }

    /// Where a payload field sits in its variant's struct.
    fn payload_position(&self, e: nazm_sema::EnumId, tag: u32, field: u32) -> u32 {
        enum_layout(self.types.enums, ety(e))
            .and_then(|l| l.variants.get(tag as usize))
            .and_then(|v| v.of_declaration.get(field as usize).copied())
            .unwrap_or(0)
    }

    fn statement(&mut self, s: &mir::Statement) {
        self.unwind_to = s
            .kind
            .unwind()
            .map_or_else(|| "nz.no.failure.edge".to_owned(), label);
        match &s.kind {
            mir::StatementKind::Assign { dst, value, .. } => {
                let v = self.rvalue(value, s.at);
                self.store(*dst, &v);
            }
            mir::StatementKind::SetField { base, path, value } => {
                let new_ty = self.slot_ty(*value);
                let new = self.load(*value);
                self.store_zero(*value);
                let steps: Vec<(u32, Ty)> = path
                    .iter()
                    .map(|f| {
                        let ty = self
                            .program
                            .types
                            .field(f.record, f.field)
                            .map_or(Ty::Int, Ty::of);
                        (self.field_position(*f), ty)
                    })
                    .collect();
                self.set_field(base.index(), &steps, new, new_ty);
            }
            mir::StatementKind::Drop(l) => {
                let ty = self.slot_ty(*l);
                if owns_anything(self.types, ty) {
                    let v = self.load(*l);
                    self.store_zero(*l);
                    self.release(v, ty);
                }
            }
            mir::StatementKind::ScopeOpen { list, .. } => {
                // The list is an ordinary sequence header, reused rather than reinvented: a
                // task handle is pointer-sized and a growable array of them is exactly what
                // `@nz.arr_new` already provides.
                self.needs_sequences = true;
                self.needs_tasks = true;
                let tids = self.fresh();
                let _ = writeln!(self.out, "  %{tids} = call ptr @nz.arr_new()");
                let null = self.fresh();
                let _ = writeln!(self.out, "  %{null} = icmp eq ptr %{tids}, null");
                self.guard(
                    null,
                    &Failure {
                        code: "N0406",
                        message: "out of memory".to_owned(),
                    },
                    s.at,
                );
                self.store(*list, &format!("%{tids}"));
            }
            mir::StatementKind::Spawn {
                list, callee, args, ..
            } => {
                let tids = self.load(*list);
                let vals: Vec<(u32, Ty)> = args.iter().map(|a| self.operand(a)).collect();
                self.spawn(tids, *callee, &vals, s.at);
            }
            mir::StatementKind::ScopeJoin { list } => {
                let tids = self.load(*list);
                self.store_zero(*list);
                self.join_one(tids);
            }
            mir::StatementKind::CheckFailed { .. } => self.check_failed(),
        }
    }

    /// An rvalue's value, as an LLVM operand to store into its destination.
    fn rvalue(&mut self, v: &mir::Rvalue, at: Span) -> String {
        match v {
            mir::Rvalue::Use(o) => format!("%{}", self.operand(o).0),
            mir::Rvalue::Copy(l) => {
                let ty = self.slot_ty(*l);
                let x = self.load(*l);
                self.retain(x, ty);
                format!("%{x}")
            }
            mir::Rvalue::Move(l) => {
                let x = self.load(*l);
                self.store_zero(*l);
                format!("%{x}")
            }
            mir::Rvalue::Neg(o) => {
                let (x, _) = self.operand(o);
                let r = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{r} = call {{i64, i1}} @llvm.ssub.with.overflow.i64(i64 0, i64 %{x})"
                );
                let ov = self.fresh();
                let _ = writeln!(self.out, "  %{ov} = extractvalue {{i64, i1}} %{r}, 1");
                self.guard(
                    ov,
                    &Failure {
                        code: "N0400",
                        message: "this negation overflows `Int`".to_owned(),
                    },
                    at,
                );
                let out = self.fresh();
                let _ = writeln!(self.out, "  %{out} = extractvalue {{i64, i1}} %{r}, 0");
                format!("%{out}")
            }
            mir::Rvalue::Arith(op, a, b) => {
                // Left, then right: MIR evaluated both into operands in that order already.
                let (x, _) = self.operand(a);
                let (y, _) = self.operand(b);
                let out = match arith(*op) {
                    op @ (ArithOp::Add | ArithOp::Sub | ArithOp::Mul) => self.checked(op, x, y, at),
                    ArithOp::Div => self.divide(x, y, at),
                    ArithOp::Rem => self.remainder(x, y, at),
                };
                format!("%{out}")
            }
            mir::Rvalue::Compare {
                op,
                operand,
                lhs,
                rhs,
                ..
            } => {
                let (a, _) = self.operand(lhs);
                let (b, _) = self.operand(rhs);
                let ty = Ty::of(*operand);
                // `Str` is a three-field aggregate: two strings are equal when they have the
                // same bytes, not when they share a buffer.
                if ty == Ty::Str {
                    let out = self.string_equality(a, b, *op == mir::CmpOp::NotEq);
                    return format!("%{out}");
                }
                // A record or an enum: its equality helper, N13. `!=` is the negation of that
                // one answer, never a second walk.
                if matches!(ty, Ty::Record(_) | Ty::Enum(_)) {
                    let same = self.equality(a, b, ty);
                    if *op == mir::CmpOp::NotEq {
                        let flipped = self.fresh();
                        let _ = writeln!(self.out, "  %{flipped} = xor i1 %{same}, true");
                        return format!("%{flipped}");
                    }
                    return format!("%{same}");
                }
                let out = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{out} = icmp {} {} %{a}, %{b}",
                    predicate(*op),
                    llvm_type(self.types, ty)
                );
                format!("%{out}")
            }
            mir::Rvalue::Call { callee, args } => {
                let vals: Vec<(u32, Ty)> = args.iter().map(|a| self.operand(a)).collect();
                let list = vals
                    .iter()
                    .map(|(v, t)| format!("{} %{v}", llvm_type(self.types, *t)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let (symbol, ret) = self
                    .sigs
                    .get(callee.index())
                    .map_or((String::from("@nz.missing"), Ty::Int), |s| {
                        (s.symbol.to_string(), s.ret)
                    });
                let out = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{out} = call {} {symbol}({list})",
                    llvm_type(self.types, ret)
                );
                // A callee that failed has already unwound to its own `ret` and handed back
                // a stand-in value. This is where that is noticed.
                self.check_failed();
                format!("%{out}")
            }
            mir::Rvalue::Primitive { op, elem, args } => {
                let vals: Vec<u32> = args.iter().map(|a| self.operand(a).0).collect();
                let out =
                    match elem {
                        Some(e) => {
                            let vec = self.program.types.vec_of(*e).map_or(VecTy(u32::MAX), |t| {
                                match Ty::of(t) {
                                    Ty::Vec(v) => v,
                                    _ => VecTy(u32::MAX),
                                }
                            });
                            self.vec_op(*op, vec, Ty::of(*e), &vals, at)
                        }
                        None => self.builtin_op(*op, &vals, at),
                    };
                match out {
                    Val::Of { value, .. } => format!("%{value}"),
                    Val::Transferred => "undef".to_owned(),
                }
            }
            mir::Rvalue::Record { record, fields } => {
                let ty = Ty::Record(rty(*record));
                let text = llvm_type(self.types, ty);
                // Complete by construction: MIR builds a record only from every field, moved
                // out of the temporaries they were evaluated into, in the order written.
                let mut built = format!("{text} zeroinitializer");
                let mut last = None;
                for (field, tmp) in fields {
                    let fty = self.slot_ty(*tmp);
                    let v = self.load(*tmp);
                    self.store_zero(*tmp);
                    let at = self.field_position(mir::FieldRef {
                        record: *record,
                        field: *field,
                    });
                    let out = self.fresh();
                    let _ = writeln!(
                        self.out,
                        "  %{out} = insertvalue {built}, {} %{v}, {at}",
                        llvm_type(self.types, fty)
                    );
                    built = format!("{text} %{out}");
                    last = Some(out);
                }
                last.map_or_else(|| "zeroinitializer".to_owned(), |o| format!("%{o}"))
            }
            mir::Rvalue::Variant {
                enumeration,
                variant,
                fields,
            } => {
                let ty = Ty::Enum(ety(*enumeration));
                let whole = llvm_type(self.types, ty);
                let tag = self.tag(*enumeration, *variant);
                let payload_ty = self.payload_type(ty, tag);
                let mut payload = format!("{payload_ty} zeroinitializer");
                for (field, tmp) in fields {
                    let fty = self.slot_ty(*tmp);
                    let v = self.load(*tmp);
                    self.store_zero(*tmp);
                    let at = self.payload_position(*enumeration, tag, *field);
                    let out = self.fresh();
                    let _ = writeln!(
                        self.out,
                        "  %{out} = insertvalue {payload}, {} %{v}, {at}",
                        llvm_type(self.types, fty)
                    );
                    payload = format!("{payload_ty} %{out}");
                }
                let tagged = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{tagged} = insertvalue {whole} zeroinitializer, i64 {tag}, 0"
                );
                let out = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{out} = insertvalue {whole} %{tagged}, {payload}, {}",
                    tag + 1
                );
                format!("%{out}")
            }
            mir::Rvalue::Field { base, field } => {
                let (b, _) = self.operand(base);
                let fty = self
                    .program
                    .types
                    .field(field.record, field.field)
                    .map_or(Ty::Int, Ty::of);
                let at = self.field_position(*field);
                let out = self.extract(b, rty(field.record), at, fty);
                // Its own reference, by MIR's law: a projection is owned by its destination.
                self.retain(out, fty);
                format!("%{out}")
            }
            mir::Rvalue::Payload {
                base,
                enumeration,
                variant,
                field,
            } => {
                let ty = Ty::Enum(ety(*enumeration));
                let whole = llvm_type(self.types, ty);
                let tag = self.tag(*enumeration, *variant);
                let payload_ty = self.payload_type(ty, tag);
                let fty = self
                    .program
                    .types
                    .payload(*enumeration, *variant, *field)
                    .map_or(Ty::Int, Ty::of);
                let at = self.payload_position(*enumeration, tag, *field);
                let b = self.load(*base);
                let slot = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{slot} = extractvalue {whole} %{b}, {}",
                    tag + 1
                );
                let out = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{out} = extractvalue {payload_ty} %{slot}, {at}    ; {}",
                    llvm_type(self.types, fty)
                );
                self.retain(out, fty);
                format!("%{out}")
            }
        }
    }

    fn terminator(&mut self, t: &mir::Terminator, ret: Ty) {
        match t {
            mir::Terminator::Goto(b) => {
                let _ = writeln!(self.out, "  br label %{}", label(*b));
            }
            mir::Terminator::Branch {
                cond,
                then,
                otherwise,
            } => {
                let (c, _) = self.operand(cond);
                let _ = writeln!(
                    self.out,
                    "  br i1 %{c}, label %{}, label %{}",
                    label(*then),
                    label(*otherwise)
                );
            }
            mir::Terminator::Switch {
                scrutinee,
                enumeration,
                arms,
            } => {
                let ty = Ty::Enum(ety(*enumeration));
                let whole = llvm_type(self.types, ty);
                let v = self.load(*scrutinee);
                let tag = self.fresh();
                let _ = writeln!(self.out, "  %{tag} = extractvalue {whole} %{v}, 0");
                // MIR names every variant exactly once, so the default is unreachable for any
                // value this program can build. It is written as the first arm rather than as
                // `unreachable`: a `switch` whose default is poison would make a compiler
                // mistake into undefined behaviour instead of a wrong answer.
                let default = arms
                    .first()
                    .map_or_else(|| "nz.unwind".to_owned(), |(_, b)| label(*b));
                let table = arms
                    .iter()
                    .map(|(v, b)| {
                        format!("i64 {}, label %{}", self.tag(*enumeration, *v), label(*b))
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let _ = writeln!(
                    self.out,
                    "  switch i64 %{tag}, label %{default} [ {table} ]"
                );
            }
            mir::Terminator::Return(l) => {
                let v = self.load(*l);
                let _ = writeln!(self.out, "  ret {} %{v}", llvm_type(self.types, ret));
            }
            // The failure exit: a stand-in value the caller never reads, because its failure
            // check leaves first.
            mir::Terminator::Unwind => {
                let _ = writeln!(
                    self.out,
                    "  ret {} {}",
                    llvm_type(self.types, ret),
                    ret.unwind_value()
                );
            }
            mir::Terminator::Exit(code) => {
                let (c, _) = self.operand(code);
                let narrow = self.fresh();
                let _ = writeln!(self.out, "  %{narrow} = trunc i64 %{c} to i32");
                let _ = writeln!(self.out, "  call void @exit(i32 %{narrow})");
                let _ = writeln!(self.out, "  unreachable");
            }
        }
    }

    /// What this unit reached, for the driver to union with every other unit's.
    fn needs(&self) -> Needs {
        Needs {
            tasks: self.needs_tasks,
            channels: self.needs_channels,
            sequences: self.needs_sequences,
            io: self.needs_io,
            join: self.needs_join,
            strings: self.needs_strings,
        }
    }

    /// Intern a constant and return `(symbol, the number of bytes to write)`.
    ///
    /// The stored array is NUL-terminated because C string constants are, and the length
    /// returned is **the text's**, without it. The two used to be the same number, so
    /// every native failure wrote its terminator to standard error after the newline —
    /// invisible in a terminal, and not the single line `crates/nazm-cli/src/main.rs`
    /// promises a caller. Nothing here treats the constant as NUL-terminated; `@nz.fail`
    /// takes a pointer and a length.
    fn intern(&mut self, text: &str) -> (String, usize) {
        if let Some((sym, _)) = self.strings.iter().find(|(_, t)| t == text) {
            return (sym.clone(), text.len());
        }
        let sym = format!("@.msg{}", self.strings.len());
        self.strings.push((sym.clone(), text.to_owned()));
        (sym, text.len())
    }

    /// Emit the branch that reports `failure` and never returns, then continue after it.
    ///
    /// The failure block ends in `unreachable`, so it is never a predecessor of anything:
    /// a `phi` downstream cannot receive an incoming value from a path that terminated.
    /// The exact bytes a failure prints, interned, as `(symbol, length)`.
    ///
    /// Split out of [`Self::guard`] because a runtime helper that can fail — the one
    /// behind `chan_recv` — has to be *given* its message: it is one function serving
    /// every call site, and the position in the text is the call site's, not its own.
    /// # The file is the span's, not the compilation's
    ///
    /// Until 2026-09-22 this named the file `nazm build` was invoked on, for every
    /// failure in every module. The line and column were already the right ones — they
    /// come from the `SourceMap` and have since N1 — so a division by zero in `lib.nz`
    /// printed `main.nz` with `lib.nz`'s position: a location that points somewhere real
    /// and wrong, which is worse than one that points nowhere. A span knows its file;
    /// asking it is the fix, and it is the same thing the rendered diagnostics do.
    fn failure_text(&mut self, failure: &Failure, span: Span) -> (String, usize) {
        let (line, col) = self.position(span);
        let text = format!(
            "nazm: error[{}]: {} at {}:{line}:{col}\n",
            failure.code,
            failure.message,
            self.names.name(span.file)
        );
        self.intern(&text)
    }

    fn guard(&mut self, cond: u32, failure: &Failure, span: Span) {
        let (sym, len) = self.failure_text(failure, span);
        let fail = self.fresh_label();
        let cont = self.fresh_label();
        let _ = writeln!(self.out, "  br i1 %{cond}, label %b{fail}, label %b{cont}");
        self.open(&format!("b{fail}"));
        let _ = writeln!(self.out, "  call void @nz.fail(ptr {sym}, i64 {len})");
        self.unwind();
        self.open(&format!("b{cont}"));
    }

    /// Join one scope and take on the first failure any of its tasks had.
    ///
    /// `@nz.fail` keeps the first failure a thread records, so adopting a task's here can
    /// never displace one the body already had — which is the precedence `docs/spec.md`
    /// states: a task that failed because the body stopped feeding it is a consequence,
    /// not a cause.
    fn join_one(&mut self, tids: u32) {
        let r = self.fresh();
        let msg = self.fresh();
        let has = self.fresh();
        let take = self.fresh_label();
        let cont = self.fresh_label();
        let _ = writeln!(
            self.out,
            "  %{r} = call {{ ptr, i64 }} @nz.scope_join(ptr %{tids})"
        );
        let _ = writeln!(self.out, "  %{msg} = extractvalue {{ ptr, i64 }} %{r}, 0");
        let _ = writeln!(self.out, "  %{has} = icmp ne ptr %{msg}, null");
        let _ = writeln!(self.out, "  br i1 %{has}, label %b{take}, label %b{cont}");
        self.open(&format!("b{take}"));
        let len = self.fresh();
        let _ = writeln!(self.out, "  %{len} = extractvalue {{ ptr, i64 }} %{r}, 1");
        let _ = writeln!(self.out, "  call void @nz.fail(ptr %{msg}, i64 %{len})");
        let _ = writeln!(self.out, "  br label %b{cont}");
        self.open(&format!("b{cont}"));
        // The list of task ids is allocated by the same allocator as a sequence and is
        // dead the moment the join returns: every task it named has ended. It is not a
        // Nazm value and no binding can hold it, so it is released here rather than
        // through the frame — and it is released on **every** way out of the scope,
        // because `join_through` is what runs on each of them.
        self.release_internal(tids);
    }

    /// `spawn f(args);` — pack the arguments, start the thread, record it for the join.
    ///
    /// # Why the arguments are copied to the heap
    ///
    /// The task outlives this stack frame by construction, so it cannot be handed a
    /// pointer into it. One allocation per spawn carries the whole argument list, and its
    /// first eight bytes are the thread handle — so the list the scope joins holds one
    /// pointer per task and needs no second allocation.
    fn spawn(&mut self, tids: u32, callee: mir::FuncId, vals: &[(u32, Ty)], span: Span) {
        self.needs_sequences = true;
        self.needs_tasks = true;

        let (callee, ret) = self.sigs.get(callee.index()).map_or_else(
            || (NativeSymbol::missing(), Ty::Int),
            |s| (s.symbol.clone(), s.ret),
        );

        // The block is a **named LLVM struct**: the handle, then this task's diagnostic
        // — message pointer, then length — then each argument in its own field.
        //
        // It used to be a hand-computed run of byte offsets, sized by a rule that read
        // "twenty-four for a `Str`, eight for everything else". That rule was true while
        // every type was a pointer or N8's string triple, and a record ends it: a record's
        // size and alignment are the target's business and nothing here can know them. So
        // the struct is declared and LLVM is asked, through a `getelementptr`, both where
        // each field sits and how large the whole thing is.
        //
        // The first three fields are fixed as `{ i64, ptr, i64 }` because the runtime
        // reads the handle at 0 and the diagnostic at 8 and 16 by offset. The diagnostic
        // lives here rather than in a slot the scope owns because each task then writes
        // only to memory nobody else touches, and the join reads it after `pthread_join` —
        // a happens-before edge, so no lock is involved.
        let block_ty = self.task_block(&vals.iter().map(|(_, t)| *t).collect::<Vec<_>>());

        // `end` first: LLVM requires numbered values to appear in increasing order, so
        // the numbers have to be taken in the order the instructions are written.
        let end = self.fresh();
        let size = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{end} = getelementptr {block_ty}, ptr null, i64 1"
        );
        let _ = writeln!(self.out, "  %{size} = ptrtoint ptr %{end} to i64");
        let blk = self.fresh();
        let _ = writeln!(self.out, "  %{blk} = call ptr @malloc(i64 %{size})");
        let null = self.fresh();
        let _ = writeln!(self.out, "  %{null} = icmp eq ptr %{blk}, null");
        self.guard(
            null,
            &Failure {
                code: "N0406",
                message: "out of memory".to_owned(),
            },
            span,
        );

        // `malloc` does not zero, and the join reads this slot for every task it joins —
        // including the ones that succeeded. Left uninitialised it would be whatever the
        // allocator last held there, and a program that never failed would report.
        let em = self.fresh();
        let el = self.fresh();
        let _ = writeln!(self.out, "  %{em} = getelementptr i8, ptr %{blk}, i64 8");
        let _ = writeln!(self.out, "  store ptr null, ptr %{em}");
        let _ = writeln!(self.out, "  %{el} = getelementptr i8, ptr %{blk}, i64 16");
        let _ = writeln!(self.out, "  store i64 0, ptr %{el}");

        // The block **takes** each argument's reference, and the trampoline releases it
        // when the task ends. That is not ceremony: a task outlives the statement that
        // started it, so
        //
        // ```nazm
        // scope { spawn worker(s); s = other(); }
        // ```
        //
        // would otherwise release `s`'s backing while the task was still reading it. The
        // reference is taken here, on the thread that already holds one, and it is the
        // reason a string may cross a task boundary at all.
        for (i, (value, ty)) in vals.iter().enumerate() {
            let slot = self.fresh();
            let _ = writeln!(
                self.out,
                "  %{slot} = getelementptr {block_ty}, ptr %{blk}, i64 0, i32 {}",
                i + TASK_HEADER_FIELDS
            );
            self.retain(*value, *ty);
            let _ = writeln!(
                self.out,
                "  store {} %{value}, ptr %{slot}",
                llvm_type(self.types, *ty)
            );
        }

        let types: Vec<Ty> = vals.iter().map(|(_, t)| *t).collect();
        let trampoline = self.trampoline(&callee, ret, &types, &block_ty);
        let rc = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{rc} = call i32 @nz.spawn(ptr %{tids}, ptr @{trampoline}, ptr %{blk})"
        );
        // A task that never started owns nothing, so what was packed for it is given
        // back before the failure is reported. Without this, the one path where a program
        // is *already* out of resources would be the path that leaked.
        let started = self.fresh();
        let cleanup = self.fresh_label();
        let after = self.fresh_label();
        let _ = writeln!(self.out, "  %{started} = icmp eq i32 %{rc}, 0");
        let _ = writeln!(
            self.out,
            "  br i1 %{started}, label %b{after}, label %b{cleanup}"
        );
        self.open(&format!("b{cleanup}"));
        for (i, (_, ty)) in vals.iter().enumerate() {
            let slot = self.fresh();
            let _ = writeln!(
                self.out,
                "  %{slot} = getelementptr {block_ty}, ptr %{blk}, i64 0, i32 {}",
                i + TASK_HEADER_FIELDS
            );
            let held = self.fresh();
            let _ = writeln!(
                self.out,
                "  %{held} = load {}, ptr %{slot}",
                llvm_type(self.types, *ty)
            );
            self.release(held, *ty);
        }
        let _ = writeln!(self.out, "  call void @free(ptr %{blk})");
        let _ = writeln!(self.out, "  br label %b{after}");
        self.open(&format!("b{after}"));

        let oom = self.fresh();
        let _ = writeln!(self.out, "  %{oom} = icmp eq i32 %{rc}, -1");
        self.guard(
            oom,
            &Failure {
                code: "N0406",
                message: "out of memory".to_owned(),
            },
            span,
        );
        let bad = self.fresh();
        let _ = writeln!(self.out, "  %{bad} = icmp sgt i32 %{rc}, 0");
        self.guard(
            bad,
            &Failure {
                code: "N0404",
                // The interpreter's wording for the same failure: each task is an
                // operating-system thread, and running out of them is a limit of the
                // machine rather than anything about the program.
                message: "this task could not be started".to_owned(),
            },
            span,
        );
    }

    /// The `void *(void *)` function a thread actually starts in.
    ///
    /// `pthread_create` takes one pointer, so every spawn site needs a small function
    /// that unpacks its own argument block and calls the Nazm function with the types
    /// that site established. One per site rather than one per callee: two spawns of the
    /// same function are the same trampoline only if their argument types match, and
    /// generating per site makes that true without having to check it.
    fn trampoline(
        &mut self,
        callee: &NativeSymbol,
        ret: Ty,
        types: &[Ty],
        block_ty: &str,
    ) -> String {
        let name = format!("nz.task{}", self.taskno());
        let mut body = String::new();
        let _ = writeln!(&mut body, "\ndefine internal ptr @{name}(ptr %a) {{");
        let _ = writeln!(&mut body, "entry:");
        let mut passed = Vec::new();
        for (i, ty) in types.iter().enumerate() {
            let _ = writeln!(
                &mut body,
                "  %p{i} = getelementptr {block_ty}, ptr %a, i64 0, i32 {}",
                i + TASK_HEADER_FIELDS
            );
            let _ = writeln!(
                &mut body,
                "  %v{i} = load {}, ptr %p{i}",
                llvm_type(self.types, *ty)
            );
            passed.push(format!("{} %v{i}", llvm_type(self.types, *ty)));
        }
        // The callee **borrows** these, exactly as an ordinary call does: the reference
        // the spawn site packed is what keeps them alive, and it is given back below,
        // once the call has returned. Whether the task succeeded or failed does not
        // change that, so the release is on the one path both take.
        for ty in types {
            match ty.owns() {
                None => {}
                Some(Owns::Ints) => self.needs_sequences = true,
                Some(Owns::Strs) => {
                    self.needs_sequences = true;
                    self.needs_strings = true;
                }
                Some(Owns::Backing) => self.needs_strings = true,
                Some(Owns::Channel) => self.needs_channels = true,
                // Nothing: a record's or an enum's runtime is its fields', and the helper
                // generated below is what reaches them. Setting flags from a partial walk
                // here would be a second answer to the same question.
                Some(Owns::Record(_) | Owns::Enum(_)) => {}
                // Never reached: a `Vec` may not cross into a task (`N0321`), and the
                // lowering refuses one by name as well.
                Some(Owns::Vec(_)) => self.needs_sequences = true,
            }
        }
        // The task's return value is discarded, which `docs/spec.md` says outright:
        // results travel through channels, so there is one mechanism rather than two.
        let ret = llvm_type(self.types, ret);
        let _ = writeln!(
            &mut body,
            "  %r = call {ret} {callee}({})",
            passed.join(", ")
        );
        // A failure ends the *task*, not the process. The diagnostic is left in this
        // task's own argument block, where the scope's join reads it after
        // `pthread_join` — so the failure is reported by the thread that owns the scope,
        // once every sibling has finished, rather than by whichever thread happened to
        // fail first while the others were still working.
        let _ = writeln!(&mut body, "  %failed = load i1, ptr @nz.failed");
        let _ = writeln!(
            &mut body,
            "  br i1 %failed, label %publish, label %finished"
        );
        let _ = writeln!(&mut body, "publish:");
        let _ = writeln!(&mut body, "  %em = load ptr, ptr @nz.errmsg");
        let _ = writeln!(&mut body, "  %el = load i64, ptr @nz.errlen");
        let _ = writeln!(&mut body, "  %emp = getelementptr i8, ptr %a, i64 8");
        let _ = writeln!(&mut body, "  store ptr %em, ptr %emp");
        let _ = writeln!(&mut body, "  %elp = getelementptr i8, ptr %a, i64 16");
        let _ = writeln!(&mut body, "  store i64 %el, ptr %elp");
        let _ = writeln!(&mut body, "  br label %finished");
        let _ = writeln!(&mut body, "finished:");
        for (i, ty) in types.iter().enumerate() {
            match ty.owns() {
                None => {}
                Some(Owns::Ints) => {
                    let _ = writeln!(&mut body, "  call void @nz.seq_release(ptr %v{i})");
                }
                Some(Owns::Strs) => {
                    let _ = writeln!(&mut body, "  call void @nz.strs_release(ptr %v{i})");
                }
                Some(Owns::Backing) => {
                    let _ = writeln!(
                        &mut body,
                        "  %o{i} = extractvalue {{ ptr, i64, ptr }} %v{i}, 2"
                    );
                    let _ = writeln!(&mut body, "  call void @nz.str_release(ptr %o{i})");
                }
                Some(Owns::Channel) => {
                    let _ = writeln!(&mut body, "  call void @nz.chan_release(ptr %v{i})");
                }
                // The same helper the ordinary release path calls, which is why a record
                // or an enum crossing a task boundary needs nothing written here. For an
                // enum that means the task releases the **active** variant's payload and
                // nothing else, by the same branch every other release takes.
                Some(Owns::Record(id)) => {
                    if let Some(helper) = self.record_helper(id, false) {
                        let t = llvm_type(self.types, *ty);
                        let _ = writeln!(&mut body, "  call void @{helper}({t} %v{i})");
                    }
                }
                Some(Owns::Enum(id)) => {
                    if let Some(helper) = self.enum_helper(id, false) {
                        let t = llvm_type(self.types, *ty);
                        let _ = writeln!(&mut body, "  call void @{helper}({t} %v{i})");
                    }
                }
                // Never reached, for the reason above; released correctly if it were.
                Some(Owns::Vec(id)) => {
                    let helper = self
                        .vec_helper(id)
                        .unwrap_or_else(|| "nz.seq_release".to_owned());
                    let _ = writeln!(&mut body, "  call void @{helper}(ptr %v{i})");
                }
            }
        }
        let _ = writeln!(&mut body, "  ret ptr null");
        let _ = writeln!(&mut body, "}}");
        self.tasks.push_str(&body);
        name
    }

    /// The named LLVM type of a task's argument block, declared on first use.
    ///
    /// `{ i64 handle, ptr message, i64 length, … arguments }`. The first three are fixed
    /// and must stay so: `@nz.spawn` writes the thread handle at offset 0 and the join
    /// reads the diagnostic at 8 and 16, by offset, from the runtime's own C.
    fn task_block(&mut self, types: &[Ty]) -> String {
        if let Some((_, name)) = self.blocks.iter().find(|(t, _)| t == types) {
            return name.clone();
        }
        let name = format!("%nz.blk{}", self.blocks.len());
        let fields = std::iter::once("i64".to_owned())
            .chain(std::iter::once("ptr".to_owned()))
            .chain(std::iter::once("i64".to_owned()))
            .chain(types.iter().map(|t| llvm_type(self.types, *t)))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(self.block_types, "{name} = type {{ {fields} }}");
        self.blocks.push((types.to_vec(), name.clone()));
        name
    }

    fn taskno(&mut self) -> usize {
        let n = self.task_count;
        self.task_count += 1;
        n
    }

    /// The size in bytes of one element of this type, as an SSA value.
    ///
    /// Asked of LLVM rather than computed here, by the address of element 1 of an array at
    /// null — the same question the task machinery asks of an argument block. Semantic
    /// stages know no byte size (`docs/architecture.md` §7.10), and neither does this one:
    /// the target's layout answers it.
    fn stride(&mut self, elem: Ty) -> u32 {
        let t = llvm_type(self.types, elem);
        let at = self.fresh();
        let _ = writeln!(self.out, "  %{at} = getelementptr {t}, ptr null, i64 1");
        let size = self.fresh();
        let _ = writeln!(self.out, "  %{size} = ptrtoint ptr %{at} to i64");
        size
    }

    /// One `Vec[T]` operation, with its arguments already evaluated.
    ///
    /// The header is the one every sequence has — `{ i64 len, i64 cap, ptr data, i64 rc }`
    /// from `@nz.seq_new` — so a `Vec` is reclaimed, counted and grown by the runtime that
    /// already reclaims, counts and grows an `Ints`. What is `T`'s is the element's layout,
    /// taken from LLVM, and what an element owns, taken from `T`'s own law.
    fn vec_op(&mut self, op: Builtin, vec: VecTy, elem: Ty, vals: &[u32], span: Span) -> Val {
        let arg = |i: usize| vals.get(i).copied().unwrap_or_default();
        let t = llvm_type(self.types, elem);
        self.needs_sequences = true;
        match op {
            Builtin::VecNew => {
                let h = self.fresh();
                let _ = writeln!(self.out, "  %{h} = call ptr @nz.seq_new()");
                let null = self.fresh();
                let _ = writeln!(self.out, "  %{null} = icmp eq ptr %{h}, null");
                self.guard(
                    null,
                    &Failure {
                        code: "N0406",
                        message: "out of memory".to_owned(),
                    },
                    span,
                );
                Val::Of {
                    value: h,
                    ty: Ty::Vec(vec),
                }
            }
            Builtin::VecLen => {
                let n = self.fresh();
                let _ = writeln!(self.out, "  %{n} = load i64, ptr %{}", arg(0));
                Val::Of {
                    value: n,
                    ty: Ty::Int,
                }
            }
            Builtin::VecGet => {
                let slot = self.sequence_slot(arg(0), arg(1), elem, span);
                let v = self.fresh();
                let _ = writeln!(self.out, "  %{v} = load {t}, ptr %{slot}");
                // **Its own copy**, by `T`'s law, before anything else can happen to the
                // slot: the element may be overwritten, popped or released the moment this
                // expression is done, and what it returned must not care. N8's `strs_get`
                // lesson, generic.
                if elem.owns().is_some() {
                    self.retain(v, elem);
                }
                Val::Of { value: v, ty: elem }
            }
            Builtin::VecSet => {
                let slot = self.sequence_slot(arg(0), arg(1), elem, span);
                // The order is the correctness: the new value's reference is taken
                // **before** the old one is released, so storing a copy of the element
                // itself — `vec_set(xs, 0, vec_get(xs, 0))` — cannot drop a count to zero
                // in between.
                let old = self.fresh();
                let _ = writeln!(self.out, "  %{old} = load {t}, ptr %{slot}");
                self.retain(arg(2), elem);
                let _ = writeln!(self.out, "  store {t} %{}, ptr %{slot}", arg(2));
                self.release(old, elem);
                Val::Of {
                    value: arg(1),
                    ty: Ty::Int,
                }
            }
            Builtin::VecPush => {
                // Growth first, and nothing taken until it has succeeded: a failed growth
                // leaves the vector as it was and the caller's value untouched. The
                // runtime relocates the elements with `realloc` — a move of storage, not a
                // copy of values, so no element's count changes (`docs/architecture.md`
                // §7.12 says why every current value may be moved by its bytes).
                let size = self.stride(elem);
                let data = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{data} = call ptr @nz.arr_grow(ptr %{}, i64 %{size})",
                    arg(0)
                );
                let null = self.fresh();
                let _ = writeln!(self.out, "  %{null} = icmp eq ptr %{data}, null");
                self.guard(
                    null,
                    &Failure {
                        code: "N0406",
                        message: "out of memory".to_owned(),
                    },
                    span,
                );
                let len = self.fresh();
                let _ = writeln!(self.out, "  %{len} = load i64, ptr %{}", arg(0));
                let slot = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{slot} = getelementptr {t}, ptr %{data}, i64 %{len}"
                );
                // The vector keeps what it is given, so it takes a copy of its own; the
                // caller's value is untouched, which is rule 2.
                self.retain(arg(1), elem);
                let _ = writeln!(self.out, "  store {t} %{}, ptr %{slot}", arg(1));
                let grown = self.fresh();
                let _ = writeln!(self.out, "  %{grown} = add i64 %{len}, 1");
                let _ = writeln!(self.out, "  store i64 %{grown}, ptr %{}", arg(0));
                Val::Of {
                    value: grown,
                    ty: Ty::Int,
                }
            }
            Builtin::VecPop => {
                let len = self.fresh();
                let _ = writeln!(self.out, "  %{len} = load i64, ptr %{}", arg(0));
                let empty = self.fresh();
                let _ = writeln!(self.out, "  %{empty} = icmp sle i64 %{len}, 0");
                self.require(empty, "this sequence is empty", span);
                let last = self.fresh();
                let _ = writeln!(self.out, "  %{last} = sub i64 %{len}, 1");
                let data_field = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{data_field} = getelementptr i8, ptr %{}, i64 16",
                    arg(0)
                );
                let data = self.fresh();
                let _ = writeln!(self.out, "  %{data} = load ptr, ptr %{data_field}");
                let slot = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{slot} = getelementptr {t}, ptr %{data}, i64 %{last}"
                );
                let v = self.fresh();
                let _ = writeln!(self.out, "  %{v} = load {t}, ptr %{slot}");
                let _ = writeln!(self.out, "  store i64 %{last}, ptr %{}", arg(0));
                // A **transfer**: the vector gives up its reference to the element and MIR's
                // destination holds that same one. No retain and no release.
                Val::Of { value: v, ty: elem }
            }
            // Only the `Vec` built-ins are lowered to this operation.
            _ => {
                let _ = writeln!(self.out, "  unreachable");
                Val::Transferred
            }
        }
    }

    /// One built-in, with its arguments already evaluated.
    fn builtin_op(&mut self, op: Builtin, vals: &[u32], span: Span) -> Val {
        let arg = |i: usize| vals.get(i).copied().unwrap_or_default();

        let value = match op {
            Builtin::StrLen => {
                let (_, len) = self.str_parts(arg(0));
                return Val::Of {
                    value: len,
                    ty: Ty::Int,
                };
            }
            Builtin::StrByte => {
                let (ptr, len) = self.str_parts(arg(0));
                let i = arg(1);
                let neg = self.fresh();
                let _ = writeln!(self.out, "  %{neg} = icmp slt i64 %{i}, 0");
                let past = self.fresh();
                let _ = writeln!(self.out, "  %{past} = icmp sge i64 %{i}, %{len}");
                let bad = self.fresh();
                let _ = writeln!(self.out, "  %{bad} = or i1 %{neg}, %{past}");
                self.require(bad, "this byte index is outside the `Str`", span);
                let at = self.fresh();
                let _ = writeln!(self.out, "  %{at} = getelementptr i8, ptr %{ptr}, i64 %{i}");
                let byte = self.fresh();
                let _ = writeln!(self.out, "  %{byte} = load i8, ptr %{at}");
                let wide = self.fresh();
                // Zero-extended: a byte is `0..=255`, never negative. `sext` here would
                // make every byte above 127 a negative `Int`.
                let _ = writeln!(self.out, "  %{wide} = zext i8 %{byte} to i64");
                return Val::Of {
                    value: wide,
                    ty: Ty::Int,
                };
            }
            Builtin::StrSlice => {
                let (ptr, len) = self.str_parts(arg(0));
                let (from, to) = (arg(1), arg(2));
                let neg = self.fresh();
                let _ = writeln!(self.out, "  %{neg} = icmp slt i64 %{from}, 0");
                let inverted = self.fresh();
                let _ = writeln!(self.out, "  %{inverted} = icmp slt i64 %{to}, %{from}");
                let past = self.fresh();
                let _ = writeln!(self.out, "  %{past} = icmp sgt i64 %{to}, %{len}");
                let a = self.fresh();
                let _ = writeln!(self.out, "  %{a} = or i1 %{neg}, %{inverted}");
                let bad = self.fresh();
                let _ = writeln!(self.out, "  %{bad} = or i1 %{a}, %{past}");
                self.require(bad, "this slice is not inside the `Str`", span);
                // No copy: a slice is a pointer, a length, and **the same owner**. That
                // last field is the whole of N8's slice rule — the view keeps the
                // allocation it was cut from alive, so `str_slice` of a heap string
                // outliving the string it came from is ordinary rather than a dangling
                // pointer (`docs/architecture.md` §7.8).
                let start = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{start} = getelementptr i8, ptr %{ptr}, i64 %{from}"
                );
                let n = self.fresh();
                // Cannot overflow: the guard above establishes `0 <= from <= to`.
                let _ = writeln!(self.out, "  %{n} = sub i64 %{to}, %{from}");
                let owner = self.str_owner(arg(0));
                // Retained here rather than left to whoever binds the result, because the
                // argument is released as a temporary the moment this built-in returns:
                // `str_slice(str_concat(a, b), 1, 2)` would otherwise free the backing
                // its own result points into.
                let _ = writeln!(self.out, "  call void @nz.str_retain(ptr %{owner})");
                self.needs_strings = true;
                let out = self.make_str(start, n, &format!("%{owner}"));
                out
            }
            Builtin::StrConcat => {
                let (pa, la) = self.str_parts(arg(0));
                let (pb, lb) = self.str_parts(arg(1));
                let n = self.fresh();
                // Two lengths that each fit in memory cannot sum past `Int`, so this is
                // a plain add rather than a checked one.
                let _ = writeln!(self.out, "  %{n} = add i64 %{la}, %{lb}");
                // One byte more than needed, so an empty result still owns storage and
                // its pointer stays valid to compare.
                let room = self.fresh();
                let _ = writeln!(self.out, "  %{room} = add i64 %{n}, 1");
                let (owner, buf) = self.string_backing(room, span);
                let _ = writeln!(
                    self.out,
                    "  call void @llvm.memcpy.p0.p0.i64(ptr %{buf}, ptr %{pa}, i64 %{la}, i1 false)"
                );
                let rest = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{rest} = getelementptr i8, ptr %{buf}, i64 %{la}"
                );
                let _ = writeln!(
                    self.out,
                    "  call void @llvm.memcpy.p0.p0.i64(ptr %{rest}, ptr %{pb}, i64 %{lb}, i1 false)"
                );
                let out = self.make_str(buf, n, &format!("%{owner}"));
                out
            }
            // --- the outside world ------------------------------------------------
            //
            // Ambient: nothing in the language restricts which functions may reach the
            // file system. `docs/spec.md` says so plainly rather than implying a
            // capability system that is not here.
            Builtin::ArgsCount => {
                self.needs_io = true;
                let argc = self.fresh();
                let _ = writeln!(self.out, "  %{argc} = load i64, ptr @nz.argc");
                return Val::Of {
                    value: argc,
                    ty: Ty::Int,
                };
            }
            Builtin::Arg => {
                self.needs_io = true;
                let i = arg(0);
                let argc = self.fresh();
                let _ = writeln!(self.out, "  %{argc} = load i64, ptr @nz.argc");
                let neg = self.fresh();
                let _ = writeln!(self.out, "  %{neg} = icmp slt i64 %{i}, 0");
                let past = self.fresh();
                let _ = writeln!(self.out, "  %{past} = icmp sge i64 %{i}, %{argc}");
                let bad = self.fresh();
                let _ = writeln!(self.out, "  %{bad} = or i1 %{neg}, %{past}");
                self.require(bad, "this index is outside the arguments", span);
                // `argv[0]` is the program's own name and is deliberately not visible:
                // it is a path nobody should depend on, and excluding it is what makes
                // `arg(i)` mean the same thing interpreted and compiled.
                let shifted = self.fresh();
                let _ = writeln!(self.out, "  %{shifted} = add i64 %{i}, 1");
                let base = self.fresh();
                let _ = writeln!(self.out, "  %{base} = load ptr, ptr @nz.argv");
                let slot = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{slot} = getelementptr ptr, ptr %{base}, i64 %{shifted}"
                );
                let text = self.fresh();
                let _ = writeln!(self.out, "  %{text} = load ptr, ptr %{slot}");
                let len = self.fresh();
                let _ = writeln!(self.out, "  %{len} = call i64 @strlen(ptr %{text})");
                // **External.** The bytes are the process's, handed over by the operating
                // system and outliving every value in the program. Null owner: there is
                // nothing this program may free and nothing it has to keep alive.
                self.make_str(text, len, "null")
            }
            Builtin::Print | Builtin::EPrint => {
                let (ptr, len) = self.str_parts(arg(0));
                let fd = if op == Builtin::Print { 1 } else { 2 };
                let written = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{written} = call i64 @write(i32 {fd}, ptr %{ptr}, i64 %{len})"
                );
                return Val::Of {
                    value: written,
                    ty: Ty::Int,
                };
            }
            Builtin::ExitWith => {
                let narrow = self.fresh();
                let _ = writeln!(self.out, "  %{narrow} = trunc i64 %{} to i32", arg(0));
                let _ = writeln!(self.out, "  call void @exit(i32 %{narrow})");
                let _ = writeln!(self.out, "  unreachable");
                // The process is over, so there is no value and none is invented. This
                // is the one built-in that transfers, and the emitter already has the
                // vocabulary for that.
                return Val::Transferred;
            }
            Builtin::ReadFile => {
                self.needs_io = true;
                let (ptr, len) = self.str_parts(arg(0));
                let out = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{out} = call {{ ptr, i64, ptr }} @nz.read_file(ptr %{ptr}, i64 %{len})"
                );
                let got = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{got} = extractvalue {{ ptr, i64, ptr }} %{out}, 0"
                );
                let bad = self.fresh();
                let _ = writeln!(self.out, "  %{bad} = icmp eq ptr %{got}, null");
                self.guard(
                    bad,
                    &Failure {
                        code: "N0407",
                        message: "this file could not be read".to_owned(),
                    },
                    span,
                );
                // A file's contents are the program's own storage: one allocation, one
                // reference, held by this expression.
                return Val::Of {
                    value: out,
                    ty: Ty::Str,
                };
            }
            Builtin::WriteFile => {
                self.needs_io = true;
                let (pp, pn) = self.str_parts(arg(0));
                let (dp, dn) = self.str_parts(arg(1));
                let n = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{n} = call i64 @nz.write_file(ptr %{pp}, i64 %{pn}, ptr %{dp}, i64 %{dn})"
                );
                let bad = self.fresh();
                let _ = writeln!(self.out, "  %{bad} = icmp slt i64 %{n}, 0");
                self.guard(
                    bad,
                    &Failure {
                        code: "N0407",
                        message: "this file could not be written".to_owned(),
                    },
                    span,
                );
                return Val::Of {
                    value: n,
                    ty: Ty::Int,
                };
            }
            Builtin::FileExists => {
                self.needs_io = true;
                let (pp, pn) = self.str_parts(arg(0));
                let yes = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{yes} = call i1 @nz.file_exists(ptr %{pp}, i64 %{pn})"
                );
                return Val::Of {
                    value: yes,
                    ty: Ty::Bool,
                };
            }

            // --- sequences -------------------------------------------------------
            //
            // The handle is a pointer to a `{ i64 len, i64 cap, ptr data }` header, so
            // two bindings of one sequence are the same header and a push through
            // either is visible through both. Growth rewrites the header in place;
            // the handle never moves.
            Builtin::IntsNew | Builtin::StrsNew => {
                self.needs_sequences = true;
                let h = self.fresh();
                let _ = writeln!(self.out, "  %{h} = call ptr @nz.seq_new()");
                let null = self.fresh();
                let _ = writeln!(self.out, "  %{null} = icmp eq ptr %{h}, null");
                self.guard(
                    null,
                    &Failure {
                        code: "N0406",
                        message: "out of memory".to_owned(),
                    },
                    span,
                );
                // Born with one reference, and this expression is holding it. Whatever
                // consumes the expression takes it over; a discard releases it.
                return Val::Of {
                    value: h,
                    ty: op.lowered_ret(),
                };
            }
            Builtin::IntsLen | Builtin::StrsLen => {
                let n = self.fresh();
                // `len` is the header's first field, so the handle points straight at it.
                let _ = writeln!(self.out, "  %{n} = load i64, ptr %{}", arg(0));
                return Val::Of {
                    value: n,
                    ty: Ty::Int,
                };
            }
            Builtin::IntsGet | Builtin::StrsGet => {
                let elem = op.sequence().and_then(Ty::element).unwrap_or(Ty::Int);
                let slot = self.sequence_slot(arg(0), arg(1), elem, span);
                let v = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{v} = load {}, ptr %{slot}",
                    llvm_type(self.types, elem)
                );
                // What comes out of a `Strs` gets **its own** reference to the backing.
                // That is what makes `docs/spec.md`'s promise — a `Str` already read out
                // of a sequence is unaffected by changing the sequence — true by
                // construction rather than true because nothing was ever freed. Before
                // N8 it was the second of those.
                if elem.owns().is_some() {
                    self.retain(v, elem);
                }
                return Val::Of { value: v, ty: elem };
            }
            Builtin::IntsSet | Builtin::StrsSet => {
                let elem = op.sequence().and_then(Ty::element).unwrap_or(Ty::Int);
                let slot = self.sequence_slot(arg(0), arg(1), elem, span);
                // The order is the correctness. The new element's reference is taken
                // **before** the old one is released, so replacing an element with a
                // string that shares its backing — `strs_set(xs, 0, strs_get(xs, 0))` —
                // cannot drop the count to zero in between.
                let old = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{old} = load {}, ptr %{slot}",
                    llvm_type(self.types, elem)
                );
                self.retain(arg(2), elem);
                let _ = writeln!(
                    self.out,
                    "  store {} %{}, ptr %{slot}",
                    llvm_type(self.types, elem),
                    arg(2)
                );
                self.release(old, elem);
                // The index written. The language has no unit type, and the index is
                // the one answer that is always meaningful.
                return Val::Of {
                    value: arg(1),
                    ty: Ty::Int,
                };
            }
            Builtin::StrJoin => {
                self.needs_sequences = true;
                self.needs_join = true;
                let (sp, sn) = self.str_parts(arg(1));
                let out = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{out} = call {{ ptr, i64, ptr }} @nz.str_join(ptr %{}, ptr %{sp}, \
                     i64 %{sn})",
                    arg(0)
                );
                let got = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{got} = extractvalue {{ ptr, i64, ptr }} %{out}, 0"
                );
                let bad = self.fresh();
                let _ = writeln!(self.out, "  %{bad} = icmp eq ptr %{got}, null");
                self.guard(
                    bad,
                    &Failure {
                        code: "N0406",
                        message: "out of memory".to_owned(),
                    },
                    span,
                );
                // One allocation, one reference, and this expression is holding it.
                return Val::Of {
                    value: out,
                    ty: Ty::Str,
                };
            }
            // A channel is a pointer to a runtime header holding a bounded queue, a
            // mutex and a condition variable. Every one of these four is a
            // synchronisation point, which is why a `Chan` may cross into a task and a
            // sequence may not (`docs/spec.md`, *Structured concurrency*).
            Builtin::ChanNew => {
                self.needs_channels = true;
                let bad = self.fresh();
                let _ = writeln!(self.out, "  %{bad} = icmp slt i64 %{}, 1", arg(0));
                self.require(bad, "a channel needs a capacity of at least 1", span);
                let c = self.fresh();
                let _ = writeln!(self.out, "  %{c} = call ptr @nz.chan_new(i64 %{})", arg(0));
                let null = self.fresh();
                let _ = writeln!(self.out, "  %{null} = icmp eq ptr %{c}, null");
                self.guard(
                    null,
                    &Failure {
                        code: "N0406",
                        message: "out of memory".to_owned(),
                    },
                    span,
                );
                // Born with one reference, held by this expression — the same rule a
                // sequence follows. What consumes the expression takes it over, and a
                // channel nothing binds is released as an ordinary temporary.
                return Val::Of {
                    value: c,
                    ty: Ty::Chan,
                };
            }
            Builtin::ChanSend => {
                self.needs_channels = true;
                let ok = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{ok} = call i1 @nz.chan_send(ptr %{}, i64 %{})",
                    arg(0),
                    arg(1)
                );
                return Val::Of {
                    value: ok,
                    ty: Ty::Bool,
                };
            }
            // The received value is pushed onto the sequence by the helper rather than
            // returned and pushed here, because the push happens only when there **was**
            // a value and a call site cannot branch without a block of its own. The
            // failure text is passed in: the helper is shared and the position is not.
            Builtin::ChanRecv => {
                self.needs_channels = true;
                self.needs_sequences = true;
                let (sym, len) = self.failure_text(
                    &Failure {
                        code: "N0406",
                        message: "out of memory".to_owned(),
                    },
                    span,
                );
                let ok = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{ok} = call i1 @nz.chan_recv(ptr %{}, ptr %{}, ptr {sym}, i64 {len})",
                    arg(0),
                    arg(1)
                );
                // The helper reports a failed allocation by recording it and returning
                // `false`, which is otherwise how it says the channel is closed and
                // drained. Without this check `while chan_recv(c, buf)` would read an
                // allocation failure as an orderly end and carry on.
                self.check_failed();
                return Val::Of {
                    value: ok,
                    ty: Ty::Bool,
                };
            }
            Builtin::ChanClose => {
                self.needs_channels = true;
                let _ = writeln!(self.out, "  call void @nz.chan_close(ptr %{})", arg(0));
                // The language has no unit type and `chan_close` is specified to produce
                // `Int`. Zero, materialised rather than invented: a constant cannot be
                // returned as an SSA value without an instruction to hold it.
                let z = self.fresh();
                let _ = writeln!(self.out, "  %{z} = add i64 0, 0");
                return Val::Of {
                    value: z,
                    ty: Ty::Int,
                };
            }
            Builtin::IntsPop | Builtin::StrsPop => {
                let seq = op.sequence().unwrap_or(Ty::Ints);
                let elem = seq.element().unwrap_or(Ty::Int);
                let len = self.fresh();
                let _ = writeln!(self.out, "  %{len} = load i64, ptr %{}", arg(0));
                let empty = self.fresh();
                let _ = writeln!(self.out, "  %{empty} = icmp sle i64 %{len}, 0");
                self.require(empty, "this sequence is empty", span);
                let last = self.fresh();
                let _ = writeln!(self.out, "  %{last} = sub i64 %{len}, 1");
                let data_field = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{data_field} = getelementptr i8, ptr %{}, i64 16",
                    arg(0)
                );
                let data = self.fresh();
                let _ = writeln!(self.out, "  %{data} = load ptr, ptr %{data_field}");
                let slot = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{slot} = getelementptr {}, ptr %{data}, i64 %{last}",
                    llvm_type(self.types, elem)
                );
                let v = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{v} = load {}, ptr %{slot}",
                    llvm_type(self.types, elem)
                );
                // The length is written back *after* the value is read, so the item is
                // still inside the sequence while it is being fetched. Capacity is left
                // alone: a stack that shrinks and grows should not reallocate.
                let _ = writeln!(self.out, "  store i64 %{last}, ptr %{}", arg(0));
                // A **transfer**, not a copy: shortening the sequence gives up its
                // reference to the element, and this expression takes that same one over.
                // No retain, no release, and the count is unchanged — which is what makes
                // popping in a loop cost nothing.
                return Val::Of { value: v, ty: elem };
            }
            Builtin::IntsPush | Builtin::StrsPush => {
                self.needs_sequences = true;
                let seq = op.sequence().unwrap_or(Ty::Ints);
                let elem = seq.element().unwrap_or(Ty::Int);
                let size = seq.element_size().unwrap_or(8);
                let data = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{data} = call ptr @nz.arr_grow(ptr %{}, i64 {size})",
                    arg(0)
                );
                let null = self.fresh();
                let _ = writeln!(self.out, "  %{null} = icmp eq ptr %{data}, null");
                self.guard(
                    null,
                    &Failure {
                        code: "N0406",
                        message: "out of memory".to_owned(),
                    },
                    span,
                );
                let len = self.fresh();
                let _ = writeln!(self.out, "  %{len} = load i64, ptr %{}", arg(0));
                let slot = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{slot} = getelementptr {}, ptr %{data}, i64 %{len}",
                    llvm_type(self.types, elem)
                );
                // The sequence keeps what it is given, so it takes a reference of its
                // own. The caller's value is untouched and is still valid after the call,
                // which is `docs/spec.md`'s rule 2 — a built-in borrows its arguments —
                // combined with the fact that this one *stores* one.
                self.retain(arg(1), elem);
                let _ = writeln!(
                    self.out,
                    "  store {} %{}, ptr %{slot}",
                    llvm_type(self.types, elem),
                    arg(1)
                );
                let grown = self.fresh();
                let _ = writeln!(self.out, "  %{grown} = add i64 %{len}, 1");
                let _ = writeln!(self.out, "  store i64 %{grown}, ptr %{}", arg(0));
                return Val::Of {
                    value: grown,
                    ty: Ty::Int,
                };
            }
            Builtin::StrFromByte => {
                let b = arg(0);
                let low = self.fresh();
                let _ = writeln!(self.out, "  %{low} = icmp slt i64 %{b}, 0");
                let high = self.fresh();
                let _ = writeln!(self.out, "  %{high} = icmp sgt i64 %{b}, 255");
                let bad = self.fresh();
                let _ = writeln!(self.out, "  %{bad} = or i1 %{low}, %{high}");
                self.require(bad, "this is not a byte value", span);
                let room = self.fresh();
                let _ = writeln!(self.out, "  %{room} = add i64 1, 0");
                let (owner, buf) = self.string_backing(room, span);
                let narrow = self.fresh();
                let _ = writeln!(self.out, "  %{narrow} = trunc i64 %{b} to i8");
                let _ = writeln!(self.out, "  store i8 %{narrow}, ptr %{buf}");
                let one = self.fresh();
                let _ = writeln!(self.out, "  %{one} = add i64 1, 0");
                let out = self.make_str(buf, one, &format!("%{owner}"));
                out
            }
            // The generic built-ins are lowered to `Expr::Vec` and never reach here: their
            // types are the site's, and this function has none.
            Builtin::VecNew
            | Builtin::VecLen
            | Builtin::VecGet
            | Builtin::VecSet
            | Builtin::VecPush
            | Builtin::VecPop => {
                let _ = writeln!(self.out, "  unreachable");
                return Val::Transferred;
            }
            Builtin::IntToStr => {
                // 20 digits is the widest `Int` (`-9223372036854775808`), plus the NUL
                // `snprintf` insists on writing.
                let room = self.fresh();
                let _ = writeln!(self.out, "  %{room} = add i64 21, 0");
                let (owner, buf) = self.string_backing(room, span);
                let written = self.fresh();
                let _ = writeln!(
                    self.out,
                    "  %{written} = call i32 (ptr, i64, ptr, ...) @snprintf(ptr %{buf}, \
                     i64 21, ptr @.dec, i64 %{})",
                    arg(0)
                );
                let n = self.fresh();
                let _ = writeln!(self.out, "  %{n} = sext i32 %{written} to i64");
                let out = self.make_str(buf, n, &format!("%{owner}"));
                out
            }
        };
        Val::Of { value, ty: Ty::Str }
    }

    /// `==` and `!=` on two `Str`s: equal lengths, then equal bytes.
    ///
    /// The length test guards the comparison rather than merely preceding it — `memcmp`
    /// is only reached when both strings really are that long, so no path reads past
    /// either buffer. Emitted as a branch and a `phi` for exactly that reason; an `and`
    /// of two computed values would have evaluated the `memcmp` first.
    fn string_equality(&mut self, a: u32, b: u32, negate: bool) -> u32 {
        let (pa, la) = self.str_parts(a);
        let (pb, lb) = self.str_parts(b);
        let same_len = self.fresh();
        let _ = writeln!(self.out, "  %{same_len} = icmp eq i64 %{la}, %{lb}");

        let compare = format!("b{}", self.fresh_label());
        let join = format!("b{}", self.fresh_label());
        let short = self.current.clone();
        let _ = writeln!(
            self.out,
            "  br i1 %{same_len}, label %{compare}, label %{join}"
        );

        self.open(&compare);
        let diff = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{diff} = call i32 @memcmp(ptr %{pa}, ptr %{pb}, i64 %{la})"
        );
        let equal = self.fresh();
        let _ = writeln!(self.out, "  %{equal} = icmp eq i32 %{diff}, 0");
        let from_compare = self.current.clone();
        let _ = writeln!(self.out, "  br label %{join}");

        self.open(&join);
        let out = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{out} = phi i1 [ false, %{short} ], [ %{equal}, %{from_compare} ]"
        );
        if !negate {
            return out;
        }
        let flipped = self.fresh();
        let _ = writeln!(self.out, "  %{flipped} = xor i1 %{out}, true");
        flipped
    }

    fn checked(&mut self, op: ArithOp, a: u32, b: u32, span: Span) -> u32 {
        let intrinsic = match op {
            ArithOp::Add => "sadd",
            ArithOp::Sub => "ssub",
            ArithOp::Mul => "smul",
            // Division is guarded by comparison, not by an intrinsic: there is no
            // `sdiv.with.overflow`.
            ArithOp::Div | ArithOp::Rem => unreachable!("guarded separately"),
        };
        let r = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{r} = call {{i64, i1}} @llvm.{intrinsic}.with.overflow.i64(i64 %{a}, i64 %{b})"
        );
        let ov = self.fresh();
        let _ = writeln!(self.out, "  %{ov} = extractvalue {{i64, i1}} %{r}, 1");
        self.guard(
            ov,
            &Failure {
                code: "N0400",
                message: format!("this {} overflows `Int`", op.overflow_noun()),
            },
            span,
        );
        let out = self.fresh();
        let _ = writeln!(self.out, "  %{out} = extractvalue {{i64, i1}} %{r}, 0");
        out
    }

    /// Both of division's failures, in the order the interpreter checks them.
    fn divide(&mut self, a: u32, b: u32, span: Span) -> u32 {
        let zero = self.fresh();
        let _ = writeln!(self.out, "  %{zero} = icmp eq i64 %{b}, 0");
        self.guard(
            zero,
            &Failure {
                code: "N0401",
                message: "division by zero".to_owned(),
            },
            span,
        );

        let overflows = self.min_over_minus_one(a, b);
        self.guard(
            overflows,
            &Failure {
                code: "N0400",
                message: "this division overflows `Int`".to_owned(),
            },
            span,
        );

        let out = self.fresh();
        let _ = writeln!(self.out, "  %{out} = sdiv i64 %{a}, %{b}");
        out
    }

    /// Remainder, whose *only* failure is a zero divisor.
    ///
    /// `Int::MIN % -1` is mathematically zero and zero is representable, so `docs/spec.md`
    /// specifies it as `0`. The `srem` instruction faults on that pair anyway — it
    /// computes the quotient — so the pair is answered with a constant and never reaches
    /// the instruction. Getting this wrong would give `%` a second failure case the
    /// language says it does not have.
    fn remainder(&mut self, a: u32, b: u32, span: Span) -> u32 {
        let zero = self.fresh();
        let _ = writeln!(self.out, "  %{zero} = icmp eq i64 %{b}, 0");
        self.guard(
            zero,
            &Failure {
                code: "N0401",
                message: "division by zero".to_owned(),
            },
            span,
        );

        let special = self.min_over_minus_one(a, b);
        let is_zero = format!("b{}", self.fresh_label());
        let compute = format!("b{}", self.fresh_label());
        let join = format!("b{}", self.fresh_label());
        let _ = writeln!(
            self.out,
            "  br i1 %{special}, label %{is_zero}, label %{compute}"
        );
        self.open(&compute);
        let r = self.fresh();
        let _ = writeln!(self.out, "  %{r} = srem i64 %{a}, %{b}");
        let _ = writeln!(self.out, "  br label %{join}");
        self.open(&is_zero);
        let _ = writeln!(self.out, "  br label %{join}");
        self.open(&join);
        let out = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{out} = phi i64 [ %{r}, %{compute} ], [ 0, %{is_zero} ]"
        );
        out
    }

    /// `a == Int::MIN && b == -1` — the one operand pair `sdiv` and `srem` fault on.
    fn min_over_minus_one(&mut self, a: u32, b: u32) -> u32 {
        let is_min = self.fresh();
        let _ = writeln!(
            self.out,
            "  %{is_min} = icmp eq i64 %{a}, -9223372036854775808"
        );
        let is_m1 = self.fresh();
        let _ = writeln!(self.out, "  %{is_m1} = icmp eq i64 %{b}, -1");
        let both = self.fresh();
        let _ = writeln!(self.out, "  %{both} = and i1 %{is_min}, %{is_m1}");
        both
    }
}

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
        }
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

/// Everything a Nazm unit may refer to and does not define.
///
/// Emitted into **every** unit, unconditionally. A `declare` costs nothing — LLVM emits no
/// symbol reference for one that is never called — and the alternative is deciding, per
/// unit, which declarations it happens to need, which is a class of bug ("the IR was
/// rejected because something was not declared") for no benefit.
fn declarations(out: &mut String) {
    for d in [
        "declare {i64, i1} @llvm.sadd.with.overflow.i64(i64, i64)",
        "declare {i64, i1} @llvm.ssub.with.overflow.i64(i64, i64)",
        "declare {i64, i1} @llvm.smul.with.overflow.i64(i64, i64)",
        "declare {i64, i1} @llvm.uadd.with.overflow.i64(i64, i64)",
        "declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)",
        "declare i32 @printf(ptr noundef, ...)",
        "declare i32 @snprintf(ptr noundef, i64, ptr noundef, ...)",
        "declare ptr @malloc(i64)",
        "declare ptr @realloc(ptr, i64)",
        "declare void @free(ptr)",
        "declare ptr @getenv(ptr)",
        "declare i32 @memcmp(ptr, ptr, i64)",
        "declare i64 @write(i32, ptr, i64)",
        "declare void @exit(i32) noreturn",
        "declare i64 @strlen(ptr)",
        "declare ptr @fopen(ptr, ptr)",
        "declare i32 @fseek(ptr, i64, i32)",
        "declare i64 @ftell(ptr)",
        "declare i64 @fread(ptr, i64, i64, ptr)",
        "declare i64 @fwrite(ptr, i64, i64, ptr)",
        "declare i32 @fclose(ptr)",
        "declare i32 @pthread_create(ptr, ptr, ptr, ptr)",
        "declare i32 @pthread_join(ptr, ptr)",
        "declare i32 @pthread_mutex_init(ptr, ptr)",
        "declare i32 @pthread_mutex_lock(ptr)",
        "declare i32 @pthread_mutex_unlock(ptr)",
        "declare i32 @pthread_cond_init(ptr, ptr)",
        "declare i32 @pthread_cond_wait(ptr, ptr)",
        "declare i32 @pthread_cond_broadcast(ptr)",
        "declare i32 @pthread_mutex_destroy(ptr)",
        "declare i32 @pthread_cond_destroy(ptr)",
        "declare {i64, i1} @llvm.umul.with.overflow.i64(i64, i64)",
    ] {
        let _ = writeln!(out, "{d}");
    }
}

/// The runtime's own surface, as a unit that does not define it sees it.
///
/// The globals are `external`: one definition lives in the runtime unit and every other
/// object binds to it. Before N5 they were `internal` and there was only ever one module,
/// so the question did not arise.
fn runtime_declarations(out: &mut String) {
    for d in [
        "@nz.failed = external thread_local global i1",
        "@nz.errmsg = external thread_local global ptr",
        "@nz.errlen = external thread_local global i64",
        "@nz.argc = external global i64",
        "@nz.argv = external global ptr",
        "@nz.seq_allocated = external global i64",
        "@nz.seq_reclaimed = external global i64",
        "@nz.str_allocated = external global i64",
        "@nz.str_reclaimed = external global i64",
        "@nz.chan_allocated = external global i64",
        "@nz.chan_reclaimed = external global i64",
        "declare void @nz.fail(ptr, i64)",
        "declare void @nz.report() noreturn",
        "declare {ptr, i64, ptr} @nz.str_join(ptr, ptr, i64)",
        "declare i1 @nz.file_exists(ptr, i64)",
        "declare {ptr, i64, ptr} @nz.read_file(ptr, i64)",
        "declare i64 @nz.write_file(ptr, i64, ptr, i64)",
        "declare ptr @nz.arr_new()",
        "declare ptr @nz.arr_grow(ptr, i64)",
        "declare void @nz.arr_retain(ptr)",
        "declare i1 @nz.arr_release(ptr)",
        "declare ptr @nz.seq_new()",
        "declare void @nz.seq_release(ptr)",
        "declare void @nz.strs_release(ptr)",
        "declare ptr @nz.str_alloc(i64)",
        "declare void @nz.str_retain(ptr)",
        "declare void @nz.str_release(ptr)",
        "declare i32 @nz.spawn(ptr, ptr, ptr)",
        "declare {ptr, i64} @nz.scope_join(ptr)",
        "declare ptr @nz.chan_new(i64)",
        "declare i1 @nz.chan_send(ptr, i64)",
        "declare i1 @nz.chan_recv(ptr, ptr, ptr, i64)",
        "declare void @nz.chan_close(ptr)",
        "declare void @nz.chan_retain(ptr)",
        "declare void @nz.chan_release(ptr)",
    ] {
        let _ = writeln!(out, "{d}");
    }
}

/// The header every generated artefact carries.
fn preamble(out: &mut String, source: &str) {
    let _ = writeln!(out, "; Generated by nazm build. Do not edit.");
    let _ = writeln!(out, "; {TARGET_NOTE}");
    let _ = writeln!(out, "source_filename = \"{source}\"");
    out.push('\n');
}

/// Emit one unit: its type definitions, its external declarations, its functions' bodies from
/// MIR, their helpers and trampolines, and the constants they interned.
pub fn emit_unit(native: &Native, unit: &Unit, names: &SourceNames<'_>) -> (String, Needs) {
    let mut e = Emitter {
        out: String::new(),
        next: 0,
        next_label: 0,
        current: "entry".to_owned(),
        strings: Vec::new(),
        blobs: Vec::new(),
        names,
        types: Layouts {
            records: &unit.records,
            enums: &unit.enums,
            vecs: &unit.vecs,
        },
        program: &native.program,
        sigs: &native.sigs,
        slot_types: Vec::new(),
        unwind_to: String::new(),
        helpers: String::new(),
        generated: std::collections::BTreeSet::new(),
        generated_enums: std::collections::BTreeSet::new(),
        generated_eq: std::collections::BTreeSet::new(),
        generated_vecs: std::collections::BTreeSet::new(),
        blocks: Vec::new(),
        block_types: String::new(),
        tasks: String::new(),
        needs_tasks: false,
        task_count: 0,
        needs_channels: false,
        needs_sequences: false,
        needs_io: false,
        needs_join: false,
        needs_strings: false,
    };

    for id in &unit.functions {
        if let (Some(f), Some(sig)) = (native.program.function(*id), native.sigs.get(id.index())) {
            emit_function(&mut e, f, sig);
        }
    }

    let mut head = String::new();
    preamble(&mut head, &unit.source);
    declarations(&mut head);
    runtime_declarations(&mut head);
    head.push('\n');

    let layouts = Layouts {
        records: &unit.records,
        enums: &unit.enums,
        vecs: &unit.vecs,
    };
    for r in &unit.records {
        let fields = r
            .fields
            .iter()
            .map(|t| llvm_type(layouts, *t))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(head, "{} = type {{ {fields} }}    ; {}", r.name, r.source);
    }
    if !unit.records.is_empty() {
        head.push('\n');
    }

    // Each variant's payload struct, then the enum itself: `{ i64, %V0, %V1, … }`. Payloads
    // first because the enum's definition names them.
    for en in &unit.enums {
        for v in &en.variants {
            let fields = v
                .fields
                .iter()
                .map(|t| llvm_type(layouts, *t))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(
                head,
                "{} = type {{ {fields} }}    ; {}.{}",
                v.name, en.source, v.source
            );
        }
        let slots = std::iter::once("i64".to_owned())
            .chain(en.variants.iter().map(|v| v.name.to_string()))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(head, "{} = type {{ {slots} }}    ; {}", en.name, en.source);
    }
    if !unit.enums.is_empty() {
        head.push('\n');
    }

    if !e.block_types.is_empty() {
        head.push_str(&e.block_types);
        head.push('\n');
    }

    // The Nazm definitions this module calls and does not own. One line each, in the order
    // `lower` sorted them, so two builds of one module produce one text.
    for x in &unit.externals {
        let params = x
            .params
            .iter()
            .map(|t| llvm_type(layouts, *t))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(
            head,
            "declare {} {}({params})",
            llvm_type(layouts, x.ret),
            x.symbol
        );
    }
    if !unit.externals.is_empty() {
        head.push('\n');
    }

    // `int_to_str`'s format, inlined at its call sites, so it belongs to whichever unit uses
    // one. `private` linkage makes it a local label, so every unit may have its own.
    let _ = writeln!(
        head,
        "@.dec = private unnamed_addr constant [5 x i8] c\"%lld\\00\""
    );
    constants(&mut head, &e);
    head.push('\n');

    // Helpers before the trampolines, and both after the bodies: order in a module is not
    // meaningful to LLVM, and a fixed one is what makes two builds produce one text.
    (head + &e.out + &e.helpers + &e.tasks, e.needs())
}

/// The `Str` literals and failure messages one unit interned.
fn constants(head: &mut String, e: &Emitter<'_>) {
    // `Str` literals: the bytes, and a constant pair naming them. An empty literal still
    // gets a byte of storage so that its pointer is valid to hand to `memcmp`.
    for (sym, bytes) in &e.blobs {
        let stored = if bytes.is_empty() { &[0u8][..] } else { bytes };
        let _ = writeln!(
            head,
            "{sym} = private unnamed_addr constant [{} x i8] c\"{}\"",
            stored.len(),
            escape_bytes(stored)
        );
        // The owner is `null`: a literal is immortal, so nothing has to keep it alive
        // and nothing may free it. That is the third field doing its job in the cheapest
        // case there is — a constant, decided at compile time, costing no instruction.
        let _ = writeln!(
            head,
            "{sym}.v = private unnamed_addr constant {{ ptr, i64, ptr }} \
             {{ ptr {sym}, i64 {}, ptr null }}",
            bytes.len()
        );
    }
    for (sym, text) in &e.strings {
        let _ = writeln!(
            head,
            "{sym} = private unnamed_addr constant [{} x i8] c\"{}\\00\"",
            text.len() + 1,
            escape(text)
        );
    }
}

/// Emit the program's runtime support: defined once, for the whole executable.
///
/// # Ownership, as a rule rather than as a sequence of renamings
///
/// Everything with a definition that must exist **exactly once per program** lives here
/// and nowhere else: the failure state, the failure and reporting helpers, and each helper
/// set the program reached. They are `hidden` rather than `internal` so the linker can
/// bind another object's call to them, and `hidden` rather than external so the finished
/// executable exports none of them — a Nazm program has no ABI and this is where that is
/// enforced.
#[must_use]
pub fn emit_runtime(needs: Needs) -> String {
    let needs = needs.settled();
    let mut out = String::new();
    preamble(&mut out, "<nazm runtime>");
    declarations(&mut out);
    out.push('\n');

    // The process's arguments, stashed by the entry wrapper so that `args_count` and
    // `arg` need no parameter threaded through every Nazm function. Only where the
    // program reads them: a program that carries scaffolding it never reaches is the easy
    // mistake here, and `tests/build.rs` has checked for it since before there were units.
    if needs.io {
        let _ = writeln!(out, "@nz.argc = hidden global i64 0");
        let _ = writeln!(out, "@nz.argv = hidden global ptr null");
        let _ = writeln!(
            out,
            "@.rb = private unnamed_addr constant [3 x i8] c\"rb\\00\""
        );
        let _ = writeln!(
            out,
            "@.wb = private unnamed_addr constant [3 x i8] c\"wb\\00\""
        );
    }

    // The failure state is **thread-local**: two tasks can fail at once and neither may
    // see the other's message. Only the *first* failure on a thread is kept, so a second
    // one caused by continuing with the stand-in value of the first cannot overwrite it.
    let _ = writeln!(out, "@nz.failed = hidden thread_local global i1 false");
    let _ = writeln!(out, "@nz.errmsg = hidden thread_local global ptr null");
    let _ = writeln!(out, "@nz.errlen = hidden thread_local global i64 0");
    out.push('\n');

    // The accounting, unconditionally. Six counters cost six words in the data segment
    // and buy one thing: **every** program can be asked what it allocated and what it
    // reclaimed, including one that allocates nothing. Until N8 these lived inside the
    // sequence runtime, so a program with no sequence printed no report at all — which
    // reads as "nothing to say" and means "the instrument was not built in".
    for c in [
        "@nz.seq_allocated",
        "@nz.seq_reclaimed",
        "@nz.str_allocated",
        "@nz.str_reclaimed",
        "@nz.chan_allocated",
        "@nz.chan_reclaimed",
    ] {
        let _ = writeln!(out, "{c} = hidden global i64 0");
    }
    out.push('\n');

    // Not `llvm.trap`: a process killed by SIGILL is not "stops with a diagnostic".
    //
    // **It records and returns; it does not exit.** `docs/spec.md` says a failure inside a
    // `scope` is reported at the closing brace, after every task has been joined — so the
    // thread that fails has to be able to walk back out to that brace.
    let _ = writeln!(out, "define hidden void @nz.fail(ptr %m, i64 %n) {{");
    let _ = writeln!(out, "entry:");
    let _ = writeln!(out, "  %had = load i1, ptr @nz.failed");
    let _ = writeln!(out, "  br i1 %had, label %done, label %record");
    let _ = writeln!(out, "record:");
    let _ = writeln!(out, "  store i1 true, ptr @nz.failed");
    let _ = writeln!(out, "  store ptr %m, ptr @nz.errmsg");
    let _ = writeln!(out, "  store i64 %n, ptr @nz.errlen");
    let _ = writeln!(out, "  br label %done");
    let _ = writeln!(out, "done:");
    let _ = writeln!(out, "  ret void");
    let _ = writeln!(out, "}}");
    out.push('\n');
    // The one place a diagnostic is written and the process ends: reached from the entry
    // wrapper, once every task has been joined.
    let _ = writeln!(out, "define hidden void @nz.report() noreturn {{");
    let _ = writeln!(out, "entry:");
    let _ = writeln!(out, "  %m = load ptr, ptr @nz.errmsg");
    let _ = writeln!(out, "  %n = load i64, ptr @nz.errlen");
    let _ = writeln!(out, "  %w = call i64 @write(i32 2, ptr %m, i64 %n)");
    let _ = writeln!(out, "  call void @exit(i32 2)");
    let _ = writeln!(out, "  unreachable");
    let _ = writeln!(out, "}}");

    if needs.sequences {
        out.push_str(SEQUENCE_RUNTIME);
    }
    if needs.strings {
        out.push_str(STRING_RUNTIME);
    }
    if needs.io {
        out.push_str(IO_RUNTIME);
    }
    if needs.join {
        out.push_str(JOIN_RUNTIME);
    }
    if needs.tasks {
        out.push_str(TASK_RUNTIME);
    }
    if needs.channels {
        out.push_str(CHANNEL_RUNTIME);
    }
    out.push('\n');
    out
}

/// Emit the platform entry point: one per executable, and the only thing that prints.
///
/// # Why this is its own unit
///
/// It is the one place where the language's world meets the operating system's. Nazm's
/// `main` is an ordinary definition with an ordinary symbol derived from its `DefKey`;
/// `@main` is a C entry point with a C signature. Keeping them in separate artefacts is
/// what makes that separation visible rather than implied — and it is what stops an
/// imported module's function named `main` from ever being a process entry, because the
/// symbol this calls is the **root** module's, decided by [`crate::lower::plan`].
///
/// `roots` is how many capability parameters `main` takes (N37). Each is handed a `0`:
/// authority was checked when the program was, and the word is all that is left of it.
#[must_use]
pub fn emit_entry(entry: &NativeSymbol, roots: usize, needs: Needs) -> String {
    let needs = needs.settled();
    let mut out = String::new();
    preamble(&mut out, "<nazm entry>");
    declarations(&mut out);
    runtime_declarations(&mut out);
    let root_types = vec!["i64"; roots].join(", ");
    let root_args = vec!["i64 0"; roots].join(", ");
    let _ = writeln!(out, "declare i64 {entry}({root_types})");
    out.push('\n');
    // `%lld\n`: the full signed 64-bit value in decimal. Encoding the result in the exit
    // status instead would truncate it to 8 bits, which for a language whose `Int` is
    // 64-bit would be a lie in the common case.
    let _ = writeln!(
        out,
        "@.fmt = private unnamed_addr constant [6 x i8] c\"%lld\\0A\\00\""
    );
    // Unconditional since N8. Three classes of storage are reclaimable now and a program
    // with none of them is still a program whose accounting can be asked for — and an
    // instrument that is absent exactly where there is nothing to report is an instrument
    // that cannot distinguish "nothing" from "not measured".
    //
    // Lengths are computed, never counted by hand: a constant whose declared size
    // disagrees with its bytes is rejected by clang at best and truncates at worst.
    let name = MEMORY_VARIABLE;
    let _ = writeln!(
        out,
        "@.memvar = private unnamed_addr constant [{} x i8] c\"{}\\00\"",
        name.len() + 1,
        escape(name)
    );
    let _ = writeln!(
        out,
        "@.memfmt = private unnamed_addr constant [{} x i8] c\"{}\\0A\\00\"",
        MEMORY_FORMAT.len() + 2,
        escape(MEMORY_FORMAT)
    );
    let _ = writeln!(out, "@.membuf = private global [256 x i8] zeroinitializer");
    out.push('\n');

    let _ = writeln!(out, "define i32 @main(i32 %argc, ptr %argv) {{");
    let _ = writeln!(out, "entry:");
    if needs.io {
        // One fewer than `argc`, because `argv[0]` is the program's own name and is
        // deliberately not visible.
        let _ = writeln!(out, "  %argc64 = sext i32 %argc to i64");
        let _ = writeln!(out, "  %nargs = sub i64 %argc64, 1");
        let _ = writeln!(out, "  store i64 %nargs, ptr @nz.argc");
        let _ = writeln!(out, "  store ptr %argv, ptr @nz.argv");
    }
    let _ = writeln!(out, "  %result = call i64 {entry}({root_args})");
    {
        // Before the failure is reported, not after: `@nz.report` ends the process and
        // never returns, so a report emitted on the success path alone would say nothing
        // about exactly the runs where reclamation is hardest to get right. `main` has
        // already returned by here, so the counts are final either way.
        //
        // It is a runtime diagnostic, off unless asked for. It is not a built-in because
        // no Nazm program should be able to observe how its own storage is managed:
        // `docs/spec.md` states *when* storage is reclaimed and deliberately not *how*.
        let _ = writeln!(out, "  %want = call ptr @getenv(ptr @.memvar)");
        let _ = writeln!(out, "  %quiet = icmp eq ptr %want, null");
        let _ = writeln!(
            out,
            "  br i1 %quiet, label %nz.checked, label %nz.memreport"
        );
        let _ = writeln!(out, "nz.memreport:");
        let mut fields = Vec::new();
        for (kind, allocated, reclaimed) in [
            ("seq", "@nz.seq_allocated", "@nz.seq_reclaimed"),
            ("str", "@nz.str_allocated", "@nz.str_reclaimed"),
            ("chan", "@nz.chan_allocated", "@nz.chan_reclaimed"),
        ] {
            let _ = writeln!(out, "  %{kind}a = load i64, ptr {allocated}");
            let _ = writeln!(out, "  %{kind}r = load i64, ptr {reclaimed}");
            let _ = writeln!(out, "  %{kind}l = sub i64 %{kind}a, %{kind}r");
            fields.push(format!("i64 %{kind}a, i64 %{kind}r, i64 %{kind}l"));
        }
        let _ = writeln!(
            out,
            "  %memout = call i32 (ptr, i64, ptr, ...) @snprintf(ptr @.membuf, i64 256, \
             ptr @.memfmt, {})",
            fields.join(", ")
        );
        let _ = writeln!(out, "  %memlen = sext i32 %memout to i64");
        let _ = writeln!(
            out,
            "  %memwrote = call i64 @write(i32 2, ptr @.membuf, i64 %memlen)"
        );
        let _ = writeln!(out, "  br label %nz.checked");
        let _ = writeln!(out, "nz.checked:");
    }
    // The single place a diagnostic is written. By the time control is here every scope
    // `main` opened has been joined, so nothing is still running when the process ends —
    // which is the difference between reporting a failure and abandoning the program
    // where it happened.
    let _ = writeln!(out, "  %failed = load i1, ptr @nz.failed");
    let _ = writeln!(
        out,
        "  br i1 %failed, label %nz.reported, label %nz.finished"
    );
    let _ = writeln!(out, "nz.reported:");
    let _ = writeln!(out, "  call void @nz.report()");
    let _ = writeln!(out, "  unreachable");
    let _ = writeln!(out, "nz.finished:");
    let _ = writeln!(
        out,
        "  %print = call i32 (ptr, ...) @printf(ptr @.fmt, i64 %result)"
    );
    let _ = writeln!(out, "  ret i32 0");
    let _ = writeln!(out, "}}");
    out
}

/// `str_join`, as generated LLVM.
///
/// A loop, so it cannot be inlined at the call site the way the other built-ins are: it
/// measures every part, allocates **once**, and copies. That is the whole point — the
/// same result built with `str_concat` in a loop is quadratic in time and memory, and
/// with nothing ever freed the compiler written in Nazm exhausted memory assembling its
/// own output before this existed.
///
/// Signals allocation failure by returning a null pointer, which the call site turns
/// into `N0406` at the right span.
const JOIN_RUNTIME: &str = r"
define hidden { ptr, i64, ptr } @nz.str_join(ptr %h, ptr %sp, i64 %sn) {
entry:
  %n = load i64, ptr %h
  %dp = getelementptr i8, ptr %h, i64 16
  %data = load ptr, ptr %dp
  br label %sum
sum:
  %i = phi i64 [ 0, %entry ], [ %i2, %sumbody ]
  %acc = phi i64 [ 0, %entry ], [ %acc2, %sumbody ]
  %more = icmp slt i64 %i, %n
  br i1 %more, label %sumbody, label %alloc
sumbody:
  %e = getelementptr { ptr, i64, ptr }, ptr %data, i64 %i
  %lp = getelementptr i8, ptr %e, i64 8
  %li = load i64, ptr %lp
  %acc2 = add i64 %acc, %li
  %i2 = add i64 %i, 1
  br label %sum
alloc:
  ; One separator between each pair, so n-1 of them, and none for an empty sequence.
  %less = sub i64 %n, 1
  %neg = icmp slt i64 %less, 0
  %gaps = select i1 %neg, i64 0, i64 %less
  %sepbytes = mul i64 %gaps, %sn
  %total = add i64 %acc, %sepbytes
  ; One byte more, so an empty result still owns storage and its pointer stays valid.
  %room = add i64 %total, 1
  %own = call ptr @nz.str_alloc(i64 %room)
  %bad = icmp eq ptr %own, null
  br i1 %bad, label %fail, label %copy
copy:
  %buf = getelementptr i8, ptr %own, i64 16
  br label %loop
loop:
  %j = phi i64 [ 0, %copy ], [ %j2, %tail ]
  %off = phi i64 [ 0, %copy ], [ %off3, %tail ]
  %more2 = icmp slt i64 %j, %n
  br i1 %more2, label %body, label %done
body:
  %first = icmp eq i64 %j, 0
  br i1 %first, label %tail, label %gap
gap:
  %d1 = getelementptr i8, ptr %buf, i64 %off
  call void @llvm.memcpy.p0.p0.i64(ptr %d1, ptr %sp, i64 %sn, i1 false)
  %off1 = add i64 %off, %sn
  br label %tail
tail:
  %off2 = phi i64 [ %off, %body ], [ %off1, %gap ]
  %e2 = getelementptr { ptr, i64, ptr }, ptr %data, i64 %j
  %p2 = load ptr, ptr %e2
  %l2p = getelementptr i8, ptr %e2, i64 8
  %l2 = load i64, ptr %l2p
  %d2 = getelementptr i8, ptr %buf, i64 %off2
  call void @llvm.memcpy.p0.p0.i64(ptr %d2, ptr %p2, i64 %l2, i1 false)
  %off3 = add i64 %off2, %l2
  %j2 = add i64 %j, 1
  br label %loop
done:
  %a = insertvalue { ptr, i64, ptr } undef, ptr %buf, 0
  %b = insertvalue { ptr, i64, ptr } %a, i64 %total, 1
  %b2 = insertvalue { ptr, i64, ptr } %b, ptr %own, 2
  ret { ptr, i64, ptr } %b2
fail:
  %z = insertvalue { ptr, i64, ptr } undef, ptr null, 0
  %z2 = insertvalue { ptr, i64, ptr } %z, i64 0, 1
  %z3 = insertvalue { ptr, i64, ptr } %z2, ptr null, 2
  ret { ptr, i64, ptr } %z3
}
";

/// The file-system helpers, as generated LLVM.
///
/// Here rather than at the call sites because each is a dozen basic blocks and none of
/// them needs a source position: every failure is signalled by a null pointer or a
/// negative count, and the *call site* turns that into `N0407` at the right span. That
/// split is the same one the sequence helpers use, for the same reason.
///
/// A Nazm `Str` is not NUL-terminated — the specification makes an embedded zero an
/// ordinary byte — so `@nz.cstr` copies the path into a terminated buffer before any
/// libc call sees it. Passing the bytes directly would truncate a path at the first
/// zero, or read past the end of one that has none.
const IO_RUNTIME: &str = r"
define hidden ptr @nz.cstr(ptr %p, i64 %n) {
entry:
  %room = add i64 %n, 1
  %buf = call ptr @malloc(i64 %room)
  %bad = icmp eq ptr %buf, null
  br i1 %bad, label %fail, label %ok
fail:
  ret ptr null
ok:
  call void @llvm.memcpy.p0.p0.i64(ptr %buf, ptr %p, i64 %n, i1 false)
  %end = getelementptr i8, ptr %buf, i64 %n
  store i8 0, ptr %end
  ret ptr %buf
}

define hidden i1 @nz.file_exists(ptr %p, i64 %n) {
entry:
  %path = call ptr @nz.cstr(ptr %p, i64 %n)
  %nopath = icmp eq ptr %path, null
  br i1 %nopath, label %no, label %open
open:
  %f = call ptr @fopen(ptr %path, ptr @.rb)
  ; The terminated copy has done its job the moment `fopen` returns. Until N8 every file
  ; operation leaked one of these.
  call void @free(ptr %path)
  %missing = icmp eq ptr %f, null
  br i1 %missing, label %no, label %yes
yes:
  %ignored = call i32 @fclose(ptr %f)
  ret i1 true
no:
  ret i1 false
}

define hidden { ptr, i64, ptr } @nz.read_file(ptr %p, i64 %n) {
entry:
  %path = call ptr @nz.cstr(ptr %p, i64 %n)
  %nopath = icmp eq ptr %path, null
  br i1 %nopath, label %fail, label %open
open:
  %f = call ptr @fopen(ptr %path, ptr @.rb)
  call void @free(ptr %path)
  %missing = icmp eq ptr %f, null
  br i1 %missing, label %fail, label %measure
measure:
  ; SEEK_END is 2 and SEEK_SET is 0 on every target this is verified against.
  %e = call i32 @fseek(ptr %f, i64 0, i32 2)
  %len = call i64 @ftell(ptr %f)
  %unmeasurable = icmp slt i64 %len, 0
  br i1 %unmeasurable, label %closefail, label %reserve
reserve:
  %s = call i32 @fseek(ptr %f, i64 0, i32 0)
  ; One byte more than the file, so an empty file still owns storage and its pointer
  ; stays valid to compare.
  %room = add i64 %len, 1
  %own = call ptr @nz.str_alloc(i64 %room)
  %nobuf = icmp eq ptr %own, null
  br i1 %nobuf, label %closefail, label %read
read:
  %buf = getelementptr i8, ptr %own, i64 16
  %got = call i64 @fread(ptr %buf, i64 1, i64 %len, ptr %f)
  %c = call i32 @fclose(ptr %f)
  %short = icmp ne i64 %got, %len
  br i1 %short, label %partial, label %done
partial:
  ; A short read is a failure, and the storage it was going to fill is released rather
  ; than abandoned.
  call void @nz.str_release(ptr %own)
  br label %fail
done:
  %a = insertvalue { ptr, i64, ptr } undef, ptr %buf, 0
  %b = insertvalue { ptr, i64, ptr } %a, i64 %len, 1
  %b2 = insertvalue { ptr, i64, ptr } %b, ptr %own, 2
  ret { ptr, i64, ptr } %b2
closefail:
  %c2 = call i32 @fclose(ptr %f)
  br label %fail
fail:
  %z = insertvalue { ptr, i64, ptr } undef, ptr null, 0
  %z2 = insertvalue { ptr, i64, ptr } %z, i64 0, 1
  %z3 = insertvalue { ptr, i64, ptr } %z2, ptr null, 2
  ret { ptr, i64, ptr } %z3
}

define hidden i64 @nz.write_file(ptr %pp, i64 %pn, ptr %dp, i64 %dn) {
entry:
  %path = call ptr @nz.cstr(ptr %pp, i64 %pn)
  %nopath = icmp eq ptr %path, null
  br i1 %nopath, label %fail, label %open
open:
  %f = call ptr @fopen(ptr %path, ptr @.wb)
  call void @free(ptr %path)
  %missing = icmp eq ptr %f, null
  br i1 %missing, label %fail, label %write
write:
  %got = call i64 @fwrite(ptr %dp, i64 1, i64 %dn, ptr %f)
  %c = call i32 @fclose(ptr %f)
  %short = icmp ne i64 %got, %dn
  br i1 %short, label %fail, label %done
done:
  ret i64 %got
fail:
  ret i64 -1
}
";

/// The two sequence helpers, as generated LLVM rather than a linked runtime crate.
///
/// Generated because that is the smallest thing that works: `docs/architecture.md` keeps
/// `nazm-rt` for when a runtime genuinely needs stack switching and a custom allocator,
/// and neither of these does. They are emitted only into programs that use a sequence.
///
/// **Bounds checks are not here.** They live at each call site, because the diagnostic
/// has to name that call's source position and a shared helper has only one. What is
/// here is allocation and growth, which have no position to report — both signal failure
/// by returning null, and the call site turns that into `N0406`.
///
/// The header is `{ i64 len, i64 cap, ptr data, i64 rc }`, 32 bytes. The handle is its
/// address and never moves, which is what makes a sequence a reference: growth rewrites
/// `data` and `cap` in place and every holder sees it.
///
/// # The reference count, and why it needs no atomic
///
/// `rc` is the number of live references, as `docs/spec.md`'s *memory constitution* counts
/// them: the bindings holding this handle, plus the one it is travelling in. When it
/// reaches zero the data and the header are freed, which is the whole of reclamation.
///
/// It is an ordinary `add`, not an atomic one, and that is a consequence of a rule the
/// language already had rather than an optimisation taken on trust. `N0321` refuses to let
/// a sequence cross into a task, so **no two threads can ever hold the same handle**, so
/// no two threads can ever touch the same count. A rule written for data-race safety pays
/// for itself a second time here; if it were ever relaxed, this would have to become an
/// atomic in the same commit.
///
/// # The counters
///
/// `@nz.seq_allocated` and `@nz.seq_reclaimed` exist so that a test can assert reclamation
/// happened rather than infer it from resident memory, which is an allocator's opinion. The
/// entry wrapper prints them when `NAZM_MEMORY_REPORT` is set in the environment and says
/// nothing otherwise. They are a runtime diagnostic, not a language feature: no Nazm
/// program can read them, and `docs/spec.md` does not mention them.
const SEQUENCE_RUNTIME: &str = r"
define hidden ptr @nz.arr_new() {
entry:
  %h = call ptr @malloc(i64 32)
  %bad = icmp eq ptr %h, null
  br i1 %bad, label %fail, label %ok
fail:
  ret ptr null
ok:
  store i64 0, ptr %h
  %capp = getelementptr i8, ptr %h, i64 8
  store i64 0, ptr %capp
  %datap = getelementptr i8, ptr %h, i64 16
  store ptr null, ptr %datap
  ; Born with one reference: the expression that produced it. Whoever consumes that
  ; expression either takes the reference over or releases it.
  %rcp = getelementptr i8, ptr %h, i64 24
  store i64 1, ptr %rcp
  ret ptr %h
}

define hidden void @nz.arr_retain(ptr %h) {
entry:
  %none = icmp eq ptr %h, null
  br i1 %none, label %done, label %bump
bump:
  %rcp = getelementptr i8, ptr %h, i64 24
  %rc = load i64, ptr %rcp
  %rc2 = add i64 %rc, 1
  store i64 %rc2, ptr %rcp
  br label %done
done:
  ret void
}

define hidden i1 @nz.arr_release(ptr %h) {
entry:
  %none = icmp eq ptr %h, null
  br i1 %none, label %kept, label %drop
drop:
  %rcp = getelementptr i8, ptr %h, i64 24
  %rc = load i64, ptr %rcp
  %rc2 = sub i64 %rc, 1
  store i64 %rc2, ptr %rcp
  %last = icmp sle i64 %rc2, 0
  br i1 %last, label %free, label %kept
free:
  ; The data buffer first, then the header: after the header is freed its `data` field
  ; may not be read, and reading it is how this would become a use-after-free of its own.
  %datap = getelementptr i8, ptr %h, i64 16
  %data = load ptr, ptr %datap
  call void @free(ptr %data)
  call void @free(ptr %h)
  ret i1 true
kept:
  ret i1 false
}

; The counted pair. `@nz.arr_new` and `@nz.arr_release` are the allocator; these two are
; the allocator plus the bookkeeping that makes reclamation checkable. The scope machinery
; uses the uncounted pair for its list of task ids, which is not a Nazm value and must not
; appear in a report about the program's sequences.
define hidden ptr @nz.seq_new() {
entry:
  %h = call ptr @nz.arr_new()
  %bad = icmp eq ptr %h, null
  br i1 %bad, label %done, label %count
count:
  ; Atomic, unlike the reference count beside it: a *task* may create a sequence of its
  ; own, so two threads can reach this counter even though `N0321` stops them reaching
  ; one sequence. Until N8 this was an ordinary load-add-store and a concurrent program
  ; could under-report what it had allocated.
  %c = atomicrmw add ptr @nz.seq_allocated, i64 1 monotonic
  br label %done
done:
  ret ptr %h
}

define hidden void @nz.seq_release(ptr %h) {
entry:
  %freed = call i1 @nz.arr_release(ptr %h)
  br i1 %freed, label %count, label %done
count:
  %c = atomicrmw add ptr @nz.seq_reclaimed, i64 1 monotonic
  br label %done
done:
  ret void
}

define hidden ptr @nz.arr_grow(ptr %h, i64 %elem) {
entry:
  %len = load i64, ptr %h
  %capp = getelementptr i8, ptr %h, i64 8
  %cap = load i64, ptr %capp
  %datap = getelementptr i8, ptr %h, i64 16
  %full = icmp sge i64 %len, %cap
  br i1 %full, label %grow, label %done
grow:
  ; Doubling, with a floor of four. `realloc(null, n)` is `malloc(n)`, so the first
  ; growth needs no special case. Both products are checked: an element may be a record
  ; or an enum of any size since N11, and a wrapped size would allocate less than the
  ; caller is about to write.
  %twice.r = call {i64, i1} @llvm.umul.with.overflow.i64(i64 %cap, i64 2)
  %twice = extractvalue {i64, i1} %twice.r, 0
  %twice.o = extractvalue {i64, i1} %twice.r, 1
  %tiny = icmp slt i64 %twice, 4
  %want = select i1 %tiny, i64 4, i64 %twice
  %bytes.r = call {i64, i1} @llvm.umul.with.overflow.i64(i64 %want, i64 %elem)
  %bytes = extractvalue {i64, i1} %bytes.r, 0
  %bytes.o = extractvalue {i64, i1} %bytes.r, 1
  %over = or i1 %twice.o, %bytes.o
  br i1 %over, label %oom, label %sized
sized:
  %old = load ptr, ptr %datap
  %new = call ptr @realloc(ptr %old, i64 %bytes)
  %failed = icmp eq ptr %new, null
  br i1 %failed, label %oom, label %kept
kept:
  ; The header is updated only after the allocation succeeded, so a failed growth
  ; leaves the sequence exactly as it was rather than half-resized.
  store ptr %new, ptr %datap
  store i64 %want, ptr %capp
  br label %done
oom:
  ret ptr null
done:
  %data = load ptr, ptr %datap
  ret ptr %data
}
";

/// A string's backing allocation, as generated LLVM.
///
/// The layout is `{ i64 rc, i64 length, bytes… }`, and the **owner** field of a `Str` is
/// this block's address while its **pointer** field is `owner + 16` or anywhere after it.
/// That separation is the whole of N8's string model: the bytes a value names and the
/// allocation that keeps them valid are different addresses, so a slice can move one
/// without losing the other.
///
/// # Null is a backing
///
/// A literal's owner and a process argument's owner are both null, and every operation
/// here starts by testing for it. So *immortal* and *external* need no tag of their own
/// and no branch anywhere else in the compiler: they are simply strings that own nothing,
/// and retaining or releasing one costs a comparison.
///
/// # Why this count is atomic and a sequence's is not
///
/// `docs/spec.md` lets a `Str` cross into a task and refuses to let a sequence cross
/// (`N0321`). A task can therefore be handed a string, slice it, bind the slice — and two
/// sibling tasks doing that to the same backing really are two threads adjusting one
/// count. A sequence's count cannot be reached by two threads at all, which is why that
/// one stays an ordinary `add`. Neither choice is a guess: each follows from a rule the
/// language already had.
///
/// The decrement is `release` and the free is preceded by an `acquire` fence, which is the
/// standard pairing: it is what makes every other thread's writes to the buffer visible to
/// the thread that frees it.
const STRING_RUNTIME: &str = r"
define hidden ptr @nz.str_alloc(i64 %n) {
entry:
  ; Checked, because a length that overflows here would allocate less than the caller is
  ; about to write. `docs/spec.md` reports it as N0406 at the call site.
  %r = call {i64, i1} @llvm.uadd.with.overflow.i64(i64 %n, i64 16)
  %over = extractvalue {i64, i1} %r, 1
  br i1 %over, label %fail, label %ask
ask:
  %room = extractvalue {i64, i1} %r, 0
  %b = call ptr @malloc(i64 %room)
  %bad = icmp eq ptr %b, null
  br i1 %bad, label %fail, label %ok
fail:
  ret ptr null
ok:
  store i64 1, ptr %b
  %lenp = getelementptr i8, ptr %b, i64 8
  store i64 %n, ptr %lenp
  %c = atomicrmw add ptr @nz.str_allocated, i64 1 monotonic
  ret ptr %b
}

define hidden void @nz.str_retain(ptr %b) {
entry:
  ; Null is immortal or external storage: a literal, or a process argument. Nothing to
  ; count, and nothing this program may ever free.
  %none = icmp eq ptr %b, null
  br i1 %none, label %done, label %bump
bump:
  %old = atomicrmw add ptr %b, i64 1 monotonic
  br label %done
done:
  ret void
}

define hidden void @nz.str_release(ptr %b) {
entry:
  %none = icmp eq ptr %b, null
  br i1 %none, label %done, label %drop
drop:
  %old = atomicrmw sub ptr %b, i64 1 release
  %last = icmp eq i64 %old, 1
  br i1 %last, label %free, label %done
free:
  fence acquire
  call void @free(ptr %b)
  %c = atomicrmw add ptr @nz.str_reclaimed, i64 1 monotonic
  br label %done
done:
  ret void
}

; A `Strs` owns a reference to every string in it, so its storage cannot be released by
; the helper an `Ints` uses. Elements first, then the array, then the header — the order
; matters, because reading an element out of a buffer that has already been freed is the
; use-after-free this whole milestone exists to make impossible.
define hidden void @nz.strs_release(ptr %h) {
entry:
  %none = icmp eq ptr %h, null
  br i1 %none, label %done, label %drop
drop:
  %rcp = getelementptr i8, ptr %h, i64 24
  %rc = load i64, ptr %rcp
  %rc2 = sub i64 %rc, 1
  store i64 %rc2, ptr %rcp
  %last = icmp sle i64 %rc2, 0
  br i1 %last, label %contents, label %done
contents:
  %n = load i64, ptr %h
  %dp = getelementptr i8, ptr %h, i64 16
  %data = load ptr, ptr %dp
  br label %test
test:
  %i = phi i64 [ 0, %contents ], [ %next, %body ]
  %more = icmp slt i64 %i, %n
  br i1 %more, label %body, label %storage
body:
  %e = getelementptr { ptr, i64, ptr }, ptr %data, i64 %i
  %op = getelementptr i8, ptr %e, i64 16
  %owner = load ptr, ptr %op
  call void @nz.str_release(ptr %owner)
  %next = add i64 %i, 1
  br label %test
storage:
  call void @free(ptr %data)
  call void @free(ptr %h)
  %c = atomicrmw add ptr @nz.seq_reclaimed, i64 1 monotonic
  br label %done
done:
  ret void
}
";

/// One function, from its MIR.
///
/// Every local gets a slot in the entry block; the ones that can own anything are zeroed
/// there, and the parameters are stored into theirs. Then each MIR block becomes a label with
/// its statements and its terminator, in MIR's order.
fn emit_function(e: &mut Emitter<'_>, f: &mir::Function, sig: &Sig) {
    e.next = 0;
    e.current = "entry".to_owned();
    e.slot_types = f.locals.iter().map(|l| l.ty.value().map(Ty::of)).collect();

    let params = sig
        .params
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{} %p{i}", llvm_type(e.types, *t)))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(
        e.out,
        "define {}{} {}({params}) {{   ; {}",
        sig.linkage.llvm(),
        llvm_type(e.types, sig.ret),
        sig.symbol,
        f.name
    );
    let _ = writeln!(e.out, "entry:");
    for (i, local) in f.locals.iter().enumerate() {
        let l = mir::LocalId(u32::try_from(i).unwrap_or(u32::MAX));
        let ty = e.slot_llvm(l);
        let shown = match local.ty.value() {
            Some(t) => Ty::of(t).name(),
            None => "tasks",
        };
        let _ = writeln!(
            e.out,
            "  %slot{i} = alloca {ty}            ; {} : {shown}",
            local.name
        );
    }
    // Zero every slot that can own something, parameters excepted: a drop of a local that
    // holds nothing then releases the zero, which the runtime ignores.
    for (i, _) in f.locals.iter().enumerate().skip(f.params as usize) {
        let l = mir::LocalId(u32::try_from(i).unwrap_or(u32::MAX));
        let owning = match e.slot_types.get(i).copied().flatten() {
            Some(t) => owns_anything(e.types, t),
            None => true,
        };
        if owning {
            e.store_zero(l);
        }
    }
    for i in 0..f.params as usize {
        let l = mir::LocalId(u32::try_from(i).unwrap_or(u32::MAX));
        e.store(l, &format!("%p{i}"));
    }
    let _ = writeln!(e.out, "  br label %bb0");
    for (i, b) in f.blocks.iter().enumerate() {
        e.open(&label(mir::BlockId(u32::try_from(i).unwrap_or(u32::MAX))));
        for s in &b.stmts {
            e.statement(s);
        }
        e.terminator(&b.term, sig.ret);
    }
    let _ = writeln!(e.out, "}}");
    e.out.push('\n');
}

fn rty(r: nazm_sema::RecordId) -> RecordTy {
    RecordTy(u32::try_from(r.index()).unwrap_or(u32::MAX))
}

fn ety(e: nazm_sema::EnumId) -> EnumTy {
    EnumTy(u32::try_from(e.index()).unwrap_or(u32::MAX))
}

/// The same escaping for arbitrary bytes, which a `Str` literal may contain.
fn escape_bytes(bytes: &[u8]) -> String {
    let mut out = String::new();
    for b in bytes {
        if *b == b'"' || *b == b'\\' || !(0x20..0x7f).contains(b) {
            let _ = write!(out, "\\{b:02X}");
        } else {
            out.push(*b as char);
        }
    }
    out
}

/// LLVM's constant-string escaping: `\XX` for anything not printable ASCII.
fn escape(s: &str) -> String {
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

/// Starting a task, and waiting for one, as generated LLVM.
///
/// # The slot is reserved before the thread starts
///
/// `@nz.spawn` records the task **first** and undoes the record if `pthread_create`
/// fails. A thread that started and could not be recorded would never be joined, and a
/// scope that cannot join a task it started is not a scope — so the failure that must not
/// happen is "started but untracked", and this is the order that makes it impossible.
///
/// # What is stored
///
/// One pointer per task: the argument block, whose first eight bytes `pthread_create`
/// writes the thread handle into. The block is allocated by the spawn site anyway, so
/// using its head as the handle's storage costs nothing and keeps the list to one
/// allocation.
///
/// Returns `0` on success, `-1` when the list could not grow, and `1` when the thread
/// could not be started. The call site turns those into `N0406` and `N0404`.
const TASK_RUNTIME: &str = r"
define hidden i32 @nz.spawn(ptr %h, ptr %fn, ptr %blk) {
entry:
  %data = call ptr @nz.arr_grow(ptr %h, i64 8)
  %oom = icmp eq ptr %data, null
  br i1 %oom, label %nomem, label %record
nomem:
  ret i32 -1
record:
  %len = load i64, ptr %h
  %slot = getelementptr i64, ptr %data, i64 %len
  %raw = ptrtoint ptr %blk to i64
  store i64 %raw, ptr %slot
  %next = add i64 %len, 1
  store i64 %next, ptr %h
  %rc = call i32 @pthread_create(ptr %blk, ptr null, ptr %fn, ptr %blk)
  %bad = icmp ne i32 %rc, 0
  br i1 %bad, label %undo, label %ok
undo:
  store i64 %len, ptr %h
  ret i32 1
ok:
  ret i32 0
}

; Joins every task and reports the first failure any of them had, as { message, length },
; or { null, 0 } if none did.
;
; **Every** task is joined before anything is reported, including after one has been found
; to have failed — returning at the first would leave the rest running, and a scope that
; does not contain its tasks is not a scope. The failure taken is the earliest by position
; in the list, which is the order the tasks were spawned in, so which one is reported does
; not depend on which thread happened to finish first.
;
; No lock is needed. A task writes its own diagnostic into its own argument block before
; its thread ends, and this reads that block only after `pthread_join` has returned for
; it, which is a happens-before edge.
define hidden { ptr, i64 } @nz.scope_join(ptr %h) {
entry:
  ; The count is read once. Nothing can be added while this runs: the scope's body has
  ; finished by every path that reaches here, and a task cannot spawn into its parent.
  %n = load i64, ptr %h
  %dp = getelementptr i8, ptr %h, i64 16
  %data = load ptr, ptr %dp
  br label %test
test:
  %i = phi i64 [ 0, %entry ], [ %next, %after ]
  %msg = phi ptr [ null, %entry ], [ %msg2, %after ]
  %len = phi i64 [ 0, %entry ], [ %len2, %after ]
  %more = icmp slt i64 %i, %n
  br i1 %more, label %body, label %done
body:
  %ep = getelementptr i64, ptr %data, i64 %i
  %raw = load i64, ptr %ep
  %blk = inttoptr i64 %raw to ptr
  %tid = load ptr, ptr %blk
  %rc = call i32 @pthread_join(ptr %tid, ptr null)
  ; Joined, so the thread has ended and nothing will read this block again. Its arguments
  ; were released by the trampoline on its way out; this is the block itself, one
  ; `malloc` per `spawn` that until N8 nothing ever gave back.
  ; The task's own diagnostic, at a fixed offset in the block it was given.
  %emp = getelementptr i8, ptr %blk, i64 8
  %em = load ptr, ptr %emp
  %elp = getelementptr i8, ptr %blk, i64 16
  %el = load i64, ptr %elp
  %none = icmp eq ptr %msg, null
  %this = icmp ne ptr %em, null
  %take = and i1 %none, %this
  ; Read before the block is freed, and the message itself is a constant in the object's
  ; read-only data rather than anything the block owns — so what is carried out stays
  ; valid after the free below.
  call void @free(ptr %blk)
  br i1 %take, label %first, label %after
first:
  br label %after
after:
  %msg2 = phi ptr [ %em, %first ], [ %msg, %body ]
  %len2 = phi i64 [ %el, %first ], [ %len, %body ]
  %next = add i64 %i, 1
  br label %test
done:
  %r0 = insertvalue { ptr, i64 } undef, ptr %msg, 0
  %r1 = insertvalue { ptr, i64 } %r0, i64 %len, 1
  ret { ptr, i64 } %r1
}
";

/// A bounded channel, as generated LLVM: one mutex, one condition variable, one ring.
///
/// The header is
/// `{ i64 cap, i64 len, i64 head, i64 closed, ptr data, ptr mutex, ptr cond, i64 rc }`.
///
/// # When a channel may be destroyed, and why no waiter can be left
///
/// At `rc == 0`, and the argument is structural rather than hopeful. A reference is a
/// binding that holds the handle, a value on its way out of a `return`, or the argument
/// block of a task that has not been joined. A thread blocked in `chan_send` or
/// `chan_recv` reached it **from a frame that holds the handle**, because a built-in
/// borrows its argument and the caller keeps the reference across the call — so a waiter
/// implies a live reference, and `rc == 0` implies no waiter. The count is also why
/// closing is not destruction: `chan_close` changes what the channel does and touches no
/// storage at all.
///
/// The count is atomic for the reason a string backing's is: `docs/spec.md` lets a `Chan`
/// cross into a task, so two tasks really can bind the same channel at once.
///
/// # One condition variable, broadcast every time
///
/// Senders and receivers wait on the same variable and every change wakes all of them.
/// Not the fastest arrangement and the obviously correct one — `docs/spec.md` promises no
/// fairness, and a signal-one scheme has to be right about *which* one, which is where
/// the lost-wakeup bugs live. The interpreter's `Channel` is built the same way, so the
/// two agree by construction rather than by testing.
///
/// # The waits are loops
///
/// `pthread_cond_wait` may return without the condition holding. Every wait here is
/// inside a loop that re-reads the state, which is the only correct way to use one.
const CHANNEL_RUNTIME: &str = r"
define hidden ptr @nz.chan_new(i64 %cap) {
entry:
  %c = call ptr @malloc(i64 64)
  %bad = icmp eq ptr %c, null
  br i1 %bad, label %fail, label %ring
fail:
  ret ptr null
; A channel is four allocations, and a failure part-way through must not leave the earlier
; ones behind. One label per stage, each freeing exactly what has been taken by then —
; which is `docs/spec.md`'s failure law applied to the runtime's own storage, not only to
; the program's.
freec:
  call void @free(ptr %c)
  ret ptr null
freed:
  call void @free(ptr %d)
  call void @free(ptr %c)
  ret ptr null
freem:
  call void @free(ptr %m)
  call void @free(ptr %d)
  call void @free(ptr %c)
  ret ptr null
ring:
  ; Checked: a capacity large enough to overflow this product would allocate a ring
  ; smaller than the channel then believes it has.
  %r = call {i64, i1} @llvm.umul.with.overflow.i64(i64 %cap, i64 8)
  %over = extractvalue {i64, i1} %r, 1
  br i1 %over, label %freec, label %ask
ask:
  %bytes = extractvalue {i64, i1} %r, 0
  %d = call ptr @malloc(i64 %bytes)
  %badd = icmp eq ptr %d, null
  br i1 %badd, label %freec, label %lock
lock:
  ; Sized generously on purpose: pthread_mutex_t and pthread_cond_t are opaque and
  ; differently sized on every platform, and there is no header to ask from here.
  ; Over-allocating is what makes this portable without a size to get wrong.
  %m = call ptr @malloc(i64 128)
  %badm = icmp eq ptr %m, null
  br i1 %badm, label %freed, label %cond
cond:
  %v = call ptr @malloc(i64 128)
  %badv = icmp eq ptr %v, null
  br i1 %badv, label %freem, label %init
init:
  %mi = call i32 @pthread_mutex_init(ptr %m, ptr null)
  %vi = call i32 @pthread_cond_init(ptr %v, ptr null)
  store i64 %cap, ptr %c
  %lenp = getelementptr i8, ptr %c, i64 8
  store i64 0, ptr %lenp
  %headp = getelementptr i8, ptr %c, i64 16
  store i64 0, ptr %headp
  %closedp = getelementptr i8, ptr %c, i64 24
  store i64 0, ptr %closedp
  %datap = getelementptr i8, ptr %c, i64 32
  store ptr %d, ptr %datap
  %mp = getelementptr i8, ptr %c, i64 40
  store ptr %m, ptr %mp
  %vp = getelementptr i8, ptr %c, i64 48
  store ptr %v, ptr %vp
  ; Born with one reference, held by the expression that created it.
  %rcp = getelementptr i8, ptr %c, i64 56
  store i64 1, ptr %rcp
  %n = atomicrmw add ptr @nz.chan_allocated, i64 1 monotonic
  ret ptr %c
}

define hidden void @nz.chan_retain(ptr %c) {
entry:
  %none = icmp eq ptr %c, null
  br i1 %none, label %done, label %bump
bump:
  %rcp = getelementptr i8, ptr %c, i64 56
  %old = atomicrmw add ptr %rcp, i64 1 monotonic
  br label %done
done:
  ret void
}

; The whole of a channel goes at once: the condition variable, the mutex, the ring and the
; header. Destroying the two primitives is not optional — `pthread_mutex_destroy` releases
; what the platform allocated behind an opaque handle, and freeing the memory without it
; leaks whatever that was. Neither can be in use: nothing holds a reference, so no thread
; is inside a send, a receive or a close.
define hidden void @nz.chan_release(ptr %c) {
entry:
  %none = icmp eq ptr %c, null
  br i1 %none, label %done, label %drop
drop:
  %rcp = getelementptr i8, ptr %c, i64 56
  %old = atomicrmw sub ptr %rcp, i64 1 release
  %last = icmp eq i64 %old, 1
  br i1 %last, label %destroy, label %done
destroy:
  fence acquire
  %vp = getelementptr i8, ptr %c, i64 48
  %v = load ptr, ptr %vp
  %dv = call i32 @pthread_cond_destroy(ptr %v)
  call void @free(ptr %v)
  %mp = getelementptr i8, ptr %c, i64 40
  %m = load ptr, ptr %mp
  %dm = call i32 @pthread_mutex_destroy(ptr %m)
  call void @free(ptr %m)
  %datap = getelementptr i8, ptr %c, i64 32
  %data = load ptr, ptr %datap
  call void @free(ptr %data)
  call void @free(ptr %c)
  %n = atomicrmw add ptr @nz.chan_reclaimed, i64 1 monotonic
  br label %done
done:
  ret void
}

define hidden i1 @nz.chan_send(ptr %c, i64 %v) {
entry:
  %mp = getelementptr i8, ptr %c, i64 40
  %m = load ptr, ptr %mp
  %vp = getelementptr i8, ptr %c, i64 48
  %cv = load ptr, ptr %vp
  %lenp = getelementptr i8, ptr %c, i64 8
  %headp = getelementptr i8, ptr %c, i64 16
  %closedp = getelementptr i8, ptr %c, i64 24
  %datap = getelementptr i8, ptr %c, i64 32
  %l = call i32 @pthread_mutex_lock(ptr %m)
  br label %check
check:
  ; Closed is tested first and on every pass, so a channel closed while this sender
  ; waited refuses rather than queueing into something nobody will drain.
  %closed = load i64, ptr %closedp
  %isclosed = icmp ne i64 %closed, 0
  br i1 %isclosed, label %refuse, label %space
space:
  %len = load i64, ptr %lenp
  %cap = load i64, ptr %c
  %full = icmp sge i64 %len, %cap
  br i1 %full, label %wait, label %store
wait:
  %w = call i32 @pthread_cond_wait(ptr %cv, ptr %m)
  br label %check
refuse:
  %u1 = call i32 @pthread_mutex_unlock(ptr %m)
  ret i1 false
store:
  %head = load i64, ptr %headp
  %sum = add i64 %head, %len
  %idx = urem i64 %sum, %cap
  %data = load ptr, ptr %datap
  %slot = getelementptr i64, ptr %data, i64 %idx
  store i64 %v, ptr %slot
  %n1 = add i64 %len, 1
  store i64 %n1, ptr %lenp
  %b = call i32 @pthread_cond_broadcast(ptr %cv)
  %u2 = call i32 @pthread_mutex_unlock(ptr %m)
  ret i1 true
}

define hidden i1 @nz.chan_recv(ptr %c, ptr %seq, ptr %msg, i64 %mn) {
entry:
  %mp = getelementptr i8, ptr %c, i64 40
  %m = load ptr, ptr %mp
  %vp = getelementptr i8, ptr %c, i64 48
  %cv = load ptr, ptr %vp
  %lenp = getelementptr i8, ptr %c, i64 8
  %headp = getelementptr i8, ptr %c, i64 16
  %closedp = getelementptr i8, ptr %c, i64 24
  %datap = getelementptr i8, ptr %c, i64 32
  %l = call i32 @pthread_mutex_lock(ptr %m)
  br label %check
check:
  ; A queued value wins over the close flag, which is what makes closing end a channel
  ; rather than discard what is in it.
  %len = load i64, ptr %lenp
  %empty = icmp sle i64 %len, 0
  br i1 %empty, label %maybe, label %take
maybe:
  %closed = load i64, ptr %closedp
  %isclosed = icmp ne i64 %closed, 0
  br i1 %isclosed, label %drained, label %wait
wait:
  %w = call i32 @pthread_cond_wait(ptr %cv, ptr %m)
  br label %check
drained:
  %u1 = call i32 @pthread_mutex_unlock(ptr %m)
  ret i1 false
take:
  %head = load i64, ptr %headp
  %cap = load i64, ptr %c
  %data = load ptr, ptr %datap
  %slot = getelementptr i64, ptr %data, i64 %head
  %val = load i64, ptr %slot
  %h1 = add i64 %head, 1
  %nh = urem i64 %h1, %cap
  store i64 %nh, ptr %headp
  %n1 = sub i64 %len, 1
  store i64 %n1, ptr %lenp
  %b = call i32 @pthread_cond_broadcast(ptr %cv)
  %u2 = call i32 @pthread_mutex_unlock(ptr %m)
  ; The push happens after the unlock. Growing a sequence may call `realloc`, and holding
  ; a channel's mutex across an allocation would make every other task on it wait for the
  ; allocator.
  %grown = call ptr @nz.arr_grow(ptr %seq, i64 8)
  %oom = icmp eq ptr %grown, null
  br i1 %oom, label %nomem, label %push
nomem:
  ; Records and returns, like every other failure. `false` is the stand-in `chan_recv`
  ; hands back while unwinding; the caller checks the flag before it reads it.
  call void @nz.fail(ptr %msg, i64 %mn)
  ret i1 false
push:
  %sn = load i64, ptr %seq
  %sslot = getelementptr i64, ptr %grown, i64 %sn
  store i64 %val, ptr %sslot
  %sn1 = add i64 %sn, 1
  store i64 %sn1, ptr %seq
  ret i1 true
}

define hidden void @nz.chan_close(ptr %c) {
entry:
  %mp = getelementptr i8, ptr %c, i64 40
  %m = load ptr, ptr %mp
  %vp = getelementptr i8, ptr %c, i64 48
  %cv = load ptr, ptr %vp
  %l = call i32 @pthread_mutex_lock(ptr %m)
  %closedp = getelementptr i8, ptr %c, i64 24
  store i64 1, ptr %closedp
  %b = call i32 @pthread_cond_broadcast(ptr %cv)
  %u = call i32 @pthread_mutex_unlock(ptr %m)
  ret void
}
";
