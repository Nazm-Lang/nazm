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
            other => errors.push(refuse(
                a.name_span,
                format!("`@{other}` is not an attribute Nazm has"),
                "the attributes are `@cfg`, `@test`, `@bench`, `@fuzz`, `@deprecated` and \
                 `@section`; there are no user-defined attributes",
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
        }
        keep
    });
    program
}
