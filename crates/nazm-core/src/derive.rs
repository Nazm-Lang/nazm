//! Derived serialisation (R1c, L2; `docs/spec.md`, *Derived serialisation*): what `@derive(json)`
//! means, as Nazm text.
//!
//! The loader hands every project file to [`expand`] before reading its imports. A file none of
//! whose `struct`s or `enum`s derives anything comes back unchanged; otherwise the functions each
//! derive means are appended — ordinary Nazm, checked, lowered and run by every implementation like
//! the code around them — with the `use` of `@std/json` they need. Appended, so every span the file
//! had keeps its offset and every generated span is a real, distinct one: the checker keys its
//! tables by span. What a derive cannot carry is refused by the checker (`check/attributes.rs`),
//! and nothing is generated for it.

use std::fmt::Write as _;

use nazm_syntax::ast::{AttrArg, Attribute, Field, TypeName};

/// Whether `attrs` derive `format`.
#[must_use]
pub fn derives(attrs: &[Attribute], format: &str) -> bool {
    attrs.iter().any(|a| {
        a.name == "derive"
            && a.args.as_deref().is_some_and(|args| {
                args.iter()
                    .any(|x| matches!(x, AttrArg::Word { name, .. } if name == format))
            })
    })
}

/// The formats there are to derive.
pub const FORMATS: &[&str] = &["json", "toml"];

/// The integers a JSON number here carries: every one `Int` holds.
const INTEGERS: &[&str] = &["Int", "Int8", "Int16", "Int32", "UInt8", "UInt16", "UInt32"];

fn plain(t: &TypeName) -> bool {
    t.qualifier.is_none() && t.function.is_none() && t.length.is_none()
}

fn args(t: &TypeName) -> &[TypeName] {
    t.args.as_deref().unwrap_or(&[])
}

/// Why a field of type `t` cannot be carried, or `None` when it can.
#[must_use]
pub fn refusal(t: &TypeName) -> Option<String> {
    if t.function.is_some() {
        return Some("a function has no JSON form".to_owned());
    }
    if t.length.is_some() {
        return Some("an array has no JSON form here; a `Vec` has".to_owned());
    }
    let n = t.name.as_str();
    if plain(t) && args(t).is_empty() {
        if INTEGERS.contains(&n) || matches!(n, "Float64" | "Float32" | "Bool" | "Str") {
            return None;
        }
        if matches!(n, "UInt64" | "Int128" | "UInt128") {
            return Some(format!(
                "`{n}` has values past `Int`, which every derived integer is read through; convert \
                 it to a type that has none"
            ));
        }
        if nazm_sema::Type::from_name(n).is_some() {
            return Some(format!("`{n}` has no JSON form"));
        }
    }
    if plain(t) && matches!(n, "Option" | "Vec" | "List" | "Chain" | "Box") {
        return match args(t) {
            [inner] => refusal(inner),
            _ => Some(format!("`{n}` takes one type")),
        };
    }
    if !args(t).is_empty() {
        return Some(format!(
            "`{n}` is generic, and a derive is for a type that is not"
        ));
    }
    if n.starts_with('(') {
        return Some("a tuple has no JSON form here; a record has".to_owned());
    }
    // A record or an enum: its own derive carries it.
    None
}

/// Why a field of type `t` cannot be carried in `format`, or `None` when it can: TOML has no
/// `null`, so an `Option` there is a field's whole type or nothing (L2c).
#[must_use]
pub fn refusal_for(format: &str, t: &TypeName) -> Option<String> {
    if let Some(why) = refusal(t) {
        return Some(why);
    }
    if format == "toml" {
        let inner_option = |x: &TypeName| {
            fn has(x: &TypeName) -> bool {
                (plain(x) && x.name == "Option") || args(x).iter().any(has)
            }
            args(x).iter().any(has)
        };
        if inner_option(t) {
            return Some(
                "TOML has no `null`, so an `Option` may only be a field's whole type".to_owned(),
            );
        }
    }
    None
}

/// The record and enum names a field's type reaches, unqualified: `Option[Vec[Point]]` reaches
/// `Point`.
#[must_use]
pub fn named(t: &TypeName) -> Vec<&TypeName> {
    let n = t.name.as_str();
    if plain(t) && matches!(n, "Option" | "Vec" | "List" | "Chain" | "Box") {
        return args(t).iter().flat_map(named).collect();
    }
    let builtin = INTEGERS.contains(&n)
        || matches!(n, "Float64" | "Float32" | "Bool" | "Str")
        || nazm_sema::Type::from_name(n).is_some();
    if plain(t) && args(t).is_empty() && !builtin {
        vec![t]
    } else {
        Vec::new()
    }
}

/// `HttpRequest` as `http_request`.
#[must_use]
pub fn snake(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (i, c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() {
            let prev = i.checked_sub(1).and_then(|j| chars.get(j));
            let next = chars.get(i + 1);
            let boundary = prev.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit())
                || (prev.is_some_and(char::is_ascii_uppercase)
                    && next.is_some_and(char::is_ascii_lowercase));
            if boundary {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(*c);
        }
    }
    out
}

/// A type as written.
fn text(t: &TypeName) -> String {
    let mut s = String::new();
    if let Some(q) = &t.qualifier {
        let _ = write!(s, "{}::", q.name);
    }
    s.push_str(&t.name);
    if let Some(a) = &t.args {
        let inner: Vec<String> = a.iter().map(text).collect();
        let _ = write!(s, "[{}]", inner.join(", "));
    }
    s
}

/// A derived type's function `suffix`, named as its module sees it.
fn named_fn(t: &TypeName, suffix: &str) -> String {
    let q = t
        .qualifier
        .as_ref()
        .map_or(String::new(), |q| format!("{}::", q.name));
    format!("{q}{}_{suffix}", snake(&t.name))
}

/// `s` as the text of a Nazm string literal.
fn literal(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// One derived type's generated text, its helpers numbered within it.
struct Gen<'a> {
    base: String,
    vis: &'a str,
    out: String,
    /// Each field type's encode and decode helper, by its text.
    helpers: Vec<String>,
    /// The derived types a field names, for the schema's `$defs`.
    refs: Vec<TypeName>,
}

impl Gen<'_> {
    /// The number of the helper pair for `t`, written on first use.
    fn helper(&mut self, t: &TypeName) -> usize {
        let key = text(t);
        if let Some(k) = self.helpers.iter().position(|h| *h == key) {
            return k;
        }
        let k = self.helpers.len();
        self.helpers.push(key.clone());
        let ty = key;
        let (write, read) = self.bodies(t, &ty);
        let base = &self.base;
        let _ = write!(
            self.out,
            "fn {base}_json_w{k}(doc: JsonDoc, v: {ty}) -> Int ! {{}} {{\n{write}\n}}\n\n\
             fn {base}_json_r{k}(doc: JsonDoc, node: Int, at: Str) -> Result[{ty}, Str] ! {{}} {{\n{read}\n}}\n\n"
        );
        k
    }

    /// A helper pair's bodies for `t`, written `ty`.
    fn bodies(&mut self, t: &TypeName, ty: &str) -> (String, String) {
        let ok = |v: &str| format!("Result[{ty}, Str].Ok(value: {v})");
        let err = |what: &str| format!("Result[{ty}, Str].Err(error: f\"{{at}}: {what}\")");
        let n = t.name.as_str();
        let base = self.base.clone();
        if plain(t) && args(t).is_empty() {
            if INTEGERS.contains(&n) {
                let w = if n == "Int" {
                    "    json_add_int(doc, v)".to_owned()
                } else {
                    "    json_add_int(doc, Int(v))".to_owned()
                };
                let fit = if n == "Int" {
                    ok("i")
                } else {
                    format!(
                        "match try_convert[{n}](i) {{ Option.Some(value: x) => {}, Option.None() => {}, }}",
                        ok("x"),
                        err(&format!("{{i}} is outside `{n}`"))
                    )
                };
                let r = format!(
                    "    match json_int(doc, node) {{\n        Option.Some(value: i) => {fit},\n        Option.None() => {},\n    }}",
                    err("expected an integer")
                );
                return (w, r);
            }
            if n == "Float64" || n == "Float32" {
                let w = if n == "Float64" {
                    "    json_add_float(doc, v)"
                } else {
                    "    json_add_float(doc, Float64(v))"
                };
                let value = if n == "Float64" {
                    ok("x")
                } else {
                    ok("Float32(x)")
                };
                let r = format!(
                    "    match json_float(doc, node) {{\n        Option.Some(value: x) => {value},\n        Option.None() => {},\n    }}",
                    err("expected a number")
                );
                return (w.to_owned(), r);
            }
            if n == "Bool" || n == "Str" {
                let (add, get, what) = if n == "Bool" {
                    ("json_add_bool", "json_bool", "expected `true` or `false`")
                } else {
                    ("json_add_str", "json_str", "expected a string")
                };
                let r = format!(
                    "    match {get}(doc, node) {{\n        Option.Some(value: x) => {},\n        Option.None() => {},\n    }}",
                    ok("x"),
                    err(what)
                );
                return (format!("    {add}(doc, v)"), r);
            }
        }
        if plain(t) && matches!(n, "Option" | "Vec" | "List" | "Chain" | "Box") {
            let inner = args(t).first().cloned().unwrap_or_else(|| t.clone());
            let it = text(&inner);
            let k = self.helper(&inner);
            let w = format!("{base}_json_w{k}");
            let r = format!("{base}_json_r{k}");
            return match n {
                "Option" => (
                    format!(
                        "    match v {{\n        Option.Some(value: x) => {w}(doc, x),\n        Option.None() => json_add_null(doc),\n    }}"
                    ),
                    format!(
                        "    if json_kind(doc, node) == \"null\" {{ return {}; }}\n    \
                         match {r}(doc, node, at) {{\n        Result.Ok(value: x) => {},\n        Result.Err(error: e) => Result[{ty}, Str].Err(error: e),\n    }}",
                        ok(&format!("Option[{it}].None()")),
                        ok(&format!("Option[{it}].Some(value: x)"))
                    ),
                ),
                "Box" => (
                    format!("    {w}(doc, box_get(v))"),
                    format!(
                        "    match {r}(doc, node, at) {{\n        Result.Ok(value: x) => {},\n        Result.Err(error: e) => Result[{ty}, Str].Err(error: e),\n    }}",
                        ok("box_new(x)")
                    ),
                ),
                _ => {
                    let done = if n == "Chain" {
                        ok("chain_from_vec(out)")
                    } else {
                        ok("out")
                    };
                    (
                        format!(
                            "    let items = ints_new();\n    for x in v {{ ints_push(items, {w}(doc, x)); }}\n    json_add_array(doc, items)"
                        ),
                        format!(
                            "    if json_kind(doc, node) != \"array\" {{ return {}; }}\n    \
                             let out = vec_new[{it}]();\n    \
                             for i in 0..json_len(doc, node) {{\n        \
                             let item = match json_at(doc, node, i) {{ Option.Some(value: c) => c, Option.None() => 0 - 1, }};\n        \
                             match {r}(doc, item, f\"{{at}}[{{i}}]\") {{\n            \
                             Result.Ok(value: x) => {{ vec_push(out, x); }},\n            \
                             Result.Err(error: e) => {{ return Result[{ty}, Str].Err(error: e); }},\n        }};\n    }}\n    {done}",
                            err("expected an array")
                        ),
                    )
                }
            };
        }
        // A derived record or enum.
        if !self.refs.iter().any(|r| text(r) == text(t)) {
            self.refs.push(t.clone());
        }
        (
            format!("    {}(doc, v)", named_fn(t, "json_node")),
            format!("    {}(doc, node, at)", named_fn(t, "json_read")),
        )
    }

    /// The members of a field list, written into `keys` and `values`; each field read as
    /// `{from}{name}`.
    fn write_fields(&mut self, members: &[Member<'_>], from: &str) -> String {
        let mut s = String::from("    let keys = strs_new();\n    let values = ints_new();\n");
        for m in members {
            let k = self.helper(&m.field.ty);
            let base = &self.base;
            let name = &m.field.name;
            let key = literal(&m.key);
            if m.codec.omit_none {
                let _ = writeln!(
                    s,
                    "    if let Option.Some(value: x) = {from}{name} {{\n        strs_push(keys, {key});\n        \
                     ints_push(values, {base}_json_w{k}(doc, {from}{name}));\n    }}"
                );
            } else {
                let _ = writeln!(
                    s,
                    "    strs_push(keys, {key});\n    ints_push(values, {base}_json_w{k}(doc, {from}{name}));"
                );
            }
        }
        s
    }

    /// A field list read out of the object `obj` at path `at`, into bindings `f_<name>`; a
    /// failure returns `Err` as a `Result[owner, Str]`. `known` adds the names besides the
    /// members' that `deny_unknown` lets through (an enum's tag).
    fn read_fields(
        &mut self,
        members: &[Member<'_>],
        owner: &str,
        obj: &str,
        at: &str,
        deny: Option<&[String]>,
    ) -> String {
        let mut s = String::new();
        if let Some(extra) = deny {
            let mut names: Vec<String> = members.iter().map(|m| m.key.clone()).collect();
            names.extend(extra.iter().cloned());
            let test = if names.is_empty() {
                "false".to_owned()
            } else {
                names
                    .iter()
                    .map(|n| format!("k == {}", literal(n)))
                    .collect::<Vec<_>>()
                    .join(" || ")
            };
            let _ = writeln!(
                s,
                "    for i in 0..json_len(doc, {obj}) {{\n        \
                 let k = match json_key_at(doc, {obj}, i) {{ Option.Some(value: x) => x, Option.None() => \"\", }};\n        \
                 if !({test}) {{ return Result[{owner}, Str].Err(error: f\"{{{at}}}: unknown member `{{k}}`\"); }}\n    }}"
            );
        }
        for m in members {
            let f = m.field;
            let k = self.helper(&f.ty);
            let base = &self.base;
            let missing = if let Some(empty) = m.codec.default.then(|| empty_value(&f.ty)).flatten()
            {
                empty
            } else if plain(&f.ty) && f.ty.name == "Option" {
                format!(
                    "Option[{}].None()",
                    text(args(&f.ty).first().unwrap_or(&f.ty))
                )
            } else {
                format!(
                    "{{ return Result[{owner}, Str].Err(error: f\"{{{at}}}: no member `{}`\"); }}",
                    m.key
                )
            };
            let _ = writeln!(
                s,
                "    let f_{name} = match json_get(doc, {obj}, {key}) {{\n        \
                 Option.Some(value: n) => match {base}_json_r{k}(doc, n, f\"{{{at}}}.{path}\") {{\n            \
                 Result.Ok(value: x) => x,\n            \
                 Result.Err(error: e) => {{ return Result[{owner}, Str].Err(error: e); }},\n        }},\n        \
                 Option.None() => {missing},\n    }};",
                name = f.name,
                key = literal(&m.key),
                path = m.key,
            );
        }
        s
    }
}

/// What `@codec(…)` says of a type (R1c, L2b).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeCodec {
    pub rename_all: Option<String>,
    pub deny_unknown: bool,
    pub tag: Option<String>,
    pub untagged: bool,
}

/// What `@codec(…)` says of a field or a variant (R1c, L2b).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldCodec {
    pub rename: Option<String>,
    pub default: bool,
    pub omit_none: bool,
}

/// The cases `rename_all` names.
pub const CASES: &[&str] = &[
    "camelCase",
    "snake_case",
    "kebab-case",
    "PascalCase",
    "SCREAMING_SNAKE_CASE",
];

fn codec_args(attrs: &[Attribute]) -> impl Iterator<Item = &AttrArg> {
    attrs
        .iter()
        .filter(|a| a.name == "codec")
        .flat_map(|a| a.args.as_deref().unwrap_or(&[]))
}

/// A type's `@codec(…)`.
#[must_use]
pub fn type_codec(attrs: &[Attribute]) -> TypeCodec {
    let mut c = TypeCodec::default();
    for a in codec_args(attrs) {
        match a {
            AttrArg::Pair { key, value, .. } if key == "rename_all" => {
                c.rename_all = Some(value.clone());
            }
            AttrArg::Pair { key, value, .. } if key == "tag" => c.tag = Some(value.clone()),
            AttrArg::Word { name, .. } if name == "deny_unknown" => c.deny_unknown = true,
            AttrArg::Word { name, .. } if name == "untagged" => c.untagged = true,
            _ => {}
        }
    }
    c
}

/// A field's or a variant's `@codec(…)`.
#[must_use]
pub fn field_codec(attrs: &[Attribute]) -> FieldCodec {
    let mut c = FieldCodec::default();
    for a in codec_args(attrs) {
        match a {
            AttrArg::Pair { key, value, .. } if key == "rename" => c.rename = Some(value.clone()),
            AttrArg::Word { name, .. } if name == "default" => c.default = true,
            AttrArg::Word { name, .. } if name == "omit_none" => c.omit_none = true,
            _ => {}
        }
    }
    c
}

/// `name` — snake case or Pascal case — written in `case`.
#[must_use]
pub fn recase(name: &str, case: &str) -> String {
    let words: Vec<String> = snake(name)
        .split('_')
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect();
    let capital = |w: &str| {
        let mut c = w.chars();
        c.next().map_or(String::new(), |f| {
            f.to_ascii_uppercase().to_string() + c.as_str()
        })
    };
    match case {
        "camelCase" => words
            .iter()
            .enumerate()
            .map(|(i, w)| if i == 0 { w.clone() } else { capital(w) })
            .collect(),
        "PascalCase" => words.iter().map(|w| capital(w)).collect(),
        "kebab-case" => words.join("-"),
        "SCREAMING_SNAKE_CASE" => words.join("_").to_ascii_uppercase(),
        _ => words.join("_"),
    }
}

/// The empty value `default` gives a field of type `t`, as Nazm text; `None` for a type that
/// has none.
#[must_use]
pub fn empty_value(t: &TypeName) -> Option<String> {
    if !plain(t) {
        return None;
    }
    let n = t.name.as_str();
    match (n, args(t)) {
        ("Int", []) => Some("0".to_owned()),
        (_, []) if INTEGERS.contains(&n) => Some(format!("{n}(0)")),
        ("Float64", []) => Some("0.0".to_owned()),
        ("Float32", []) => Some("Float32(0.0)".to_owned()),
        ("Bool", []) => Some("false".to_owned()),
        ("Str", []) => Some("\"\"".to_owned()),
        ("Option", [inner]) => Some(format!("Option[{}].None()", text(inner))),
        ("Vec" | "List", [inner]) => Some(format!("vec_new[{}]()", text(inner))),
        ("Chain", [inner]) => Some(format!("chain_empty[{}]()", text(inner))),
        _ => None,
    }
}

/// One field as its encoding sees it: its name in the document and its `@codec`.
struct Member<'a> {
    field: &'a Field,
    key: String,
    codec: FieldCodec,
}

/// A field list's members, `rename_all` applied where no `rename` is.
fn members<'a>(fields: &'a [Field], rename_all: Option<&str>) -> Vec<Member<'a>> {
    fields
        .iter()
        .map(|f| {
            let codec = field_codec(&f.attrs);
            let key = codec.rename.clone().unwrap_or_else(|| {
                rename_all.map_or_else(|| f.name.clone(), |c| recase(&f.name, c))
            });
            Member {
                field: f,
                key,
                codec,
            }
        })
        .collect()
}

/// A variant's name in the document.
fn variant_key(attrs: &[Attribute], name: &str, rename_all: Option<&str>) -> String {
    field_codec(attrs)
        .rename
        .unwrap_or_else(|| rename_all.map_or_else(|| name.to_owned(), |c| recase(name, c)))
}

/// A field's JSON Schema, as JSON text.
fn schema_of(t: &TypeName) -> String {
    let n = t.name.as_str();
    if plain(t) && args(t).is_empty() {
        let bounds = |lo: i64, hi: i64| {
            format!("{{\"type\":\"integer\",\"minimum\":{lo},\"maximum\":{hi}}}")
        };
        match n {
            "Int" => return "{\"type\":\"integer\"}".to_owned(),
            "Int8" => return bounds(-128, 127),
            "Int16" => return bounds(-32768, 32767),
            "Int32" => return bounds(-2_147_483_648, 2_147_483_647),
            "UInt8" => return bounds(0, 255),
            "UInt16" => return bounds(0, 65535),
            "UInt32" => return bounds(0, 4_294_967_295),
            "Float64" | "Float32" => return "{\"type\":\"number\"}".to_owned(),
            "Bool" => return "{\"type\":\"boolean\"}".to_owned(),
            "Str" => return "{\"type\":\"string\"}".to_owned(),
            _ => {}
        }
    }
    if plain(t) && matches!(n, "Option" | "Vec" | "List" | "Chain" | "Box") {
        let inner = args(t).first().map_or_else(String::new, schema_of);
        return match n {
            "Option" => format!("{{\"anyOf\":[{inner},{{\"type\":\"null\"}}]}}"),
            "Box" => inner,
            _ => format!("{{\"type\":\"array\",\"items\":{inner}}}"),
        };
    }
    format!("{{\"$ref\":\"#/$defs/{n}\"}}")
}

/// An object schema of `members`, with `before` — a tag's property — first; those neither
/// optional nor defaulted required.
fn object_schema(members: &[Member<'_>], before: Option<(&str, &str)>, closed: bool) -> String {
    let mut props: Vec<String> = Vec::new();
    let mut required: Vec<String> = Vec::new();
    if let Some((tag, value)) = before {
        props.push(format!("\"{tag}\":{{\"const\":\"{value}\"}}"));
        required.push(format!("\"{tag}\""));
    }
    for m in members {
        props.push(format!("\"{}\":{}", m.key, schema_of(&m.field.ty)));
        let optional = plain(&m.field.ty) && m.field.ty.name == "Option";
        if !optional && !m.codec.default {
            required.push(format!("\"{}\"", m.key));
        }
    }
    let close = if closed {
        ",\"additionalProperties\":false"
    } else {
        ""
    };
    format!(
        "{{\"type\":\"object\",\"properties\":{{{}}},\"required\":[{}]{close}}}",
        props.join(","),
        required.join(",")
    )
}

/// One derived type: its kind, name, whether it is public, and its fields or variants.
enum Shape<'a> {
    Record(&'a [Field]),
    Enum(Vec<(&'a str, &'a [Attribute], &'a [Field])>),
}

#[allow(clippy::too_many_lines)]
fn generate(name: &str, public: bool, codec: &TypeCodec, shape: &Shape<'_>) -> String {
    let mut g = Gen {
        base: snake(name),
        vis: if public { "pub " } else { "" },
        out: String::new(),
        helpers: Vec::new(),
        refs: Vec::new(),
    };
    let base = g.base.clone();
    let vis = g.vis;
    let rename_all = codec.rename_all.as_deref();
    let deny: Option<&[String]> = codec.deny_unknown.then_some(&[]);
    let (node, read, def) = match shape {
        Shape::Record(fields) => {
            let ms = members(fields, rename_all);
            let w = g.write_fields(&ms, "v.");
            let r = g.read_fields(&ms, name, "node", "at", deny);
            let inits: Vec<String> = fields
                .iter()
                .map(|f| format!("{0}: f_{0}", f.name))
                .collect();
            (
                format!("{w}    json_add_object(doc, keys, values)"),
                format!(
                    "    if json_kind(doc, node) != \"object\" {{ return Result[{name}, Str].Err(error: f\"{{at}}: expected an object\"); }}\n\
                     {r}    Result[{name}, Str].Ok(value: {name}({}))",
                    inits.join(", ")
                ),
                object_schema(&ms, None, codec.deny_unknown),
            )
        }
        Shape::Enum(variants) => {
            let tag = codec.tag.as_deref();
            let mut arms = String::new();
            let mut units = String::new();
            let mut tries = String::new();
            let mut one_of = Vec::new();
            for (j, (v, attrs, fields)) in variants.iter().enumerate() {
                let key = variant_key(attrs, v, rename_all);
                let lit = literal(&key);
                let ms = members(fields, None);
                let binds: Vec<String> = fields
                    .iter()
                    .map(|f| format!("{0}: f_{0}", f.name))
                    .collect();
                let pattern = format!("{name}.{v}({})", binds.join(", "));
                // How a value of this variant is written.
                let arm = match (tag, codec.untagged, fields.is_empty()) {
                    (Some(t), _, true) => format!(
                        "    let keys = strs_new();\n    let values = ints_new();\n    \
                         strs_push(keys, {});\n    ints_push(values, json_add_str(doc, {lit}));\n    \
                         json_add_object(doc, keys, values)",
                        literal(t)
                    ),
                    (Some(t), _, false) => {
                        let w = g.write_fields(&ms, "f_");
                        let w = w.replacen(
                            "    let values = ints_new();\n",
                            &format!(
                                "    let values = ints_new();\n    strs_push(keys, {});\n    ints_push(values, json_add_str(doc, {lit}));\n",
                                literal(t)
                            ),
                            1,
                        );
                        format!("{w}    json_add_object(doc, keys, values)")
                    }
                    (None, _, true) => format!("    json_add_str(doc, {lit})"),
                    (None, true, false) => {
                        let w = g.write_fields(&ms, "f_");
                        format!("{w}    json_add_object(doc, keys, values)")
                    }
                    (None, false, false) => {
                        let w = g.write_fields(&ms, "f_");
                        format!(
                            "{w}    let outer = strs_new();\n    strs_push(outer, {lit});\n    \
                             let inner = ints_new();\n    ints_push(inner, json_add_object(doc, keys, values));\n    \
                             json_add_object(doc, outer, inner)"
                        )
                    }
                };
                let _ = writeln!(arms, "        {pattern} => {{\n{arm}\n        }},");
                // How it is read: a helper per variant, from the object holding its fields.
                let extra: Vec<String> = tag.map(|t| vec![t.to_owned()]).unwrap_or_default();
                let r = g.read_fields(
                    &ms,
                    name,
                    "obj",
                    "at",
                    codec.deny_unknown.then_some(extra.as_slice()),
                );
                let _ = write!(
                    g.out,
                    "fn {base}_json_v{j}(doc: JsonDoc, obj: Int, at: Str) -> Result[{name}, Str] ! {{}} {{\n    \
                     if json_kind(doc, obj) != \"object\" {{ return Result[{name}, Str].Err(error: f\"{{at}}: expected an object\"); }}\n\
                     {r}    Result[{name}, Str].Ok(value: {pattern})\n}}\n\n"
                );
                match (tag, codec.untagged, fields.is_empty()) {
                    (Some(_), _, _) => {
                        let _ = writeln!(
                            tries,
                            "    if tag == {lit} {{ return {base}_json_v{j}(doc, node, at); }}"
                        );
                    }
                    (None, _, true) => {
                        let _ = writeln!(
                            units,
                            "        if tag == {lit} {{ return Result[{name}, Str].Ok(value: {pattern}); }}"
                        );
                    }
                    (None, true, false) => {
                        let _ = writeln!(
                            tries,
                            "    match {base}_json_v{j}(doc, node, at) {{ Result.Ok(value: x) => {{ return Result[{name}, Str].Ok(value: x); }}, Result.Err(error: e) => {{}}, }};"
                        );
                    }
                    (None, false, false) => {
                        let _ = writeln!(
                            tries,
                            "    if tag == {lit} {{ return {base}_json_v{j}(doc, body, f\"{{at}}.{{tag}}\"); }}"
                        );
                    }
                }
                one_of.push(match (tag, codec.untagged, fields.is_empty()) {
                    (Some(t), _, _) => object_schema(&ms, Some((t, &key)), codec.deny_unknown),
                    (None, _, true) => format!("{{\"const\":\"{key}\"}}"),
                    (None, true, false) => object_schema(&ms, None, codec.deny_unknown),
                    (None, false, false) => format!(
                        "{{\"type\":\"object\",\"properties\":{{\"{key}\":{}}},\"required\":[\"{key}\"],\"additionalProperties\":false}}",
                        object_schema(&ms, None, codec.deny_unknown)
                    ),
                });
            }
            let strings = format!(
                "    if json_kind(doc, node) == \"string\" {{\n        \
                 let tag = match json_str(doc, node) {{ Option.Some(value: s) => s, Option.None() => \"\", }};\n\
                 {units}        return Result[{name}, Str].Err(error: f\"{{at}}: `{{tag}}` is not a variant of `{name}` without a payload\");\n    }}\n"
            );
            let read = match (tag, codec.untagged) {
                (Some(t), _) => format!(
                    "    if json_kind(doc, node) != \"object\" {{ return Result[{name}, Str].Err(error: f\"{{at}}: expected an object\"); }}\n    \
                     let tag = match json_get(doc, node, {tl}) {{\n        \
                     Option.Some(value: n) => match json_str(doc, n) {{ Option.Some(value: s) => s, Option.None() => \"\", }},\n        \
                     Option.None() => {{ return Result[{name}, Str].Err(error: f\"{{at}}: no member `{t}`\"); }},\n    }};\n\
                     {tries}    Result[{name}, Str].Err(error: f\"{{at}}: `{{tag}}` is not a variant of `{name}`\")",
                    tl = literal(t)
                ),
                (None, true) => format!(
                    "{strings}{tries}    Result[{name}, Str].Err(error: f\"{{at}}: matches no variant of `{name}`\")"
                ),
                (None, false) => format!(
                    "{strings}    if json_kind(doc, node) != \"object\" || json_len(doc, node) != 1 {{\n        \
                     return Result[{name}, Str].Err(error: f\"{{at}}: expected a variant's name, or an object of one member\");\n    }}\n    \
                     let tag = match json_key_at(doc, node, 0) {{ Option.Some(value: s) => s, Option.None() => \"\", }};\n    \
                     let body = match json_value_at(doc, node, 0) {{ Option.Some(value: n) => n, Option.None() => 0 - 1, }};\n\
                     {tries}    Result[{name}, Str].Err(error: f\"{{at}}: `{{tag}}` is not a variant of `{name}` with a payload\")"
                ),
            };
            (
                format!("    match v {{\n{arms}    }}"),
                read,
                // Untagged, the first variant that decodes is taken, so the schema is `anyOf`:
                // `oneOf` would refuse a document two variants accept.
                format!(
                    "{{\"{}\":[{}]}}",
                    if codec.untagged { "anyOf" } else { "oneOf" },
                    one_of.join(",")
                ),
            )
        }
    };
    let mut defs_calls = String::new();
    for r in &g.refs {
        let _ = writeln!(defs_calls, "    {}(seen, defs);", named_fn(r, "json_defs"));
    }
    let entry = literal(&format!("\"{name}\":{def}"));
    let head = literal(&format!(
        "{{\"$schema\":\"https://json-schema.org/draft/2020-12/schema\",\"$ref\":\"#/$defs/{name}\",\"$defs\":{{"
    ));
    let mut s = std::mem::take(&mut g.out);
    let _ = write!(
        s,
        "{vis}fn {base}_json_node(doc: JsonDoc, v: {name}) -> Int ! {{}} {{\n{node}\n}}\n\n\
         {vis}fn {base}_json_read(doc: JsonDoc, node: Int, at: Str) -> Result[{name}, Str] ! {{}} {{\n{read}\n}}\n\n\
         {vis}fn {base}_json_defs(seen: Strs, defs: Strs) -> Int ! {{}} {{\n    \
         for i in 0..strs_len(seen) {{ if strs_get(seen, i) == {lit} {{ return 0; }} }}\n    \
         strs_push(seen, {lit});\n    strs_push(defs, {entry});\n{defs_calls}    0\n}}\n\n\
         {vis}fn {base}_to_json(v: {name}) -> Str ! {{}} {{\n    \
         let doc = json_new();\n    let root = {base}_json_node(doc, v);\n    json_encode(json_with_root(doc, root))\n}}\n\n\
         {vis}fn {base}_from_json(text: Str) -> Result[{name}, Str] ! {{}} {{\n    \
         match json_parse_floats(text) {{\n        \
         Result.Ok(value: doc) => {base}_json_read(doc, json_root(doc), \"$\"),\n        \
         Result.Err(error: e) => Result[{name}, Str].Err(error: e),\n    }}\n}}\n\n\
         {vis}fn {base}_json_schema() -> Str ! {{}} {{\n    \
         let seen = strs_new();\n    let defs = strs_new();\n    {base}_json_defs(seen, defs);\n    \
         str_concat(str_concat({head}, str_join(defs, \",\")), \"}}}}\")\n}}\n\n",
        lit = literal(name),
    );
    s
}

// --- `@derive(toml)` (L2c) -------------------------------------------------------------------

/// One derived type's TOML text, its helpers numbered within it.
struct TomlGen {
    base: String,
    out: String,
    helpers: Vec<String>,
}

impl TomlGen {
    /// The number of the helper pair for `t`, written on first use: `w` makes a `Toml` of a
    /// value, `r` reads one back.
    fn helper(&mut self, t: &TypeName) -> usize {
        let key = text(t);
        if let Some(k) = self.helpers.iter().position(|h| *h == key) {
            return k;
        }
        let k = self.helpers.len();
        self.helpers.push(key.clone());
        let ty = key;
        let (write, read) = self.bodies(t, &ty);
        let base = &self.base;
        let _ = write!(
            self.out,
            "fn {base}_toml_w{k}(v: {ty}) -> Toml ! {{}} {{\n{write}\n}}\n\n\
             fn {base}_toml_r{k}(x: Toml, at: Str) -> Result[{ty}, Str] ! {{}} {{\n{read}\n}}\n\n"
        );
        k
    }

    fn bodies(&mut self, t: &TypeName, ty: &str) -> (String, String) {
        let ok = |v: &str| format!("Result[{ty}, Str].Ok(value: {v})");
        let err = |what: &str| format!("Result[{ty}, Str].Err(error: f\"{{at}}: {what}\")");
        let n = t.name.as_str();
        let base = self.base.clone();
        if plain(t) && args(t).is_empty() {
            if INTEGERS.contains(&n) {
                let w = if n == "Int" {
                    "    Toml.Int(value: v)"
                } else {
                    "    Toml.Int(value: Int(v))"
                };
                let fit = if n == "Int" {
                    ok("i")
                } else {
                    format!(
                        "match try_convert[{n}](i) {{ Option.Some(value: y) => {}, Option.None() => {}, }}",
                        ok("y"),
                        err(&format!("{{i}} is outside `{n}`"))
                    )
                };
                let r = format!(
                    "    if let Toml.Int(value: i) = x {{ {fit} }} else {{ {} }}",
                    err("expected an integer")
                );
                return (w.to_owned(), r);
            }
            if n == "Float64" || n == "Float32" {
                let (w, cast) = if n == "Float64" {
                    ("    Toml.Float(value: v)", "f")
                } else {
                    ("    Toml.Float(value: Float64(v))", "Float32(f)")
                };
                let from_int = if n == "Float64" {
                    "Float64(i)".to_owned()
                } else {
                    "Float32(Float64(i))".to_owned()
                };
                let r = format!(
                    "    if let Toml.Float(value: f) = x {{ return {}; }}\n    \
                     if let Toml.Int(value: i) = x {{ return {}; }}\n    {}",
                    ok(cast),
                    ok(&from_int),
                    err("expected a number")
                );
                return (w.to_owned(), r);
            }
            if n == "Bool" || n == "Str" {
                let (variant, what) = if n == "Bool" {
                    ("Bool", "expected `true` or `false`")
                } else {
                    ("Str", "expected a string")
                };
                return (
                    format!("    Toml.{variant}(value: v)"),
                    format!(
                        "    if let Toml.{variant}(value: y) = x {{ {} }} else {{ {} }}",
                        ok("y"),
                        err(what)
                    ),
                );
            }
        }
        if plain(t) && matches!(n, "Vec" | "List" | "Chain" | "Box") {
            let inner = args(t).first().cloned().unwrap_or_else(|| t.clone());
            let it = text(&inner);
            let k = self.helper(&inner);
            let w = format!("{base}_toml_w{k}");
            let r = format!("{base}_toml_r{k}");
            if n == "Box" {
                return (
                    format!("    {w}(box_get(v))"),
                    format!(
                        "    match {r}(x, at) {{\n        Result.Ok(value: y) => {},\n        Result.Err(error: e) => Result[{ty}, Str].Err(error: e),\n    }}",
                        ok("box_new(y)")
                    ),
                );
            }
            let done = if n == "Chain" {
                ok("chain_from_vec(out)")
            } else {
                ok("out")
            };
            return (
                format!(
                    "    let items = vec_new[Toml]();\n    for y in v {{ vec_push(items, {w}(y)); }}\n    \
                     Toml.Array(items: chain_from_vec(items))"
                ),
                format!(
                    "    if let Toml.Array(items: items) = x {{\n        \
                     let out = vec_new[{it}]();\n        \
                     for i in 0..chain_len(items) {{\n            \
                     match {r}(chain_get(items, i), f\"{{at}}[{{i}}]\") {{\n                \
                     Result.Ok(value: y) => {{ vec_push(out, y); }},\n                \
                     Result.Err(error: e) => {{ return Result[{ty}, Str].Err(error: e); }},\n            }};\n        }}\n        \
                     {done}\n    }} else {{ {} }}",
                    err("expected an array")
                ),
            );
        }
        (
            format!("    {}(v)", named_fn(t, "toml_value")),
            format!("    {}(x, at)", named_fn(t, "toml_read")),
        )
    }

    /// The members of a field list as a table's keys and values; each field read as
    /// `{from}{name}`. An `Option` that is `None` is left out.
    fn write_fields(
        &mut self,
        members: &[Member<'_>],
        from: &str,
        first: Option<(&str, &str)>,
    ) -> String {
        let mut s =
            String::from("    let keys = vec_new[Str]();\n    let values = vec_new[Toml]();\n");
        if let Some((tag, value)) = first {
            let _ = writeln!(
                s,
                "    vec_push(keys, {});\n    vec_push(values, Toml.Str(value: {}));",
                literal(tag),
                literal(value)
            );
        }
        for m in members {
            let name = &m.field.name;
            let key = literal(&m.key);
            let optional = plain(&m.field.ty) && m.field.ty.name == "Option";
            if optional {
                let inner = args(&m.field.ty)
                    .first()
                    .cloned()
                    .unwrap_or_else(|| m.field.ty.clone());
                let k = self.helper(&inner);
                let base = &self.base;
                let _ = writeln!(
                    s,
                    "    if let Option.Some(value: y) = {from}{name} {{\n        vec_push(keys, {key});\n        \
                     vec_push(values, {base}_toml_w{k}(y));\n    }}"
                );
            } else {
                let k = self.helper(&m.field.ty);
                let base = &self.base;
                let _ = writeln!(
                    s,
                    "    vec_push(keys, {key});\n    vec_push(values, {base}_toml_w{k}({from}{name}));"
                );
            }
        }
        s.push_str("    Toml.Table(keys: chain_from_vec(keys), values: chain_from_vec(values))");
        s
    }

    /// A field list read out of the table `obj` at path `at`, into bindings `f_<name>`.
    fn read_fields(
        &mut self,
        members: &[Member<'_>],
        owner: &str,
        obj: &str,
        at: &str,
        deny: Option<&[String]>,
    ) -> String {
        let mut s = String::new();
        if let Some(extra) = deny {
            let mut names: Vec<String> = members.iter().map(|m| m.key.clone()).collect();
            names.extend(extra.iter().cloned());
            let test = if names.is_empty() {
                "false".to_owned()
            } else {
                names
                    .iter()
                    .map(|n| format!("k == {}", literal(n)))
                    .collect::<Vec<_>>()
                    .join(" || ")
            };
            let _ = writeln!(
                s,
                "    if let Toml.Table(keys: ks, values: vs) = {obj} {{\n        \
                 for i in 0..chain_len(ks) {{\n            let k = chain_get(ks, i);\n            \
                 if !({test}) {{ return Result[{owner}, Str].Err(error: f\"{{{at}}}: unknown key `{{k}}`\"); }}\n        }}\n    }}"
            );
        }
        for m in members {
            let f = m.field;
            let optional = plain(&f.ty) && f.ty.name == "Option";
            let (k, present) = if optional {
                let inner = args(&f.ty).first().cloned().unwrap_or_else(|| f.ty.clone());
                (self.helper(&inner), true)
            } else {
                (self.helper(&f.ty), false)
            };
            let base = &self.base;
            let missing = if let Some(empty) = m.codec.default.then(|| empty_value(&f.ty)).flatten()
            {
                empty
            } else if optional {
                format!(
                    "Option[{}].None()",
                    text(args(&f.ty).first().unwrap_or(&f.ty))
                )
            } else {
                format!(
                    "{{ return Result[{owner}, Str].Err(error: f\"{{{at}}}: no key `{}`\"); }}",
                    m.key
                )
            };
            let got = if present {
                format!(
                    "Option[{}].Some(value: y)",
                    text(args(&f.ty).first().unwrap_or(&f.ty))
                )
            } else {
                "y".to_owned()
            };
            let _ = writeln!(
                s,
                "    let f_{name} = match toml_get({obj}, {key}) {{\n        \
                 Option.Some(value: n) => match {base}_toml_r{k}(n, f\"{{{at}}}.{path}\") {{\n            \
                 Result.Ok(value: y) => {got},\n            \
                 Result.Err(error: e) => {{ return Result[{owner}, Str].Err(error: e); }},\n        }},\n        \
                 Option.None() => {missing},\n    }};",
                name = f.name,
                key = literal(&m.key),
                path = m.key,
            );
        }
        s
    }
}

#[allow(clippy::too_many_lines)]
fn generate_toml(name: &str, public: bool, codec: &TypeCodec, shape: &Shape<'_>) -> String {
    let mut g = TomlGen {
        base: snake(name),
        out: String::new(),
        helpers: Vec::new(),
    };
    let base = g.base.clone();
    let vis = if public { "pub " } else { "" };
    let rename_all = codec.rename_all.as_deref();
    let deny: Option<&[String]> = codec.deny_unknown.then_some(&[]);
    let (value, read, record) = match shape {
        Shape::Record(fields) => {
            let ms = members(fields, rename_all);
            let w = g.write_fields(&ms, "v.", None);
            let r = g.read_fields(&ms, name, "x", "at", deny);
            let inits: Vec<String> = fields
                .iter()
                .map(|f| format!("{0}: f_{0}", f.name))
                .collect();
            (
                w,
                format!(
                    "    if !toml_is_table(x) {{ return Result[{name}, Str].Err(error: f\"{{at}}: expected a table\"); }}\n\
                     {r}    Result[{name}, Str].Ok(value: {name}({}))",
                    inits.join(", ")
                ),
                true,
            )
        }
        Shape::Enum(variants) => {
            let tag = codec.tag.as_deref();
            let mut arms = String::new();
            let mut units = String::new();
            let mut tries = String::new();
            for (j, (v, attrs, fields)) in variants.iter().enumerate() {
                let key = variant_key(attrs, v, rename_all);
                let lit = literal(&key);
                let ms = members(fields, None);
                let binds: Vec<String> = fields
                    .iter()
                    .map(|f| format!("{0}: f_{0}", f.name))
                    .collect();
                let pattern = format!("{name}.{v}({})", binds.join(", "));
                let arm = match (tag, codec.untagged, fields.is_empty()) {
                    (Some(t), _, _) => g.write_fields(&ms, "f_", Some((t, &key))),
                    (None, _, true) => format!("    Toml.Str(value: {lit})"),
                    (None, true, false) => g.write_fields(&ms, "f_", None),
                    (None, false, false) => {
                        let w = g.write_fields(&ms, "f_", None);
                        format!(
                            "    let body = {{\n{w}\n    }};\n    let outer = vec_new[Str]();\n    vec_push(outer, {lit});\n    \
                             let inner = vec_new[Toml]();\n    vec_push(inner, body);\n    \
                             Toml.Table(keys: chain_from_vec(outer), values: chain_from_vec(inner))"
                        )
                    }
                };
                let _ = writeln!(arms, "        {pattern} => {{\n{arm}\n        }},");
                let extra: Vec<String> = tag.map(|t| vec![t.to_owned()]).unwrap_or_default();
                let r = g.read_fields(
                    &ms,
                    name,
                    "obj",
                    "at",
                    codec.deny_unknown.then_some(extra.as_slice()),
                );
                let _ = write!(
                    g.out,
                    "fn {base}_toml_v{j}(obj: Toml, at: Str) -> Result[{name}, Str] ! {{}} {{\n    \
                     if !toml_is_table(obj) {{ return Result[{name}, Str].Err(error: f\"{{at}}: expected a table\"); }}\n\
                     {r}    Result[{name}, Str].Ok(value: {pattern})\n}}\n\n"
                );
                match (tag, codec.untagged, fields.is_empty()) {
                    (Some(_), _, _) => {
                        let _ = writeln!(
                            tries,
                            "    if tag == {lit} {{ return {base}_toml_v{j}(x, at); }}"
                        );
                    }
                    (None, _, true) => {
                        let _ = writeln!(
                            units,
                            "        if s == {lit} {{ return Result[{name}, Str].Ok(value: {pattern}); }}"
                        );
                    }
                    (None, true, false) => {
                        let _ = writeln!(
                            tries,
                            "    match {base}_toml_v{j}(x, at) {{ Result.Ok(value: y) => {{ return Result[{name}, Str].Ok(value: y); }}, Result.Err(error: e) => {{}}, }};"
                        );
                    }
                    (None, false, false) => {
                        let _ = writeln!(
                            tries,
                            "    if tag == {lit} {{ return {base}_toml_v{j}(body, f\"{{at}}.{{tag}}\"); }}"
                        );
                    }
                }
            }
            let strings = format!(
                "    if let Toml.Str(value: s) = x {{\n{units}        return Result[{name}, Str].Err(error: f\"{{at}}: `{{s}}` is not a variant of `{name}` without a payload\");\n    }}\n"
            );
            let read = match (tag, codec.untagged) {
                (Some(t), _) => format!(
                    "    let tag = match toml_get(x, {tl}) {{\n        \
                     Option.Some(value: n) => if let Toml.Str(value: s) = n {{ s }} else {{ \"\" }},\n        \
                     Option.None() => {{ return Result[{name}, Str].Err(error: f\"{{at}}: no key `{t}`\"); }},\n    }};\n\
                     {tries}    Result[{name}, Str].Err(error: f\"{{at}}: `{{tag}}` is not a variant of `{name}`\")",
                    tl = literal(t)
                ),
                (None, true) => format!(
                    "{strings}{tries}    Result[{name}, Str].Err(error: f\"{{at}}: matches no variant of `{name}`\")"
                ),
                (None, false) => format!(
                    "{strings}    let mut tag = \"\";\n    let mut body = x;\n    let mut one = false;\n    \
                     if let Toml.Table(keys: ks, values: vs) = x {{\n        \
                     if chain_len(ks) == 1 {{ tag = chain_get(ks, 0); body = chain_get(vs, 0); one = true; }}\n    }}\n    \
                     if !one {{ return Result[{name}, Str].Err(error: f\"{{at}}: expected a variant's name, or a table of one key\"); }}\n\
                     {tries}    Result[{name}, Str].Err(error: f\"{{at}}: `{{tag}}` is not a variant of `{name}` with a payload\")"
                ),
            };
            (format!("    match v {{\n{arms}    }}"), read, false)
        }
    };
    let mut s = std::mem::take(&mut g.out);
    let _ = write!(
        s,
        "{vis}fn {base}_toml_value(v: {name}) -> Toml ! {{}} {{\n{value}\n}}\n\n\
         {vis}fn {base}_toml_read(x: Toml, at: Str) -> Result[{name}, Str] ! {{}} {{\n{read}\n}}\n\n"
    );
    if record {
        let _ = write!(
            s,
            "{vis}fn {base}_to_toml(v: {name}) -> Str ! {{}} {{\n    toml_encode({base}_toml_value(v))\n}}\n\n\
             {vis}fn {base}_from_toml(text: Str) -> Result[{name}, Str] ! {{}} {{\n    \
             match toml_parse(text) {{\n        \
             Result.Ok(value: t) => {base}_toml_read(t, \"$\"),\n        \
             Result.Err(error: e) => Result[{name}, Str].Err(error: e),\n    }}\n}}\n\n"
        );
    }
    s
}

/// `text` with the functions its derives mean appended, or unchanged when it derives nothing.
#[must_use]
pub fn expand(text: &str) -> String {
    if !text.contains("@derive") {
        return text.to_owned();
    }
    let (program, _) = nazm_syntax::parse_source(text);
    let mut generated = String::new();
    let mut used: Vec<&str> = Vec::new();
    for format in FORMATS {
        // A type the program declares without this derive is refused at the field (the
        // checker); nothing is generated for a type with such a field, so the report is that one.
        let underived = |t: &TypeName| {
            let declared = program.records.iter().any(|r| r.name == t.name)
                || program.enums.iter().any(|e| e.name == t.name);
            let derived = program
                .records
                .iter()
                .any(|r| r.name == t.name && derives(&r.attrs, format))
                || program
                    .enums
                    .iter()
                    .any(|e| e.name == t.name && derives(&e.attrs, format));
            declared && !derived
        };
        // A field whose `@codec` its type does not allow is refused by the checker too: an
        // `omit_none` on what is not an `Option`, a `default` on a type with no empty value.
        let codec_ok = |f: &Field| {
            let c = field_codec(&f.attrs);
            let optional = plain(&f.ty) && f.ty.name == "Option";
            !(c.omit_none && !optional) && !(c.default && empty_value(&f.ty).is_none())
        };
        let fields_ok = |fields: &[Field]| {
            fields.iter().all(|f| {
                refusal_for(format, &f.ty).is_none()
                    && !named(&f.ty).into_iter().any(underived)
                    && codec_ok(f)
            })
        };
        let gen = |name: &str, public: bool, codec: &TypeCodec, shape: &Shape<'_>| {
            if *format == "json" {
                generate(name, public, codec, shape)
            } else {
                generate_toml(name, public, codec, shape)
            }
        };
        let before = generated.len();
        for r in &program.records {
            if derives(&r.attrs, format) && r.type_params.is_empty() && fields_ok(&r.fields) {
                generated.push_str(&gen(
                    &r.name,
                    r.pub_span.is_some(),
                    &type_codec(&r.attrs),
                    &Shape::Record(&r.fields),
                ));
            }
        }
        for e in &program.enums {
            if derives(&e.attrs, format)
                && e.type_params.is_empty()
                && e.variants.iter().all(|v| fields_ok(&v.fields))
            {
                let variants = e
                    .variants
                    .iter()
                    .map(|v| (v.name.as_str(), v.attrs.as_slice(), v.fields.as_slice()))
                    .collect();
                generated.push_str(&gen(
                    &e.name,
                    e.pub_span.is_some(),
                    &type_codec(&e.attrs),
                    &Shape::Enum(variants),
                ));
            }
        }
        if generated.len() > before {
            used.push(format);
        }
    }
    if generated.is_empty() {
        return text.to_owned();
    }
    let mut out = text.to_owned();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("\n// --- generated by `@derive` (docs/spec.md, *Derived serialisation*) ---\n\n");
    for format in used {
        let _ = writeln!(out, "use \"@std/{format}\";");
    }
    out.push('\n');
    out.push_str(&generated);
    out
}
