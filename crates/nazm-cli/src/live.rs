//! The interactive tier (N65, `docs/architecture.md` §7.67): `nazm repl`, `nazm comptime` and
//! `nazm reload-check`, all over the interpreter and the checker — the same settled IR a build
//! starts from, never a second meaning.
//!
//! The REPL holds a session of definitions and checks the whole session on every edit, so a
//! redefinition is checked against every definition that uses it, and refused by name if it
//! breaks one. Comptime evaluates a parameterless pure function under the interpreter's budgets:
//! with no parameter it holds no capability, and pure it cannot borrow its caller's, so nothing
//! of the host is reachable. A reload check compares two versions of a program and says, by name,
//! what a running image could take and what needs a restart.

use std::collections::BTreeMap;
use std::io::{BufRead, Write as _};
use std::path::Path;
use std::process::ExitCode;

use nazm_diag::{codes, Diagnostic};
use nazm_span::Span;

use crate::exit;

/// One definition of a session, by the name it defines.
#[derive(Debug, Clone)]
struct Definition {
    name: String,
    text: String,
}

/// A REPL session: definitions in the order first given, a redefinition in its predecessor's
/// place, so a name keeps its position — and its durable identity, which is the module's key and
/// the name (§7.10).
#[derive(Debug, Default)]
pub(crate) struct Session {
    defs: Vec<Definition>,
}

/// What one line of input did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Reply {
    Defined(String),
    Redefined(String),
    Value(String),
    /// Diagnostics, rendered, or a refusal in words.
    Refused(String),
    Listing(String),
    Reset,
    Quit,
}

/// The name a definition line defines, if it is one.
fn defines(line: &str) -> Option<String> {
    let rest = line.strip_prefix("pub ").unwrap_or(line);
    let rest = ["fn ", "struct ", "enum "]
        .iter()
        .find_map(|k| rest.strip_prefix(k))?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

impl Session {
    fn source(&self, defs: &[Definition], tail: &str) -> String {
        let mut s = String::new();
        for d in defs {
            s.push_str(&d.text);
            s.push('\n');
        }
        s.push_str(tail);
        s
    }

    /// Which definition a diagnostic's span falls in, by the session text's layout.
    fn owner(defs: &[Definition], at: Span) -> Option<String> {
        let mut start = 0usize;
        for d in defs {
            let end = start + d.text.len() + 1;
            if (start..end).contains(&(at.start as usize)) {
                return Some(d.name.clone());
            }
            start = end;
        }
        None
    }

    fn render(source: &str, diagnostics: &[Diagnostic]) -> String {
        let (map, _) = nazm_core::one_file(source);
        diagnostics.iter().map(|d| d.render(&map)).collect()
    }

    /// Answer one line.
    pub(crate) fn line(&mut self, line: &str) -> Reply {
        let line = line.trim();
        match line {
            ":quit" | ":q" => return Reply::Quit,
            ":reset" => {
                self.defs.clear();
                return Reply::Reset;
            }
            ":defs" => {
                return Reply::Listing(self.defs.iter().map(|d| format!("{}\n", d.text)).collect())
            }
            _ => {}
        }
        if let Some(name) = defines(line) {
            if name == "main" {
                return Reply::Refused(
                    "`main` is the REPL's: an expression line is evaluated as its body".to_owned(),
                );
            }
            let mut next = self.defs.clone();
            let replaced = next.iter().position(|d| d.name == name);
            let def = Definition {
                name: name.clone(),
                text: line.to_owned(),
            };
            match replaced {
                Some(i) => {
                    if let Some(slot) = next.get_mut(i) {
                        *slot = def;
                    }
                }
                None => next.push(def),
            }
            let source = self.source(&next, "fn main() -> Int { 0 }\n");
            let diagnostics = nazm_core::check_source(&source);
            if !diagnostics.is_empty() {
                // A redefinition that breaks another definition is refused by that one's name:
                // no definition is ever left checked against a predecessor that is gone.
                let broken: Vec<String> = diagnostics
                    .iter()
                    .filter_map(|d| Self::owner(&next, d.primary.span))
                    .filter(|o| *o != name)
                    .collect();
                let head = match (replaced.is_some(), broken.first()) {
                    (true, Some(other)) => format!(
                        "redefining `{name}` is refused: `{other}` no longer checks against it\n"
                    ),
                    _ => String::new(),
                };
                return Reply::Refused(head + &Self::render(&source, &diagnostics));
            }
            self.defs = next;
            return if replaced.is_some() {
                Reply::Redefined(name)
            } else {
                Reply::Defined(name)
            };
        }
        self.evaluate(line)
    }

    /// An expression: the body of `main`, as an `Int`, then a `Bool`, then a `Str`.
    fn evaluate(&self, expr: &str) -> Reply {
        let wrappers = [
            format!("fn main(io: IoCap) -> Int ! {{ io }} {{ {expr} }}\n"),
            format!("fn main(io: IoCap) -> Int ! {{ io }} {{ if {expr} {{ 1 }} else {{ 0 }} }}\n"),
            format!(
                "fn main(io: IoCap) -> Int ! {{ io }} {{ print({expr}); print(\"\\n\"); 0 }}\n"
            ),
        ];
        let mut first = None;
        for (kind, w) in wrappers.iter().enumerate() {
            let source = self.source(&self.defs, w);
            let diagnostics = nazm_core::check_source(&source);
            if !diagnostics.is_empty() {
                first.get_or_insert_with(|| Self::render(&source, &diagnostics));
                continue;
            }
            return match nazm_core::run(&source) {
                nazm_core::Outcome::Value(v) => Reply::Value(match kind {
                    0 => v.to_string(),
                    1 => (v.to_string() == "1").to_string(),
                    _ => String::new(),
                }),
                nazm_core::Outcome::RuntimeFailed(d) => Reply::Refused(Self::render(&source, &[d])),
                nazm_core::Outcome::CheckFailed(ds) => Reply::Refused(Self::render(&source, &ds)),
            };
        }
        Reply::Refused(first.unwrap_or_default())
    }
}

/// `nazm repl`: a line at a time from standard input.
pub(crate) fn repl() -> ExitCode {
    let mut session = Session::default();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = session.line(&line);
        let text = match reply {
            Reply::Defined(n) => format!("defined `{n}`\n"),
            Reply::Redefined(n) => format!("redefined `{n}`\n"),
            Reply::Value(v) if v.is_empty() => String::new(),
            Reply::Value(v) => format!("= {v}\n"),
            Reply::Refused(r) => r,
            Reply::Listing(l) => l,
            Reply::Reset => "the session is empty\n".to_owned(),
            Reply::Quit => break,
        };
        let _ = out.write_all(text.as_bytes());
        let _ = out.flush();
    }
    ExitCode::SUCCESS
}

/// `nazm comptime FILE FUNCTION`: evaluate a parameterless pure function under budgets.
pub(crate) fn comptime(file: &Path, function: &str, limits: nazm_core::eval::Limits) -> ExitCode {
    let loaded = match crate::opened::opened(file, None, false) {
        Ok(l) => l,
        Err(code) => return ExitCode::from(code),
    };
    let root = loaded.graph.module_of(nazm_span::FileId::ROOT);
    let outcome = nazm_core::on_lowered(
        &loaded.map,
        &loaded.graph,
        nazm_core::Entry::Absent,
        |core, _| {
            let Some(f) = core
                .functions
                .iter()
                .find(|f| f.name == function && Some(f.module) == root)
            else {
                return Err(vec![Diagnostic::error(
                    codes::COMPTIME_REFUSED,
                    Span::synthetic(),
                    format!("there is no function `{function}` in this module to evaluate"),
                )]);
            };
            // The sandbox is the authority model (§7.67): a function of no parameters holds
            // no capability, and a pure one cannot exercise its caller's.
            let reason = if f.params > 0 {
                Some("it has parameters, so it is not a constant")
            } else if !f.effects.is_pure() {
                Some(
                    "it is not pure: it declares or requires an effect, so it could reach the host",
                )
            } else {
                None
            };
            if let Some(why) = reason {
                return Err(vec![Diagnostic::error(
                    codes::COMPTIME_REFUSED,
                    f.name_span,
                    format!("`{function}` cannot be evaluated at compile time: {why}"),
                )
                .primary_label("not evaluable at compile time")
                .help(
                    "comptime evaluates a function of no parameters declared `! {}`",
                )]);
            }
            let id = f.def;
            let mut interpreter = nazm_core::Interpreter::new(core.clone()).with_limits(limits);
            interpreter
                .run_constant(id)
                .map_err(|e| vec![*e.diagnostic])
        },
    );
    match outcome {
        Ok(v) => {
            println!("{v}");
            ExitCode::SUCCESS
        }
        Err(diagnostics) => {
            crate::emit(&diagnostics, &loaded.map, false);
            ExitCode::from(exit::CHECK_FAILED)
        }
    }
}

/// What a running image's code and data depend on, per definition: signatures and layouts.
#[derive(Debug, Default, PartialEq, Eq)]
struct Shape {
    functions: BTreeMap<String, (String, String)>,
    records: BTreeMap<String, String>,
    enums: BTreeMap<String, String>,
}

fn shape_of(file: &Path) -> Result<Shape, ExitCode> {
    let loaded = crate::opened::opened(file, None, false).map_err(ExitCode::from)?;
    let root = loaded.graph.module_of(nazm_span::FileId::ROOT);
    nazm_core::on_lowered(
        &loaded.map,
        &loaded.graph,
        nazm_core::Entry::Absent,
        |core, res| {
            let mut s = Shape::default();
            let types = res.types();
            for f in res.functions().iter().filter(|f| Some(f.module) == root) {
                let params: Vec<String> = f.params.iter().map(|t| types.show(*t)).collect();
                let effects = f
                    .declared_effects
                    .or(f.inferred_effects)
                    .unwrap_or_default();
                let signature = format!(
                    "({}) -> {} ! {{{}}}",
                    params.join(", "),
                    types.show(f.ret),
                    effects
                        .iter()
                        .map(|e| e.name())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                let body = core
                    .functions
                    .iter()
                    .find(|g| g.name == f.name && Some(g.module) == root)
                    .and_then(|g| loaded.map.slice(g.span))
                    .unwrap_or_default()
                    .to_owned();
                s.functions.insert(f.name.clone(), (signature, body));
            }
            for r in res
                .records()
                .all()
                .iter()
                .filter(|r| Some(r.module) == root)
            {
                let mut fields: Vec<String> = r
                    .fields
                    .iter()
                    .map(|f| format!("{}: {}", f.name, types.show(f.ty)))
                    .collect();
                // Layout is by name (§7.9): declaration order is not part of it.
                fields.sort();
                s.records.insert(r.name.clone(), fields.join(", "));
            }
            for e in res.enums().all().iter().filter(|e| Some(e.module) == root) {
                let mut variants: Vec<String> = e
                    .variants
                    .iter()
                    .map(|v| {
                        let fields: Vec<String> = v
                            .fields
                            .iter()
                            .map(|f| format!("{}: {}", f.name, types.show(f.ty)))
                            .collect();
                        format!("{}({})", v.name, fields.join(", "))
                    })
                    .collect();
                variants.sort();
                s.enums.insert(e.name.clone(), variants.join(" | "));
            }
            Ok(s)
        },
    )
    .map_err(|d| {
        crate::emit(&d, &loaded.map, false);
        ExitCode::from(exit::CHECK_FAILED)
    })
}

/// One verdict of a reload check.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Change {
    pub(crate) name: String,
    /// `reloadable` or `restart`.
    pub(crate) verdict: &'static str,
    pub(crate) why: String,
}

fn compare(old: &Shape, new: &Shape) -> Vec<Change> {
    let mut out = Vec::new();
    let mut push = |name: &str, verdict, why: String| {
        out.push(Change {
            name: name.to_owned(),
            verdict,
            why,
        });
    };
    for (n, (sig, body)) in &old.functions {
        match new.functions.get(n) {
            None => push(n, "restart", "removed: running code may still call it".to_owned()),
            Some((s, _)) if s != sig => push(
                n,
                "restart",
                format!("its signature changed from `{sig}` to `{s}`: every caller was compiled against the old one"),
            ),
            Some((_, b)) if b != body => push(n, "reloadable", "its body changed and its signature did not".to_owned()),
            Some(_) => {}
        }
    }
    for n in new
        .functions
        .keys()
        .filter(|n| !old.functions.contains_key(*n))
    {
        push(
            n,
            "reloadable",
            "added: nothing running calls it yet".to_owned(),
        );
    }
    for (kind, (o, n)) in [
        ("record", (&old.records, &new.records)),
        ("enum", (&old.enums, &new.enums)),
    ] {
        for (name, layout) in o {
            match n.get(name) {
                None => push(name, "restart", format!("the {kind} was removed: live values have its layout")),
                Some(l) if l != layout => push(
                    name,
                    "restart",
                    format!("the {kind}'s layout changed from `{layout}` to `{l}`: live values have the old one"),
                ),
                Some(_) => {}
            }
        }
        for name in n.keys().filter(|k| !o.contains_key(*k)) {
            push(
                name,
                "reloadable",
                format!("the {kind} was added: no live value has it"),
            );
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// `nazm reload-check OLD NEW`: one `nazm.reload-check/1` line, and exit 0 only if every change
/// can be taken by a running image.
pub(crate) fn reload_check(old: &Path, new: &Path) -> ExitCode {
    let (a, b) = match (shape_of(old), shape_of(new)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    let changes = compare(&a, &b);
    let restart = changes.iter().any(|c| c.verdict == "restart");
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let items: Vec<String> = changes
        .iter()
        .map(|c| {
            format!(
                "{{\"name\":\"{}\",\"verdict\":\"{}\",\"why\":\"{}\"}}",
                esc(&c.name),
                c.verdict,
                esc(&c.why)
            )
        })
        .collect();
    println!(
        "{{\"schema\":\"nazm.reload-check/1\",\"runtime_abi\":{},\"outcome\":\"{}\",\"changes\":[{}]}}",
        nazm_lir::runtime::RUNTIME_ABI_REVISION,
        if restart { "restart" } else { "reloadable" },
        items.join(",")
    );
    if restart {
        for c in changes.iter().filter(|c| c.verdict == "restart") {
            eprintln!("nazm: `{}` needs a restart: {}", c.name, c.why);
        }
        ExitCode::from(exit::CHECK_FAILED)
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_definition_line_is_told_from_an_expression() {
        assert_eq!(defines("fn f(x: Int) -> Int { x }"), Some("f".to_owned()));
        assert_eq!(defines("pub struct P { x: Int }"), Some("P".to_owned()));
        assert_eq!(defines("enum E { A, B }"), Some("E".to_owned()));
        assert_eq!(defines("f(2) + 1"), None);
    }

    #[test]
    fn a_session_defines_evaluates_and_redefines_without_stale_definitions() {
        let mut s = Session::default();
        assert_eq!(
            s.line("fn g(x: Int) -> Int { x + 1 }"),
            Reply::Defined("g".into())
        );
        assert_eq!(
            s.line("fn f(x: Int) -> Int { g(x) * 2 }"),
            Reply::Defined("f".into())
        );
        assert_eq!(s.line("f(3)"), Reply::Value("8".into()));
        assert_eq!(
            s.line("fn g(x: Int) -> Int { x + 10 }"),
            Reply::Redefined("g".into())
        );
        // `f` now calls the new `g`: nothing of the old one survives.
        assert_eq!(s.line("f(3)"), Reply::Value("26".into()));
        assert_eq!(s.line("f(3) > 20"), Reply::Value("true".into()));
    }

    #[test]
    fn a_redefinition_that_breaks_a_caller_is_refused_by_the_caller_s_name() {
        let mut s = Session::default();
        s.line("fn g(x: Int) -> Int { x + 1 }");
        s.line("fn f(x: Int) -> Int { g(x) * 2 }");
        let Reply::Refused(r) = s.line("fn g(x: Bool) -> Int { 0 }") else {
            panic!("accepted");
        };
        assert!(
            r.starts_with("redefining `g` is refused: `f` no longer checks against it"),
            "{r}"
        );
        // The session is what it was.
        assert_eq!(s.line("f(1)"), Reply::Value("4".into()));
    }

    #[test]
    fn main_is_the_repl_s_and_a_failure_is_reported() {
        let mut s = Session::default();
        assert!(
            matches!(s.line("fn main() -> Int { 1 }"), Reply::Refused(r) if r.contains("`main` is the REPL's"))
        );
        assert!(
            matches!(s.line("9223372036854775807 + 1"), Reply::Refused(r) if r.contains("N0400"))
        );
        assert_eq!(s.line(":reset"), Reply::Reset);
        assert_eq!(s.line(":quit"), Reply::Quit);
    }

    #[test]
    fn a_reload_tells_a_body_from_a_signature_and_a_layout() {
        let shape = |fs: &[(&str, &str, &str)], rs: &[(&str, &str)]| Shape {
            functions: fs
                .iter()
                .map(|(n, s, b)| ((*n).to_owned(), ((*s).to_owned(), (*b).to_owned())))
                .collect(),
            records: rs
                .iter()
                .map(|(n, l)| ((*n).to_owned(), (*l).to_owned()))
                .collect(),
            enums: BTreeMap::new(),
        };
        let old = shape(
            &[
                ("f", "(Int) -> Int ! {}", "a"),
                ("g", "() -> Int ! {}", "b"),
            ],
            &[("P", "x: Int")],
        );
        let body = shape(
            &[
                ("f", "(Int) -> Int ! {}", "a2"),
                ("g", "() -> Int ! {}", "b"),
            ],
            &[("P", "x: Int")],
        );
        let c = compare(&old, &body);
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].name.as_str(), c[0].verdict), ("f", "reloadable"));
        let sig = shape(
            &[
                ("f", "(Bool) -> Int ! {}", "a"),
                ("g", "() -> Int ! {}", "b"),
            ],
            &[("P", "x: Int, y: Int")],
        );
        let c = compare(&old, &sig);
        assert_eq!(
            c.iter()
                .map(|c| (c.name.as_str(), c.verdict))
                .collect::<Vec<_>>(),
            [("P", "restart"), ("f", "restart")]
        );
        let gone = shape(&[("f", "(Int) -> Int ! {}", "a")], &[("P", "x: Int")]);
        assert_eq!(compare(&old, &gone)[0].verdict, "restart");
    }
}
