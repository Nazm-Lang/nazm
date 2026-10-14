//! The task suite: eight tasks over the seven N33 scenarios, one to three per category, each
//! with its two contexts and the truths its oracle scores against.
//!
//! **Context-arm law.** A task has exactly two contexts. The baseline is the N33 scenario's
//! naive texts, each under a `==> label <==` line naming the file (or catalogue entry) it is,
//! as an agent that opened them would know. The N33 context is `nazm repo --task`'s JSON for
//! the scenario's request, exactly as printed, never trimmed. Each is addressed by the BLAKE3
//! of its bytes.
//!
//! **Oracle law.** Every truth is read from an authority — the service's packet, a
//! documentation section, the mutation catalogue parsed independently — and never from either
//! context. Where a truth is a statement a section makes, the section's text is checked for
//! it here, so a reworded section stops the build instead of leaving a stale truth.
//!
//! **Sufficiency law.** Every task is meant to be solvable from either context: each fact's
//! witness (a name, a phrase) must occur in both, or the suite is not built. So an answer that
//! the context is insufficient is always a failure here, and never a correct refusal.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::scenarios::{self, Scenario};

/// Which context a request carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arm {
    Baseline,
    N33,
}

impl Arm {
    pub const ALL: [Self; 2] = [Self::Baseline, Self::N33];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::N33 => "n33",
        }
    }
}

/// The five task categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Understand,
    Edit,
    Diagnose,
    Docs,
    Tests,
}

impl Category {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Understand => "understand",
            Self::Edit => "edit",
            Self::Diagnose => "diagnose",
            Self::Docs => "docs",
            Self::Tests => "tests",
        }
    }
}

/// What one answer field must hold. A set's facts are its members; a scalar is one fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Truth {
    /// Definitions, each `file::name`.
    Definitions { items: BTreeSet<String> },
    /// One definition, `file::name`.
    Definition { value: String },
    /// Test functions, by name without their module path.
    Tests { items: BTreeSet<String> },
    /// Test targets, by name.
    Targets { items: BTreeSet<String> },
    /// Sections that must be cited, as `document:id`; `known` is every id the document has,
    /// and citing one of them beyond `required` is neither credit nor a wrong claim.
    Citations {
        required: BTreeSet<String>,
        known: BTreeSet<String>,
    },
    /// A value, and every spelling of it that is accepted (compared lowercase, without
    /// backticks, with runs of whitespace as one space).
    Text { value: String, accept: Vec<String> },
    /// A signature, compared with runs of whitespace as one space.
    Signature { value: String },
    /// A yes-or-no statement.
    Flag { value: bool },
}

impl Truth {
    /// How many facts this field is.
    #[must_use]
    pub fn required(&self) -> usize {
        match self {
            Self::Definitions { items } | Self::Tests { items } | Self::Targets { items } => {
                items.len()
            }
            Self::Citations { required, .. } => required.len(),
            Self::Definition { .. }
            | Self::Text { .. }
            | Self::Signature { .. }
            | Self::Flag { .. } => 1,
        }
    }

    fn placeholder(&self) -> Value {
        match self {
            Self::Definitions { .. } => json!(["<file>::<name>"]),
            Self::Definition { .. } => json!("<file>::<name>"),
            Self::Tests { .. } => json!(["<test function name>"]),
            Self::Targets { .. } => json!(["<test target name>"]),
            Self::Citations { .. } => json!(["<document>:<section id>"]),
            Self::Text { .. } => json!("<string>"),
            Self::Signature { .. } => json!("<signature>"),
            Self::Flag { .. } => json!("<true or false>"),
        }
    }
}

/// One field of a task's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Field {
    pub name: &'static str,
    pub truth: Truth,
}

/// One arm's context: its exact bytes, their digest, and the text a claim's identifier is
/// looked for in when deciding whether the context supported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    pub arm: Arm,
    pub text: String,
    pub digest: String,
    /// The baseline's text, or every string in the N33 JSON unescaped, one after another —
    /// with every run of whitespace as one space.
    pub searchable: String,
}

impl Context {
    fn new(arm: Arm, text: String) -> Result<Self, String> {
        let searchable = match arm {
            Arm::Baseline => text.clone(),
            Arm::N33 => {
                let v: Value = serde_json::from_str(&text)
                    .map_err(|e| format!("the N33 context is not JSON: {e}"))?;
                let mut out = String::new();
                strings(&v, &mut out);
                out
            }
        };
        Ok(Self {
            arm,
            digest: crate::digest(text.as_bytes()),
            text,
            searchable: squash(&searchable),
        })
    }

    /// Whether `needle` occurs in the context, with runs of whitespace as one space.
    #[must_use]
    pub fn mentions(&self, needle: &str) -> bool {
        self.searchable.contains(&squash(needle))
    }
}

/// `s` with every run of whitespace as one space.
#[must_use]
pub fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strings(v: &Value, out: &mut String) {
    match v {
        Value::String(s) => {
            out.push_str(s);
            out.push('\n');
        }
        Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
        Value::Object(o) => o.values().for_each(|x| strings(x, out)),
        _ => {}
    }
}

/// One task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: &'static str,
    pub category: Category,
    /// The N33 scenario whose contexts it uses.
    pub scenario: &'static str,
    pub wording: String,
    pub fields: Vec<Field>,
    /// Baseline, then N33.
    pub contexts: [Context; 2],
    /// What must occur in both contexts for the task to be solvable from either.
    pub witnesses: Vec<String>,
}

impl Task {
    #[must_use]
    pub const fn context(&self, arm: Arm) -> &Context {
        match arm {
            Arm::Baseline => &self.contexts[0],
            Arm::N33 => &self.contexts[1],
        }
    }

    /// How many facts a complete answer has.
    #[must_use]
    pub fn required(&self) -> usize {
        self.fields.iter().map(|f| f.truth.required()).sum()
    }

    /// The answer contract: the one JSON shape every reply must have.
    #[must_use]
    pub fn contract(&self) -> String {
        let mut answer = serde_json::Map::new();
        for f in &self.fields {
            answer.insert(f.name.to_owned(), f.truth.placeholder());
        }
        let v = json!({"answer": Value::Object(answer), "insufficient_context": false});
        serde_json::to_string_pretty(&v).unwrap_or_default()
    }
}

/// The suite, and the digest of everything that decides a result: every task's wording,
/// contract, truths and context digests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suite {
    pub tasks: Vec<Task>,
    pub digest: String,
}

impl Suite {
    #[must_use]
    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }
}

/// The pilot's tasks: the two smallest contexts, one diagnose and one test selection. A pilot
/// is its own benchmark identity and never evidence.
pub const PILOT: &[&str] = &["T4", "T8"];

/// `analyse.nz::fn build_children` → `analyse.nz::build_children`.
fn def(identity: &str) -> String {
    let (file, rest) = identity.split_once("::").unwrap_or(("", identity));
    let name = rest.rsplit(' ').next().unwrap_or(rest);
    format!("{file}::{name}")
}

/// `test:nazm-cli:test:selfhost::a_b` → (`selfhost`, `a_b`); a lib's module path is dropped.
fn killer(id: &str) -> (String, String) {
    let (target, name) = id.split_once("::").unwrap_or((id, ""));
    let target = target.rsplit(':').next().unwrap_or(target);
    let name = name.rsplit("::").next().unwrap_or(name);
    (target.to_owned(), name.to_owned())
}

/// The whole-file baseline: each text under the line naming it.
fn baseline(s: &Scenario) -> String {
    let mut out = String::new();
    for (label, text) in s.labels.iter().zip(&s.naive) {
        out.push_str("==> ");
        out.push_str(label);
        out.push_str(" <==\n");
        out.push_str(text);
        if !text.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

fn contexts(s: &Scenario) -> Result<[Context; 2], String> {
    Ok([
        Context::new(Arm::Baseline, baseline(s))?,
        Context::new(Arm::N33, s.json.clone())?,
    ])
}

/// Every id `document` has, as `document:id`.
fn known(document: &str) -> Result<BTreeSet<String>, String> {
    match nazm_docs::index(&scenarios::root(), Some(document)) {
        nazm_docs::IndexResult::Available(ix) => Ok(ix
            .documents
            .iter()
            .flat_map(|d| d.sections.iter())
            .map(|s| {
                if s.id.contains(':') {
                    s.id.clone()
                } else {
                    format!("{document}:{}", s.id)
                }
            })
            .collect()),
        _ => Err(format!(
            "the documentation index of {document} is unavailable"
        )),
    }
}

/// `body` must say `phrase`, or the truth resting on it is stale.
fn says(id: &str, body: &str, phrase: &str) -> Result<(), String> {
    if body.contains(phrase) {
        Ok(())
    } else {
        Err(format!(
            "{id} no longer says {phrase:?}: the truth resting on it is stale"
        ))
    }
}

fn text(value: &str, accept: &[&str]) -> Truth {
    Truth::Text {
        value: value.to_owned(),
        accept: accept.iter().map(|s| (*s).to_owned()).collect(),
    }
}

fn set(items: impl IntoIterator<Item = String>) -> BTreeSet<String> {
    items.into_iter().collect()
}

/// The killer tests of catalogue entries in `file` whose `find` text lies inside the span.
fn killers_inside(
    catalogue: &[(String, String, String, String, Vec<String>)],
    file: &str,
    (start, end): (u32, u32),
) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(scenarios::root().join(file))
        .map_err(|e| format!("{file}: {e}"))?;
    let mut out = Vec::new();
    for (_, path, find, _, killers) in catalogue {
        if path != file {
            continue;
        }
        let at = text
            .find(find.as_str())
            .ok_or_else(|| format!("a {file} mutation's text is not in it"))?;
        if at >= start as usize && at + find.len() <= end as usize {
            out.extend(killers.iter().cloned());
        }
    }
    Ok(out)
}

/// Builds the suite from this checkout.
///
/// # Errors
///
/// When a scenario is missing, an authority is unavailable, a truth's evidence has changed,
/// or a fact's witness is absent from either context.
#[allow(clippy::too_many_lines)]
pub fn build() -> Result<Suite, String> {
    let rows = scenarios::all();
    let row = |name: &str| {
        rows.iter()
            .find(|r| r.name == name)
            .ok_or_else(|| format!("the N33 scenario {name:?} is missing"))
    };
    let catalogue = scenarios::catalogue();
    let mut tasks = Vec::new();
    let mut add = |id: &'static str,
                   category: Category,
                   scenario: &'static str,
                   wording: &str,
                   fields: Vec<Field>,
                   witnesses: Vec<String>|
     -> Result<(), String> {
        let r = row(scenario)?;
        let wording = format!("Task {id} ({}). {wording}", category.name());
        tasks.push(Task {
            id,
            category,
            scenario,
            wording,
            fields,
            contexts: contexts(r)?,
            witnesses,
        });
        Ok(())
    };

    // T1 — understand the checker's entry point.
    let p = scenarios::packet("compiler/emit.nz", "analyse.nz::fn check_program");
    let signature = p
        .target
        .signature
        .clone()
        .ok_or("check_program has no signature")?;
    let calls = set(p
        .dependencies
        .iter()
        .filter(|d| d.kind == "function")
        .filter_map(|d| d.identity.definition.as_deref())
        .filter(|k| *k != "analyse.nz::fn check_program")
        .map(def));
    let types = set(p
        .related_types
        .iter()
        .filter_map(|t| t.identity.definition.as_deref())
        .map(def));
    let mut w: Vec<String> = calls
        .iter()
        .chain(&types)
        .map(|d| d.rsplit("::").next().unwrap_or(d).to_owned())
        .collect();
    w.push(signature.clone());
    add(
        "T1",
        Category::Understand,
        "A understand check_program",
        "In the compiler sources, take the function `check_program` in `analyse.nz`. Give its \
         signature exactly as declared, from `fn` through its return type; every function \
         defined in the compiler sources that its body refers to directly; and every record \
         type defined in the compiler sources that its signature names. Name each definition \
         as `<file>::<name>`: the file's name without its directory, then the definition's \
         name.",
        vec![
            Field {
                name: "signature",
                truth: Truth::Signature { value: signature },
            },
            Field {
                name: "calls",
                truth: Truth::Definitions { items: calls },
            },
            Field {
                name: "signature_types",
                truth: Truth::Definitions { items: types },
            },
        ],
        w,
    )?;

    // T2 and T3 — attach_core, in every compilation that has it.
    let mut deps = BTreeSet::new();
    let mut callers = BTreeSet::new();
    for r in ["compiler/emit.nz", "compiler/check.nz"] {
        let p = scenarios::packet(r, "module.nz::fn attach_core");
        deps.extend(
            p.dependencies
                .iter()
                .filter(|d| d.kind == "function")
                .filter_map(|d| d.identity.definition.as_deref())
                .filter(|k| *k != "module.nz::fn attach_core")
                .map(def),
        );
        for c in p.callers.items.unwrap_or_default() {
            if let Some(k) = c.identity.definition.as_deref() {
                callers.insert(def(k));
            }
        }
    }
    let names = |s: &BTreeSet<String>| -> Vec<String> {
        s.iter()
            .map(|d| d.rsplit("::").next().unwrap_or(d).to_owned())
            .collect()
    };
    let mut w = names(&deps);
    w.extend(names(&callers));
    add(
        "T2",
        Category::Understand,
        "B edit attach_core",
        "In the compiler sources, take the function `attach_core` in `module.nz`. Which \
         functions defined in the compiler sources does its body refer to directly, and which \
         functions call it directly? Name each definition as `<file>::<name>`: the file's name \
         without its directory, then the definition's name.",
        vec![
            Field {
                name: "dependencies",
                truth: Truth::Definitions {
                    items: deps.clone(),
                },
            },
            Field {
                name: "callers",
                truth: Truth::Definitions {
                    items: callers.clone(),
                },
            },
        ],
        w,
    )?;

    let decl = scenarios::packet("compiler/emit.nz", "module.nz::fn attach_core")
        .target
        .source;
    let guarding = killers_inside(&catalogue, "compiler/module.nz", (decl.start, decl.end))?;
    let tests = set(guarding.iter().map(|k| killer(k).1));
    let targets = set(guarding.iter().map(|k| killer(k).0));
    let mut edited = callers.clone();
    edited.insert("module.nz::attach_core".to_owned());
    let mut w = names(&edited);
    w.extend(tests.iter().cloned());
    w.extend(targets.iter().cloned());
    add(
        "T3",
        Category::Edit,
        "B edit attach_core",
        "You are about to add a parameter to the function `attach_core` in `module.nz`. Do not \
         write the patch; plan it. List every definition whose source must change for the \
         compiler sources to keep checking: the function itself and each of its direct \
         callers. List the tests that guard `attach_core`'s own source: the tests the mutation \
         catalogue names as killers of its entries whose mutated text lies inside \
         `attach_core`, each by its function name without its module path. List the test \
         targets those tests are in, by name. Name each definition as `<file>::<name>`: the \
         file's name without its directory, then the definition's name.",
        vec![
            Field {
                name: "definitions_to_change",
                truth: Truth::Definitions { items: edited },
            },
            Field {
                name: "tests",
                truth: Truth::Tests { items: tests },
            },
            Field {
                name: "test_targets",
                truth: Truth::Targets { items: targets },
            },
        ],
        w,
    )?;

    // T4 — diagnose N0300, the fixed-overhead stress case: kept, whatever it shows.
    let c = row("C diagnose N0300")?;
    let seed = c
        .context
        .get("items")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .ok_or("the N0300 context has no seed")?;
    let message = seed
        .pointer("/content/message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let owner = scenarios::packet(
        "examples/bad/return_type_mismatch.nz",
        "return_type_mismatch.nz::fn describe",
    );
    let declared = owner
        .target
        .signature
        .as_deref()
        .and_then(|s| s.rsplit("-> ").next())
        .ok_or("describe has no return type")?
        .trim()
        .to_owned();
    let span = |k: &str| {
        seed.pointer(&format!("/content/primary/{k}"))
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| format!("the N0300 diagnostic has no {k}"))
    };
    let program =
        std::fs::read_to_string(scenarios::root().join("examples/bad/return_type_mismatch.nz"))
            .map_err(|e| format!("return_type_mismatch.nz: {e}"))?;
    let offending = program
        .get(span("start")?..span("end")?)
        .ok_or("the N0300 span is not in the program")?
        .to_owned();
    says(
        "the N0300 diagnostic",
        message,
        &format!("but the function returns `{declared}`"),
    )?;
    let bare = offending.trim_matches('"').to_owned();
    add(
        "T4",
        Category::Diagnose,
        "C diagnose N0300",
        "Checking the program reports diagnostic N0300. Name the function that contains the \
         error as `<file>::<name>` (the file's name without its directory, then the \
         function's name), the return type that function declares, and the offending \
         expression exactly as it is written in the source.",
        vec![
            Field {
                name: "function",
                truth: Truth::Definition {
                    value: "return_type_mismatch.nz::describe".to_owned(),
                },
            },
            Field {
                name: "declared_return_type",
                truth: text(&declared, &[&declared]),
            },
            Field {
                name: "offending_expression",
                truth: text(
                    &offending,
                    &[
                        &offending,
                        &bare,
                        &format!("return {offending}"),
                        &format!("return {offending};"),
                    ],
                ),
            },
        ],
        vec!["describe".to_owned(), declared.clone(), offending.clone()],
    )?;

    // T5 — a specification question, whose answer spans a superseded section and its successor.
    let returns = scenarios::section("spec:returns").body;
    let ret = scenarios::section("spec:return").body;
    let evidence = [
        (
            "spec:returns",
            &returns,
            "**The current executable language is 1.0**",
        ),
        (
            "spec:returns",
            &returns,
            "`return` exists and is implemented",
        ),
        (
            "spec:return",
            &ret,
            "leaves the enclosing **function**, not merely the enclosing loop",
        ),
        (
            "spec:return",
            &ret,
            "**`return;` with no value is not supported** in a function with a result type",
        ),
    ];
    for (id, body, phrase) in evidence {
        says(id, body, phrase)?;
    }
    add(
        "T5",
        Category::Docs,
        "D understand spec:returns",
        "Answer from the specification. Does the current executable language have a `return` \
         statement? Which version is the current executable language? What does \
         `return expression;` leave: the enclosing function, or the enclosing loop? Is \
         `return;` with no value supported in a function that has a result type? Cite every \
         specification section your answers rest on as `spec:<id>`, where `<id>` is the section's `<!-- id: ... -->` anchor.",
        vec![
            Field {
                name: "has_return_statement",
                truth: Truth::Flag { value: true },
            },
            Field {
                name: "current_version",
                truth: text(
                    "0.10.0",
                    &["0.10.0", "v0.10.0", "nazm 0.10", "0.10", "1.0 draft"],
                ),
            },
            Field {
                name: "return_leaves",
                truth: text(
                    "function",
                    &[
                        "function",
                        "the function",
                        "enclosing function",
                        "the enclosing function",
                    ],
                ),
            },
            Field {
                name: "bare_return_supported",
                truth: Truth::Flag { value: false },
            },
            Field {
                name: "citations",
                truth: Truth::Citations {
                    required: set(["spec:returns".to_owned(), "spec:return".to_owned()]),
                    known: known("spec")?,
                },
            },
        ],
        evidence
            .iter()
            .map(|(_, _, p)| (*p).to_owned())
            .chain(["id: returns".to_owned(), "id: return ".to_owned()])
            .collect(),
    )?;

    // T6 — a grammar question, through the guide rule generated from it.
    let record = scenarios::section("grammar:record").body;
    let evidence = [
        "record = [ visibility ] , \"struct\" , IDENT , [ type_parameters ] , \"{\" , [ fields ] , \"}\" ;",
        "reordering two fields changes neither the type",
    ];
    for phrase in evidence {
        says("grammar:record", &record, phrase)?;
    }
    add(
        "T6",
        Category::Docs,
        "D understand guide record",
        "Answer from the grammar. Which keyword begins a record declaration? May a visibility \
         modifier precede it? May a record declare type parameters? May a record declare no \
         fields at all? Does the order a record's fields are declared in change its type? \
         Cite the grammar production your answers rest on as `grammar:<production name>`.",
        vec![
            Field {
                name: "keyword",
                truth: text("struct", &["struct"]),
            },
            Field {
                name: "visibility_allowed",
                truth: Truth::Flag { value: true },
            },
            Field {
                name: "type_parameters_allowed",
                truth: Truth::Flag { value: true },
            },
            Field {
                name: "no_fields_allowed",
                truth: Truth::Flag { value: true },
            },
            Field {
                name: "field_order_changes_type",
                truth: Truth::Flag { value: false },
            },
            Field {
                name: "citations",
                truth: Truth::Citations {
                    required: set(["grammar:record".to_owned()]),
                    known: known("grammar")?,
                },
            },
        ],
        evidence.iter().map(|p| (*p).to_owned()).collect(),
    )?;

    // T7 and T8 — test selection, for a package and for one definition.
    let mut docs_killers = Vec::new();
    for (_, path, _, _, killers) in &catalogue {
        if path.starts_with("crates/nazm-docs/") {
            docs_killers.extend(killers.iter().cloned());
        }
    }
    let tests = set(docs_killers.iter().map(|k| killer(k).1));
    if tests.len() != set(docs_killers.iter().cloned()).len() {
        return Err("two nazm-docs killers share a function name".to_owned());
    }
    add(
        "T7",
        Category::Tests,
        "E tests of nazm-docs",
        "A change is made somewhere in the crate `nazm-docs` (its files under \
         `crates/nazm-docs/`). Which tests does the mutation catalogue name as killers of its \
         entries for that crate's files? Give each test by its function name, without its \
         module path.",
        vec![Field {
            name: "tests",
            truth: Truth::Tests {
                items: tests.clone(),
            },
        }],
        tests.into_iter().collect(),
    )?;

    let decl = scenarios::packet("compiler/emit.nz", "module.nz::fn core_module")
        .target
        .source;
    let tests = set(
        killers_inside(&catalogue, "compiler/module.nz", (decl.start, decl.end))?
            .iter()
            .map(|k| killer(k).1),
    );
    add(
        "T8",
        Category::Tests,
        "E tests of core_module",
        "You changed the body of the function `core_module` in `module.nz`. Which tests does \
         the mutation catalogue name as killers of its entries whose mutated text lies inside \
         `core_module`? Give each test by its function name, without its module path.",
        vec![Field {
            name: "tests",
            truth: Truth::Tests {
                items: tests.clone(),
            },
        }],
        tests.into_iter().collect(),
    )?;

    for t in &tasks {
        for c in &t.contexts {
            for w in &t.witnesses {
                if !c.mentions(w) {
                    return Err(format!(
                        "{}: the {} context does not contain {w:?}, so the task is not \
                         solvable from it",
                        t.id,
                        c.arm.name()
                    ));
                }
            }
        }
        if t.contexts[0].digest == t.contexts[1].digest {
            return Err(format!("{}: both arms carry the same context", t.id));
        }
    }
    let digest = crate::digest(canonical(&tasks).as_bytes());
    Ok(Suite { tasks, digest })
}

/// Everything about the tasks that decides a result, as one JSON text.
fn canonical(tasks: &[Task]) -> String {
    let v: Vec<Value> = tasks
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "category": t.category,
                "scenario": t.scenario,
                "wording": t.wording,
                "contract": t.contract(),
                "fields": t.fields,
                "contexts": t.contexts.iter().map(|c| json!({"arm": c.arm, "digest": c.digest})).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({"suite": crate::SUITE_VERSION, "tasks": v}).to_string()
}
