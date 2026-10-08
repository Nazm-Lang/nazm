//! `nazm bindgen HEADER [-o FILE]` (N74, `docs/architecture.md` §7.76): the supported subset of a C
//! header as `extern "C"` declarations.
//!
//! Only what crosses the boundary by `docs/spec.md`'s rules is translated: a parameter of `int64_t`
//! or `bool`, or `const char *` as N55's borrowed `Str`; a result of `int64_t` or `bool`. Since N85
//! (§7.86): an incomplete `struct S;` is an opaque handle `extern "C" struct S;` — `typedef struct S
//! S;` names it too — taken as `struct S *` and returned as `Option[S]`, since a header never says a
//! pointer is not null; a complete struct of `int64_t`, `bool`, handles and such structs is an
//! `extern "C" struct S { … }` taken as `const struct S *`; and a `const char *` result is an
//! `Option[Str]`. Every other prototype, type declaration and macro is refused with the reason —
//! nothing is widened, narrowed or guessed, so no declaration lies about what C receives. The declarations are checked by the
//! checker before they are written; a declaration it refuses is a defect here, and nothing is written.
//!
//! This reads declarations, not C: a header that needs its preprocessor to be understood is outside
//! the subset, and its `#` lines are refused rather than expanded.

use std::path::Path;
use std::process::ExitCode;

use serde_json::{json, Value};

use crate::exit;

/// The C types a header has declared, so far, that cross (N85): opaque handles and C structs.
#[derive(Default)]
struct Known {
    handles: Vec<String>,
    structs: Vec<String>,
}

impl Known {
    /// `struct S *` or `S *` naming a handle: its name.
    fn handle(&self, ty: &str) -> Option<String> {
        let t = ty.strip_suffix(" *")?;
        let t = t.strip_prefix("struct ").unwrap_or(t);
        self.handles.iter().find(|h| *h == t).cloned()
    }

    /// `const struct S *` or `const S *` naming a C struct: its name.
    fn borrowed(&self, ty: &str) -> Option<String> {
        let t = ty.strip_prefix("const ")?.strip_suffix(" *")?;
        let t = t.strip_prefix("struct ").unwrap_or(t);
        self.structs.iter().find(|h| *h == t).cloned()
    }

    /// A field of a C struct: `int64_t`, `bool`, a handle, or a struct by value.
    fn field(&self, ty: &str) -> Result<String, String> {
        match ty {
            "int64_t" => return Ok("Int".to_owned()),
            "bool" | "_Bool" => return Ok("Bool".to_owned()),
            _ => {}
        }
        if let Some(h) = self.handle(ty) {
            return Ok(h);
        }
        let t = ty.strip_prefix("struct ").unwrap_or(ty);
        if self.structs.iter().any(|s| s == t) {
            return Ok(t.to_owned());
        }
        Err(format!(
            "a field of `{ty}`: a C struct's fields are `int64_t`, `bool`, a handle or another \
             such struct"
        ))
    }
}

/// What crosses, in each position, and as what.
fn param(ty: &str, known: &Known) -> Result<String, String> {
    match ty {
        "int64_t" => Ok("Int".to_owned()),
        "bool" | "_Bool" => Ok("Bool".to_owned()),
        "const char *" | "char const *" => Ok("Str".to_owned()),
        _ => known
            .handle(ty)
            .or_else(|| known.borrowed(ty))
            .ok_or_else(|| refusal(ty, false)),
    }
}

fn result(ty: &str, known: &Known) -> Result<String, String> {
    match ty {
        "int64_t" => Ok("Int".to_owned()),
        "bool" | "_Bool" => Ok("Bool".to_owned()),
        // C still owns it; Nazm copies it, and a header never promises it is not null (N85).
        "const char *" | "char const *" => Ok("Option[Str]".to_owned()),
        _ => known
            .handle(ty)
            .map(|h| format!("Option[{h}]"))
            .ok_or_else(|| refusal(ty, true)),
    }
}

fn refusal(ty: &str, is_result: bool) -> String {
    if ty == "void" && is_result {
        return "it returns nothing; a foreign function returns `Int` or `Bool`".to_owned();
    }
    if ty.starts_with("struct ") && !ty.contains('*') {
        return format!("`{ty}` by value: a struct crosses only by `const` pointer, borrowed");
    }
    if ty.contains('*') {
        return format!(
            "`{ty}` is a pointer to nothing this header declares as a handle or a C struct; a \
             mutable buffer C writes into does not cross"
        );
    }
    if ty.starts_with("struct ") || ty.starts_with("union ") || ty.starts_with("enum ") {
        return format!("`{ty}` has no C layout Nazm promises");
    }
    if ["float", "double", "long double"].contains(&ty) {
        return format!("`{ty}`: no floating-point type crosses");
    }
    format!("`{ty}` is not `int64_t` or `bool`; Nazm's `Int` is exactly `int64_t`, and widening would change what C receives")
}

/// The header without comments, as one string.
fn strip(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b.get(i..).is_some_and(|r| r.starts_with(b"/*")) {
            i = text[i + 2..].find("*/").map_or(b.len(), |e| i + 2 + e + 2);
            out.push(' ');
        } else if b.get(i..).is_some_and(|r| r.starts_with(b"//")) {
            i = text[i..].find('\n').map_or(b.len(), |e| i + e);
        } else {
            let c = text[i..].chars().next().unwrap_or(' ');
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

fn identifier(s: &str) -> bool {
    let mut c = s.chars();
    c.next()
        .is_some_and(|f| f.is_ascii_alphabetic() || f == '_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

/// `const char * s` → (`const char *`, Some(`s`)); `int64_t` → (`int64_t`, None). The last word
/// is a name when something precedes it and it is not itself part of a type.
fn split_declarator(d: &str) -> (String, Option<String>) {
    const TYPE_WORDS: &[&str] = &[
        "int64_t", "bool", "_Bool", "void", "char", "int", "long", "short", "unsigned", "signed",
        "float", "double", "const", "volatile", "*", "size_t",
    ];
    let spaced = d.replace('*', " * ");
    let mut words: Vec<&str> = spaced.split_whitespace().collect();
    let tagged = words
        .first()
        .is_some_and(|w| ["struct", "union", "enum"].contains(w));
    let named = words.len() > if tagged { 2 } else { 1 }
        && words
            .last()
            .is_some_and(|w| identifier(w) && !TYPE_WORDS.contains(w));
    let name = if named {
        words.pop().map(str::to_owned)
    } else {
        None
    };
    (words.join(" ").replace("* *", "**"), name)
}

/// A type declaration (N85): `Ok(Some(decl))` for one that crosses, `Ok(None)` for one already
/// known, `Err(why)` for one that does not, and `None` for a statement that is no type
/// declaration at all.
fn type_decl(stmt: &str, known: &mut Known) -> Option<Result<Option<String>, String>> {
    let s = stmt.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.starts_with("union ") || s.starts_with("enum ") || s.starts_with("typedef union ") {
        return (!s.contains('('))
            .then(|| Err("a union or an enum: no C layout of one crosses".to_owned()));
    }
    // `typedef struct S S;`: the same handle or struct under its own name, nothing new.
    if let Some(rest) = s.strip_prefix("typedef struct ") {
        let words: Vec<&str> = rest.split(' ').collect();
        return Some(match words.as_slice() {
            [a, b] if a == b && identifier(a) => {
                if !known.structs.iter().chain(&known.handles).any(|k| k == a) {
                    known.handles.push((*a).to_owned());
                    Ok(Some(format!("pub extern \"C\" struct {a};")))
                } else {
                    Ok(None)
                }
            }
            _ => Err(
                "a typedef naming a struct by another name; write `typedef struct S S;`".to_owned(),
            ),
        });
    }
    if s.starts_with("typedef ") {
        return Some(Err(
            "a typedef: only `typedef struct S S;` is read".to_owned()
        ));
    }
    let rest = s.strip_prefix("struct ")?;
    if s.contains('(') && !s.contains('{') {
        return None;
    }
    // `struct S;`: incomplete, so an opaque handle.
    if identifier(rest) {
        if known.handles.iter().any(|h| h == rest) {
            return Some(Ok(None));
        }
        known.handles.push(rest.to_owned());
        return Some(Ok(Some(format!("pub extern \"C\" struct {rest};"))));
    }
    let (Some(open), Some(close)) = (rest.find('{'), rest.rfind('}')) else {
        return Some(Err(
            "a struct declaration that is not `struct S;` or `struct S { … }`".to_owned(),
        ));
    };
    let name = rest[..open].trim();
    if !identifier(name) || !rest[close + 1..].trim().is_empty() {
        return Some(Err(
            "a struct with a declarator after it, or no name".to_owned()
        ));
    }
    let mut fields = Vec::new();
    for f in rest[open + 1..close]
        .split(';')
        .map(str::trim)
        .filter(|f| !f.is_empty())
    {
        if f.contains('[') || f.contains(':') {
            return Some(Err(format!(
                "`{name}` has an array or a bit-field; neither crosses"
            )));
        }
        let (ty, fname) = split_declarator(f);
        let Some(fname) = fname.filter(|n| identifier(n)) else {
            return Some(Err(format!(
                "a field of `{name}` has no name that could be read"
            )));
        };
        match known.field(&ty) {
            Ok(nz) => fields.push(format!("{fname}: {nz}")),
            Err(why) => return Some(Err(format!("`{name}`: {why}"))),
        }
    }
    if fields.is_empty() {
        return Some(Err(format!("`{name}` has no fields")));
    }
    known.handles.retain(|h| h != name);
    known.structs.push(name.to_owned());
    Some(Ok(Some(format!(
        "pub extern \"C\" struct {name} {{ {} }}",
        fields.join(", ")
    ))))
}

/// One statement: a translated declaration, or why not.
fn one(stmt: &str, known: &Known) -> Result<String, String> {
    let s = stmt.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = s.strip_prefix("extern ").unwrap_or(&s).to_owned();
    if s.starts_with("typedef ")
        || ((s.starts_with("struct ") || s.starts_with("union ") || s.starts_with("enum "))
            && !s.contains('('))
    {
        return Err("a type declaration; no C type's layout crosses".to_owned());
    }
    let (Some(open), Some(close)) = (s.find('('), s.rfind(')')) else {
        return Err("not a function prototype".to_owned());
    };
    if close + 1 != s.len() {
        return Err("not a plain prototype: something follows its parameter list".to_owned());
    }
    let head = s[..open].trim();
    let params = s[open + 1..close].trim();
    if params.contains('(') {
        return Err(
            "a function pointer: a callback crosses only as an export's name, which a header \
             cannot say; declare it by hand with a `fn(…) -> …` parameter"
                .to_owned(),
        );
    }
    let (ret, name) = split_declarator(head);
    let Some(name) = name.filter(|n| identifier(n)) else {
        return Err("no function name could be read".to_owned());
    };
    let ret = result(&ret, known)?;
    let mut out = Vec::new();
    if !(params.is_empty() || params == "void") {
        for (i, p) in params.split(',').enumerate() {
            let p = p.trim();
            if p == "..." {
                return Err("variadic: a variable argument list does not cross".to_owned());
            }
            let (ty, pname) = split_declarator(p);
            let nz = param(&ty, known).map_err(|why| format!("parameter {}: {why}", i + 1))?;
            let pname = pname
                .filter(|n| identifier(n) && !n.starts_with('_'))
                .unwrap_or_else(|| format!("a{i}"));
            out.push(format!("{pname}: {nz}"));
        }
    }
    Ok(format!(
        "pub extern \"C\" fn {name}({}) -> {ret} = \"{name}\";",
        out.join(", ")
    ))
}

pub(crate) fn run(header: &Path, out: Option<&Path>) -> ExitCode {
    let text = match std::fs::read_to_string(header) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nazm: cannot read {}: {e}", header.display());
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };
    let clean = strip(&text);
    let mut translated: Vec<Value> = Vec::new();
    let mut refused: Vec<Value> = Vec::new();
    let mut decls = Vec::new();
    let mut body = String::new();
    for line in clean.lines() {
        let t = line.trim();
        if let Some(directive) = t.strip_prefix('#') {
            let d = directive.trim();
            // An include guard and an include are how a header is shaped, not what it declares.
            if !(d.starts_with("ifndef")
                || d.starts_with("ifdef")
                || d.starts_with("define") && d.split_whitespace().count() == 2
                || d.starts_with("endif")
                || d.starts_with("include")
                || d.starts_with("pragma once")
                || d.starts_with("if ")
                || d.starts_with("else"))
            {
                refused.push(json!({ "c": t, "reason": "a preprocessor directive: a macro is not expanded or translated" }));
            } else if d.starts_with("define") && d.split_whitespace().count() > 2 {
                refused.push(json!({ "c": t, "reason": "a macro: not expanded or translated" }));
            }
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    // `extern "C" {` blocks, written for C++ readers, change nothing here.
    let body = body
        .replace("extern \"C\" {", " ")
        .replace("extern \"C\"{", " ");
    // Statements end at a `;` outside braces, so a struct's body stays one statement.
    let mut statements = Vec::new();
    let (mut depth, mut current) = (0usize, String::new());
    for c in body.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => {
                statements.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    // The types first, in the header's order (N85): a prototype may name any of them.
    let mut known = Known::default();
    let mut types: Vec<String> = Vec::new();
    let mut prototypes: Vec<&str> = Vec::new();
    for stmt in &statements {
        let s = stmt.trim().trim_start_matches('}').trim();
        if s.is_empty() {
            continue;
        }
        let c = s.split_whitespace().collect::<Vec<_>>().join(" ");
        match type_decl(s, &mut known) {
            Some(Ok(Some(d))) => {
                translated.push(json!({ "c": format!("{c};"), "nazm": d }));
                types.push(d);
            }
            Some(Ok(None)) => {}
            Some(Err(reason)) => refused.push(json!({ "c": format!("{c};"), "reason": reason })),
            None => prototypes.push(s),
        }
    }
    let declared: String = types.iter().map(|d| format!("{d}\n")).collect();
    decls.extend(types.iter().cloned());
    for s in prototypes {
        let c = s.split_whitespace().collect::<Vec<_>>().join(" ");
        match one(s, &known) {
            // Each declaration is held by the checker on its own: a name the language keeps for
            // itself, or a symbol the native runtime declares, is refused with the checker's word.
            Ok(d) => match nazm_core::check_source(&format!("{declared}{d}\nfn main() -> Int {{ 0 }}\n")).first() {
                None => {
                    translated.push(json!({ "c": format!("{c};"), "nazm": d }));
                    decls.push(d);
                }
                Some(p) => refused.push(json!({
                    "c": format!("{c};"),
                    "reason": format!("the checker refuses its declaration: {} {}", p.code, p.message),
                })),
            },
            Err(reason) => refused.push(json!({ "c": format!("{c};"), "reason": reason })),
        }
    }
    let module = format!(
        "// Generated by `nazm bindgen {}`: the prototypes whose every type crosses the boundary.\n\
         // Each needs a `ForeignCap` to call, and `nazm build --link` to link C's definition.\n{}{}",
        header.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        decls.iter().map(|d| format!("{d}\n")).collect::<String>(),
        ""
    );
    // Held by the checker before anything is written: a declaration it refuses is a defect here.
    let probe = format!("{module}fn main() -> Int {{ 0 }}\n");
    let problems = nazm_core::check_source(&probe);
    if !problems.is_empty() {
        eprintln!("nazm: internal: a generated declaration does not check; nothing was written:");
        for p in &problems {
            eprintln!("  {} {}", p.code, p.message);
        }
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }
    let report = json!({
        "schema": "nazm.bindgen/1",
        "header": header.file_name().map(|n| n.to_string_lossy().into_owned()),
        "translated": translated,
        "refused": refused,
    });
    match out {
        Some(path) => {
            if let Err(e) = std::fs::write(path, &module) {
                eprintln!("nazm: cannot write {}: {e}", path.display());
                return ExitCode::from(exit::TOOLCHAIN_FAILED);
            }
            println!("{report}");
        }
        None => {
            print!("{module}");
            for r in &refused {
                eprintln!(
                    "nazm: refused `{}`: {}",
                    r["c"].as_str().unwrap_or(""),
                    r["reason"].as_str().unwrap_or("")
                );
            }
        }
    }
    if refused.is_empty() {
        ExitCode::from(exit::OK)
    } else {
        ExitCode::from(exit::CHECK_FAILED)
    }
}
