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
    /// The DWARF type of each kind a variable has, made on first use (N58).
    types: BTreeMap<&'static str, usize>,
    vars: bool,
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

    /// The DWARF type of a variable of `kind`: `int`, `bool` or `str` (N58).
    fn ty(&mut self, kind: &str) -> Option<usize> {
        let key: &'static str = match kind {
            "int" => "int",
            "bool" => "bool",
            "str" => "str",
            _ => return None,
        };
        if let Some(&id) = self.types.get(key) {
            return Some(id);
        }
        let id = match key {
            "int" => self
                .node("!DIBasicType(name: \"Int\", size: 64, encoding: DW_ATE_signed)".to_owned()),
            "bool" => self
                .node("!DIBasicType(name: \"Bool\", size: 8, encoding: DW_ATE_boolean)".to_owned()),
            _ => {
                let int = self.ty("int")?;
                let byte = self.node(
                    "!DIBasicType(name: \"u8\", size: 8, encoding: DW_ATE_unsigned_char)"
                        .to_owned(),
                );
                let bytes = self.node(format!(
                    "!DIDerivedType(tag: DW_TAG_pointer_type, baseType: !{byte}, size: 64)"
                ));
                let opaque = self.node(
                    "!DIDerivedType(tag: DW_TAG_pointer_type, baseType: null, size: 64)".to_owned(),
                );
                let data = self.node(format!(
                    "!DIDerivedType(tag: DW_TAG_member, name: \"data\", baseType: !{bytes}, size: 64, offset: 0)"
                ));
                let len = self.node(format!(
                    "!DIDerivedType(tag: DW_TAG_member, name: \"len\", baseType: !{int}, size: 64, offset: 64)"
                ));
                let owner = self.node(format!(
                    "!DIDerivedType(tag: DW_TAG_member, name: \"owner\", baseType: !{opaque}, size: 64, offset: 128)"
                ));
                let members = self.node(format!("!{{!{data}, !{len}, !{owner}}}"));
                self.node(format!(
                    "!DICompositeType(tag: DW_TAG_structure_type, name: \"Str\", size: 192, elements: !{members})"
                ))
            }
        };
        self.types.insert(key, id);
        Some(id)
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

/// A source block's lexical scope (N91): its first and last positions, and its node.
struct Lexical {
    from: (u32, u32),
    to: (u32, u32),
    node: usize,
}

/// The innermost lexical block holding `at`, or the subprogram. Blocks nest — a source block is
/// inside another or apart from it — so the innermost is the containing one that starts last.
fn scope_of(blocks: &[Lexical], sp: usize, at: (u32, u32)) -> usize {
    blocks
        .iter()
        .filter(|b| b.from <= at && at <= b.to)
        .max_by_key(|b| b.from)
        .map_or(sp, |b| b.node)
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
        types: BTreeMap::new(),
        vars: false,
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
            "distinct !DISubprogram(name: \"{}\", scope: !{fnode}, file: !{fnode}, line: {fline}, type: !3, scopeLine: {fline}, spFlags: DISPFlagDefinition, unit: !0)",
            escape(&name)
        ));
        any = true;
        // The function's lexical blocks (N91): one per distinct scope its variables name,
        // outermost first so each finds its parent among those already made.
        let mut ranges: Vec<((u32, u32), (u32, u32))> = lines
            .get(i + 1..end)
            .unwrap_or(&[])
            .iter()
            .filter_map(|l| l.trim().strip_prefix("; nz.var "))
            .filter_map(|rest| {
                let parts: Vec<&str> = rest.split(' ').collect();
                match parts[..] {
                    [_, _, _, _, a, b, c, d, _] => {
                        let n: Vec<u32> =
                            [a, b, c, d].iter().filter_map(|x| x.parse().ok()).collect();
                        match n[..] {
                            [a, b, c, d] if a > 0 => Some(((a, b), (c, d))),
                            _ => None,
                        }
                    }
                    _ => None,
                }
            })
            .collect();
        ranges.sort_by(|x, y| x.0.cmp(&y.0).then(y.1.cmp(&x.1)));
        ranges.dedup();
        let mut blocks: Vec<Lexical> = Vec::new();
        for (from, to) in ranges {
            let parent = scope_of(&blocks, sp, from);
            let node = meta.node(format!(
                "distinct !DILexicalBlock(scope: !{parent}, file: !{fnode}, line: {}, column: {})",
                from.0, from.1
            ));
            blocks.push(Lexical { from, to, node });
        }
        let (head, tail) = line.split_at(open);
        let _ = writeln!(out, "{head} !dbg !{sp}{tail}");
        let mut here = (fline, 0u32);
        // The prologue — slots, parameter stores, zeroing — until the first statement's marker:
        // it carries no line (N58), so a debugger's breakpoint on the function lands on its first
        // statement, after the parameters are in their slots, never on a slot not yet written.
        // A call there still needs a location, and gets the function's line.
        let mut prologue = true;
        // The first statement's position, which the entry block's branch takes: LLVM marks the
        // end of the prologue at the entry block's first instruction with a line, and the branch
        // is the one after every slot is written.
        let first = lines
            .get(i + 1..end)
            .unwrap_or(&[])
            .iter()
            .find_map(|l| l.trim().strip_prefix("; nz.loc "))
            .and_then(|rest| match numbers(rest)[..] {
                [_, ln, col, ..] => Some((ln, col)),
                _ => None,
            })
            .unwrap_or((fline, 0));
        for l in lines.get(i + 1..end).unwrap_or(&[]) {
            if let Some(rest) = l.trim().strip_prefix("; nz.loc ") {
                if let [_, ln, col, ..] = numbers(rest)[..] {
                    here = (ln, col);
                }
                prologue = false;
                continue;
            }
            if prologue && l.trim_start().starts_with("br ") {
                let loc = meta.location(scope_of(&blocks, sp, first), first.0, first.1);
                out.push_str(&with_location(l, loc));
                out.push('\n');
                continue;
            }
            if prologue
                && instruction(l)
                && !l.contains(" call ")
                && !l.trim_start().starts_with("call ")
            {
                out.push_str(l);
                out.push('\n');
                continue;
            }
            if l.trim().starts_with("; nz.fn ") {
                continue;
            }
            // A variable (N58): `; nz.var SLOT ARG LINE KIND NAME` becomes its declaration on
            // the slot, positioned at the binding's line.
            if let Some(rest) = l.trim().strip_prefix("; nz.var ") {
                let parts: Vec<&str> = rest.split(' ').collect();
                if let [slot, arg, line, kind, from_line, from_column, .., name] = parts[..] {
                    if let (Some(t), Ok(arg), Ok(line)) =
                        (meta.ty(kind), arg.parse::<u32>(), line.parse::<u32>())
                    {
                        let from = (
                            from_line.parse::<u32>().unwrap_or(0),
                            from_column.parse::<u32>().unwrap_or(0),
                        );
                        // Its own block's node, found by where the block starts.
                        let scope = blocks
                            .iter()
                            .find(|b| from.0 > 0 && b.from == from)
                            .map_or(sp, |b| b.node);
                        let argument = if arg > 0 {
                            format!("arg: {arg}, ")
                        } else {
                            String::new()
                        };
                        let var = meta.node(format!(
                            "!DILocalVariable(name: \"{}\", {argument}scope: !{scope}, file: !{fnode}, line: {line}, type: !{t})",
                            escape(name)
                        ));
                        let loc = meta.location(scope, line, 0);
                        meta.vars = true;
                        // `x3*`: the variable holds the value's address (a borrowed parameter).
                        let (storage, expr) = match slot.strip_suffix('*') {
                            Some(s) => (s, "!DIExpression(DW_OP_deref)"),
                            None => (slot, "!DIExpression()"),
                        };
                        let _ = writeln!(
                            out,
                            "  call void @llvm.dbg.declare(metadata ptr %{storage}, metadata !{var}, metadata {expr}), !dbg !{loc}"
                        );
                    }
                }
                continue;
            }
            if instruction(l) {
                let loc = meta.location(scope_of(&blocks, sp, here), here.0, here.1);
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
    if meta.vars {
        out.push_str("declare void @llvm.dbg.declare(metadata, metadata, metadata)\n");
    }
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
        // The prologue carries no line (N58).
        assert!(
            out.contains("  %slot0 = alloca i64            ; n : Int\n"),
            "{out}"
        );
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
