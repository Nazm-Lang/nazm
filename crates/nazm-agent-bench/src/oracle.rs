//! The oracle: an answer, scored fact by fact against the suite's truths, offline.
//!
//! **A fact** is one member of a set field or one scalar field. It is correct or it is
//! missing; there is no partial credit inside a fact and no rating of prose.
//!
//! **A claim** is something the answer asserts that the truths do not contain. It is
//! *hallucinated* when it is false and the supplied context does not support it either (its
//! identifier does not occur there, or it names a section that does not exist); it is a
//! *wrong repository fact* when the context has it and the answer misread it. A citation of a
//! real section beyond the required ones is recorded and neither credited nor counted wrong.
//!
//! **Solved** means every fact is correct and no claim is false, with a valid answer and no
//! refusal. Every task is solvable from either context (the suite's sufficiency law), so a
//! refusal is always a failure here.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::suite::{Arm, Context, Task, Truth};

/// One fact and whether the answer has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fact {
    pub field: String,
    pub fact: String,
    pub ok: bool,
}

/// What an unmatched claim is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    /// False, and the context has what it names.
    Wrong,
    /// False, and nothing in the context supports it.
    Hallucinated,
    /// A real section cited beyond the required ones.
    ExtraCitation,
}

/// One claim the truths do not contain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub field: String,
    pub value: String,
    pub kind: ClaimKind,
}

/// Why a scored answer is not solved, the most serious reason first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    InvalidStructuredOutput,
    Refusal,
    HallucinatedFact,
    WrongRepositoryFact,
    MissingRequiredFact,
}

/// An answer's score.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Score {
    pub required: usize,
    pub correct: usize,
    pub missing: usize,
    pub wrong: usize,
    pub hallucinated: usize,
    pub extra_citations: usize,
    pub refusal: bool,
    /// Why the reply is not a valid answer, if it is not.
    pub invalid: Option<String>,
    pub solved: bool,
    pub failure: Option<Failure>,
    pub facts: Vec<Fact>,
    pub claims: Vec<Claim>,
}

/// The reply's JSON object: the text itself, or the one fenced block it consists of.
///
/// # Errors
///
/// When the reply is not one JSON object.
pub fn parse(reply: &str) -> Result<serde_json::Map<String, Value>, String> {
    let mut t = reply.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let body = rest.split_once('\n').map_or("", |(_, b)| b);
        t = body.trim_end().strip_suffix("```").unwrap_or(body).trim();
    }
    match serde_json::from_str::<Value>(t) {
        Ok(Value::Object(o)) => Ok(o),
        Ok(_) => Err("the reply is JSON but not an object".to_owned()),
        Err(e) => Err(format!("the reply is not JSON: {e}")),
    }
}

use crate::suite::squash;

/// The last fenced block of a reply that is prose around it — the secondary, lenient reading
/// of an answer that broke the contract by explaining itself. `None` when there is no fence
/// or the reply is already one JSON object.
#[must_use]
pub fn fenced(reply: &str) -> Option<String> {
    if parse(reply).is_ok() {
        return None;
    }
    let start = reply.rfind("```")?;
    let before = reply.get(..start)?;
    let open = before.rfind("```")?;
    let block = reply.get(open..start + 3)?;
    parse(block).ok().map(|_| block.to_owned())
}

fn plain(s: &str) -> String {
    squash(&s.replace('`', "").to_lowercase())
        .trim_end_matches('.')
        .trim()
        .to_owned()
}

/// `def:compiler:module.nz::fn attach_core`, `compiler/module.nz::attach_core`,
/// `module.nz::attach_core()` → (`Some("module.nz")`, `attach_core`); `attach_core` → (None, …).
#[must_use]
pub fn definition(s: &str) -> (Option<String>, String) {
    let mut s = s.trim().trim_matches('`').trim();
    if let Some(rest) = s.strip_prefix("def:") {
        s = rest;
        if let Some((package, rest)) = s.split_once(':') {
            if !rest.starts_with(':') && !package.contains('.') {
                s = rest;
            }
        }
    }
    let parts: Vec<&str> = s.split("::").collect();
    let last = parts.last().copied().unwrap_or(s).trim();
    let last = ["fn ", "record ", "struct ", "enum "]
        .iter()
        .fold(last, |acc, p| acc.strip_prefix(p).unwrap_or(acc));
    let name = last.split('(').next().unwrap_or(last).trim().to_owned();
    let file = parts
        .iter()
        .rev()
        .skip(1)
        .find(|p| p.ends_with(".nz"))
        .map(|p| p.rsplit('/').next().unwrap_or(p).to_owned());
    (file, name)
}

/// A test by its function name: the last path segment, without `()`.
#[must_use]
pub fn test_name(s: &str) -> String {
    let s = s.trim().trim_matches('`').trim();
    let s = s.rsplit("::").next().unwrap_or(s);
    s.trim_end_matches("()").trim().to_owned()
}

/// A test target by its name: `target:nazm-cli:test:selfhost`, `tests/selfhost.rs` and
/// `nazm-cli selfhost` are all `selfhost`.
#[must_use]
pub fn target_name(s: &str) -> String {
    let s = s.trim().trim_matches('`').trim();
    let s = s.strip_suffix(".rs").unwrap_or(s);
    s.rsplit([':', '/', ' ']).next().unwrap_or(s).to_owned()
}

/// A citation as `document:id`: `spec.md#return` and `docs/spec.md:return` are `spec:return`.
#[must_use]
pub fn citation(s: &str) -> String {
    let s = s.trim().trim_matches('`').trim().to_lowercase();
    let s = s.strip_prefix("docs/").unwrap_or(&s).to_owned();
    s.replace(".md#", ":")
        .replace(".ebnf#", ":")
        .replace(".md:", ":")
        .replace(".ebnf:", ":")
        .replace('#', ":")
}

fn strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|x| match x {
                Value::String(s) => Some(s.clone()),
                Value::Null => None,
                other => Some(other.to_string()),
            })
            .filter(|s| !s.trim().is_empty())
            .collect(),
        Some(Value::String(s)) if !s.trim().is_empty() => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn scalar(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::Array(a)) if a.len() == 1 => scalar(a.first()),
        _ => None,
    }
}

fn flag(v: Option<&Value>) -> Option<bool> {
    match v {
        Some(Value::Bool(b)) => Some(*b),
        Some(Value::String(s)) => match s.trim().to_lowercase().as_str() {
            "true" | "yes" => Some(true),
            "false" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

struct Tally<'a> {
    context: &'a Context,
    facts: Vec<Fact>,
    claims: Vec<Claim>,
}

impl Tally<'_> {
    fn fact(&mut self, field: &str, fact: &str, ok: bool) {
        self.facts.push(Fact {
            field: field.to_owned(),
            fact: fact.to_owned(),
            ok,
        });
    }

    /// A false claim: hallucinated unless the context mentions `witness`.
    fn false_claim(&mut self, field: &str, value: &str, witness: &str) {
        let kind = if !witness.is_empty() && self.context.mentions(witness) {
            ClaimKind::Wrong
        } else {
            ClaimKind::Hallucinated
        };
        self.claim(field, value, kind);
    }

    fn claim(&mut self, field: &str, value: &str, kind: ClaimKind) {
        self.claims.push(Claim {
            field: field.to_owned(),
            value: value.to_owned(),
            kind,
        });
    }

    fn definitions(&mut self, field: &str, truths: &[String], given: &[String]) {
        let truths: Vec<(String, String)> = truths
            .iter()
            .map(|t| {
                let (f, n) = definition(t);
                (f.unwrap_or_default(), n)
            })
            .collect();
        let mut hit = vec![false; truths.len()];
        let mut seen = std::collections::BTreeSet::new();
        for g in given {
            let (file, name) = definition(g);
            if !seen.insert((file.clone(), name.clone())) {
                continue;
            }
            let found: Vec<usize> = truths
                .iter()
                .enumerate()
                .filter(|(_, (f, n))| *n == name && file.as_ref().is_none_or(|x| x == f))
                .map(|(i, _)| i)
                .collect();
            match found.as_slice() {
                [i] => {
                    if let Some(h) = hit.get_mut(*i) {
                        *h = true;
                    }
                }
                _ => self.false_claim(field, g, &name),
            }
        }
        for ((f, n), ok) in truths.iter().zip(hit) {
            self.fact(field, &format!("{f}::{n}"), ok);
        }
    }

    fn named(
        &mut self,
        field: &str,
        truths: &std::collections::BTreeSet<String>,
        given: &[String],
        norm: fn(&str) -> String,
    ) {
        let mut got = std::collections::BTreeSet::new();
        for g in given {
            let n = norm(g);
            if truths.contains(&n) {
                got.insert(n);
            } else if got.insert(n.clone()) {
                self.false_claim(field, g, &n);
            }
        }
        for t in truths {
            self.fact(field, t, got.contains(t));
        }
    }
}

/// Scores `reply` for `task` under `arm`.
#[must_use]
pub fn score(task: &Task, arm: Arm, reply: &str) -> Score {
    let mut tally = Tally {
        context: task.context(arm),
        facts: Vec::new(),
        claims: Vec::new(),
    };
    let object = parse(reply);
    let empty = serde_json::Map::new();
    let (answer, refusal, mut invalid) = match &object {
        Ok(o) => {
            let refusal = flag(o.get("insufficient_context")) == Some(true);
            match o.get("answer") {
                Some(Value::Object(a)) => (a, refusal, None),
                _ if refusal => (&empty, true, None),
                _ => (
                    &empty,
                    false,
                    Some("the reply has no answer object".to_owned()),
                ),
            }
        }
        Err(e) => (&empty, false, Some(e.clone())),
    };
    for field in &task.fields {
        let name = field.name;
        let v = answer.get(name);
        match &field.truth {
            Truth::Definitions { items } => {
                let truths: Vec<String> = items.iter().cloned().collect();
                tally.definitions(name, &truths, &strings(v));
            }
            Truth::Definition { value } => {
                let given: Vec<String> = scalar(v).into_iter().collect();
                tally.definitions(name, std::slice::from_ref(value), &given);
            }
            Truth::Tests { items } => tally.named(name, items, &strings(v), test_name),
            Truth::Targets { items } => tally.named(name, items, &strings(v), target_name),
            Truth::Citations { required, known } => {
                let mut got = std::collections::BTreeSet::new();
                for g in strings(v) {
                    let c = citation(&g);
                    if required.contains(&c) {
                        got.insert(c);
                    } else if known.contains(&c) {
                        tally.claim(name, &g, ClaimKind::ExtraCitation);
                    } else {
                        tally.claim(name, &g, ClaimKind::Hallucinated);
                    }
                }
                for r in required {
                    tally.fact(name, r, got.contains(r));
                }
            }
            Truth::Text { value, accept } => {
                let ok = match scalar(v) {
                    Some(g) => {
                        let p = plain(&g);
                        let ok = accept.iter().any(|a| plain(a) == p);
                        if !ok {
                            let kind = if tally.context.searchable.to_lowercase().contains(&p) {
                                ClaimKind::Wrong
                            } else {
                                ClaimKind::Hallucinated
                            };
                            tally.claim(name, &g, kind);
                        }
                        ok
                    }
                    None => false,
                };
                tally.fact(name, value, ok);
            }
            Truth::Signature { value } => {
                let ok = match scalar(v) {
                    Some(g) => {
                        let s = squash(g.trim().trim_matches('`'));
                        let s = s.strip_prefix("pub ").unwrap_or(&s);
                        let s = s.strip_suffix('{').unwrap_or(s).trim().to_owned();
                        let ok = s == squash(value);
                        if !ok {
                            tally.false_claim(name, &g, &s);
                        }
                        ok
                    }
                    None => false,
                };
                tally.fact(name, value, ok);
            }
            Truth::Flag { value } => {
                let ok = match flag(v) {
                    Some(b) => {
                        if b != *value {
                            tally.claim(name, &b.to_string(), ClaimKind::Wrong);
                        }
                        b == *value
                    }
                    None => false,
                };
                tally.fact(name, &value.to_string(), ok);
            }
        }
    }
    if invalid.is_none() && !refusal && answer.is_empty() {
        invalid = Some("the answer object is empty".to_owned());
    }
    let count = |k: ClaimKind| tally.claims.iter().filter(|c| c.kind == k).count();
    let required = task.required();
    let correct = tally.facts.iter().filter(|f| f.ok).count();
    let wrong = count(ClaimKind::Wrong);
    let hallucinated = count(ClaimKind::Hallucinated);
    let solved = invalid.is_none() && !refusal && correct == required && wrong + hallucinated == 0;
    let failure = if solved {
        None
    } else if invalid.is_some() {
        Some(Failure::InvalidStructuredOutput)
    } else if refusal {
        Some(Failure::Refusal)
    } else if hallucinated > 0 {
        Some(Failure::HallucinatedFact)
    } else if wrong > 0 {
        Some(Failure::WrongRepositoryFact)
    } else {
        Some(Failure::MissingRequiredFact)
    };
    Score {
        required,
        correct,
        missing: required.saturating_sub(correct),
        wrong,
        hallucinated,
        extra_citations: count(ClaimKind::ExtraCitation),
        refusal,
        invalid,
        solved,
        failure,
        facts: tally.facts,
        claims: tally.claims,
    }
}
