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
pub const FORMATS: &[&str] = &["json"];

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
                "`{n}` has values no JSON integer here carries; convert it to a type that has"
            ));
        }
        if nazm_sema::Type::from_name(n).is_some() {
            return Some(format!("`{n}` has no JSON form"));
        }
    }
    if plain(t) && matches!(n, "Option" | "Vec" | "List" | "Box") {
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

/// The record and enum names a field's type reaches, unqualified: `Option[Vec[Point]]` reaches
/// `Point`.
#[must_use]
pub fn named(t: &TypeName) -> Vec<&TypeName> {
    let n = t.name.as_str();
    if plain(t) && matches!(n, "Option" | "Vec" | "List" | "Box") {
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
        if plain(t) && matches!(n, "Option" | "Vec" | "List" | "Box") {
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
                    let done = if n == "List" {
                        ok("list_from_vec(out)")
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

    /// A field list's members, encoded into `keys` and `values` from the bindings `f_<name>`.
    fn write_fields(&mut self, fields: &[Field], from: &str) -> String {
        let mut s = String::from("    let keys = strs_new();\n    let values = ints_new();\n");
        for f in fields {
            let k = self.helper(&f.ty);
            let base = &self.base;
            let _ = writeln!(
                s,
                "    strs_push(keys, {});\n    ints_push(values, {base}_json_w{k}(doc, {from}{}));",
                literal(&f.name),
                f.name
            );
        }
        s
    }

    /// A field list read out of the object `obj` at path `at`, into bindings `f_<name>`; a
    /// failure returns `Err` as a `Result[owner, Str]`.
    fn read_fields(&mut self, fields: &[Field], owner: &str, obj: &str, at: &str) -> String {
        let mut s = String::new();
        for f in fields {
            let k = self.helper(&f.ty);
            let base = &self.base;
            let optional = plain(&f.ty) && f.ty.name == "Option";
            let missing = if optional {
                format!(
                    "Option[{}].None()",
                    text(args(&f.ty).first().unwrap_or(&f.ty))
                )
            } else {
                format!(
                    "{{ return Result[{owner}, Str].Err(error: f\"{{{at}}}: no member `{}`\"); }}",
                    f.name
                )
            };
            let _ = writeln!(
                s,
                "    let f_{name} = match json_get(doc, {obj}, {key}) {{\n        \
                 Option.Some(value: n) => match {base}_json_r{k}(doc, n, f\"{{{at}}}.{name}\") {{\n            \
                 Result.Ok(value: x) => x,\n            \
                 Result.Err(error: e) => {{ return Result[{owner}, Str].Err(error: e); }},\n        }},\n        \
                 Option.None() => {missing},\n    }};",
                name = f.name,
                key = literal(&f.name),
            );
        }
        s
    }
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
    if plain(t) && matches!(n, "Option" | "Vec" | "List" | "Box") {
        let inner = args(t).first().map_or_else(String::new, schema_of);
        return match n {
            "Option" => format!("{{\"anyOf\":[{inner},{{\"type\":\"null\"}}]}}"),
            "Box" => inner,
            _ => format!("{{\"type\":\"array\",\"items\":{inner}}}"),
        };
    }
    format!("{{\"$ref\":\"#/$defs/{n}\"}}")
}

/// A field list's schema: an object of its members, those not optional required.
fn object_schema(fields: &[Field]) -> String {
    let props: Vec<String> = fields
        .iter()
        .map(|f| format!("\"{}\":{}", f.name, schema_of(&f.ty)))
        .collect();
    let required: Vec<String> = fields
        .iter()
        .filter(|f| !(plain(&f.ty) && f.ty.name == "Option"))
        .map(|f| format!("\"{}\"", f.name))
        .collect();
    format!(
        "{{\"type\":\"object\",\"properties\":{{{}}},\"required\":[{}]}}",
        props.join(","),
        required.join(",")
    )
}

/// One derived type: its kind, name, whether it is public, and its fields or variants.
enum Shape<'a> {
    Record(&'a [Field]),
    Enum(Vec<(&'a str, &'a [Field])>),
}

fn generate(name: &str, public: bool, shape: &Shape<'_>) -> String {
    let mut g = Gen {
        base: snake(name),
        vis: if public { "pub " } else { "" },
        out: String::new(),
        helpers: Vec::new(),
        refs: Vec::new(),
    };
    let base = g.base.clone();
    let vis = g.vis;
    let (node, read, def) = match shape {
        Shape::Record(fields) => {
            let w = g.write_fields(fields, "v.");
            let r = g.read_fields(fields, name, "node", "at");
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
                object_schema(fields),
            )
        }
        Shape::Enum(variants) => {
            let mut arms = String::new();
            let mut units = String::new();
            let mut payloads = String::new();
            let mut one_of = Vec::new();
            for (v, fields) in variants {
                if fields.is_empty() {
                    let _ = writeln!(
                        arms,
                        "        {name}.{v}() => json_add_str(doc, {}),",
                        literal(v)
                    );
                    let _ = writeln!(
                        units,
                        "        if tag == {} {{ return Result[{name}, Str].Ok(value: {name}.{v}()); }}",
                        literal(v)
                    );
                    one_of.push(format!("{{\"const\":\"{v}\"}}"));
                } else {
                    let binds: Vec<String> = fields
                        .iter()
                        .map(|f| format!("{0}: f_{0}", f.name))
                        .collect();
                    let w = g.write_fields(fields, "f_");
                    let _ = writeln!(
                        arms,
                        "        {name}.{v}({}) => {{\n{w}        let outer = strs_new();\n        strs_push(outer, {});\n        \
                         let inner = ints_new();\n        ints_push(inner, json_add_object(doc, keys, values));\n        \
                         json_add_object(doc, outer, inner)\n        }},",
                        binds.join(", "),
                        literal(v)
                    );
                    let r = g.read_fields(fields, name, "body", "path");
                    let _ = writeln!(
                        payloads,
                        "    if tag == {} {{\n        \
                         if json_kind(doc, body) != \"object\" {{ return Result[{name}, Str].Err(error: f\"{{path}}: expected an object\"); }}\n\
                         {r}        return Result[{name}, Str].Ok(value: {name}.{v}({}));\n    }}",
                        literal(v),
                        binds.join(", ")
                    );
                    one_of.push(format!(
                        "{{\"type\":\"object\",\"properties\":{{\"{v}\":{}}},\"required\":[\"{v}\"],\"additionalProperties\":false}}",
                        object_schema(fields)
                    ));
                }
            }
            (
                format!("    match v {{\n{arms}    }}"),
                format!(
                    "    if json_kind(doc, node) == \"string\" {{\n        \
                     let tag = match json_str(doc, node) {{ Option.Some(value: s) => s, Option.None() => \"\", }};\n\
                     {units}        return Result[{name}, Str].Err(error: f\"{{at}}: `{{tag}}` is not a variant of `{name}` without a payload\");\n    }}\n    \
                     if json_kind(doc, node) != \"object\" || json_len(doc, node) != 1 {{\n        \
                     return Result[{name}, Str].Err(error: f\"{{at}}: expected a variant's name, or an object of one member\");\n    }}\n    \
                     let tag = match json_key_at(doc, node, 0) {{ Option.Some(value: s) => s, Option.None() => \"\", }};\n    \
                     let body = match json_value_at(doc, node, 0) {{ Option.Some(value: n) => n, Option.None() => 0 - 1, }};\n    \
                     let path = f\"{{at}}.{{tag}}\";\n\
                     {payloads}    Result[{name}, Str].Err(error: f\"{{at}}: `{{tag}}` is not a variant of `{name}` with a payload\")"
                ),
                format!("{{\"oneOf\":[{}]}}", one_of.join(",")),
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

/// `text` with the functions its derives mean appended, or unchanged when it derives nothing.
#[must_use]
pub fn expand(text: &str) -> String {
    if !text.contains("@derive") {
        return text.to_owned();
    }
    let (program, _) = nazm_syntax::parse_source(text);
    let mut generated = String::new();
    // A type the program declares without this derive is refused at the field (the checker);
    // nothing is generated for a type with such a field, so the report is that one.
    let underived = |t: &TypeName| {
        let declared = program.records.iter().any(|r| r.name == t.name)
            || program.enums.iter().any(|e| e.name == t.name);
        let derived = program
            .records
            .iter()
            .any(|r| r.name == t.name && derives(&r.attrs, "json"))
            || program
                .enums
                .iter()
                .any(|e| e.name == t.name && derives(&e.attrs, "json"));
        declared && !derived
    };
    let fields_ok = |fields: &[Field]| {
        fields
            .iter()
            .all(|f| refusal(&f.ty).is_none() && !named(&f.ty).into_iter().any(underived))
    };
    for r in &program.records {
        if derives(&r.attrs, "json") && r.type_params.is_empty() && fields_ok(&r.fields) {
            generated.push_str(&generate(
                &r.name,
                r.pub_span.is_some(),
                &Shape::Record(&r.fields),
            ));
        }
    }
    for e in &program.enums {
        if derives(&e.attrs, "json")
            && e.type_params.is_empty()
            && e.variants.iter().all(|v| fields_ok(&v.fields))
        {
            let variants = e
                .variants
                .iter()
                .map(|v| (v.name.as_str(), v.fields.as_slice()))
                .collect();
            generated.push_str(&generate(
                &e.name,
                e.pub_span.is_some(),
                &Shape::Enum(variants),
            ));
        }
    }
    if generated.is_empty() {
        return text.to_owned();
    }
    let mut out = text.to_owned();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(
        "\n// --- generated by `@derive(json)` (docs/spec.md, *Derived serialisation*) ---\n\n",
    );
    out.push_str("use \"@std/json\";\n\n");
    out.push_str(&generated);
    out
}
