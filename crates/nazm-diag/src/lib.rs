//! Diagnostics as data first, prose second.
//!
//! Every diagnostic carries a **stable code**, a span, and where possible a suggested
//! repair. The terminal rendering is a view of that structure, not the structure itself.
//!
//! The reason is the project's whole premise: an agent that has to regex a prose message
//! to find out what went wrong spends turns discovering what the compiler already knew.
//! A code it can match on, a span it can edit at, and a suggestion it can apply turn a
//! repair loop into a single step. Codes are therefore part of the contract — renaming
//! one is a breaking change, and they are never reused.

use nazm_span::{SourceMap, Span};
use serde::{Deserialize, Serialize};

/// A stable diagnostic code. The number never changes meaning and is never reused.
///
/// A `Cow` rather than a `&'static str`: the constants below want to be `const`, and a
/// diagnostic read back from JSON owns its string. Borrowing covers the first, owning the
/// second, and callers compare them without caring which.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Code(pub std::borrow::Cow<'static, str>);

impl Code {
    #[must_use]
    pub const fn new(s: &'static str) -> Self {
        Self(std::borrow::Cow::Borrowed(s))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Every code the v0.1 front end can produce.
///
/// Listed in one place on purpose: a code introduced ad hoc at its throw site is a code
/// nobody documents, and the set an agent can expect to see is part of the language's
/// interface.
pub mod codes {
    use super::Code;

    // Lexing
    pub const UNKNOWN_CHARACTER: Code = Code::new("N0001");
    pub const UNTERMINATED_STRING: Code = Code::new("N0002");
    pub const INTEGER_TOO_LARGE: Code = Code::new("N0003");
    /// A backslash in a string literal that is not one of the escapes `docs/spec.md` lists
    /// (N49): an unknown letter, `\x` without two hex digits, or `\x` above `7F`.
    pub const INVALID_ESCAPE: Code = Code::new("N0004");

    // Parsing
    /// An expression or block nested past the parser's limit. A parse-time bound, so
    /// every later pass over the tree — checking, evaluating, and dropping it — inherits
    /// it without needing a limit of its own.
    pub const NESTING_TOO_DEEP: Code = Code::new("N0102");
    pub const UNEXPECTED_TOKEN: Code = Code::new("N0100");
    pub const UNSUPPORTED_CONSTRUCT: Code = Code::new("N0101");

    // Names and scope
    pub const UNDEFINED_VARIABLE: Code = Code::new("N0200");
    pub const UNDEFINED_FUNCTION: Code = Code::new("N0201");
    pub const ALREADY_DEFINED_IN_SCOPE: Code = Code::new("N0202");
    pub const DUPLICATE_FUNCTION: Code = Code::new("N0203");
    pub const MISSING_MAIN: Code = Code::new("N0204");
    /// `use "path";` named a file that could not be read.
    pub const MODULE_NOT_FOUND: Code = Code::new("N0205");
    /// A name that exists in an imported module, and is private there.
    ///
    /// Separate from [`UNDEFINED_FUNCTION`] on purpose. "Not defined" is true from the
    /// caller's position and useless: the reader can see the definition in the file they
    /// just imported. Saying that it exists and is private names the repair.
    pub const PRIVATE_DEFINITION: Code = Code::new("N0206");
    /// Two directly imported modules export the same name.
    ///
    /// Reported where the ambiguity is created — the second `use` — because the language
    /// has no qualified path with which a later mention could say which one it meant.
    pub const AMBIGUOUS_IMPORT: Code = Code::new("N0207");
    /// An import brings in a name this module also defines.
    pub const IMPORT_COLLIDES_WITH_DEFINITION: Code = Code::new("N0208");
    /// `use "PATH" as NAME;` where `NAME` is already a function, a type or another import's
    /// name in the importing module (N49).
    pub const ALIAS_COLLIDES: Code = Code::new("N0209");
    /// `NAME::item` where `NAME` is no alias, or the aliased module exports no `item` (N49).
    pub const UNKNOWN_QUALIFIED_NAME: Code = Code::new("N0210");
    /// A `pub use` names something the module it re-exports from does not export (N103).
    pub const REEXPORT_UNKNOWN: Code = Code::new("N0211");
    /// A module offers one name twice, its own or a re-export's (N103).
    pub const REEXPORT_COLLIDES: Code = Code::new("N0212");
    /// Modules re-export from each other in a cycle (N103).
    pub const REEXPORT_CYCLE: Code = Code::new("N0213");

    // Types
    pub const TYPE_MISMATCH: Code = Code::new("N0300");
    pub const CONDITION_NOT_BOOL: Code = Code::new("N0301");
    pub const BRANCH_TYPES_DIFFER: Code = Code::new("N0302");
    pub const WRONG_ARGUMENT_COUNT: Code = Code::new("N0303");
    pub const INVALID_OPERANDS: Code = Code::new("N0304");
    pub const UNKNOWN_TYPE: Code = Code::new("N0305");

    // Mutation
    pub const ASSIGN_TO_IMMUTABLE: Code = Code::new("N0310");
    pub const MUTABLE_PARAMETER: Code = Code::new("N0311");

    /// `break` or `continue` outside a loop.
    pub const NOT_IN_A_LOOP: Code = Code::new("N0312");
    /// A statement that provably cannot run.
    pub const UNREACHABLE_STATEMENT: Code = Code::new("N0313");
    /// `break` or `continue` where a value is expected.
    pub const CONTROL_FLOW_AS_VALUE: Code = Code::new("N0314");

    /// Runaway recursion, caught before the stack is exhausted.
    pub const CALL_DEPTH_LIMIT: Code = Code::new("N0403");

    // Execution
    pub const INTEGER_OVERFLOW: Code = Code::new("N0400");
    pub const DIVISION_BY_ZERO: Code = Code::new("N0401");
    /// The interpreter's iteration guard, not a language rule. See `docs/spec.md`.
    pub const ITERATION_LIMIT: Code = Code::new("N0402");
    /// The host refused what running needs: the interpreter could not start, or — in a native
    /// build — the operating system would not start a task's thread (§7.45, *The failure
    /// model*). Not the program's fault, and said so plainly rather than reported as a failure
    /// of the code being run.
    pub const INTERPRETER_UNAVAILABLE: Code = Code::new("N0404");

    /// An index outside a value's bounds: `str_byte` past the end, a slice whose range
    /// is inverted or runs off either end. A *language* rule, unlike `N0402`/`N0403`:
    /// `docs/spec.md` specifies the failure, so every implementation must produce it.
    pub const INDEX_OUT_OF_BOUNDS: Code = Code::new("N0405");

    /// The allocator refused. Reported where an implementation can detect it: a compiled
    /// program checks what `malloc` returned. The interpreter cannot produce this,
    /// because its host aborts the process on allocation failure — a stated difference
    /// between the two, not a gap in the specification.
    pub const ALLOCATION_FAILED: Code = Code::new("N0406");

    /// A file could not be read or written. The path is in the help, not the message,
    /// so the two implementations report the same sentence.
    pub const IO_FAILED: Code = Code::new("N0407");

    /// A compiled program's call would exhaust its thread's stack (N49): the runtime's
    /// stack check, made on entry to every function in a call cycle.
    pub const STACK_EXHAUSTED: Code = Code::new("N0408");

    /// A foreign function returned null where its declaration promised a value (N85): a
    /// handle result, or a `Str` result, not written as an `Option`. Checked after C returns
    /// and before any Nazm code sees the pointer.
    pub const NULL_FROM_C: Code = Code::new("N0409");

    /// A precondition that did not hold as the function was entered (N87): a `requires`
    /// clause evaluated `false`.
    pub const PRECONDITION_FAILED: Code = Code::new("N0410");

    /// A postcondition that did not hold as the function returned (N87): an `ensures` clause
    /// evaluated `false` with `result` the value being returned.
    pub const POSTCONDITION_FAILED: Code = Code::new("N0411");

    /// A shift or rotation by a negative amount, or a shift by the operand's width or more
    /// (Gate 2): refused rather than masked or left to the machine.
    pub const SHIFT_OUT_OF_RANGE: Code = Code::new("N0412");
    /// Bytes that are not UTF-8 made into a `Text`, a `Text` cut or read away from a scalar
    /// boundary, or an `Int` that is not a Unicode scalar value made into one (Gate 2).
    pub const NOT_UTF8: Code = Code::new("N0413");

    /// `spawn` outside a `scope`.
    pub const SPAWN_OUTSIDE_SCOPE: Code = Code::new("N0320");
    /// A value that may not cross into a task: an `Ints` or a `Strs`, which are handles
    /// to unsynchronised shared mutable storage — or a record that contains one, at any
    /// depth, in which case the message names the field path that made it illegal.
    pub const ILLEGAL_TASK_CAPTURE: Code = Code::new("N0321");

    // Records. N9 allocated nine codes from the unused range and renumbered none:
    // `docs/diagnostics.md` makes a code permanent, and a shifted code is worse than an
    // ugly one. Where a record failure is the same *semantic category* as an existing
    // one, the existing code is reused rather than duplicated under a new number —
    // assigning to an immutable binding is `N0310` whether or not a field is involved,
    // an unknown type name is `N0305`, `==` on a record is `N0304`, and an ambiguous or
    // colliding imported name is `N0207`/`N0208`. A code per syntax spelling would make
    // the set larger and mean less.
    /// Two types declared with one name in one module. Records and enums share the type
    /// namespace, so `struct Item` beside `enum Item` is this.
    pub const DUPLICATE_TYPE: Code = Code::new("N0330");
    /// A type declared with a built-in type's name. Built-ins may not be shadowed:
    /// `Int` means the same thing in every module of every program.
    pub const BUILTIN_TYPE_REDEFINED: Code = Code::new("N0331");
    /// Two fields of one record, or of one variant's payload, with one name. Also a
    /// pattern that names one payload field twice.
    pub const DUPLICATE_FIELD: Code = Code::new("N0332");
    /// A field named in a construction, a projection or a pattern that the record or
    /// variant does not have.
    pub const UNKNOWN_FIELD: Code = Code::new("N0333");
    /// A construction that does not give every field a value, or a pattern that does not
    /// name every payload field. There are no defaults and there is no rest pattern.
    pub const MISSING_FIELD: Code = Code::new("N0334");
    /// `.field` on something that is not a record — including an enum, whose payload is
    /// reached by `match` and not by projection.
    pub const INVALID_PROJECTION: Code = Code::new("N0335");
    /// A user-defined type that contains itself by value, directly or through others. Not
    /// a safety problem — a layout that does not exist.
    pub const RECURSIVE_RECORD: Code = Code::new("N0336");
    /// A public function, record or enum whose signature, fields or payloads name a
    /// private type, which an importer could not resolve.
    pub const PRIVATE_TYPE_EXPOSED: Code = Code::new("N0337");
    /// User-defined types nested inside one another past the stated bound.
    pub const TYPE_NESTING_TOO_DEEP: Code = Code::new("N0338");

    // Enums and pattern matching. N10 allocated seven codes and renumbered none, and
    // reused an existing code wherever the failure is the same *semantic category*: a
    // missing payload field in a pattern is `N0334` because it is the same mistake as a
    // missing field in a construction, a payload type mismatch is `N0300`, `==` on an
    // enum is `N0304`, a private payload type is `N0337`, and a layout cycle through a
    // variant is `N0336`. What is genuinely new is what only a sum type can get wrong.
    /// An enum with no variants. A type with no values raises `Never` semantics the
    /// language has not settled, so it is refused rather than admitted by accident.
    pub const EMPTY_ENUM: Code = Code::new("N0339");
    /// Two variants of one enum with one name.
    pub const DUPLICATE_VARIANT: Code = Code::new("N0340");
    /// A variant named in a construction or a pattern that the enum does not have.
    pub const UNKNOWN_VARIANT: Code = Code::new("N0341");
    /// `match` on a value that is not an enum. There is no literal or record matching.
    pub const MATCH_ON_NON_ENUM: Code = Code::new("N0342");
    /// A `match` that does not name every variant of its enum. There is no wildcard arm
    /// and no runtime trap standing in for this check.
    pub const NON_EXHAUSTIVE_MATCH: Code = Code::new("N0343");
    /// A `match` that names one variant twice. The second arm can never be selected.
    pub const DUPLICATE_MATCH_ARM: Code = Code::new("N0344");
    /// A pattern whose variant belongs to a different enum than the scrutinee. There is
    /// no structural variant matching.
    pub const WRONG_ENUM_IN_PATTERN: Code = Code::new("N0345");

    // N11: generics. Allocated after the enum block without renumbering, and only for what
    // is genuinely new. A parameter named like a built-in is `N0331`, an unknown parameter
    // name is `N0305`, an argument that disagrees with explicit type arguments is `N0300`,
    // arithmetic or `==` on an unconstrained `T` is `N0304`, projecting or matching one is
    // `N0335` or `N0342`, passing one to a task is `N0321`, a written type nested too deep
    // is `N0338`, and an inline layout cycle through a generic type is `N0336` — each the
    // same mistake it already named.
    /// Two type parameters of one declaration with one name.
    pub const DUPLICATE_TYPE_PARAMETER: Code = Code::new("N0350");
    /// Type arguments written on something that takes none: a non-generic definition, a
    /// built-in type, or a type parameter.
    pub const TYPE_ARGUMENTS_ON_NON_GENERIC: Code = Code::new("N0351");
    /// A generic definition, or `Vec`, written where a complete type is required but
    /// without its type arguments.
    pub const MISSING_TYPE_ARGUMENTS: Code = Code::new("N0352");
    /// A generic definition applied to the wrong number of type arguments.
    pub const WRONG_TYPE_ARGUMENT_COUNT: Code = Code::new("N0353");
    /// A generic call whose arguments do not determine every type parameter. The expected
    /// result type never does, so `make()` for `fn make[T]() -> T` is this.
    pub const CANNOT_INFER_TYPE_ARGUMENT: Code = Code::new("N0354");
    /// Two arguments of one call that give one type parameter different types.
    pub const CONFLICTING_TYPE_ARGUMENTS: Code = Code::new("N0355");
    /// A generic `main`: there is no caller to choose its type arguments.
    pub const GENERIC_MAIN: Code = Code::new("N0356");
    /// A generic function whose native instances would never end — polymorphic recursion,
    /// `f[T]` calling `f[Vec[T]]`. `nazm build` only; the interpreter needs no instances.
    pub const UNBOUNDED_SPECIALISATION: Code = Code::new("N0357");
    /// More native generic instances than one program may have, or one nested too deeply.
    /// `nazm build` only; a stated bound, not a crash.
    pub const GENERIC_INSTANCE_LIMIT: Code = Code::new("N0358");
    /// A type that can own itself through a reference-counted container — `Node {
    /// children: Vec[Node] }`. Its layout is finite; the refusal is of a value no count
    /// could reclaim. The inline case, with no finite layout, is `N0336`.
    pub const OWNERSHIP_CYCLE: Code = Code::new("N0359");

    // N12: typed error values. `Result` and `Option` are ordinary generic enums declared by
    // the core prelude, so constructing, matching and returning them raise the codes every
    // enum does; these are the propagation operator's, and the prelude's reserved names.
    /// `?` applied to something that is not the core prelude's `Result` — an `Option`, a
    /// project enum shaped like one, anything else.
    pub const PROPAGATE_NON_RESULT: Code = Code::new("N0360");
    /// `?` in a function whose return type is not the core prelude's `Result`.
    pub const PROPAGATE_OUTSIDE_RESULT: Code = Code::new("N0361");
    /// `?` whose operand's error type is not exactly the enclosing function's. There is no
    /// conversion between error types.
    pub const PROPAGATE_ERROR_MISMATCH: Code = Code::new("N0362");
    /// A project type named like one of the core prelude's, which every module sees.
    pub const PRELUDE_TYPE_REDECLARED: Code = Code::new("N0363");

    // N14: a type parameter may require equality. `==` on a parameter whose declaration does
    // not require it is still `N0304`; a requirement written on a record's or an enum's
    // parameter, which N14 does not support, is `N0101`.
    /// A type argument, written or inferred, that does not have a capability its parameter
    /// requires — a concrete type without equality, or a caller's own parameter whose
    /// declaration does not require it.
    pub const UNSATISFIED_REQUIREMENT: Code = Code::new("N0364");
    /// A requirement naming something that is not a capability: `T: Ord`. `Equality` is
    /// the only one.
    pub const UNKNOWN_REQUIREMENT: Code = Code::new("N0365");

    // N36: effects. A malformed effect set — no `{`, a word that is not a name — is the
    // parser's `N0100`.
    /// A function whose declared effect set lacks an effect its body requires.
    pub const UNDECLARED_EFFECT: Code = Code::new("N0366");
    /// A name in an effect set that is not an effect: `! { network }`.
    pub const UNKNOWN_EFFECT: Code = Code::new("N0367");
    /// An effect written twice in one set: `! { io, io }`.
    pub const DUPLICATE_EFFECT: Code = Code::new("N0368");
    /// An operation whose authority the function does not hold (N37): a body with a
    /// declared effect set that calls `print` with no `IoCap` in scope. Not an effect
    /// error — the effect may be declared — and never fixed automatically: authority is
    /// threaded through parameters, which is a change to every caller.
    pub const MISSING_AUTHORITY: Code = Code::new("N0369");
    /// An attempt to build a capability: `IoCap(...)`. Authority is not constructible.
    pub const CAPABILITY_CONSTRUCTED: Code = Code::new("N0370");
    /// A `main` parameter that is not a capability: the runtime can supply nothing else.
    pub const MAIN_PARAMETER: Code = Code::new("N0371");
    /// A restricted flow (N38): in a function that declares its effects, the path given to
    /// `write_file` derives from a file's contents or from storage the checker does not
    /// follow — directly, or through a function that writes to a path it is given.
    pub const RESTRICTED_FLOW: Code = Code::new("N0372");
    /// A foreign declaration names an ABI other than `"C"` (N42).
    pub const FOREIGN_ABI: Code = Code::new("N0380");
    /// A foreign declaration takes or returns a type that cannot cross the C boundary (N42).
    pub const FOREIGN_TYPE: Code = Code::new("N0381");
    /// A foreign declaration's symbol is not a C identifier a linker can be handed (N42).
    pub const FOREIGN_SYMBOL: Code = Code::new("N0382");
    /// Two foreign declarations of one symbol disagree about its signature (N42).
    pub const FOREIGN_CONFLICT: Code = Code::new("N0383");
    /// A foreign declaration has type parameters or a written effect set (N42).
    pub const FOREIGN_SHAPE: Code = Code::new("N0384");
    /// `nazm run` was asked to run a program that calls a foreign function (N42).
    pub const FOREIGN_NOT_INTERPRETED: Code = Code::new("N0385");

    // Function values and closures (N50, `docs/architecture.md` §7.52)
    /// A closure captures a `let mut` binding or a capability (N50).
    pub const CAPTURE_REFUSED: Code = Code::new("N0386");
    /// A function that cannot be a value — generic, foreign or built-in — is used as one, or
    /// a closure is written inside a generic function (N50).
    pub const NOT_A_FUNCTION_VALUE: Code = Code::new("N0387");

    // Effect parameters (N51, `docs/architecture.md` §7.53)
    /// An effect parameter where none may be: a second one, one on a foreign function, on
    /// `main`, on a function that declares no effect set, or named inside a type's arguments.
    pub const EFFECT_PARAMETER: Code = Code::new("N0388");
    /// A call binds its callee's effect parameter to two different sets (N51).
    pub const CONFLICTING_EFFECT_ARGUMENT: Code = Code::new("N0389");

    // Generic channels (N53, `docs/architecture.md` §7.55)
    /// A `Chan[T]` whose `T` may not cross into a task (N53).
    pub const CHANNEL_ELEMENT: Code = Code::new("N0390");
    /// An export (N55) that is not public, declares an effect, is generic, or takes or returns
    /// a type other than `Int` and `Bool`.
    pub const EXPORT_SHAPE: Code = Code::new("N0391");

    // A freestanding target (N62, `docs/architecture.md` §7.64)
    /// A device-register built-in (`mmio_read32`, `mmio_write32`) reached by something that
    /// has no device registers: `nazm run`, or a build for a hosted target.
    pub const MACHINE_ONLY: Code = Code::new("N0392");

    // The interactive tier (N65, `docs/architecture.md` §7.67)
    /// A function `nazm comptime` will not evaluate: it has parameters, or it is not pure, so it
    /// could reach the host through authority it holds or borrows.
    pub const COMPTIME_REFUSED: Code = Code::new("N0393");

    // Accelerators (N67, `docs/architecture.md` §7.69)
    /// A function `nazm accel` will not run as a kernel: not a pure `(Int) -> Int` over `Int`s
    /// and `Bool`s, or reaching something with no device form.
    pub const KERNEL_INELIGIBLE: Code = Code::new("N0394");

    // Contracts (N69, `docs/architecture.md` §7.71)
    /// A module `nazm contract` was given that does not have a contract's shape.
    pub const NOT_A_CONTRACT: Code = Code::new("N0395");
    /// A transaction that would change the contract's conserved quantity: rejected, the state
    /// unchanged.
    pub const CONSERVATION_BROKEN: Code = Code::new("N0396");

    // The EVM backend (N70, `docs/architecture.md` §7.72)
    /// A sender whose address is not an `Int`, as the language's `caller` must be.
    pub const CALLER_NOT_INT: Code = Code::new("N0397");
    /// Calldata that is not a known entrypoint's selector and `int64` arguments.
    pub const BAD_CALLDATA: Code = Code::new("N0398");
    /// A contract construct the EVM backend does not emit, or a storage upgrade it refuses.
    pub const EVM_UNSUPPORTED: Code = Code::new("N0399");

    // Packages (N46, `docs/architecture.md` §7.48)
    /// A manifest that cannot be read as `nazm.package/1`: bad TOML, an unknown key, a name or
    /// version of the wrong shape, or a path that leaves nothing to build.
    pub const MANIFEST_INVALID: Code = Code::new("N0500");
    /// A dependency's directory or manifest is not there.
    pub const PACKAGE_NOT_FOUND: Code = Code::new("N0501");
    /// A dependency's manifest states another version than the one declared for it.
    pub const VERSION_MISMATCH: Code = Code::new("N0502");
    /// Two packages of one name in one build.
    pub const PACKAGE_CONFLICT: Code = Code::new("N0503");
    /// A package depends on itself, through its dependencies.
    pub const PACKAGE_CYCLE: Code = Code::new("N0504");
    /// `--locked`, and there is no lockfile.
    pub const LOCK_MISSING: Code = Code::new("N0505");
    /// `--locked`, and the packages resolved are not the ones the lockfile records.
    pub const LOCK_MISMATCH: Code = Code::new("N0506");
    /// A package's bytes no longer match the digest the lockfile records for it.
    pub const DIGEST_MISMATCH: Code = Code::new("N0507");
    /// An import names a package the importing package does not declare.
    pub const UNKNOWN_DEPENDENCY: Code = Code::new("N0508");
    /// A package built as a program has no `main`.
    pub const NO_ENTRY: Code = Code::new("N0509");

    // Restriction profiles (N47, `docs/architecture.md` §7.49)
    /// A program that breaks a rule of a profile in force: the rule, and its witnesses.
    pub const PROFILE_VIOLATION: Code = Code::new("N0510");
    /// A profile named on the command line or in a manifest that does not exist.
    pub const UNKNOWN_PROFILE: Code = Code::new("N0511");
    // A local registry (N56, `docs/architecture.md` §7.58)
    /// A registry index that is not `nazm.registry-index/1`, names another package, records a
    /// version twice or a digest of the wrong shape; or a publish of a version already there.
    pub const REGISTRY_INVALID: Code = Code::new("N0512");
    /// No version in the registry satisfies every requirement on a package.
    pub const VERSION_UNSATISFIABLE: Code = Code::new("N0513");
    /// A registry package holding a symbolic link or anything but files and directories.
    pub const PACKAGE_UNSAFE: Code = Code::new("N0514");

    // N78: traits and methods (`architecture.md` §7.79). The checker's N03xx block is full;
    // these are its continuation. A trait method's malformed syntax is `N0101`, an unknown
    // name in a bound or an impl is `N0305` when it names no type and `N0610` when it names
    // a type that is not a trait, a duplicate method in a trait or an impl is `N0203`.
    /// Two impls of one trait for one type in one compilation.
    pub const IMPL_CONFLICT: Code = Code::new("N0600");
    /// An impl in a module that declares neither the trait nor the type.
    pub const IMPL_ORPHAN: Code = Code::new("N0601");
    /// An impl that does not define every method of its trait.
    pub const IMPL_MISSING_METHOD: Code = Code::new("N0602");
    /// An impl method its trait does not declare, or one whose signature is not the
    /// trait's with `Self` replaced by the impl's type, effects included.
    pub const IMPL_METHOD_MISMATCH: Code = Code::new("N0603");
    /// An impl for a type that cannot have one: a generic type, a `Vec`, a channel, a
    /// function type, a capability or a type parameter.
    pub const IMPL_TYPE_REFUSED: Code = Code::new("N0604");
    /// A method call no trait in scope provides for the receiver's type.
    pub const NO_METHOD: Code = Code::new("N0605");
    /// A method call more than one trait in scope provides: the trait must be named.
    pub const AMBIGUOUS_METHOD: Code = Code::new("N0606");
    /// A trait written where a type is expected. There are no trait objects.
    pub const TRAIT_NOT_A_TYPE: Code = Code::new("N0607");
    /// A method named without being called: methods are not values.
    pub const METHOD_NOT_A_VALUE: Code = Code::new("N0608");
    /// A trait method whose first parameter is not `self: Self`.
    pub const TRAIT_METHOD_SELF: Code = Code::new("N0609");
    /// A bound, an impl or a qualified call naming a type that is not a trait.
    pub const NOT_A_TRAIT: Code = Code::new("N0610");

    // N85: FFI v3 (`architecture.md` §7.86).
    /// An opaque handle (`extern "C" struct S;`) built or taken apart: it has no fields to
    /// name, and only C makes one.
    pub const OPAQUE_HANDLE: Code = Code::new("N0611");
    /// A callback argument to C that is not an export's name: only an export has a C entry
    /// that needs no environment and lives as long as the process.
    pub const CALLBACK_NOT_EXPORT: Code = Code::new("N0612");
    /// `c_errno()` in a build for a target with no `errno`: a freestanding or WebAssembly one.
    pub const ERRNO_UNAVAILABLE: Code = Code::new("N0613");

    // N87: contracts (`architecture.md` §7.88).
    /// A contract clause that is not made only of literals, names, operators, field reads and
    /// calls of pure functions.
    pub const CONTRACT_SHAPE: Code = Code::new("N0614");
    /// A call whose arguments are all literals and that breaks its callee's precondition: it
    /// would fail every time it ran.
    pub const PRECONDITION_REFUTED: Code = Code::new("N0615");

    // Gate 1-C1: closure environments in the ownership graph (`docs/spec.md`, *Cycles*).
    /// A closure capturing a value that can hold a function value behind a counted handle —
    /// a `Vec[fn() -> Int]`, a `Vec` of records with a function field, a record holding one.
    /// The only way a closure's environment could come to own itself, and so a value no count
    /// could reclaim; decided by the captured type, where the closure is written.
    pub const CAPTURE_CYCLE: Code = Code::new("N0616");

    // Gate 2: the general-purpose foundation (`docs/general-purpose.md`).
    /// A shift whose amount is a constant outside `0..width` (Gate 2): it would stop the
    /// program every time it ran (`N0412`), so it is refused where it is written.
    pub const SHIFT_AMOUNT_REFUSED: Code = Code::new("N0617");
    /// An array over 64 KiB, or of no elements (Gate 2): a larger buffer is a `Vec`.
    pub const ARRAY_TOO_LARGE: Code = Code::new("N0618");
    /// An array whose element owns something — a `Str`, a handle, a function value — rather
    /// than plain data (Gate 2).
    pub const ARRAY_ELEMENT: Code = Code::new("N0621");
    /// A constant's value, or an array's length, that is not computed from literals, other
    /// constants, operators and numeric conversions (Gate 2).
    pub const NOT_A_CONSTANT: Code = Code::new("N0622");
    /// A constant whose computation overflows, divides by zero or shifts out of range (Gate 2).
    pub const CONSTANT_FAILS: Code = Code::new("N0623");
    /// An index that is not into an array, or a constant index outside it (Gate 2).
    pub const ARRAY_INDEX: Code = Code::new("N0624");
    /// An assignment to a static, which is read-only — changed, if an `Atomic`, only by the
    /// atomic operations (Gate 3, `docs/spec.md`, *Statics*).
    pub const STATIC_ASSIGNED: Code = Code::new("N0625");
    /// A board's manifest the build cannot honour: a key it does not know, a value of the wrong
    /// kind or out of range (Gate 3, `docs/spec.md`, *Boards*). Reported by `nazm build`.
    pub const BOARD_MANIFEST: Code = Code::new("N0626");
    /// An interrupt handler of a shape the board cannot call, two handlers for one interrupt, or
    /// one for an interrupt the board does not have (Gate 3, `docs/spec.md`, *Boards*).
    pub const INTERRUPT_HANDLER: Code = Code::new("N0627");
    /// An attribute the language does not have, on a declaration it does not apply to, or with
    /// arguments it does not take (Gate 2, `docs/spec.md`, *Attributes*).
    pub const ATTRIBUTE: Code = Code::new("N0619");

    // Compiler defects
    /// The compiler contradicted itself (N39): a program that parsed and checked with no
    /// diagnostic could not be lowered to Core IR, or its Core IR broke an invariant every
    /// later stage relies on. Never about the program — about the compiler — and nothing
    /// is run or built after one.
    pub const COMPILER_DEFECT: Code = Code::new("N0900");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
}

/// The identifier every emitted diagnostic object carries in its `schema` field.
///
/// `name/major`. The major number changes only when a consumer written against the
/// previous one would be **wrong** — a field removed or renamed, or one whose meaning
/// changed. Adding a field, adding a diagnostic code, and adding a variant to an
/// existing enum are all compatible changes, because a consumer is required to ignore
/// what it does not recognise. `docs/diagnostics.md` states the rule in full and is the
/// document a tool author reads.
pub const SCHEMA: &str = "nazm.diagnostic/1";

/// The published form of a diagnostic: `nazm.diagnostic/1`, exactly.
///
/// # Why this is a separate set of types
///
/// The compiler's [`Diagnostic`] is a semantic value whose spans carry a
/// [`FileId`](nazm_span::FileId) and offsets local to that file. The published shape is
/// older than that representation: it carries `{ start, end }` with **no file**, at
/// offsets into the concatenation of every source file in load order. Both are correct
/// for their purpose, and they are not the same type.
///
/// Until 2026-09-22 the conversion lived in `Span`'s own `Serialize`, which made every
/// serialisation of a span lossy — including serialisations that had nothing to do with
/// this schema and no reason to discard a file. `nazm-span` no longer depends on serde at
/// all, so the conversion has to happen here, at the boundary that actually owns the wire
/// format, and it happens in one function: [`Diagnostic::published`].
///
/// A consumer sees no difference. That is the requirement.
pub mod wire {
    use serde::{Deserialize, Serialize};

    /// A span as the schema publishes it: offsets into the concatenation, and no file.
    ///
    /// Named `Span` on the wire, because that is what it has always been called there.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename = "Span")]
    pub struct PublishedSpan {
        pub start: u32,
        pub end: u32,
    }

    /// A [`Label`](super::Label) as published.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename = "Label")]
    pub struct PublishedLabel {
        pub span: PublishedSpan,
        pub message: String,
    }

    /// A [`Fix`](super::Fix) as published.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename = "Fix")]
    pub struct PublishedFix {
        pub span: PublishedSpan,
        pub replacement: String,
        pub description: String,
        pub applicability: super::Applicability,
        pub precondition: String,
    }

    /// A [`Diagnostic`](super::Diagnostic) as published.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename = "Diagnostic")]
    pub struct PublishedDiagnostic {
        pub code: super::Code,
        pub severity: super::Severity,
        pub message: String,
        pub primary: PublishedLabel,
        pub secondary: Vec<PublishedLabel>,
        pub help: Option<String>,
        pub fixes: Vec<PublishedFix>,
    }

    /// One diagnostic as it goes over the wire: the diagnostic, and the shape it is in.
    ///
    /// The version travels with **every object** rather than in a header, because the
    /// output is one JSON object per line and a consumer may read any single line of it —
    /// a header would be a version that half the readers never see.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Report {
        pub schema: String,
        #[serde(flatten)]
        pub diagnostic: PublishedDiagnostic,
    }
}

pub use wire::Report;

/// A span with something to say about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub span: Span,
    pub message: String,
}

/// Whether an edit may be applied without a person looking at it.
///
/// Two values, because there are two genuinely different situations and collapsing them
/// is what makes an automatic repair tool untrustworthy. A tool that applies everything
/// eventually applies a guess; a tool that applies nothing is a linter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Applicability {
    /// Correct whenever the diagnostic is correct, and changes nothing else. A tool may
    /// apply it unattended.
    Automatic,
    /// Resolves the diagnostic, and may not be what was meant. The `precondition` says
    /// when it is the right edit; a tool presents it rather than applying it.
    NeedsReview,
}

/// An edit that would resolve the diagnostic.
///
/// Machine-applicable: `replace this span with this text`. Not advice in prose, which a
/// tool cannot act on without guessing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub span: Span,
    pub replacement: String,
    pub description: String,
    pub applicability: Applicability,
    /// When this edit is the right one, in one sentence.
    ///
    /// **Always present**, including on an [`Applicability::Automatic`] fix, where it
    /// states the reason the edit is unconditional. A fix with no precondition written
    /// down is a fix whose author did not ask when it could be wrong, and this field
    /// existing is what forces the question at every site.
    pub precondition: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: Code,
    pub severity: Severity,
    pub message: String,
    /// The span the error is *about*. Always present: a diagnostic without a position
    /// makes the reader search, which is the cost this design exists to remove.
    pub primary: Label,
    /// Additional spans that explain it — a previous definition, a conflicting branch.
    pub secondary: Vec<Label>,
    /// Why, or what to do. Prose, and optional.
    pub help: Option<String>,
    pub fixes: Vec<Fix>,
}

impl Diagnostic {
    #[must_use]
    pub fn error(code: Code, span: Span, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            code,
            severity: Severity::Error,
            primary: Label {
                span,
                message: message.clone(),
            },
            message,
            secondary: Vec::new(),
            help: None,
            fixes: Vec::new(),
        }
    }

    /// Give the primary span its own, narrower label.
    #[must_use]
    pub fn primary_label(mut self, message: impl Into<String>) -> Self {
        self.primary.message = message.into();
        self
    }

    #[must_use]
    pub fn secondary(mut self, span: Span, message: impl Into<String>) -> Self {
        self.secondary.push(Label {
            span,
            message: message.into(),
        });
        self
    }

    #[must_use]
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// An edit a tool may apply unattended. `why` says what makes it unconditional.
    #[must_use]
    pub fn fix_automatically(
        self,
        span: Span,
        replacement: impl Into<String>,
        description: impl Into<String>,
        why: impl Into<String>,
    ) -> Self {
        self.with_fix(
            span,
            replacement,
            description,
            Applicability::Automatic,
            why,
        )
    }

    /// An edit that resolves the diagnostic but may not be what was meant. `when` says
    /// the condition under which it is right.
    #[must_use]
    pub fn fix_with_review(
        self,
        span: Span,
        replacement: impl Into<String>,
        description: impl Into<String>,
        when: impl Into<String>,
    ) -> Self {
        self.with_fix(
            span,
            replacement,
            description,
            Applicability::NeedsReview,
            when,
        )
    }

    fn with_fix(
        mut self,
        span: Span,
        replacement: impl Into<String>,
        description: impl Into<String>,
        applicability: Applicability,
        precondition: impl Into<String>,
    ) -> Self {
        self.fixes.push(Fix {
            span,
            replacement: replacement.into(),
            description: description.into(),
            applicability,
            precondition: precondition.into(),
        });
        self
    }

    /// The machine-readable form. One JSON object per diagnostic, carrying the version
    /// of the shape it is in.
    ///
    /// See `docs/diagnostics.md` for the schema and what may change inside a version.
    pub fn to_json(&self, map: &SourceMap) -> Result<String, serde_json::Error> {
        serde_json::to_string(&Report {
            schema: SCHEMA.to_owned(),
            diagnostic: self.published(map),
        })
    }

    /// This diagnostic in the shape `nazm.diagnostic/1` publishes.
    ///
    /// The **one** place the internal (file, file-local offset) representation becomes the
    /// external (no file, concatenation offset) one. Keeping it to a single function is
    /// what stops the two drifting: a span added anywhere in a `Diagnostic` is converted
    /// here or it is not published at all — and since `Span` cannot be serialised, "or it
    /// is not published at all" is enforced by the compiler rather than by review.
    #[must_use]
    pub fn published(&self, map: &SourceMap) -> wire::PublishedDiagnostic {
        let span = |s: Span| {
            let (start, end) = map.global(s);
            wire::PublishedSpan { start, end }
        };
        let label = |l: &Label| wire::PublishedLabel {
            span: span(l.span),
            message: l.message.clone(),
        };
        wire::PublishedDiagnostic {
            code: self.code.clone(),
            severity: self.severity,
            message: self.message.clone(),
            primary: label(&self.primary),
            secondary: self.secondary.iter().map(label).collect(),
            help: self.help.clone(),
            fixes: self
                .fixes
                .iter()
                .map(|f| wire::PublishedFix {
                    span: span(f.span),
                    replacement: f.replacement.clone(),
                    description: f.description.clone(),
                    applicability: f.applicability,
                    precondition: f.precondition.clone(),
                })
                .collect(),
        }
    }

    /// The human-readable form: message, location, source line, and a caret.
    #[must_use]
    pub fn render(&self, map: &SourceMap) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let at = map.line_col(self.primary.span);

        let _ = writeln!(out, "error[{}]: {}", self.code, self.message);
        // The span says which file it is in, so this is a lookup rather than the binary
        // search over a merged buffer it used to be.
        let _ = writeln!(
            out,
            "  --> {}:{}:{}",
            map.name(self.primary.span.file),
            at.line,
            at.column
        );

        let line = map.line_text(self.primary.span);
        let gutter = at.line.to_string();
        let pad = " ".repeat(gutter.len());

        let _ = writeln!(out, "{pad} |");
        let _ = writeln!(out, "{gutter} | {line}");

        // The caret spans the offending text, clamped to the line so a multi-line span
        // does not draw a caret across the whole file.
        let caret_col = at.column.saturating_sub(1) as usize;
        let width = usize::try_from(self.primary.span.len()).unwrap_or(1).max(1);
        let remaining = line.chars().count().saturating_sub(caret_col).max(1);
        let _ = writeln!(
            out,
            "{pad} | {}{} {}",
            " ".repeat(caret_col),
            "^".repeat(width.min(remaining)),
            self.primary.message
        );

        for s in &self.secondary {
            let sat = map.line_col(s.span);
            // A secondary in *another* file names it. Without this, "declared here" in a
            // multi-file program gives a line number with no file to look it up in —
            // and silently means the wrong file's line. Comparing `FileId`s rather than
            // names also settles the case two files share a basename.
            let here = self.primary.span.file;
            let there = s.span.file;
            let where_ = if here == there {
                format!("{}:{}", sat.line, sat.column)
            } else {
                format!("{}:{}:{}", map.name(there), sat.line, sat.column)
            };
            let _ = writeln!(out, "{pad} = at {where_}: {}", s.message);
        }
        if let Some(h) = &self.help {
            let _ = writeln!(out, "{pad} = help: {h}");
        }
        for f in &self.fixes {
            // An automatic fix and one needing review get different words, because a
            // reader deciding whether to run `nazm fix --apply` is deciding exactly
            // that. The precondition is printed only for the one where it is a question
            // being asked of them; the JSON carries it either way.
            match f.applicability {
                Applicability::Automatic => {
                    let _ = writeln!(out, "{pad} = fix: {} -> `{}`", f.description, f.replacement);
                }
                Applicability::NeedsReview => {
                    let _ = writeln!(
                        out,
                        "{pad} = suggestion: {} -> `{}`",
                        f.description, f.replacement
                    );
                    let _ = writeln!(out, "{pad}   right when: {}", f.precondition);
                }
            }
        }
        out
    }
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
    use nazm_span::{FileId, SourceMap};

    #[test]
    fn a_diagnostic_round_trips_as_json() {
        // The machine-readable form is the contract; the rendering is a view of it.
        let d = Diagnostic::error(
            codes::UNDEFINED_VARIABLE,
            Span::new(FileId::ROOT, 4, 5),
            "`x` is not defined",
        )
        .help("declare it with `let x = ...;` first")
        .fix_with_review(
            Span::new(FileId::ROOT, 4, 5),
            "y",
            "did you mean `y`?",
            "`y` is the name that was meant",
        );
        // A one-file map: local and published offsets coincide, which is what makes the
        // round trip below an identity rather than a rebase.
        let map = SourceMap::single("x.nz", "fn main() -> Int { x }");
        let json = d.to_json(&map).unwrap();
        assert!(json.contains("N0200"), "{json}");
        // The version travels with the object, and the object still reads back as a
        // plain diagnostic: a consumer that does not care about the envelope is not made
        // to parse one.
        assert!(json.contains(SCHEMA), "{json}");
        let published = d.published(&map);
        assert_eq!(
            serde_json::from_str::<wire::PublishedDiagnostic>(&json).unwrap(),
            published
        );
        assert_eq!(
            serde_json::from_str::<Report>(&json).unwrap(),
            Report {
                schema: SCHEMA.to_owned(),
                diagnostic: published,
            }
        );
    }

    /// The bytes `nazm.diagnostic/1` has always emitted, for a diagnostic in the second
    /// file of a two-file program.
    ///
    /// This is the test the preflight of 2026-09-22 exists for. The published span is
    /// `{ start, end }` with **no file**, at offsets into the concatenation of every file
    /// in load order — `helper.nz` begins at 17, so its byte 21 is published as 38. Moving
    /// the conversion out of `Span::serialize` and into [`Diagnostic::published`] changed
    /// where it happens and changed nothing a consumer can see.
    #[test]
    fn the_published_shape_is_unchanged_by_spans_knowing_their_file() {
        let root = "use \"helper.nz\";\n";
        let helper = "fn helper() -> Int { bogus }\n";
        let map = SourceMap::from_parts(vec![
            ("main.nz".to_owned(), root.to_owned()),
            ("helper.nz".to_owned(), helper.to_owned()),
        ]);
        let helper_id = map.ids().nth(1).unwrap();
        // Byte 21 of `helper.nz`: the `bogus` that is not defined.
        let d = Diagnostic::error(
            codes::UNDEFINED_VARIABLE,
            Span::new(helper_id, 21, 26),
            "`bogus` is not defined",
        );
        let json = d.to_json(&map).unwrap();
        assert_eq!(
            json,
            r#"{"schema":"nazm.diagnostic/1","code":"N0200","severity":"error","message":"`bogus` is not defined","primary":{"span":{"start":38,"end":43},"message":"`bogus` is not defined"},"secondary":[],"help":null,"fixes":[]}"#
        );
        // The three ways this could have regressed, stated separately so a failure says
        // which one it was.
        assert!(!json.contains("\"file\""), "{json}");
        assert!(json.contains(r#""start":38"#), "{json}");
        assert_eq!(map.global(Span::new(helper_id, 21, 26)), (38, 43));
    }

    /// A span keeps its file **because it cannot be serialised at all**.
    ///
    /// The regression this guards against is not a wrong value, it is a plausible one: a
    /// span that went out through a generic serializer as `{start, end}` and came back as
    /// `FileId::ROOT`, naming the wrong file with complete confidence. `nazm-span` has no
    /// serde dependency, so the mistake is now a compile error; what remains testable is
    /// that publishing is explicit and that the internal value is untouched by it.
    #[test]
    fn publishing_a_diagnostic_does_not_move_the_diagnostic() {
        let map = SourceMap::from_parts(vec![
            ("a.nz".to_owned(), "fn a() -> Int { 1 }\n".to_owned()),
            ("b.nz".to_owned(), "x\n".to_owned()),
        ]);
        let b = map.ids().nth(1).unwrap();
        let d = Diagnostic::error(codes::UNDEFINED_VARIABLE, Span::new(b, 0, 1), "`x`");
        let published = d.published(&map);
        assert_eq!(published.primary.span.start, 20);
        // The diagnostic still points where it pointed: file-local, in `b.nz`.
        assert_eq!(d.primary.span, Span::new(b, 0, 1));
        assert_eq!(map.name(d.primary.span.file), "b.nz");
        // And publishing twice is publishing once, twice — not a rebase of a rebase.
        assert_eq!(d.published(&map), published);
    }

    #[test]
    fn a_suggestion_is_rendered_with_the_condition_it_rests_on() {
        // The difference between a fix and a suggestion is the whole of what makes an
        // automatic repair tool trustworthy, so it is visible in the terminal too and
        // not only in the JSON.
        let map = SourceMap::single("x.nz", "fn main() -> Int { 1 }");
        let d = Diagnostic::error(
            codes::UNDEFINED_VARIABLE,
            Span::new(FileId::ROOT, 19, 20),
            "no",
        )
        .fix_with_review(
            Span::new(FileId::ROOT, 19, 20),
            "2",
            "use `2`",
            "two was meant",
        );
        let out = d.render(&map);
        assert!(out.contains("suggestion: use `2`"), "{out}");
        assert!(out.contains("right when: two was meant"), "{out}");

        let d = Diagnostic::error(
            codes::UNDEFINED_VARIABLE,
            Span::new(FileId::ROOT, 19, 20),
            "no",
        )
        .fix_automatically(Span::new(FileId::ROOT, 19, 20), "2", "use `2`", "always");
        let out = d.render(&map);
        assert!(out.contains("fix: use `2`"), "{out}");
        assert!(
            !out.contains("right when"),
            "an automatic fix asks the reader nothing: {out}"
        );
    }

    #[test]
    fn rendering_points_at_the_offending_text() {
        let map = SourceMap::single("x.nz", "fn main() -> Int {\n  qux\n}");
        let at = u32::try_from(map.text(FileId::ROOT).find("qux").unwrap()).unwrap();
        let d = Diagnostic::error(
            codes::UNDEFINED_VARIABLE,
            Span::new(FileId::ROOT, at, at + 3),
            "`qux` is not defined",
        );
        let out = d.render(&map);

        assert!(out.contains("error[N0200]"), "{out}");
        assert!(out.contains("x.nz:2:3"), "{out}");
        assert!(out.contains("  qux"), "{out}");
        assert!(out.contains("^^^"), "the caret must cover the name: {out}");
    }

    #[test]
    fn every_code_is_distinct() {
        // A reused code means two different failures are indistinguishable to anything
        // matching on it, which is the one thing codes exist to prevent.
        let all = [
            codes::UNKNOWN_CHARACTER,
            codes::UNTERMINATED_STRING,
            codes::INTEGER_TOO_LARGE,
            codes::UNEXPECTED_TOKEN,
            codes::UNSUPPORTED_CONSTRUCT,
            codes::UNDEFINED_VARIABLE,
            codes::UNDEFINED_FUNCTION,
            codes::ALREADY_DEFINED_IN_SCOPE,
            codes::DUPLICATE_FUNCTION,
            codes::MISSING_MAIN,
            codes::TYPE_MISMATCH,
            codes::CONDITION_NOT_BOOL,
            codes::BRANCH_TYPES_DIFFER,
            codes::WRONG_ARGUMENT_COUNT,
            codes::INVALID_OPERANDS,
            codes::UNKNOWN_TYPE,
            codes::ASSIGN_TO_IMMUTABLE,
            codes::MUTABLE_PARAMETER,
            codes::NOT_IN_A_LOOP,
            codes::UNREACHABLE_STATEMENT,
            codes::CONTROL_FLOW_AS_VALUE,
            codes::INTEGER_OVERFLOW,
            codes::DIVISION_BY_ZERO,
            codes::ITERATION_LIMIT,
            codes::CALL_DEPTH_LIMIT,
        ];
        let mut seen: Vec<&str> = all.iter().map(Code::as_str).collect();
        let before = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), before, "a diagnostic code is used twice");
    }
}
