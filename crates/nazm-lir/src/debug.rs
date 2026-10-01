//! Debug information (N48): DWARF, as LLVM metadata, for a `--debug` build.
//!
//! `docs/architecture.md` §7.50. When asked, the emitter writes a marker comment before the
//! instructions of each MIR statement and terminator — `; nz.loc FILE LINE COLUMN`, the source
//! position MIR recorded for it — and a `; nz.fn FILE LINE` marker for each function. This pass
//! turns them into a compile unit, one subprogram per function, and a location on every
//! instruction, so a debugger can stop in a Nazm function, show its file and line, and step by
//! statement. Nothing else changes: without `--debug` no marker is written and the text is what
//! it always was. Compiler-made code — a drop, a join, a helper — carries the position of the
//! statement it belongs to or, where it belongs to none, its function's line; no position is
//! invented.

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Whether a line inside a function is one instruction this pass may attach a location to:
/// not a label, a comment, a blank, or a line of a multi-line `switch`.
fn instruction(line: &str) -> bool {
    line.starts_with("  ")
        && !line.starts_with("    ")
        && !line.trim_start().starts_with(';')
        && !line.trim_end().ends_with('[')
        && !line.trim().is_empty()
}

/// Insert `, !dbg !N` into `line` before its trailing comment, if it has one.
fn with_location(line: &str, node: usize) -> String {
    match line.find(" ;") {
        Some(at) => {
            let (code, comment) = line.split_at(at);
            format!("{}, !dbg !{node}{comment}", code.trim_end())
        }
        None => format!("{line}, !dbg !{node}"),
    }
}

fn escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'"' | b'\\' => {
                let _ = write!(out, "\\{b:02X}");
            }
            0x20..=0x7e => out.push(char::from(b)),
            _ => {
                let _ = write!(out, "\\{b:02X}");
            }
        }
    }
    out
}

/// The metadata a unit accumulates: nodes after the fixed ones — !0 the compile unit, !1 and !2
/// the module flags, !3 the subroutine type, !4 its empty type list.
struct Meta<'a> {
    nodes: Vec<String>,
    next: usize,
    files: BTreeMap<u32, usize>,
    locations: BTreeMap<(usize, u32, u32), usize>,
    name: &'a dyn Fn(u32) -> String,
}

impl Meta<'_> {
    fn node(&mut self, body: String) -> usize {
        let id = self.next;
        self.next += 1;
        self.nodes.push(format!("!{id} = {body}"));
        id
    }

    fn file(&mut self, idx: u32) -> usize {
        if let Some(&id) = self.files.get(&idx) {
            return id;
        }
        let shown = escape(&(self.name)(idx));
        let id = self.node(format!("!DIFile(filename: \"{shown}\", directory: \".\")"));
        self.files.insert(idx, id);
        id
    }

    fn location(&mut self, sp: usize, line: u32, column: u32) -> usize {
        if let Some(&id) = self.locations.get(&(sp, line, column)) {
            return id;
        }
        let id = self.node(format!(
            "!DILocation(line: {line}, column: {column}, scope: !{sp})"
        ));
        self.locations.insert((sp, line, column), id);
        id
    }
}

fn numbers(rest: &str) -> Vec<u32> {
    rest.split(' ')
        .filter_map(|x| x.parse::<u32>().ok())
        .collect()
}

/// Turn the markers in `text` into debug metadata. `file` names a file by its index.
#[must_use]
pub(crate) fn attach(text: &str, file: &dyn Fn(u32) -> String) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    let mut meta = Meta {
        nodes: Vec::new(),
        next: 5,
        files: BTreeMap::new(),
        locations: BTreeMap::new(),
        name: file,
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    let mut any = false;
    while i < lines.len() {
        let line = lines.get(i).copied().unwrap_or("");
        let Some(open) = line
            .starts_with("define ")
            .then(|| line.rfind(" {"))
            .flatten()
        else {
            out.push_str(line);
            out.push('\n');
            i += 1;
            continue;
        };
        // The function's own marker, if the emitter wrote one: the first `nz.fn` in its body.
        let mut end = i + 1;
        let mut declared: Option<(u32, u32)> = None;
        while let Some(l) = lines.get(end) {
            if *l == "}" {
                break;
            }
            if declared.is_none() {
                if let Some(rest) = l.trim().strip_prefix("; nz.fn ") {
                    if let [f, n, ..] = numbers(rest)[..] {
                        declared = Some((f, n));
                    }
                }
            }
            end += 1;
        }
        let symbol: String = line
            .find('@')
            .and_then(|at| line.get(at + 1..))
            .map(|rest| rest.chars().take_while(|c| *c != '(').collect())
            .unwrap_or_default();
        let name = line
            .rfind("; ")
            .and_then(|at| line.get(at + 2..))
            .map_or(symbol.clone(), |n| n.trim().to_owned());
        let (fidx, fline) = declared.unwrap_or((0, 0));
        let fnode = meta.file(fidx);
        let sp = meta.node(format!(
            "distinct !DISubprogram(name: \"{}\", linkageName: \"{}\", scope: !{fnode}, file: !{fnode}, line: {fline}, type: !3, scopeLine: {fline}, spFlags: DISPFlagDefinition, unit: !0)",
            escape(&name),
            escape(&symbol)
        ));
        any = true;
        let (head, tail) = line.split_at(open);
        let _ = writeln!(out, "{head} !dbg !{sp}{tail}");
        let mut here = (fline, 0u32);
        for l in lines.get(i + 1..end).unwrap_or(&[]) {
            if let Some(rest) = l.trim().strip_prefix("; nz.loc ") {
                if let [_, ln, col, ..] = numbers(rest)[..] {
                    here = (ln, col);
                }
                continue;
            }
            if l.trim().starts_with("; nz.fn ") {
                continue;
            }
            if instruction(l) {
                let loc = meta.location(sp, here.0, here.1);
                out.push_str(&with_location(l, loc));
            } else {
                out.push_str(l);
            }
            out.push('\n');
        }
        if let Some(close) = lines.get(end) {
            out.push_str(close);
            out.push('\n');
        }
        i = end + 1;
    }
    if !any {
        return text.to_owned();
    }
    let unit_file = meta.files.values().next().copied().unwrap_or(5);
    out.push('\n');
    out.push_str("!llvm.dbg.cu = !{!0}\n");
    out.push_str("!llvm.module.flags = !{!1, !2}\n");
    let _ = writeln!(
        out,
        "!0 = distinct !DICompileUnit(language: DW_LANG_C99, file: !{unit_file}, producer: \"nazm\", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug)"
    );
    out.push_str("!1 = !{i32 7, !\"Dwarf Version\", i32 4}\n");
    out.push_str("!2 = !{i32 2, !\"Debug Info Version\", i32 3}\n");
    out.push_str("!3 = !DISubroutineType(types: !4)\n");
    out.push_str("!4 = !{}\n");
    for n in meta.nodes {
        out.push_str(&n);
        out.push('\n');
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn markers_become_locations_and_nothing_else_moves() {
        let text = "define i64 @f(i64 %p0) {   ; f\nentry:\n  ; nz.fn 0 3\n  %slot0 = alloca i64            ; n : Int\n  ; nz.loc 0 4 5\n  %1 = add i64 %p0, 1\n  switch i64 %1, label %d [\n    i64 0, label %a\n  ]\n  ret i64 %1\n}\n";
        let out = attach(text, &|_| "prog.nz".to_owned());
        assert!(out.contains("define i64 @f(i64 %p0) !dbg !"), "{out}");
        assert!(out.contains("%slot0 = alloca i64, !dbg !"), "{out}");
        assert!(
            out.contains("; n : Int"),
            "the comment is kept after the location: {out}"
        );
        assert!(
            out.contains("switch i64 %1, label %d [\n    i64 0, label %a\n  ]"),
            "{out}"
        );
        assert!(out.contains("line: 4, column: 5"), "{out}");
        assert!(out.contains("DISubprogram(name: \"f\""), "{out}");
        assert!(out.contains("filename: \"prog.nz\""), "{out}");
        // No function, nothing to attach: the text comes back as it was.
        assert_eq!(attach("; nothing\n", &|_| String::new()), "; nothing\n");
    }
}
