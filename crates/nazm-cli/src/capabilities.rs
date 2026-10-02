//! `nazm capabilities` — what this build actually supports, from its own tables.
//!
//! # Why a command rather than a page in the README
//!
//! A written feature list is a claim about a build that existed when someone typed it.
//! This reads the tables the compiler itself dispatches on — `Intrinsic::ALL`, the
//! backend's `Builtin`, the `Type` names — so the answer is the same fact the compiler
//! acts on. A built-in cannot appear here and be missing from the interpreter, because
//! the interpreter's table is where the name came from.
//!
//! What it cannot do is invent knowledge the tables do not hold. Whether a *construct*
//! compiles — `scope`, recursion, a block used as an expression — is decided by code in
//! `nazm-lir::lower`, not by a table, so those are listed here with the evidence named:
//! each is a claim this file makes and the test suite checks, and each says which test.

use std::process::ExitCode;

use nazm_core::{Intrinsic, Type};
use nazm_lir::ir::Builtin;

use crate::exit;

/// The version every `nazm capabilities` JSON object carries. See `docs/diagnostics.md`.
const SCHEMA: &str = "nazm.capabilities/1";

#[derive(Debug, serde::Serialize)]
struct Inventory {
    schema: &'static str,
    /// The compiler's own version, so a report can be attributed to a build.
    version: &'static str,
    /// What `nazm build` has been verified against, verbatim from the emitter.
    native_target: &'static str,
    types: Vec<&'static str>,
    /// Every capability type — the authority a parameter can carry — whether or not it is in
    /// `types` (N74): `types` is the checker's fixed vocabulary, unchanged since it entered the
    /// checker's identity.
    authority: Vec<&'static str>,
    /// Every target `nazm build --target` accepts.
    targets: Vec<String>,
    /// Every subcommand of this build, by name.
    commands: Vec<String>,
    builtins: Vec<BuiltinRow>,
    constructs: Vec<ConstructRow>,
}

#[derive(Debug, serde::Serialize)]
struct BuiltinRow {
    name: &'static str,
    params: Vec<&'static str>,
    returns: &'static str,
    /// Every built-in in `Intrinsic::ALL` runs under `nazm run`; the field is here so a
    /// consumer reads one shape for both columns rather than assuming this one.
    interpreted: bool,
    /// Whether `nazm build` lowers it. Taken from the backend's own table, which is a
    /// separate list on purpose — a built-in the front end has and the backend does not
    /// is an ordinary coverage gap, refused by name.
    compiled: bool,
}

#[derive(Debug, serde::Serialize)]
struct ConstructRow {
    name: &'static str,
    interpreted: bool,
    compiled: bool,
    /// What establishes the row. Named rather than implied: a capability report whose
    /// entries cannot be traced to a test is a feature list with extra steps.
    evidence: &'static str,
}

/// Constructs, which no table holds.
///
/// Each row is a claim, and `crates/nazm-cli/tests/schemas.rs` checks the ones that
/// can be checked by compiling a program that uses the construct and seeing whether the
/// answer matches. A row that says `compiled: false` must be *refused*, not merely
/// absent — that is the same distinction the bootstrap subset draws.
const CONSTRUCTS: &[ConstructRow] = &[
    ConstructRow {
        name: "functions, parameters and direct calls",
        interpreted: true,
        compiled: true,
        evidence: "nazm-cli/tests/build.rs",
    },
    ConstructRow {
        name: "recursion, direct or mutual",
        interpreted: true,
        compiled: true,
        evidence: "nazm-cli/tests/usability.rs; a stack check on entry to every function in a call cycle, N0408 (docs/spec.md, Native recursion)",
    },
    ConstructRow {
        name: "`if` in statement and value position",
        interpreted: true,
        compiled: true,
        evidence: "nazm-cli/tests/build.rs",
    },
    ConstructRow {
        name: "`while`, `break`, `continue`, `return`",
        interpreted: true,
        compiled: true,
        evidence: "nazm-cli/tests/build.rs",
    },
    ConstructRow {
        name: "a block used as an expression: a `match` arm, an initialiser, an argument",
        interpreted: true,
        compiled: true,
        evidence: "nazm-cli/tests/value_blocks.rs; the Nazm-written compiler too (selfhost.rs)",
    },
    ConstructRow {
        name: "`struct` records: named fields, construction, projection, replacement",
        interpreted: true,
        compiled: true,
        evidence: "nazm-core/tests/records.rs, nazm-cli/tests/memory.rs",
    },
    ConstructRow {
        name: "`enum` variants and exhaustive `match`",
        interpreted: true,
        compiled: true,
        evidence: "nazm-core/tests/enums.rs, nazm-cli/tests/memory.rs",
    },
    ConstructRow {
        name: "first-order generics: generic records, enums and functions, `Vec[T]`",
        interpreted: true,
        compiled: true,
        evidence: "nazm-core/tests/generics.rs, nazm-cli/tests/generics.rs, \
                   nazm-cli/tests/memory.rs; natively monomorphised, polymorphic recursion \
                   refused (N0357)",
    },
    ConstructRow {
        name: "`==` and `!=` derived on records, enums and concrete generic instances",
        interpreted: true,
        compiled: true,
        evidence: "nazm-core/tests/equality.rs, nazm-cli/tests/equality.rs; none on a \
                   sequence, a `Vec` or a `Chan`, or on anything containing one",
    },
    ConstructRow {
        name: "a type parameter that requires equality: `fn same[T: Equality]`",
        interpreted: true,
        compiled: true,
        evidence: "nazm-core/tests/constraints.rs, nazm-cli/tests/constraints.rs; functions \
                   only, `Equality` the only requirement, nothing emitted at run time",
    },
    ConstructRow {
        name: "modules: `use`, relative resolution, diamonds, cycles",
        interpreted: true,
        compiled: true,
        evidence: "nazm-cli/tests/cli.rs; no visibility control (docs/spec.md)",
    },
    ConstructRow {
        name: "structured concurrency: `scope`, `spawn`, `Chan`",
        interpreted: true,
        compiled: true,
        evidence: "nazm-core/tests/concurrency.rs, nazm-cli/tests/concurrent.rs",
    },
    ConstructRow {
        name: "checked arithmetic: overflow, division and remainder by zero",
        interpreted: true,
        compiled: true,
        evidence: "nazm-cli/tests/build.rs, at -O0 and -O2",
    },
    ConstructRow {
        name: "string escapes: `\\n` `\\t` `\\r` `\\0` `\\\\` `\\\"` `\\xHH`",
        interpreted: true,
        compiled: true,
        evidence: "nazm-cli/tests/usability.rs; decoded once, by the lexer (docs/spec.md, String escapes)",
    },
    ConstructRow {
        name: "logical operators: `&&`, `||` (short-circuit) and `!`",
        interpreted: true,
        compiled: true,
        evidence: "nazm-cli/tests/usability.rs; lowered to `if` in Core IR (docs/spec.md, Logical operators)",
    },
    ConstructRow {
        name: "qualified names: `use \"path\" as m;` and `m::item`",
        interpreted: true,
        compiled: true,
        evidence: "nazm-cli/tests/usability.rs, nazm-service/tests/qualified.rs",
    },
    ConstructRow {
        name: "function values: closures, named functions as values, indirect calls",
        interpreted: true,
        compiled: true,
        evidence: "nazm-cli/tests/closures.rs; one closure object and calling convention under both backends (docs/spec.md, Function values and closures)",
    },
    ConstructRow {
        name: "a call to a foreign (C) function",
        interpreted: false,
        compiled: true,
        evidence: "deliberate: `nazm run` refuses a foreign call before running anything (N0385); nazm-cli/tests/ffi.rs",
    },
    ConstructRow {
        name: "an iteration budget that stops a runaway loop",
        interpreted: true,
        compiled: false,
        evidence: "stated in README.md: a compiled program has no budget and does not stop",
    },
];

pub(crate) fn run(json: bool) -> ExitCode {
    let inventory = inventory();
    if json {
        match serde_json::to_string(&inventory) {
            Ok(line) => println!("{line}"),
            Err(_) => {
                eprintln!("nazm: the capability inventory could not be serialised");
                return ExitCode::from(exit::CHECK_FAILED);
            }
        }
        return ExitCode::from(exit::OK);
    }
    render(&inventory);
    ExitCode::from(exit::OK)
}

fn inventory() -> Inventory {
    Inventory {
        schema: SCHEMA,
        version: env!("CARGO_PKG_VERSION"),
        native_target: nazm_lir::TARGET_NOTE,
        // Built-ins only. A user-defined record is not part of the language's
        // vocabulary — it exists in one program — and listing one here would confuse an
        // inventory of what the *compiler* has with an inventory of what a *source file*
        // declares. `docs/spec.md` keeps the two apart.
        types: Type::ALL.iter().filter_map(|t| t.builtin_name()).collect(),
        authority: nazm_core::sema::CapabilityKind::ALL
            .iter()
            .map(|k| k.type_name())
            .collect(),
        targets: crate::inspect::TARGETS
            .iter()
            .map(|t| (*t).to_owned())
            .collect(),
        commands: <crate::Cli as clap::CommandFactory>::command()
            .get_subcommands()
            .map(|c| c.get_name().to_owned())
            .collect(),
        builtins: Intrinsic::ALL
            .iter()
            .map(|i| BuiltinRow {
                name: i.name(),
                // In the table's own vocabulary since N11, which spells a generic
                // built-in's `T` and `Vec[T]` as well as every built-in type.
                params: i.params().iter().map(|t| t.spelling()).collect(),
                returns: i.ret().spelling(),
                interpreted: true,
                compiled: Builtin::by_name(i.name()).is_some(),
            })
            .collect(),
        constructs: CONSTRUCTS
            .iter()
            .map(|c| ConstructRow {
                name: c.name,
                interpreted: c.interpreted,
                compiled: c.compiled,
                evidence: c.evidence,
            })
            .collect(),
    }
}

fn render(i: &Inventory) {
    println!("nazm {}", i.version);
    println!("native target: {}", i.native_target);
    println!();
    println!("types: {}", i.types.join(", "));
    println!("authority: {}", i.authority.join(", "));
    println!("targets: {}", i.targets.join(", "));
    println!("commands: {}", i.commands.join(", "));

    // Column widths come from the contents. A constant would be right until the first
    // entry longer than it, and a misaligned table is read wrong rather than not at all.
    let signatures: Vec<String> = i
        .builtins
        .iter()
        .map(|b| format!("{}({}) -> {}", b.name, b.params.join(", "), b.returns))
        .collect();

    let w = i.constructs.iter().map(|c| c.name.len()).max().unwrap_or(0);
    println!();
    println!("  {:<w$}  run    build  evidence", "constructs");
    for c in &i.constructs {
        println!(
            "  {:<w$}  {:<5}  {:<5}  {}",
            c.name,
            yes(c.interpreted),
            yes(c.compiled),
            c.evidence
        );
    }

    let w = signatures.iter().map(String::len).max().unwrap_or(0);
    println!();
    println!("  {:<w$}  run    build", "built-ins");
    for (b, signature) in i.builtins.iter().zip(&signatures) {
        println!(
            "  {signature:<w$}  {:<5}  {:<5}",
            yes(b.interpreted),
            yes(b.compiled)
        );
    }
}

fn yes(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}
