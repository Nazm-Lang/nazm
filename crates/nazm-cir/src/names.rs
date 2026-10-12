//! Durable names for what Core IR refers to by session-local identity.
//!
//! The printed form and the digests read the same program from any checkout, so neither may
//! contain a `FnId`, a `RecordId` or a file index. A definition is named by its durable key —
//! its module's key, its kind and its name — and a type by its definition's key and its
//! arguments. A module with no durable identity (one outside any root) is named by its load
//! position, which is what every other durable form does for it and is said so in the name.

use nazm_sema::key::{DefKey, DefKind};
use nazm_sema::module::{ModuleGraph, ModuleId};
use nazm_sema::resolve::{FnId, Resolution};
use nazm_sema::types::{ParamOwner, Type};
use nazm_sema::udt::TypeNode;
use nazm_sema::{EnumId, RecordId};
use nazm_span::Span;

/// What a program's identities are called, durably.
#[derive(Debug, Clone, Copy)]
pub struct Names<'a> {
    res: &'a Resolution,
    graph: &'a ModuleGraph,
}

impl<'a> Names<'a> {
    #[must_use]
    pub fn new(res: &'a Resolution, graph: &'a ModuleGraph) -> Self {
        Self { res, graph }
    }

    /// A module: its key, or `#` and its load position where it has none.
    #[must_use]
    pub fn module(&self, m: ModuleId) -> String {
        self.graph
            .key(m)
            .map_or_else(|| format!("#{}", m.index()), |k| k.as_str().to_owned())
    }

    fn definition(&self, module: ModuleId, kind: DefKind, name: &str) -> String {
        match self.graph.key(module) {
            Some(k) => DefKey::new(k.clone(), kind, name).to_string(),
            None => format!("#{}::{} {name}", module.index(), kind.as_str()),
        }
    }

    /// A function, by its durable key.
    #[must_use]
    pub fn function(&self, id: FnId) -> String {
        self.res.function(id).map_or_else(
            || "?".to_owned(),
            |d| self.definition(d.module, DefKind::Function, &d.name),
        )
    }

    /// A type, canonically: a built-in by its name, a record or enum by its definition's key
    /// and its arguments, a `Vec` by its element, and a type parameter by its position among
    /// its owner's — `$0` for a function's first. The same type prints the same everywhere.
    #[must_use]
    pub fn ty(&self, t: Type) -> String {
        let types = self.res.types();
        match t {
            Type::Vec(v) | Type::Box(v) | Type::List(v) | Type::Cell(v) => format!(
                "{}[{}]",
                match t {
                    Type::Box(_) => "Box",
                    Type::List(_) => "List",
                    Type::Cell(_) => "Cell",
                    _ => "Vec",
                },
                types
                    .vec_element(v)
                    .map_or_else(|| "?".to_owned(), |e| self.ty(e))
            ),
            Type::ChanOf(c) => format!(
                "Chan[{}]",
                types
                    .chan_element(c)
                    .map_or_else(|| "?".to_owned(), |e| self.ty(e))
            ),
            Type::Record(_) | Type::Enum(_) => {
                let head = match types.definition_of(t) {
                    Some(TypeNode::Record(id)) => self.record_definition(id),
                    Some(TypeNode::Enum(id)) => self.enum_definition(id),
                    None => "?".to_owned(),
                };
                let args = types.type_args(t);
                if args.is_empty() {
                    head
                } else {
                    let inner: Vec<String> = args.iter().map(|a| self.ty(*a)).collect();
                    format!("{head}[{}]", inner.join(", "))
                }
            }
            Type::Param(p) => match p.owner {
                ParamOwner::Function(_) => format!("${}", p.index),
                ParamOwner::Record(_) | ParamOwner::Enum(_) | ParamOwner::Builtin(_) => {
                    format!("$?{}", p.index)
                }
                // A trait's `Self` reaches Core IR only inside a trait call's own record of
                // what it was written against (N78).
                ParamOwner::Trait(_) => "$Self".to_owned(),
            },
            other => other.builtin_name().unwrap_or("?").to_owned(),
        }
    }

    fn record_definition(&self, id: RecordId) -> String {
        self.res.records().get(id).map_or_else(
            || "?".to_owned(),
            |d| self.definition(d.module, DefKind::Record, &d.name),
        )
    }

    fn enum_definition(&self, id: EnumId) -> String {
        self.res.enums().get(id).map_or_else(
            || "?".to_owned(),
            |d| self.definition(d.module, DefKind::Enum, &d.name),
        )
    }

    /// A record's field, by name: declaration order is not part of what a record means.
    #[must_use]
    /// A trait's method, durably: `module::trait Name.method` (N78).
    pub fn trait_method(&self, id: nazm_sema::traits::TraitId, method: u32) -> String {
        self.res.traits().get(id).map_or_else(
            || "?".to_owned(),
            |d| {
                let m = d
                    .methods
                    .get(method as usize)
                    .map_or("?", |m| m.name.as_str());
                format!("{}.{m}", self.definition(d.module, DefKind::Trait, &d.name))
            },
        )
    }

    pub fn field(&self, record: RecordId, field: u32) -> String {
        self.res
            .records()
            .get(record)
            .and_then(|d| d.fields.get(field as usize))
            .map_or_else(|| "?".to_owned(), |f| f.name.clone())
    }

    /// A variant, by name.
    #[must_use]
    pub fn variant(&self, enumeration: EnumId, variant: u32) -> String {
        self.res
            .enums()
            .get(enumeration)
            .and_then(|d| d.variants.get(variant as usize))
            .map_or_else(|| "?".to_owned(), |v| v.name.clone())
    }

    /// A variant's payload field, by name.
    #[must_use]
    pub fn payload(&self, enumeration: EnumId, variant: u32, field: u32) -> String {
        self.res
            .enums()
            .get(enumeration)
            .and_then(|d| d.variants.get(variant as usize))
            .and_then(|v| v.fields.get(field as usize))
            .map_or_else(|| "?".to_owned(), |f| f.name.clone())
    }

    /// A span, as the module it is in and its byte range there — or `synthetic`.
    #[must_use]
    pub fn span(&self, s: Span) -> String {
        if s == Span::synthetic() {
            return "synthetic".to_owned();
        }
        match self.graph.module_of(s.file) {
            Some(m) => format!("{}:{}..{}", self.module(m), s.start, s.end),
            None => format!("?:{}..{}", s.start, s.end),
        }
    }
}
