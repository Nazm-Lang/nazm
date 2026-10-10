//! The board's stack bound (N63, `docs/architecture.md` §7.65): the code generator's frame size
//! for every function of the image, summed along the deepest path of the image's call graph.
//!
//! Both facts are read from what was generated, not from Nazm: the frames from clang's
//! `prologepilog` remarks while it compiles each unit, and the calls from the LLVM text it
//! compiled. The bound is that build's; a different optimisation level is a different bound.

use std::collections::BTreeMap;

/// The deepest path's bytes, and the path, or why there is no bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StackBound {
    Bytes { bytes: u64, path: Vec<String> },
    Unknown(String),
}

/// The frame sizes clang reported in `stderr`, by function.
pub(crate) fn frames(stderr: &str, into: &mut BTreeMap<String, u64>) {
    const MARK: &str = " stack bytes in function '";
    for line in stderr.lines() {
        let Some((head, rest)) = line.split_once(MARK) else {
            continue;
        };
        let Some(name) = rest.split('\'').next() else {
            continue;
        };
        let Some(n) = head.rsplit(' ').next().and_then(|n| n.parse::<u64>().ok()) else {
            continue;
        };
        into.insert(name.to_owned(), n);
    }
}

/// What each defined function calls, from LLVM text: a callee by name, or `None` for a call
/// through a value. Inline assembly and LLVM's intrinsics are not calls into a frame.
pub(crate) fn calls(texts: &[&str]) -> BTreeMap<String, Vec<Option<String>>> {
    let mut out: BTreeMap<String, Vec<Option<String>>> = BTreeMap::new();
    let mut current: Option<String> = None;
    let symbol = |s: &str| -> String {
        s.chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '$' | '-'))
            .collect()
    };
    for text in texts {
        for line in text.lines() {
            if line.starts_with("define ") {
                current = line.split_once('@').map(|(_, r)| symbol(r));
                if let Some(c) = &current {
                    out.entry(c.clone()).or_default();
                }
                continue;
            }
            if line.starts_with('}') {
                current = None;
                continue;
            }
            let Some(f) = &current else { continue };
            // A comment is not code: its words are not a call (Gate 3 — a runtime comment that
            // said "its first call" read as a call through a value).
            if line.trim_start().starts_with(';') {
                continue;
            }
            let Some((_, after)) = line.split_once("call ") else {
                continue;
            };
            if after.contains(" asm ") || after.starts_with("asm ") {
                continue;
            }
            // The callee is the first `@name(` or `%value(` after the return type.
            let callee = after.find('(').and_then(|open| {
                let head = &after[..open];
                head.rsplit(' ').next().map(str::to_owned)
            });
            let edge = match callee {
                Some(c) if c.starts_with('@') => {
                    let name = symbol(&c[1..]);
                    if name.starts_with("llvm.") {
                        continue;
                    }
                    Some(name)
                }
                _ => None,
            };
            if let Some(v) = out.get_mut(f) {
                v.push(edge);
            }
        }
    }
    out
}

/// The deepest path from `root`, in bytes.
pub(crate) fn bound(
    root: &str,
    frames: &BTreeMap<String, u64>,
    calls: &BTreeMap<String, Vec<Option<String>>>,
) -> StackBound {
    let mut memo: BTreeMap<String, (u64, Vec<String>)> = BTreeMap::new();
    let mut on_path: Vec<String> = Vec::new();
    match deepest(root, frames, calls, &mut memo, &mut on_path) {
        Ok((bytes, path)) => StackBound::Bytes { bytes, path },
        Err(e) => StackBound::Unknown(e),
    }
}

fn deepest(
    f: &str,
    frames: &BTreeMap<String, u64>,
    calls: &BTreeMap<String, Vec<Option<String>>>,
    memo: &mut BTreeMap<String, (u64, Vec<String>)>,
    on_path: &mut Vec<String>,
) -> Result<(u64, Vec<String>), String> {
    if let Some(r) = memo.get(f) {
        return Ok(r.clone());
    }
    // A WebAssembly host import (N64) runs on the host's stack, not the module's shadow stack:
    // it uses none of the bytes this bound is about.
    if f.starts_with("nz.host_") {
        return Ok((0, vec![f.to_owned()]));
    }
    if on_path.iter().any(|g| g == f) {
        return Err(format!("`{f}` is in a call cycle"));
    }
    let callees = calls
        .get(f)
        .ok_or_else(|| format!("`{f}` is not defined in the image"))?;
    // A function the text defines but the code generator reported no frame for was inlined into
    // every caller and removed: its frame is part of theirs, and what it calls they now call.
    let own = frames.get(f).copied().unwrap_or(0);
    on_path.push(f.to_owned());
    let mut best: (u64, Vec<String>) = (0, Vec::new());
    for c in callees {
        let Some(c) = c else {
            on_path.pop();
            return Err(format!(
                "`{f}` calls through a value, whose target is not known"
            ));
        };
        let r = deepest(c, frames, calls, memo, on_path)?;
        if r.0 > best.0 || best.1.is_empty() {
            best = r;
        }
    }
    on_path.pop();
    let mut path = vec![f.to_owned()];
    path.extend(best.1);
    let r = (own.saturating_add(best.0), path);
    memo.insert(f.to_owned(), r.clone());
    Ok(r)
}

/// `nazm.bounds/1`: one line of canonical JSON — the stack bound or why there is none, every
/// frame, and what is not bounded, each number labelled with its kind.
pub(crate) fn report(
    triple: &str,
    bound: &StackBound,
    frames: &BTreeMap<String, u64>,
    reserved: u64,
) -> String {
    let stack = match bound {
        StackBound::Bytes { bytes, path } => format!(
            "{{\"kind\":\"static bound\",\"bytes\":{bytes},\"path\":[{}],\"reserved\":{reserved},\"within\":{}}}",
            path.iter()
                .map(|p| format!("\"{p}\""))
                .collect::<Vec<_>>()
                .join(","),
            *bytes <= reserved
        ),
        StackBound::Unknown(reason) => format!(
            "{{\"kind\":\"unknown\",\"bytes\":null,\"reason\":\"{}\",\"reserved\":{reserved}}}",
            reason.replace('\\', "\\\\").replace('"', "\\\"")
        ),
    };
    let frames = frames
        .iter()
        .map(|(f, n)| format!("\"{f}\":{n}"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":\"nazm.bounds/1\",\"target\":\"{triple}\",\"stack\":{stack},\"frames\":{{{frames}}},\
         \"heap_bytes\":{{\"kind\":\"static bound\",\"bytes\":0}},\"wcet\":{{\"kind\":\"unknown\",\"cycles\":null}},\
         \"note\":\"the stack bound is the code generator's frame sizes summed along the image's deepest call path, for this build; a measurement is not a bound, and no execution time is bounded\"}}\n"
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn graph(edges: &[(&str, &[Option<&str>])]) -> BTreeMap<String, Vec<Option<String>>> {
        edges
            .iter()
            .map(|(f, cs)| {
                (
                    (*f).to_owned(),
                    cs.iter().map(|c| c.map(str::to_owned)).collect(),
                )
            })
            .collect()
    }

    fn sizes(fs: &[(&str, u64)]) -> BTreeMap<String, u64> {
        fs.iter().map(|(f, n)| ((*f).to_owned(), *n)).collect()
    }

    #[test]
    fn a_comment_that_says_call_is_not_a_call() {
        let text = "define void @f() {\nentry:\n  ; its first call would look like one\n  call void @g()\n  ret void\n}\n";
        let c = calls(&[text]);
        assert_eq!(c.get("f"), Some(&vec![Some("g".to_owned())]));
    }

    #[test]
    fn the_bound_is_the_deepest_path_not_the_first() {
        let calls = graph(&[
            ("root", &[Some("shallow"), Some("deep")]),
            ("shallow", &[]),
            ("deep", &[Some("leaf")]),
            ("leaf", &[]),
        ]);
        let frames = sizes(&[("root", 16), ("shallow", 64), ("deep", 48), ("leaf", 32)]);
        assert_eq!(
            bound("root", &frames, &calls),
            StackBound::Bytes {
                bytes: 96,
                path: vec!["root".into(), "deep".into(), "leaf".into()]
            }
        );
    }

    #[test]
    fn a_cycle_a_value_call_or_an_undefined_callee_has_no_bound() {
        let frames = sizes(&[("a", 8), ("b", 8)]);
        let cycle = graph(&[("a", &[Some("b")]), ("b", &[Some("a")])]);
        assert_eq!(
            bound("a", &frames, &cycle),
            StackBound::Unknown("`a` is in a call cycle".into())
        );
        let value = graph(&[("a", &[None])]);
        assert!(
            matches!(bound("a", &frames, &value), StackBound::Unknown(r) if r.contains("through a value"))
        );
        let undefined = graph(&[("a", &[Some("c")])]);
        assert!(
            matches!(bound("a", &frames, &undefined), StackBound::Unknown(r) if r.contains("not defined"))
        );
    }

    #[test]
    fn an_inlined_function_adds_nothing_and_its_callees_still_count() {
        let calls = graph(&[("a", &[Some("gone")]), ("gone", &[Some("b")]), ("b", &[])]);
        let frames = sizes(&[("a", 16), ("b", 32)]);
        assert_eq!(
            bound("a", &frames, &calls),
            StackBound::Bytes {
                bytes: 48,
                path: vec!["a".into(), "gone".into(), "b".into()]
            }
        );
    }

    #[test]
    fn frames_and_calls_are_read_from_what_was_generated() {
        let mut f = BTreeMap::new();
        frames(
            "remark: <unknown>:0:0: 80 stack bytes in function 'nz.m.x.main' [-Rpass-analysis=prologepilog]\n\
             warning: something else\n",
            &mut f,
        );
        assert_eq!(f.get("nz.m.x.main"), Some(&80));
        let c = calls(&["define i64 @f(i64 %x) {\n  %a = call i64 @g(i64 %x)\n  %b = call i64 %fp(i64 1)\n  \
                         call void asm sideeffect \"nop\", \"\"()\n  %c = call {i64, i1} @llvm.sadd.with.overflow.i64(i64 1, i64 2)\n  ret i64 %a\n}\n"]);
        assert_eq!(c.get("f"), Some(&vec![Some("g".to_owned()), None]));
    }
}
