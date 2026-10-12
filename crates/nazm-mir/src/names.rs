//! Durable names for what MIR refers to by session-local identity.
//!
//! Computed once, during lowering, from the checker's tables — and then carried in MIR, so that
//! nothing below it needs the checker to know what a function or a type is called. The type key
//! is the one `nazm-lir` derived symbols from since N11, moved here unchanged: a generic
//! instance's identity *is* its definition and its arguments' keys, and a backend symbol is a
//! spelling of that identity, never a second one.

use nazm_sema::key::{DefKey, DefKind};
use nazm_sema::module::{ModuleGraph, ModuleId};
use nazm_sema::types::Type;
use nazm_sema::udt::{TypeNode, UserTypes};

use crate::ir::Owner;

/// A definition's owner: its durable key where its module has one.
#[must_use]
pub fn owner(graph: &ModuleGraph, module: ModuleId, kind: DefKind, name: &str) -> Owner {
    match graph.key(module) {
        Some(k) => Owner::Keyed(DefKey::new(k.clone(), kind, name)),
        None => Owner::Unkeyed {
            module,
            name: name.to_owned(),
        },
    }
}

/// A type's canonical key: a built-in by its name, a record or enum by its definition and its
/// arguments, a `Vec` by its element. The same type has the same key in every unit and every
/// checkout; two different types never share one.
#[must_use]
pub fn type_key(t: Type, types: &UserTypes, graph: &ModuleGraph) -> String {
    match t {
        Type::Vec(v) | Type::Box(v) | Type::List(v) | Type::Cell(v) => format!(
            "{}({})",
            match t {
                Type::Box(_) => "Box",
                Type::List(_) => "List",
                Type::Cell(_) => "Cell",
                _ => "Vec",
            },
            types
                .vec_element(v)
                .map_or_else(|| "?".to_owned(), |e| type_key(e, types, graph))
        ),
        Type::ChanOf(c) => format!(
            "Chan({})",
            types
                .chan_element(c)
                .map_or_else(|| "?".to_owned(), |e| type_key(e, types, graph))
        ),
        Type::Record(_) | Type::Enum(_) => {
            let Some(node) = types.definition_of(t) else {
                return "?".to_owned();
            };
            let (kind, module, name) = match node {
                TypeNode::Record(id) => {
                    let Some(d) = types.records().get(id) else {
                        return "?".to_owned();
                    };
                    ("R", d.module, d.name.clone())
                }
                TypeNode::Enum(id) => {
                    let Some(d) = types.enums().get(id) else {
                        return "?".to_owned();
                    };
                    ("E", d.module, d.name.clone())
                }
            };
            let head = match graph.key(module) {
                Some(k) => format!("{kind}.m.{}.{}", encode(k.as_str()), encode(&name)),
                None => format!("{kind}.u.{}.{}", module.index(), encode(&name)),
            };
            let args = types.type_args(t);
            if args.is_empty() {
                head
            } else {
                let inner: Vec<String> = args.iter().map(|a| type_key(*a, types, graph)).collect();
                format!("{head}({})", inner.join(","))
            }
        }
        // `[T; N]` (Gate 2): its element's key and its length.
        Type::Array(a) => match types.array_info(a) {
            Some((e, n)) => format!("Array({},{n})", type_key(e, types, graph)),
            None => "?".to_owned(),
        },
        Type::Param(_) => "?".to_owned(),
        other => other.builtin_name().unwrap_or("?").to_owned(),
    }
}

/// `text` with every byte that is not an ASCII letter or digit written as `_XX`. Injective, and
/// valid in any symbol table.
#[must_use]
pub fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() {
            out.push(char::from(b));
        } else {
            out.push('_');
            out.push_str(&format!("{b:02X}"));
        }
    }
    out
}

/// An owner, as text a digest or a printed program names it by.
#[must_use]
pub fn owner_text(o: &Owner) -> String {
    match o {
        Owner::Keyed(k) => k.to_string(),
        Owner::Unkeyed { module, name } => format!("#{}::{name}", module.index()),
    }
}
