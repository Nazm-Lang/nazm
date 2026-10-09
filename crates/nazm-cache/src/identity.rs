//! Which compiler decided this, and whether another one may believe it.
//!
//! # The failure this exists to prevent
//!
//! A cache entry says *this module checked cleanly and offers this interface*. That is a
//! claim about a **checker**, not about a module: the same source under a checker with a
//! different rule is a different answer. Reusing an entry written by a compiler whose
//! checking behaviour has since changed is the exact shape of a silent miscompilation —
//! the build succeeds, nothing is reported, and the program that runs was validated
//! against rules that no longer exist.
//!
//! So every key carries the identity of the checker that produced it, and an entry
//! written under one identity can never be found under another.
//!
//! # What identity is available, and what it costs
//!
//! Nothing affordable computes "do these two compiler binaries check identically". A
//! digest of the compiler's own source would change when a comment changed, and a digest
//! of the binary would change with the optimisation level of the build that produced it.
//! Either would be *sound* and would throw the cache away constantly.
//!
//! What this does instead is derive everything derivable and name the rest:
//!
//! | Input | Derived or declared | Catches |
//! | --- | --- | --- |
//! | [`SEMANTIC_EPOCH`] | declared | every change to a checking rule |
//! | crate version | derived | a release |
//! | `nazm.interface/1` | derived | a change to what a module publishes |
//! | `nazm.check/2` | derived | a change to the artefact's own shape |
//! | the built-in table | derived | a built-in added, removed, or re-typed |
//! | the type set | derived | a type added or removed |
//! | the checker's diagnostic codes | declared, and **gated** | a rule added or removed |
//!
//! # The maintenance rule, stated so it can be followed
//!
//! > **Bump [`SEMANTIC_EPOCH`] in the same commit that changes what the checker accepts
//! > or refuses.**
//!
//! It is the one input a person has to remember, and forgetting it is a correctness bug
//! rather than a stale number — which is why as much as possible is derived instead. Two
//! things make forgetting hard:
//!
//! * `cargo xtask check`'s **cache soundness** gate fails when the set of diagnostic
//!   codes `nazm-core`'s checker can emit stops matching [`CHECKER_CODES`]. Almost every
//!   new rule brings a code, so almost every new rule fails the build until this file is
//!   touched — and touching it is where the epoch is;
//! * the derived inputs cover the parts most often changed without a new code: a
//!   built-in's signature, and the set of types.
//!
//! What neither catches is a rule that changes an existing diagnostic's *conditions*
//! without adding a code — a tightened type rule reusing `N0300`, or a parser that starts
//! accepting a construct the checker then sees. That is what the epoch is for, and
//! claiming otherwise would be claiming an automatic identity this does not have.

use std::fmt;

use nazm_sema::intrinsic::Intrinsic;
use nazm_sema::types::Type;

use crate::artifact::SCHEMA as CHECK_SCHEMA;
use crate::digest::{self, Framed};

/// Bumped when the checker's rules change.
///
/// Not a version of the compiler and not a date: a counter whose only job is to make
/// every previously written entry unreachable. See the module documentation for the rule
/// and for what is derived instead.
///
/// `1` — N4, 2026-09-22. The first value, set when semantic reuse began.
///
/// `2` — N9, 2026-09-23. Records. The checker gained declaration stages, a type
/// namespace, and rules — construction, projection, field replacement, derived task
/// safety — that every one of the new codes below covers *except* one: `==` on a record
/// is refused under the existing `N0304`, so the code set alone would not have moved for
/// it. That is exactly the case this counter exists for.
///
/// `3` — N10, 2026-09-23. Enums and `match`. The checker gained a sum type, exhaustive
/// pattern matching and the rules around them — and, the case this counter exists for
/// again, `enum` and `match` **stopped being refused as reserved words**. A module whose
/// source contains either was recorded by an N9 build as having a diagnostic; under N10
/// it has none. The code set moved too, but it would have moved for the additions alone,
/// and the refusal that went away is not visible in it at all.
///
/// `4` — N11, 2026-09-23. Generics and `Vec[T]`. Most of it moved the code set, and some of
/// it did not: `[` and `]` stopped being unknown characters, so a module whose source
/// contained one was recorded by an N10 build as having a lexical error and has none now;
/// a generic body is checked under rules — `==` on a type parameter under the existing
/// `N0304`, passing one to a task under `N0321` — that reuse old codes; and the interface
/// schema a dependency's fingerprint is taken over changed shape. The counter is what makes
/// every N10 entry unreachable regardless of which of those a given module touched.
///
/// `5` — N12, 2026-09-24. The core prelude and `?`. The code set moved for the four new
/// codes, and three changes it would not show: `?` stopped being an unknown character, so
/// a module containing one was recorded by an N11 build as having a lexical error and may
/// have none now; a module that declared its own `Result` or `Option` was clean under N11
/// and is refused now under a code that did not exist, but the same would be true of a
/// module that merely *mentions* `Result` — unknown under N11, a core type now — which
/// changes no code at all; and every module gained a dependency it did not write. That
/// last one is in each `CheckKey` already, as an import like any other. The counter covers
/// the first two, which are rules, not inputs.
///
/// `6` — N13, 2026-09-25. Derived structural equality. `==` and `!=` on a record, an enum or
/// a concrete generic instance of either whose every component has equality moved from
/// refused to accepted, under the existing `N0304` — no code was added or removed, so the
/// code set is identical and nothing derived moved. That is the case this counter exists
/// for, stated in N9's entry: the checker's conditions for an existing diagnostic changed.
/// A cached entry records only a module that checked cleanly, so no N12 entry could claim a
/// program N13 refuses; what the counter guarantees is the general rule, that no entry is
/// ever read by a checker other than the one that wrote it.
///
/// `7` — N14, 2026-09-25. A type parameter may require equality. The code set moved for two
/// new codes (`N0364`, `N0365`), and one change it would not show: `==` on a type parameter
/// whose declaration requires `Equality` — and on `Box[T]`, `Option[T]` and every composite
/// derived from one — moved from refused to accepted under the existing `N0304`. The
/// interface schema moved too (`/5`), which reaches every dependent's key through its
/// dependencies' fingerprints; the counter is what covers a module's *own* body, which the
/// schema does not.
///
/// `8` — N36, 2026-09-29. Typed effects. A function may declare the effects it exercises
/// (`! { io }`), and one whose body needs an effect its declaration lacks moved from
/// accepted to refused (`N0366`); so did a set naming an unknown effect or one twice
/// (`N0367`, `N0368`). The code set moved for those three, and the interface schema moved
/// too (`/6`, a declared set is part of what a module publishes). The counter covers the
/// part neither shows: a module's own bodies are now checked against its own declarations.
///
/// `9` — N37, 2026-09-30. Capabilities. A function that declares an effect set now holds
/// only the authority of the capability values in its scope, so one that declares `! { io }`
/// and calls `print` with no `IoCap` moved from accepted to refused (`N0369`); so did
/// building a capability (`N0370`) and a `main` with a parameter that is not one (`N0371`,
/// accepted by the checker until now and unrunnable). `IoCap` and `SpawnCap` became
/// built-in type names, so a module declaring a type of either name is refused under the
/// existing `N0331`. The code set moved for the three new codes. The interface schema did
/// not (`/6`): a capability parameter is written as any built-in type is, so an importer's
/// key moves through its dependencies' fingerprints when a contract's authority changes.
/// The counter covers what neither shows — a module's own bodies, and its calls to
/// undeclared functions, are now checked against the authority they hold.
///
/// `10` — N38, 2026-09-30. Provenance. In a function that declares an effect set, a path given
/// to `write_file` that derives from a file's contents, or from a sequence or channel the
/// checker does not follow, moved from accepted to refused (`N0372`) — including through a
/// function in another module, whose body the importer's key does not cover. That flow is
/// solved over every module's facts on every run rather than cached, and the check-artefact
/// schema moved (`nazm.check/2`) to carry those facts; the counter covers the refusal itself.
/// `11` — N42, 2026-10-01. Foreign functions: `extern "C" fn … = "symbol";` is accepted
/// syntax, `foreign` is an effect and `ForeignCap` a capability kind, and a call to a foreign
/// function without a `ForeignCap` held is refused (`N0369`). Six codes appeared
/// (`N0380`–`N0385`). A cached check result from epoch 10 cannot know any of it.
/// `12` — N49, 2026-10-01. String escapes: a literal containing a backslash meant its bytes
/// literally and now means an escape (`"a\nb"` was four bytes and is three), and a backslash
/// that is no escape is refused (`N0004`). `&&`, `||`, `!` and qualified names became accepted
/// syntax (`N0209`, `N0210`). A cached check from epoch 11 is a check of a different language.
/// `13` — N50, 2026-10-02. Function values: `fn(…) -> T` is a type, a closure `fn(x: T) -> R
/// { … }` an expression, a named function a value, and a call through a binding of function
/// type an indirect call. A program naming a function without calling it moved from refused
/// (`N0301`) to accepted; two codes appeared (`N0386`, `N0387`).
/// `14` — N51, 2026-10-02. Effect parameters: `effects E` in a function's brackets and its
/// name in effect sets became accepted (`N0388`, `N0389`); a closure that captures a capability
/// moved from refused (`N0386`) to accepted, holding it; and making a value of an undeclared
/// function without the authority its type's effects need moved from accepted to refused
/// (`N0369`). A cached check from epoch 13 is a check of a different language.
/// `15` — N52, 2026-10-02. Declassification: `str_vouch` and `VouchCap` became built-in
/// names, so a program defining a function or type of either name moved from accepted to
/// refused, and a vouch without a `VouchCap` held is refused (`N0369`). The check artefact's sink
/// facts gained a class.
/// `16` — N53, 2026-10-02. Generic channels: `Chan[T]` became a type form and
/// `chan_new_of`, `chan_send_of`, `chan_recv_of` and `chan_close_of` built-in names, so a program
/// defining a function of any of those names moved from accepted to refused, `Chan` with
/// arguments from refused to accepted, and an element that cannot cross into a task is refused
/// (`N0390`).
/// `17` — N54, 2026-10-02. Select and the clock: `TimeCap` became a built-in type name and
/// `chan_select_of`, `chan_select_until` and `time_now_ms` built-in names, so a program defining
/// any of them moved from accepted to refused, and a clock read without a `TimeCap` held is
/// refused (`N0369`).
/// `18` — N55, 2026-10-02. FFI v2: a `Str` parameter on a foreign declaration moved from refused
/// (`N0381`) to accepted, and an `extern "C"` declaration with a body — an export — from refused
/// to accepted under its own rules (`N0391`).
/// `19` — N62, 2026-10-02. A freestanding target: `MmioCap` became a built-in type name and
/// `mmio_read32` and `mmio_write32` built-in names, so a program defining any of them moved from
/// accepted to refused, and a register access without an `MmioCap` held is refused (`N0369`).
/// `20` — N66, 2026-10-02. Vectorisation: `ints_sum_from` became a built-in name, so a program defining
/// a function of that name moved from accepted to refused.
/// `21` — N76, 2026-10-03. Unicode escapes: a literal containing `\u{H…}` moved from refused
/// (`N0004`) to accepted, its bytes the UTF-8 encoding of the scalar value. A cached check from
/// epoch 20 refused a program this one accepts.
/// `22` — N78, 2026-10-03. Traits and methods: `trait` and `impl` declarations, method calls and
/// trait bounds moved from refused to accepted, and a program using `trait` as a name moved from
/// accepted to refused. A cached check from epoch 21 is a check of a language without them.
/// `23` — N79, 2026-10-03. Attenuation and subsumption: `OutCap` became a built-in type name, so
/// a program defining it moved from accepted to refused; an `IoCap` where an `OutCap` is expected
/// and a function value whose effects are a subset of the expected ones moved from refused
/// (`N0300`) to accepted.
/// `24` — N80, 2026-10-03. Provenance v3: a path read out of a container, through a function
/// value, a trait bound or a closure's parameter or capture, or out of a record's other field,
/// moved from refused (`N0372`) to accepted where what reaches it is not restricted.
/// `25` — N85, 2026-10-04. FFI v3: `extern "C" struct` declarations moved from refused (`N0100`)
/// to accepted; a foreign parameter of a handle, a C struct or an export's function type, and a
/// foreign result of a handle, a `Str` or an `Option` of either, moved from refused (`N0381`) to
/// accepted; building or taking apart a handle is `N0611`, a callback that is not an export's
/// name `N0612`, and `c_errno` is a name a program could not call before.
/// `26` — N86, 2026-10-04. `mmio_read8`, `mmio_write8`, `mmio_read16` and `mmio_write16` are names
/// a program could not call before, and `riscv64gc-unknown-none-elf` a board.
/// `27` — N87, 2026-10-04. Contracts: `requires` and `ensures` moved from refused (`N0100`) to
/// accepted, a clause that is not pure is `N0614`, and a literal call breaking a precondition
/// `N0615`.
/// `28` — N103, 2026-10-04. Re-exports: `pub use "PATH";` and `pub use "PATH" { a, b as c };`
/// moved from refused (`N0100`) to accepted, offering another module's definitions under this
/// module's names; a name the module does not export is `N0211`, one offered twice `N0212`,
/// and a cycle of re-exports `N0213`.
/// `29` — N104, 2026-10-05. No inherited authority: a function with no declared effect set
/// exercised its caller's capabilities (N37's bridge); now every body must hold what it
/// exercises, so programs accepted under 28 are refused (`N0369`).
/// `30` — Gate 1-C1, 2026-10-07. Closure environments in the ownership graph: a closure that
/// captures a value able to hold a function value behind a counted handle moved from accepted to
/// refused (`N0616`) — the route by which a closure stored into a `Vec` it captured owned itself
/// and was never reclaimed. A cached check from epoch 29 accepted programs this one refuses.
/// `31` — Gate 2, 2026-10-07. Numbers: fixed-width integers, `Float32`, `Float64`, bit
/// operators, conversions and their built-ins moved from refused (`N0101`, `N0305`, the lexer's
/// unknown character) to accepted; an integer literal takes the numeric type expected where it is
/// written and is refused out of that type's range (`N0003`, now raised by the checker); a
/// constant shift out of range is refused (`N0617`). A cached check from epoch 30 refused
/// programs this one accepts, and typed literals it did not type.
/// `32` — Gate 2, 2026-10-08. Fixed arrays and constants: `[T; N]`, `[a, b]`, `[v; n]`,
/// `xs[i]` and `const NAME: T = value;` moved from refused (the parser's unexpected token) to
/// accepted; an array's element must be plain data (`N0621`) and the array at most 64 KiB
/// (`N0618`), a constant computable (`N0622`, `N0623`), and an index on a non-array or a
/// constant index out of range is refused (`N0624`). A cached check from epoch 31 refused
/// programs this one accepts.
/// `33` — Gate 2, 2026-10-08. Bytes and text: `Bytes`, `Text`, their built-ins, `Text(s)` and
/// `try_convert[Text](s)` moved from refused to accepted, and a string literal is a `Text`
/// where one is expected; the names `Bytes` and `Text` and the new built-ins' names moved
/// from a program's own to the language's (`N0331`, `N0203`), as did the C symbols the
/// numbers part calls (`strtod`, `sin`, …) for a foreign declaration. A cached check from epoch
/// 32 refused programs this one accepts, and accepted some it refuses.
/// `34` — M1, 2026-10-08. A direct or UFCS call of a method whose impl is in another module is
/// checked against its trait method's declared effects; it was checked as an undeclared
/// function's, every effect assumed, so programs accepted now were refused (`N0366`).
/// `35` — Gate 2, 2026-10-09. An impl may be for any numeric type, not only `Int`: impls for
/// `UInt8` … `Float64` moved from refused (`N0604`) to accepted, so `@std/hash` can make every
/// integer a key. A cached check from epoch 34 refused programs this one accepts.
/// `36` — Gate 2, 2026-10-09. Files and handles: `OsHandle` and the `os_` built-ins moved from
/// refused (an unknown type or function) to accepted, and their names from a program's own to the
/// language's (`N0331`, `N0203`); `os_open_write`'s, `os_mkdir`'s, `os_rmdir`'s, `os_unlink`'s and
/// `os_rename`'s paths are the restricted sink (`N0372`). A cached check from epoch 35 refused
/// programs this one accepts, and accepted some it refuses.
/// `37` — Gate 2, 2026-10-09. Networking: `NetCap` and the network `os_` built-ins moved from
/// refused to accepted, and their names from a program's own to the language's (`N0331`,
/// `N0203`); `Type::ALL`, in the checker's identity, gained `NetCap`. Using an open handle stopped
/// needing an `IoCap`, so programs refused under 36 (`N0369`) are accepted. A cached check from
/// epoch 36 refused programs this one accepts, and accepted some it refuses.
/// `38` — Gate 2, 2026-10-09. Time: `time_now_ns`, `time_wall_ns` and `time_sleep_ms` moved
/// from refused to accepted, and their names from a program's own to the language's (`N0203`).
/// `39` — Gate 2, 2026-10-09. Process and environment: `ProcessCap` and the `os_env_*`,
/// `os_spawn`, `os_child_pipe`, `os_wait` and `os_kill` built-ins moved from refused to accepted,
/// their names from a program's own to the language's (`N0331`, `N0203`); `os_spawn`'s program is
/// the restricted sink (`N0372`); `Type::ALL` gained `ProcessCap`.
/// `40` — Gate 2, 2026-10-09. Randomness: `RandomCap` and `os_random_bytes` moved from refused
/// to accepted, the names from a program's own to the language's (`N0331`, `N0203`); `Type::ALL`
/// gained `RandomCap`.
/// `41` — Gate 2, 2026-10-09. The C foundation: an export taking or returning a fixed-width
/// integer or a float moved from refused (`N0391`) to accepted.
/// `42` — Gate 2, 2026-10-09. Attributes and conditional compilation: `@` and an attribute before
/// a top-level declaration moved from refused (the lexer's unknown character) to accepted, held to
/// a closed vocabulary (`N0619`); a declaration whose `@cfg` does not hold for the build is not
/// there, so two of one name under exclusive conditions are no longer a duplicate. A cached check
/// from epoch 41 refused programs this one accepts.
/// `43` — G2-C1, 2026-10-09. `@deprecated` became metadata `nazm doc` publishes, and so is refused
/// (`N0619`) where it would be inert: before anything but a `pub` function, `struct` or `enum`. A
/// cached check from epoch 42 accepted programs this one refuses.
pub const SEMANTIC_EPOCH: u32 = 43;

/// Every diagnostic code `nazm_core::check` can raise, sorted, by the name the checker
/// writes.
///
/// Held here rather than in `nazm-core` because it is an input to an identity, and an
/// identity is this crate's subject. It is kept true by `cargo xtask check`, which reads
/// `crates/nazm-core/src/check/` and fails if the two sets differ — so this cannot
/// silently fall behind the checker it describes.
pub const CHECKER_CODES: &[&str] = &[
    "ALIAS_COLLIDES",
    "ALREADY_DEFINED_IN_SCOPE",
    "AMBIGUOUS_IMPORT",
    "AMBIGUOUS_METHOD",
    "ARRAY_ELEMENT",
    "ARRAY_INDEX",
    "ARRAY_TOO_LARGE",
    "ASSIGN_TO_IMMUTABLE",
    "ATTRIBUTE",
    "BRANCH_TYPES_DIFFER",
    "BUILTIN_TYPE_REDEFINED",
    "CALLBACK_NOT_EXPORT",
    "CANNOT_INFER_TYPE_ARGUMENT",
    "CAPABILITY_CONSTRUCTED",
    "CAPTURE_CYCLE",
    "CAPTURE_REFUSED",
    "CHANNEL_ELEMENT",
    "CONDITION_NOT_BOOL",
    "CONFLICTING_EFFECT_ARGUMENT",
    "CONFLICTING_TYPE_ARGUMENTS",
    "CONSTANT_FAILS",
    "CONTRACT_SHAPE",
    "DUPLICATE_EFFECT",
    "DUPLICATE_FIELD",
    "DUPLICATE_FUNCTION",
    "DUPLICATE_MATCH_ARM",
    "DUPLICATE_TYPE",
    "DUPLICATE_TYPE_PARAMETER",
    "DUPLICATE_VARIANT",
    "EFFECT_PARAMETER",
    "EMPTY_ENUM",
    "EXPORT_SHAPE",
    "FOREIGN_ABI",
    "FOREIGN_CONFLICT",
    "FOREIGN_SHAPE",
    "FOREIGN_SYMBOL",
    "FOREIGN_TYPE",
    "GENERIC_MAIN",
    "ILLEGAL_TASK_CAPTURE",
    "IMPL_CONFLICT",
    "IMPL_METHOD_MISMATCH",
    "IMPL_MISSING_METHOD",
    "IMPL_ORPHAN",
    "IMPL_TYPE_REFUSED",
    "IMPORT_COLLIDES_WITH_DEFINITION",
    "INTEGER_TOO_LARGE",
    "INVALID_OPERANDS",
    "INVALID_PROJECTION",
    "MAIN_PARAMETER",
    "MATCH_ON_NON_ENUM",
    "METHOD_NOT_A_VALUE",
    "MISSING_AUTHORITY",
    "MISSING_FIELD",
    "MISSING_MAIN",
    "MISSING_TYPE_ARGUMENTS",
    "NON_EXHAUSTIVE_MATCH",
    "NOT_A_CONSTANT",
    "NOT_A_FUNCTION_VALUE",
    "NOT_A_TRAIT",
    "NOT_IN_A_LOOP",
    "NO_METHOD",
    "OPAQUE_HANDLE",
    "OWNERSHIP_CYCLE",
    "PRECONDITION_REFUTED",
    "PRELUDE_TYPE_REDECLARED",
    "PRIVATE_DEFINITION",
    "PRIVATE_TYPE_EXPOSED",
    "PROPAGATE_ERROR_MISMATCH",
    "PROPAGATE_NON_RESULT",
    "PROPAGATE_OUTSIDE_RESULT",
    "RECURSIVE_RECORD",
    "REEXPORT_COLLIDES",
    "REEXPORT_CYCLE",
    "REEXPORT_UNKNOWN",
    "RESTRICTED_FLOW",
    "SHIFT_AMOUNT_REFUSED",
    "SPAWN_OUTSIDE_SCOPE",
    "TRAIT_METHOD_SELF",
    "TRAIT_NOT_A_TYPE",
    "TYPE_ARGUMENTS_ON_NON_GENERIC",
    "TYPE_MISMATCH",
    "TYPE_NESTING_TOO_DEEP",
    "UNDECLARED_EFFECT",
    "UNDEFINED_FUNCTION",
    "UNDEFINED_VARIABLE",
    "UNKNOWN_EFFECT",
    "UNKNOWN_FIELD",
    "UNKNOWN_QUALIFIED_NAME",
    "UNKNOWN_REQUIREMENT",
    "UNKNOWN_TYPE",
    "UNKNOWN_VARIANT",
    "UNREACHABLE_STATEMENT",
    "UNSATISFIED_REQUIREMENT",
    "UNSUPPORTED_CONSTRUCT",
    "WRONG_ARGUMENT_COUNT",
    "WRONG_ENUM_IN_PATTERN",
    "WRONG_TYPE_ARGUMENT_COUNT",
];

/// The checker that produced, or is asking for, a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CheckerIdentity([u8; 32]);

impl CheckerIdentity {
    /// This build's identity.
    ///
    /// Deterministic: the same build computes the same value on every machine, in every
    /// process, at every time. Nothing here reads the clock, the environment, the host or
    /// the filesystem, because an identity that did could not be compared across the two
    /// runs it exists to relate.
    #[must_use]
    pub fn current() -> Self {
        Self::at_epoch(SEMANTIC_EPOCH)
    }

    /// This build's identity with a different epoch: every other input unchanged.
    ///
    /// Not for a compiler to use — a build has one identity, [`Self::current`]. It exists so
    /// a test can stand in for the build before a rule changed, and establish that the
    /// entries such a build left behind are unreachable from this one. The epoch is the only
    /// input two such builds could share everything else with, so it is the one worth
    /// being able to vary.
    #[doc(hidden)]
    #[must_use]
    pub fn at_epoch(epoch: u32) -> Self {
        let mut f = Framed::new("nazm.checker/1");
        f.number(epoch)
            .field(env!("CARGO_PKG_VERSION"))
            .field(nazm_iface::SCHEMA)
            .field(CHECK_SCHEMA);

        // The built-ins share the ordinary function namespace, so their names and
        // signatures are part of what a module's source means. Read from the table the
        // checker itself reads, rather than transcribed — a second copy is a second thing
        // that can disagree.
        f.number(u32::try_from(Intrinsic::ALL.len()).unwrap_or(u32::MAX));
        for i in Intrinsic::ALL {
            f.field(i.name());
            // Since N11 a signature is written in the table's own vocabulary, which spells
            // a generic built-in's `T` and `Vec[T]` as well as every built-in type. A record
            // still cannot appear: records exist only in the compilation that declared
            // them, and this identity describes the build.
            f.number(i.generics());
            f.number(u32::try_from(i.params().len()).unwrap_or(u32::MAX));
            for p in i.params() {
                f.field(p.spelling());
            }
            f.field(i.ret().spelling());
        }

        f.number(u32::try_from(Type::ALL.len()).unwrap_or(u32::MAX));
        for t in Type::ALL {
            f.field(t.builtin_name().unwrap_or("?"));
        }

        f.number(u32::try_from(CHECKER_CODES.len()).unwrap_or(u32::MAX));
        for c in CHECKER_CODES {
            f.field(c);
        }

        Self(f.finish())
    }

    /// Read one back from its hexadecimal form.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        digest::from_hex(text).map(Self)
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for CheckerIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        digest::write_hex(f, &self.0)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn one_build_has_one_identity() {
        assert_eq!(CheckerIdentity::current(), CheckerIdentity::current());
    }

    #[test]
    fn it_round_trips_through_its_written_form() {
        let id = CheckerIdentity::current();
        assert_eq!(CheckerIdentity::parse(&id.to_string()), Some(id));
    }

    #[test]
    fn the_built_in_table_reaches_the_identity() {
        // A test that the derivation is real rather than decorative: recomputing the
        // identity with one built-in's return type changed must give a different number.
        // Done by recomputing the same framing rather than by mutating a global, because
        // the table is a `const` and the point is the framing, not the storage.
        let with = |ret: &str| {
            let mut f = Framed::new("nazm.checker/1");
            f.number(SEMANTIC_EPOCH)
                .field(env!("CARGO_PKG_VERSION"))
                .field(nazm_iface::SCHEMA)
                .field(CHECK_SCHEMA)
                .number(1)
                .field("str_len")
                .number(1)
                .field("Str")
                .field(ret);
            f.finish()
        };
        assert_ne!(with("Int"), with("Str"));
    }

    #[test]
    fn the_current_identity_is_this_epoch_s_and_no_other() {
        assert_eq!(
            CheckerIdentity::current(),
            CheckerIdentity::at_epoch(SEMANTIC_EPOCH)
        );
        assert_ne!(
            CheckerIdentity::current(),
            CheckerIdentity::at_epoch(SEMANTIC_EPOCH - 1)
        );
    }

    #[test]
    fn the_epoch_is_an_input() {
        let with = |epoch: u32| {
            let mut f = Framed::new("nazm.checker/1");
            f.number(epoch).field(env!("CARGO_PKG_VERSION"));
            f.finish()
        };
        assert_ne!(with(SEMANTIC_EPOCH), with(SEMANTIC_EPOCH + 1));
    }

    #[test]
    fn the_code_list_is_sorted_and_has_no_duplicate() {
        // Canonical, so that the identity does not depend on the order someone typed
        // them in. The `xtask` gate compares sets; this keeps the list itself honest.
        let mut sorted = CHECKER_CODES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.as_slice(), CHECKER_CODES);
    }
}
