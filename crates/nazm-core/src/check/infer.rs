//! Result types left to an expression body (R1.5, D2; `docs/user/spec.md`, *Shorter forms*).
//!
//! `fn add(a: Int, b: Int) = a + b` has the type of `a + b`. A caller must see that type when it
//! is checked, so these bodies are checked first, each after the inferred functions it calls by
//! name; one that reaches itself — directly or through others — has no type to start from and
//! must write its own (`N0634`).

use std::collections::HashMap;

use nazm_diag::{codes, Diagnostic};

use super::Unit;

/// The positions in `unit.functions` of the functions whose result type is inferred, callees
/// before callers. Each one that reaches itself is reported once and placed where it was met.
pub(super) fn order(unit: &Unit<'_>, errors: &mut Vec<Diagnostic>) -> Vec<usize> {
    let inferred: HashMap<&str, usize> = unit
        .functions
        .iter()
        .enumerate()
        .filter(|(_, (_, f))| f.inferred_result && f.method_of.is_none())
        .map(|(n, (_, f))| (f.name.as_str(), n))
        .collect();
    if inferred.is_empty() {
        return Vec::new();
    }
    let calls = |n: usize| -> Vec<usize> {
        unit.functions
            .get(n)
            .and_then(|(_, f)| f.body.as_ref())
            .map(|b| {
                b.called_names()
                    .into_iter()
                    .filter_map(|name| inferred.get(name).copied())
                    .collect()
            })
            .unwrap_or_default()
    };
    // Depth-first, iteratively: `Open` while a function's callees are being placed, `Done` once
    // it is. Meeting an open one again is a cycle.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Open,
        Done,
    }
    let mut state: HashMap<usize, State> = HashMap::new();
    let mut reported: Vec<usize> = Vec::new();
    let mut out = Vec::new();
    let mut starts: Vec<usize> = inferred.values().copied().collect();
    starts.sort_unstable();
    for start in starts {
        if state.contains_key(&start) {
            continue;
        }
        let mut stack: Vec<(usize, Vec<usize>)> = vec![(start, calls(start))];
        state.insert(start, State::Open);
        while let Some((n, pending)) = stack.last_mut() {
            let n = *n;
            if let Some(next) = pending.pop() {
                match state.get(&next) {
                    Some(State::Done) => {}
                    Some(State::Open) => {
                        for (m, _) in stack.iter().skip_while(|(m, _)| *m != next) {
                            if !reported.contains(m) {
                                reported.push(*m);
                            }
                        }
                    }
                    None => {
                        state.insert(next, State::Open);
                        let callees = calls(next);
                        stack.push((next, callees));
                    }
                }
            } else {
                state.insert(n, State::Done);
                out.push(n);
                stack.pop();
            }
        }
    }
    reported.sort_unstable();
    for n in reported {
        if let Some((_, f)) = unit.functions.get(n) {
            errors.push(
                Diagnostic::error(
                    codes::RESULT_TYPE_NOT_INFERRED,
                    f.name_span,
                    format!(
                        "`{}` reaches itself, so its result type cannot come from its body",
                        f.name
                    ),
                )
                .help("write it after `:`, as `fn f(n: Int): Int = …`"),
            );
        }
    }
    out
}
