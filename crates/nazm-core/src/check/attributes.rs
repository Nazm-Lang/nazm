//! Attributes and conditional compilation (Gate 2, `docs/spec.md`, *Attributes* and *Conditional
//! compilation*): the closed vocabulary held, and every declaration whose `@cfg` does not hold for
//! the build dropped before anything resolves a name.
//!
//! The vocabulary is the compiler's: `@cfg(…)`, `@test`, `@test(fails)`, `@bench`, `@fuzz` and
//! `@deprecated` or `@deprecated("why")` — metadata `nazm doc` publishes, on a `pub` function,
//! `struct` or `enum` only. Anything else, an attribute on a declaration it does not
//! apply to, or one with arguments it does not take, is `N0619`. A condition reads only the build's
//! [`BuildConfig`] — its target's operating system, architecture and triple, the enabled features
//! and the profiles — so the same source and configuration always keep the same declarations.

use nazm_diag::{codes, Diagnostic};
use nazm_sema::{BuildConfig, ModuleGraph};
use nazm_span::Span;
use nazm_syntax::ast::{AttrArg, Attribute, Function, TypeName};
use nazm_syntax::Program;

/// What kind of declaration an attribute was written before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Function,
    Type,
    Trait,
    Impl,
    Const,
    /// A static (Gate 3): storage, which a section may hold.
    Static,
}

impl Target {
    const fn named(self) -> &'static str {
        match self {
            Self::Function => "a function",
            Self::Type => "a type",
            Self::Trait => "a trait",
            Self::Impl => "an impl",
            Self::Const => "a constant",
            Self::Static => "a static",
        }
    }
}

/// Whether `name` is an interrupt's name a manifest can give: a TOML bare key, so letters,
/// digits, `_` and `-` (Gate 3).
fn interrupt_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The interrupt `@interrupt` names on `f`, if it has one (Gate 3).
#[must_use]
pub fn interrupt_of(f: &Function) -> Option<String> {
    f.attrs
        .iter()
        .find(|a| a.name == "interrupt")
        .and_then(|a| match a.args.as_deref() {
            Some([AttrArg::Str { value, .. }]) => Some(value.clone()),
            _ => None,
        })
}

/// A handler's shape (Gate 3): what the board's interrupt path can call — no parameter or one
/// `MmioCap`, an `Int` back, and a declared effect set that is empty, since nothing but the
/// device and the program's statics is there to act on. Not generic, not `main`, not foreign.
fn handler(f: &Function, errors: &mut Vec<Diagnostic>) {
    if interrupt_of(f).is_none() {
        return;
    }
    let plain =
        |t: &TypeName, name: &str| t.qualifier.is_none() && t.args.is_none() && t.name == name;
    let params = match f.params.as_slice() {
        [] => true,
        [p] => plain(&p.ty, "MmioCap"),
        _ => false,
    };
    let quiet = f.effects.as_ref().is_some_and(|e| e.names.is_empty());
    let ordinary = f.type_params.is_empty()
        && f.effect_params.is_empty()
        && f.name != "main"
        && f.foreign.is_none()
        && f.export.is_none()
        && f.method_of.is_none();
    if !(params && plain(&f.return_type, "Int") && quiet && ordinary) {
        errors.push(
            Diagnostic::error(
                codes::INTERRUPT_HANDLER,
                f.name_span,
                format!(
                    "`@interrupt` marks a function of the shape `fn NAME() -> Int ! {{}}` or \
                     `fn NAME(m: MmioCap) -> Int ! {{}}`, and `{}` is not one",
                    f.name
                ),
            )
            .help(
                "the board calls a handler with nothing but the device's authority; it shares \
                 state through statics and atomics, and declares that it has no effect",
            ),
        );
    }
}

/// A failure hook's shape (Gate 3): `fn NAME(code: Int) -> Int ! {}`, or with an `MmioCap` after
/// the number to reach a device — the failure's number in, and nothing but the device and the
/// program's statics to act on. Not generic, not `main`, not foreign.
fn failure_hook(f: &Function, errors: &mut Vec<Diagnostic>) {
    if !f.attrs.iter().any(|a| a.name == "on_failure") {
        return;
    }
    let plain =
        |t: &TypeName, name: &str| t.qualifier.is_none() && t.args.is_none() && t.name == name;
    let takes = match f.params.as_slice() {
        [p] => plain(&p.ty, "Int"),
        [p, m] => plain(&p.ty, "Int") && plain(&m.ty, "MmioCap"),
        _ => false,
    };
    let quiet = f.effects.as_ref().is_some_and(|e| e.names.is_empty());
    let ordinary = f.type_params.is_empty()
        && f.effect_params.is_empty()
        && f.name != "main"
        && f.foreign.is_none()
        && f.export.is_none()
        && f.method_of.is_none();
    if !(takes && plain(&f.return_type, "Int") && quiet && ordinary) {
        errors.push(
            Diagnostic::error(
                codes::INTERRUPT_HANDLER,
                f.name_span,
                format!(
                    "`@on_failure` marks a function of the shape `fn NAME(code: Int) -> Int ! {{}}` \
                     or `fn NAME(code: Int, m: MmioCap) -> Int ! {{}}`, and `{}` is not one",
                    f.name
                ),
            )
            .help("the board calls the hook with the failure's number, before it stops the machine"),
        );
    }
}

/// A task's shape (Gate 3): a handler's — what the scheduler can call with nothing but the device's
/// authority — and not also a handler or a hook.
fn task(f: &Function, errors: &mut Vec<Diagnostic>) {
    if !f.attrs.iter().any(|a| a.name == "task") {
        return;
    }
    let plain =
        |t: &TypeName, name: &str| t.qualifier.is_none() && t.args.is_none() && t.name == name;
    let runs = match f.params.as_slice() {
        [] => true,
        [p] => plain(&p.ty, "MmioCap"),
        _ => false,
    };
    let quiet = f.effects.as_ref().is_some_and(|e| e.names.is_empty());
    let alone = !f
        .attrs
        .iter()
        .any(|a| a.name == "interrupt" || a.name == "on_failure");
    let ordinary = f.type_params.is_empty()
        && f.effect_params.is_empty()
        && f.name != "main"
        && f.foreign.is_none()
        && f.export.is_none()
        && f.method_of.is_none();
    if !(runs && plain(&f.return_type, "Int") && quiet && alone && ordinary) {
        errors.push(
            Diagnostic::error(
                codes::INTERRUPT_HANDLER,
                f.name_span,
                format!(
                    "`@task` marks a function of the shape `fn NAME() -> Int ! {{}}` or \
                     `fn NAME(m: MmioCap) -> Int ! {{}}`, and neither a handler nor a hook, and \
                     `{}` is not one",
                    f.name
                ),
            )
            .help("the scheduler calls a task with nothing but the device's authority"),
        );
    }
}

/// Whether `name` is a section's name a linker script can write: letters, digits, `.` and `_`
/// (Gate 3).
pub(crate) fn section_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_')
}

fn refuse(at: Span, what: String, help: &str) -> Diagnostic {
    Diagnostic::error(codes::ATTRIBUTE, at, what).help(help.to_owned())
}

/// Every attribute on a declaration of kind `target` held to the vocabulary; whether every
/// `@cfg` among them holds for `config`.
fn admitted(
    attrs: &[Attribute],
    target: Target,
    public: bool,
    at: Span,
    graph: &ModuleGraph,
    config: &BuildConfig,
    errors: &mut Vec<Diagnostic>,
) -> bool {
    let features = graph.features_of_file(at.file);
    let mut keep = true;
    let mut seen: Vec<&str> = Vec::new();
    for a in attrs {
        let name = a.name.as_str();
        if name != "cfg" && seen.contains(&name) {
            errors.push(refuse(
                a.span,
                format!("`@{name}` is written twice"),
                "an attribute other than `@cfg` says one thing once",
            ));
            continue;
        }
        seen.push(name);
        match name {
            "cfg" => match a.args.as_deref() {
                Some([cond]) => match holds(cond, config, &features) {
                    Ok(h) => keep &= h,
                    Err((at, why)) => errors.push(refuse(
                        at,
                        why,
                        "a condition is `os = \"…\"`, `arch = \"…\"`, `target = \"…\"`, `feature = \"…\"` \
                         or `profile = \"…\"`, combined with `all(…)`, `any(…)` and `not(…)`",
                    )),
                },
                _ => errors.push(refuse(
                    a.span,
                    "`@cfg` takes one condition".to_owned(),
                    "write `@cfg(os = \"linux\")`, or combine conditions with `all(…)` or `any(…)`",
                )),
            },
            "test" | "bench" | "fuzz" => {
                if target != Target::Function {
                    errors.push(refuse(
                        a.span,
                        format!("`@{name}` marks a function, and this is {}", target.named()),
                        "write it before the `fn` it marks",
                    ));
                }
                let fails = name == "test"
                    && matches!(a.args.as_deref(), Some([AttrArg::Word { name, .. }]) if name == "fails");
                if a.args.is_some() && !fails {
                    errors.push(refuse(
                        a.span,
                        format!("`@{name}` takes no arguments"),
                        if name == "test" {
                            "`@test` or `@test(fails)`"
                        } else {
                            "write it alone"
                        },
                    ));
                }
            }
            // Metadata-only (G2-C1, `docs/spec.md`, *Attributes*): no diagnostic marks a use,
            // and `nazm doc` publishes it. So it marks only what `nazm doc` publishes — a `pub`
            // function, `struct` or `enum` — and anywhere else it would be inert, which is refused.
            "deprecated" => {
                let published = public && matches!(target, Target::Function | Target::Type);
                if !published {
                    let what = if public {
                        target.named().to_owned()
                    } else {
                        format!("{}, and not `pub`", target.named())
                    };
                    errors.push(refuse(
                        a.span,
                        format!(
                            "`@deprecated` marks what an importer's API documentation shows — a \
                             `pub` function, `struct` or `enum` — and this is {what}"
                        ),
                        "it is metadata `nazm doc` publishes, and no diagnostic reports a use: \
                         write it before a `pub` function, `struct` or `enum`",
                    ));
                }
                match a.args.as_deref() {
                    None | Some([AttrArg::Str { .. }]) => {}
                    _ => errors.push(refuse(
                        a.span,
                        "`@deprecated` takes at most one string: why".to_owned(),
                        "`@deprecated` or `@deprecated(\"use `g` instead\")`",
                    )),
                }
            }
            // A board's periodic task (Gate 3, `docs/spec.md`, *Boards*): a function, its priority
            // (0–255) and its period in board-timer ticks (at least 1), both written.
            "task" => {
                if target != Target::Function {
                    errors.push(refuse(
                        a.span,
                        format!("`@task` marks a function, and this is {}", target.named()),
                        "write it before the `fn` the board's scheduler runs",
                    ));
                }
                if crate::constants::task_of(std::slice::from_ref(a)).is_none() {
                    errors.push(refuse(
                        a.span,
                        "`@task` takes `priority = \"N\"`, from 0 to 255, and `period = \"P\"`, \
                         at least 1 tick, and nothing else"
                            .to_owned(),
                        "`@task(priority = \"2\", period = \"10\")`",
                    ));
                }
            }
            // A board's failure hook (Gate 3, `docs/spec.md`, *Boards*): a function, no arguments.
            "on_failure" => {
                if target != Target::Function {
                    errors.push(refuse(
                        a.span,
                        format!(
                            "`@on_failure` marks a function, and this is {}",
                            target.named()
                        ),
                        "write it before the `fn` the board calls when the program fails",
                    ));
                }
                if a.args.is_some() {
                    errors.push(refuse(
                        a.span,
                        "`@on_failure` takes no arguments".to_owned(),
                        "write it alone: the board hands the hook the failure's number",
                    ));
                }
            }
            // A board's interrupt handler (Gate 3, `docs/spec.md`, *Boards*): a function, by the
            // name the manifest gives the interrupt. Its shape is `handler`'s to hold.
            "interrupt" => {
                if target != Target::Function {
                    errors.push(refuse(
                        a.span,
                        format!(
                            "`@interrupt` marks a function, and this is {}",
                            target.named()
                        ),
                        "write it before the `fn` that handles the interrupt",
                    ));
                }
                match a.args.as_deref() {
                    Some([AttrArg::Str { value, .. }]) if interrupt_name(value) => {}
                    _ => errors.push(refuse(
                        a.span,
                        "`@interrupt` takes one string: the interrupt's name in the board's \
                         manifest"
                            .to_owned(),
                        "`@interrupt(\"timer\")`",
                    )),
                }
            }
            // Placement in a board's image (Gate 3, `docs/spec.md`, *Boards*): code or storage
            // only — a constant has no address — under a name a linker script can write.
            "section" => {
                if !matches!(target, Target::Function | Target::Static) {
                    errors.push(refuse(
                        a.span,
                        format!(
                            "`@section` places a function or a static, and this is {}",
                            target.named()
                        ),
                        "a constant is written where it is used and has no address to place",
                    ));
                }
                match a.args.as_deref() {
                    Some([AttrArg::Str { value, .. }]) if section_name(value) => {}
                    _ => errors.push(refuse(
                        a.span,
                        "`@section` takes one string: the section's name, of letters, digits, \
                         `.` and `_`"
                            .to_owned(),
                        "`@section(\".text.boot\")`",
                    )),
                }
            }
            // Derived serialisation (R1c, L2): before a type, naming formats.
            "derive" => {
                if target != Target::Type {
                    errors.push(refuse(
                        a.span,
                        format!("`@derive` marks a `struct` or an `enum`, not {}", target.named()),
                        "write it before the type whose encoding it derives",
                    ));
                }
                let formats = crate::derive::FORMATS;
                match a.args.as_deref() {
                    Some(args) if !args.is_empty() => {
                        for x in args {
                            let known = matches!(x, AttrArg::Word { name, .. } if formats.contains(&name.as_str()));
                            if !known {
                                errors.push(refuse(
                                    x.span(),
                                    "`@derive` takes the formats it derives".to_owned(),
                                    "`@derive(json)`",
                                ));
                            }
                        }
                    }
                    _ => errors.push(refuse(
                        a.span,
                        "`@derive` takes the formats it derives".to_owned(),
                        "`@derive(json)`",
                    )),
                }
            }
            other => errors.push(refuse(
                a.name_span,
                format!("`@{other}` is not an attribute Nazm has"),
                "the attributes are `@cfg`, `@test`, `@bench`, `@fuzz`, `@deprecated`, \
                 `@section`, `@interrupt`, `@on_failure`, `@task` and `@derive`; there are no \
                 user-defined attributes",
            )),
        }
    }
    keep
}

/// A marked function's shape (Gate 2, §18–§19): one the generated entry can hand its authority
/// to, call, and judge. A test or fuzz target answers `Result[Int, Str]`, a benchmark an `Int`; a
/// fuzz target takes its input, `Bytes`, first; every other parameter is a capability, which the
/// entry holds as `main` does. Not generic, not `main`, not foreign.
fn shaped(f: &Function, errors: &mut Vec<Diagnostic>) {
    let plain =
        |t: &TypeName, name: &str| t.qualifier.is_none() && t.args.is_none() && t.name == name;
    let cap = |t: &TypeName| {
        t.qualifier.is_none()
            && t.args.is_none()
            && nazm_sema::CapabilityKind::by_type_name(&t.name).is_some()
    };
    let result = |t: &TypeName| {
        t.qualifier.is_none()
            && t.name == "Result"
            && matches!(t.args.as_deref(), Some([ok, err]) if plain(ok, "Int") && plain(err, "Str"))
    };
    for a in &f.attrs {
        let (name, shape, takes_input) = match a.name.as_str() {
            "test" => ("test", "fn NAME(capabilities…) -> Result[Int, Str]", false),
            "fuzz" => (
                "fuzz",
                "fn NAME(input: Bytes, capabilities…) -> Result[Int, Str]",
                true,
            ),
            "bench" => ("bench", "fn NAME(capabilities…) -> Int", false),
            _ => continue,
        };
        let held = if takes_input {
            match f.params.split_first() {
                Some((input, rest)) => plain(&input.ty, "Bytes") && rest.iter().all(|p| cap(&p.ty)),
                None => false,
            }
        } else {
            f.params.iter().all(|p| cap(&p.ty))
        };
        let answers = if name == "bench" {
            plain(&f.return_type, "Int")
        } else {
            result(&f.return_type)
        };
        let ordinary = f.type_params.is_empty()
            && f.effect_params.is_empty()
            && f.name != "main"
            && f.foreign.is_none()
            && f.export.is_none();
        if !(held && answers && ordinary) {
            errors.push(refuse(
                f.name_span,
                format!("`@{name}` marks a function of the shape `{shape}`, and `{}` is not one", f.name),
                "every parameter but a fuzz target's input is a capability, which the test's entry \
                 holds as `main` does; nothing generic, foreign or named `main`",
            ));
        }
    }
}

/// Whether condition `c` holds for `config`, or why it is not a condition.
fn holds(c: &AttrArg, config: &BuildConfig, features: &[String]) -> Result<bool, (Span, String)> {
    match c {
        AttrArg::Pair {
            key, value, span, ..
        } => match key.as_str() {
            "os" => Ok(config.os == *value),
            "arch" => Ok(config.arch == *value),
            "target" => Ok(config.target == *value),
            "feature" => Ok(features.iter().any(|f| f == value)),
            "profile" => Ok(config.profiles.iter().any(|p| p == value)),
            other => Err((
                *span,
                format!("`{other}` is not something a condition can ask about"),
            )),
        },
        AttrArg::Call {
            name, args, span, ..
        } => match name.as_str() {
            "all" => {
                let mut all = true;
                for a in args {
                    all &= holds(a, config, features)?;
                }
                Ok(all)
            }
            "any" => {
                let mut any = false;
                for a in args {
                    any |= holds(a, config, features)?;
                }
                Ok(any)
            }
            "not" => match args.as_slice() {
                [one] => Ok(!holds(one, config, features)?),
                _ => Err((*span, "`not(…)` takes one condition".to_owned())),
            },
            other => Err((
                *span,
                format!("`{other}(…)` is not a way to combine conditions"),
            )),
        },
        AttrArg::Str { span, .. } | AttrArg::Word { span, .. } => {
            Err((*span, "this is not a condition".to_owned()))
        }
    }
}

/// `program` as `config` builds it: every attribute held to the vocabulary, and every
/// declaration whose `@cfg` does not hold removed — an impl with its methods.
#[must_use]
pub fn configure(
    mut program: Program,
    graph: &ModuleGraph,
    errors: &mut Vec<Diagnostic>,
) -> Program {
    let config = &graph.config();
    program.records.retain(|r| {
        admitted(
            &r.attrs,
            Target::Type,
            r.pub_span.is_some(),
            r.span,
            graph,
            config,
            errors,
        )
    });
    program.enums.retain(|e| {
        admitted(
            &e.attrs,
            Target::Type,
            e.pub_span.is_some(),
            e.span,
            graph,
            config,
            errors,
        )
    });
    // What a derive cannot carry (R1c, L2): a generic type, or a field of a type with no form in
    // the format. Nothing is generated for it (`crate::derive::expand`), so this is the report.
    for format in crate::derive::FORMATS {
        let mut held: Vec<(&[nazm_syntax::ast::Field], &str, Span, bool)> = Vec::new();
        for r in &program.records {
            if crate::derive::derives(&r.attrs, format) {
                held.push((&r.fields, &r.name, r.name_span, r.type_params.is_empty()));
            }
        }
        for e in &program.enums {
            if crate::derive::derives(&e.attrs, format) {
                for v in &e.variants {
                    held.push((&v.fields, &e.name, e.name_span, e.type_params.is_empty()));
                }
            }
        }
        for (fields, name, at, plain) in held {
            if !plain {
                errors.push(refuse(
                    at,
                    format!(
                        "`{name}` is generic, and `@derive({format})` is for a type that is not"
                    ),
                    "derive it for a type with no type parameters",
                ));
                continue;
            }
            for f in fields {
                // A record or enum of the program that does not derive the format: said here,
                // where the field is, rather than as the function the generated code names.
                for t in crate::derive::named(&f.ty) {
                    let known = program.records.iter().any(|r| r.name == t.name)
                        || program.enums.iter().any(|e| e.name == t.name);
                    let derived = program
                        .records
                        .iter()
                        .any(|r| r.name == t.name && crate::derive::derives(&r.attrs, format))
                        || program
                            .enums
                            .iter()
                            .any(|e| e.name == t.name && crate::derive::derives(&e.attrs, format));
                    if known && !derived {
                        errors.push(refuse(
                            t.span,
                            format!(
                                "`@derive({format})` cannot carry the field `{}`: `{}` does not derive `{format}`",
                                f.name, t.name
                            ),
                            "write `@derive(json)` before that type too",
                        ));
                    }
                }
                if let Some(why) = crate::derive::refusal(&f.ty) {
                    errors.push(refuse(
                        f.ty.span,
                        format!(
                            "`@derive({format})` cannot carry the field `{}`: {why}",
                            f.name
                        ),
                        "give the field a type the format has: an integer `Int` holds, a float, \
                         `Bool`, `Str`, `Option`, `Vec`, `List` or `Box` of one, or a type that \
                         derives it too",
                    ));
                }
            }
        }
    }
    program.traits.retain(|t| {
        admitted(
            &t.attrs,
            Target::Trait,
            t.pub_span.is_some(),
            t.span,
            graph,
            config,
            errors,
        )
    });
    program.consts.retain(|c| {
        admitted(
            &c.attrs,
            if c.is_static {
                Target::Static
            } else {
                Target::Const
            },
            c.pub_span.is_some(),
            c.span,
            graph,
            config,
            errors,
        )
    });
    let mut dropped_impls: Vec<Span> = Vec::new();
    program.impls.retain(|i| {
        let keep = admitted(&i.attrs, Target::Impl, false, i.span, graph, config, errors);
        if !keep {
            dropped_impls.push(i.impl_span);
        }
        keep
    });
    program.functions.retain(|f| {
        if let Some(m) = &f.method_of {
            return !dropped_impls.contains(&m.impl_span);
        }
        let keep = admitted(
            &f.attrs,
            Target::Function,
            f.pub_span.is_some(),
            f.span,
            graph,
            config,
            errors,
        );
        if keep {
            shaped(f, errors);
            handler(f, errors);
            failure_hook(f, errors);
            task(f, errors);
        }
        keep
    });
    // One failure hook (Gate 3): the second is refused where it is written.
    let mut hooks = program
        .functions
        .iter()
        .filter(|f| f.attrs.iter().any(|a| a.name == "on_failure"));
    if let Some(first) = hooks.next() {
        for f in hooks {
            errors.push(
                Diagnostic::error(
                    codes::INTERRUPT_HANDLER,
                    f.name_span,
                    "the program already has a failure hook",
                )
                .secondary(first.name_span, "the first")
                .help("one hook; call one function from the other"),
            );
        }
    }
    // One handler for each interrupt (Gate 3): the second is refused where it is written.
    let mut handled: Vec<(String, Span)> = Vec::new();
    for f in &program.functions {
        if let Some(name) = interrupt_of(f) {
            if let Some((_, first)) = handled.iter().find(|(n, _)| *n == name) {
                errors.push(
                    Diagnostic::error(
                        codes::INTERRUPT_HANDLER,
                        f.name_span,
                        format!("the interrupt `{name}` already has a handler"),
                    )
                    .secondary(*first, "its handler")
                    .help("one handler for each interrupt; call one function from the other"),
                );
            } else {
                handled.push((name, f.name_span));
            }
        }
    }
    program
}
