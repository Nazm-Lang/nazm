//! Semantic context packets: one source definition, its exact source, and compact links to
//! what it is semantically connected to. `nazm.context/3` (N27; effects since N36,
//! provenance since N38).
//!
//! # The contract
//!
//! **A packet reports what the compiler already knows, and nothing it does not.** Its
//! target is a function, record or enum the checker declared from source, found through the
//! entity the reference index records at a position (N17) — never by a spelling. Every
//! section is read from a layer that already answers it:
//!
//! | Section | From |
//! | --- | --- |
//! | the target's exact source, declaration and name ranges | the outline symbol (N22) |
//! | a function's signature | the one signature renderer (N20) |
//! | a record's fields, an enum's variants and payloads, canonical order | the structure members (N21) |
//! | dependencies: every entity used inside the declaration, grouped | the reference index (N17) |
//! | related types: records and enums whose names are written inside it — as a type, a construction's head or a variant's enum | the checker's written type names (N18) |
//! | references: every use of the target | the reference index (N17) |
//! | callers and callees of a function | call hierarchy (N25), direct only |
//! | diagnostics whose primary span is inside it | the analysis |
//! | a function's effects: declared, required, published, and why | the effect checker (N36) |
//! | a function's provenance: the origins its result carries, the parameters it passes on and those that reach a restricted sink | the provenance solver (N38) |
//!
//! Capabilities, test coverage, semantic changes and semantic deltas are facts the compiler
//! does not have, and are reported **unsupported** — never as an empty list, which would
//! say they were looked for and none exist. `/1` reported effects so too; N36 gave the
//! compiler them, and since a `/1` reader may rely on that field being `unsupported`, the
//! packet that has them is `/2`. N38 adds a `provenance` section a `/2` reader does not know,
//! and an object that validates against `/2` has no such field, so the packet is `/3`. A
//! record or enum has no callers or
//! callees: those sections are **not applicable** to it, which is also not an empty list.
//!
//! # Minimum context
//!
//! The target's source is the only source text a packet carries. Everything it links to is
//! an identity, a kind, a name and a place, which is enough to ask for that definition's
//! own packet. A packet is complete for what it promises in the compilation it was built
//! from; it does not claim to be sufficient for every task, or to know every caller in a
//! project.
//!
//! # Which program, and which text
//!
//! The compilation of the document the packet is asked of, exactly as the service analysed
//! it — its root and everything it imports, and no other file: nothing is scanned. A source
//! that needed syntax recovery anywhere in that compilation has no packet, since what was
//! recovered from could be a use, a caller or a declaration. Paths are relative to the
//! compilation's source root, identities are durable definition keys (`module::kind name`)
//! where the module has one, and nothing session-local — no generation, no numeric id, no
//! host path, no time — is in a packet, so the same sources and the same question give the
//! same bytes.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use nazm_core::sema::{DefKey, DefKind, ModuleKey, ReferenceTarget};
use nazm_span::{FileId, Span};
use serde::Serialize;

use crate::hierarchy::{HierarchyFunction, Locator};
use crate::service::{identity, name_at, FileKey, LanguageService, Location, Snapshot};
use crate::structure::{members, StructureSite};
use crate::symbols::SymbolKind;

/// The schema every packet declares.
pub const SCHEMA: &str = "nazm.context/3";

/// A semantic context packet, or why there is none, exactly as `nazm.context/3` serialises
/// it: `status` says which.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum ContextResult {
    Available(Box<ContextPacket>),
    Unavailable(Unavailable),
}

/// One definition's context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextPacket {
    pub schema: &'static str,
    /// Always `available`.
    pub status: &'static str,
    /// The compilation's root module, relative to its source root.
    pub root: String,
    pub target: ContextTarget,
    /// Every entity the declaration uses, other than its own locals and itself.
    pub dependencies: Vec<ContextLink>,
    /// The records and enums the declaration writes by name — as a type, a construction's
    /// head or a variant's enum — directly: not what those types contain.
    pub related_types: Vec<ContextLink>,
    /// Every use of the target in the compilation; the declaration itself is `target.source`.
    pub references: Vec<SourceSpan>,
    pub callers: Calls,
    pub callees: Calls,
    /// The compiler's diagnostics whose primary span is inside the declaration.
    pub diagnostics: Vec<ContextDiagnostic>,
    /// A function's effects (N36); `None` for a record or an enum, which has none to have.
    pub effects: Option<ContextEffects>,
    /// A function's provenance summary (N38); `None` for a record or an enum.
    pub provenance: Option<ContextProvenance>,
    pub availability: Availability,
}

/// Where a function's result comes from, as the whole program's solve settled it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextProvenance {
    /// Origins its result carries whatever it is given, by name in origin-id order.
    pub origins: Vec<&'static str>,
    /// The parameters whose provenance its result carries, by name, in parameter order.
    pub passes_on: Vec<String>,
    /// The parameters whose value may become the path `write_file` writes to.
    pub reaches_write_path: Vec<String>,
    /// For each parameter, in order, the origins the program's calls and `spawn`s hand it.
    pub receives: Vec<ContextReceives>,
}

/// What one parameter is handed, over every call site in the program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextReceives {
    pub param: String,
    pub origins: Vec<&'static str>,
}

/// What a function may do, from the effect checker: every set by name, in effect-id order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextEffects {
    /// Its `! { … }`, or `None` where it declares none.
    pub declared: Option<Vec<&'static str>>,
    /// What its body requires: the least fixed point over the compilation's calls.
    pub required: Vec<&'static str>,
    /// What a caller in another module must assume: the declared set, or every effect.
    pub published: Vec<&'static str>,
    /// For each required effect, the first thing in the body that requires it.
    pub reasons: Vec<ContextEffectReason>,
}

/// Why a function requires one effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextEffectReason {
    pub effect: &'static str,
    /// The call or `spawn` in the body, as a source range.
    pub at: Option<SourceSpan>,
    /// `builtin`, `spawn`, `inferred`, `declared` or `undeclared`.
    pub source: &'static str,
    /// The built-in's name, for `builtin`: it is the compiler's, and has no source.
    pub builtin: Option<&'static str>,
    /// The callee, for `inferred`, `declared` and `undeclared`.
    pub callee: Option<Identity>,
}

/// The definition the packet is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextTarget {
    pub identity: Identity,
    /// `function`, `record` or `enum`.
    pub kind: &'static str,
    pub name: String,
    /// A function's signature, as the shared renderer writes it; `None` otherwise.
    pub signature: Option<String>,
    /// A record's fields, in canonical order; `None` otherwise.
    pub fields: Option<Vec<ContextField>>,
    /// An enum's variants and their payloads, in canonical order; `None` otherwise.
    pub variants: Option<Vec<ContextVariant>>,
    pub source: SourceFragment,
}

/// A durable name for an entity: the definition's key and, for a field, variant or payload
/// field, which member of it. `definition` is `None` when the module has no durable key.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Identity {
    pub definition: Option<String>,
    pub member: Option<String>,
}

/// A field or payload field: its name and its type as the definition has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
}

/// A variant and its payload fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextVariant {
    pub name: String,
    pub fields: Vec<ContextField>,
}

/// The target's exact source text, and where it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceFragment {
    pub file: String,
    pub start: u32,
    pub end: u32,
    pub name_start: u32,
    pub name_end: u32,
    pub text: String,
}

/// A byte range in a file, by its path relative to the source root.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct SourceSpan {
    pub file: String,
    pub start: u32,
    pub end: u32,
}

/// A byte range in the target's own file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Occurrence {
    pub start: u32,
    pub end: u32,
}

/// A linked entity, as compactly as it can be asked for again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextLink {
    pub identity: Identity,
    /// `function`, `record`, `enum`, `field`, `variant` or `payload_field`.
    pub kind: &'static str,
    pub name: String,
    /// Where its name is declared — always a file of the compilation.
    pub declaration: SourceSpan,
    /// Where the target's declaration mentions it, in the target's file.
    pub occurrences: Vec<Occurrence>,
}

/// Callers or callees: `available` with every direct call, or `not_applicable` — a record's
/// or an enum's — with no list at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Calls {
    pub status: &'static str,
    pub items: Option<Vec<ContextCall>>,
}

/// A function at the other end of direct calls, and each call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextCall {
    pub identity: Identity,
    pub name: String,
    pub declaration: SourceSpan,
    /// Each call's callee name, in the calling function's file.
    pub calls: Vec<SourceSpan>,
}

/// A diagnostic inside the declaration, compactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextDiagnostic {
    pub code: String,
    pub severity: &'static str,
    pub start: u32,
    pub end: u32,
    /// How many fixes the compiler attached.
    pub fixes: usize,
}

/// The sections the compiler has no facts for: each is `unsupported`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Availability {
    pub capabilities: &'static str,
    pub tests: &'static str,
    pub semantic_changes: &'static str,
    pub semantic_delta: &'static str,
}

impl Availability {
    const UNSUPPORTED: Self = Self {
        capabilities: "unsupported",
        tests: "unsupported",
        semantic_changes: "unsupported",
        semantic_delta: "unsupported",
    };
}

/// Why there is no packet, as `nazm.context/3` serialises it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unavailable {
    pub schema: &'static str,
    /// Always `unavailable`.
    pub status: &'static str,
    pub reason: NoContext,
}

/// Why there is no packet. There is never a nearby or a previous one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoContext {
    /// The document is not open at that version.
    NotCurrent,
    /// The file is not one the compilation loaded.
    NotInCompilation,
    /// The offset is not inside the file.
    OutOfRange,
    /// The offset is not on a name.
    NotAName,
    /// The name is not a function, record or enum declared from source.
    NotATarget,
    /// Some file of the compilation needed syntax recovery, or a module could not be
    /// loaded: what was lost could be a use, a caller or a declaration.
    IncompleteCompilation,
}

impl NoContext {
    /// This reason as an unavailable result.
    #[must_use]
    pub const fn result(self) -> ContextResult {
        ContextResult::Unavailable(Unavailable {
            schema: SCHEMA,
            status: "unavailable",
            reason: self,
        })
    }
}

impl ContextResult {
    /// The one canonical serialisation: compact JSON, fields in declaration order.
    ///
    /// # Panics
    ///
    /// Never: every field is a string, a number, a list or an option of them.
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a packet is plain data")
    }
}

impl LanguageService {
    /// The context packet of the definition declared or used at byte `offset` of `file`, a
    /// file of the compilation rooted at the open document `root`, at `version`.
    #[must_use]
    pub fn context(&self, root: &Path, version: i32, file: &Path, offset: u32) -> ContextResult {
        match self.packet(root, version, file, offset) {
            Ok(p) => ContextResult::Available(Box::new(p)),
            Err(why) => why.result(),
        }
    }

    fn packet(
        &self,
        root: &Path,
        version: i32,
        file: &Path,
        offset: u32,
    ) -> Result<ContextPacket, NoContext> {
        let s = self.current(root, version).ok_or(NoContext::NotCurrent)?;
        // The file, among those the compilation loaded — by identity, never by reading it.
        let wanted = FileKey::Path(identity(file));
        let f = s
            .files
            .iter()
            .position(|k| *k == wanted)
            .map(FileId::from_index)
            .ok_or(NoContext::NotInCompilation)?;
        if incomplete(s) {
            return Err(NoContext::IncompleteCompilation);
        }
        let text = s.texts.get(f.index()).ok_or(NoContext::NotInCompilation)?;
        if offset as usize > text.len() {
            return Err(NoContext::OutOfRange);
        }
        let cst = nazm_core::__syntax::parse_cst(f, text).cst;
        let at = name_at(&cst, offset).ok_or(NoContext::NotAName)?;
        let entity = s.index.target(at).ok_or(NoContext::NotATarget)?;
        let (symbol_file, symbol) = target_symbol(s, entity).ok_or(NoContext::NotATarget)?;
        let paths = Paths::of(s, root, self.source_root.as_deref());
        let res = &s.analysis.resolution;
        let decl = Span::new(symbol_file, symbol.start, symbol.end);
        let source_text = s
            .texts
            .get(symbol_file.index())
            .and_then(|t| t.get(symbol.start as usize..symbol.end as usize))
            .ok_or(NoContext::NotATarget)?
            .to_owned();
        let target_file = paths.file(symbol_file).ok_or(NoContext::NotATarget)?;

        let (signature, fields, variants) = shape(res, entity);
        let target = ContextTarget {
            identity: identity_of(s, entity),
            kind: symbol.kind_word(),
            name: symbol.name.clone(),
            signature,
            fields,
            variants,
            source: SourceFragment {
                file: target_file,
                start: symbol.start,
                end: symbol.end,
                name_start: symbol.name_start,
                name_end: symbol.name_end,
                text: source_text,
            },
        };

        // One pass over the index: every use inside the declaration, grouped by the entity
        // it is a use of. Request-local, and dropped with the packet.
        let mut used: BTreeMap<LinkKey, (ReferenceTarget, Vec<Occurrence>, Vec<Occurrence>)> =
            BTreeMap::new();
        for (t, uses) in s.index.iter() {
            if t == entity || matches!(t, ReferenceTarget::Local(_)) {
                continue;
            }
            for u in uses {
                if u.file != decl.file || u.start < decl.start || u.end > decl.end {
                    continue;
                }
                let key = link_key(s, &paths, t);
                let entry = used
                    .entry(key)
                    .or_insert_with(|| (t, Vec::new(), Vec::new()));
                let o = Occurrence {
                    start: u.start,
                    end: u.end,
                };
                entry.1.push(o);
                if res.type_name(*u).is_some() {
                    entry.2.push(o);
                }
            }
        }
        let mut dependencies = Vec::new();
        let mut related_types = Vec::new();
        for (t, mut all, mut typed) in used.into_values() {
            all.sort_unstable();
            all.dedup();
            typed.sort_unstable();
            typed.dedup();
            if !typed.is_empty()
                && matches!(t, ReferenceTarget::Record(_) | ReferenceTarget::Enum(_))
            {
                related_types.extend(link(s, &paths, t, typed));
            }
            dependencies.extend(link(s, &paths, t, all));
        }

        let mut references: Vec<SourceSpan> = s
            .index
            .uses(entity)
            .iter()
            .filter_map(|u| paths.span(*u))
            .collect();
        references.sort();
        references.dedup();

        let (callers, callees) = match entity {
            ReferenceTarget::Function(_) => {
                let locator = Locator {
                    root: identity(root),
                    version,
                    generation: s.generation,
                    file: match s.files.get(symbol_file.index()) {
                        Some(FileKey::Path(p)) => p.clone(),
                        _ => return Err(NoContext::NotATarget),
                    },
                    name_start: symbol.name_start,
                    name_end: symbol.name_end,
                };
                let incoming = self
                    .incoming_calls(&locator)
                    .map_err(|_| NoContext::NotCurrent)?;
                let outgoing = self
                    .outgoing_calls(&locator)
                    .map_err(|_| NoContext::NotCurrent)?;
                (
                    Calls::available(
                        incoming
                            .iter()
                            .filter_map(|c| call(s, &paths, &c.from, &c.calls))
                            .collect(),
                    ),
                    Calls::available(
                        outgoing
                            .iter()
                            .filter_map(|c| call(s, &paths, &c.to, &c.calls))
                            .collect(),
                    ),
                )
            }
            _ => (Calls::NOT_APPLICABLE, Calls::NOT_APPLICABLE),
        };

        let mut diagnostics: Vec<ContextDiagnostic> = s
            .analysis
            .diagnostics
            .iter()
            .filter(|d| {
                let p = d.primary.span;
                p.file == decl.file && decl.start <= p.start && p.end <= decl.end
            })
            .map(|d| ContextDiagnostic {
                code: d.code.to_string(),
                severity: match d.severity {
                    nazm_diag::Severity::Error => "error",
                },
                start: d.primary.span.start,
                end: d.primary.span.end,
                fixes: d.fixes.len(),
            })
            .collect();
        diagnostics.sort_by(|a, b| (a.start, a.end, &a.code).cmp(&(b.start, b.end, &b.code)));

        Ok(ContextPacket {
            schema: SCHEMA,
            status: "available",
            root: paths.file(FileId::ROOT).unwrap_or_default(),
            target,
            dependencies,
            related_types,
            references,
            callers,
            callees,
            diagnostics,
            effects: match entity {
                ReferenceTarget::Function(id) => effects_of(s, &paths, id),
                _ => None,
            },
            provenance: match entity {
                ReferenceTarget::Function(id) => provenance_of(s, id),
                _ => None,
            },
            availability: Availability::UNSUPPORTED,
        })
    }
}

/// A function's provenance summary, or `None` for one whose body was not checked.
fn provenance_of(s: &Snapshot, id: nazm_core::sema::FnId) -> Option<ContextProvenance> {
    let f = s.analysis.resolution.function(id)?;
    let summary = f.provenance.as_ref()?;
    let names = |ps: &[u32]| -> Vec<String> {
        ps.iter()
            .filter_map(|p| f.locals.get(*p as usize).map(|l| l.name.clone()))
            .collect()
    };
    Some(ContextProvenance {
        origins: summary.origins.names(),
        passes_on: names(&summary.params),
        reaches_write_path: names(&summary.reaches),
        receives: summary
            .receives
            .iter()
            .enumerate()
            .filter_map(|(i, o)| {
                f.locals.get(i).map(|l| ContextReceives {
                    param: l.name.clone(),
                    origins: o.names(),
                })
            })
            .collect(),
    })
}

/// A function's effects as the checker settled them, or `None` for one whose body was not
/// checked in this compilation.
fn effects_of(s: &Snapshot, paths: &Paths, id: nazm_core::sema::FnId) -> Option<ContextEffects> {
    use nazm_core::sema::{EffectSet, EffectSource};
    let res = &s.analysis.resolution;
    let f = res.function(id)?;
    let required = f.inferred_effects?;
    // An undeclared function publishes what its callers are held to assume of it: every
    // effect the compatibility bridge covers, and `foreign` only where it can be handed a
    // `ForeignCap` (N42) — the checker's own rule, so the two cannot disagree.
    let published = f
        .declared_effects
        .unwrap_or_else(|| nazm_core::assumed_effects(id, res));
    let reasons = f
        .effect_reasons
        .iter()
        .map(|r| {
            let (source, builtin, callee) = match r.source {
                EffectSource::Builtin(b) => ("builtin", Some(b.name()), None),
                EffectSource::Spawn => ("spawn", None, None),
                EffectSource::Inferred(g) => ("inferred", None, Some(g)),
                EffectSource::Declared(g) => ("declared", None, Some(g)),
                EffectSource::Undeclared(g) => ("undeclared", None, Some(g)),
                EffectSource::Indirect(_) => ("indirect", None, None),
            };
            ContextEffectReason {
                effect: r.effect.name(),
                at: paths.span(r.at),
                source,
                builtin,
                callee: callee.map(|g| identity_of(s, ReferenceTarget::Function(g))),
            }
        })
        .collect();
    Some(ContextEffects {
        declared: f.declared_effects.map(EffectSet::names),
        required: required.names(),
        published: published.names(),
        reasons,
    })
}

impl Calls {
    const NOT_APPLICABLE: Self = Self {
        status: "not_applicable",
        items: None,
    };

    const fn available(items: Vec<ContextCall>) -> Self {
        Self {
            status: "available",
            items: Some(items),
        }
    }
}

impl crate::symbols::Symbol {
    const fn kind_word(&self) -> &'static str {
        match self.kind {
            SymbolKind::Function => "function",
            SymbolKind::Record => "record",
            SymbolKind::Enum => "enum",
            SymbolKind::Variant => "variant",
            SymbolKind::Field => "field",
            SymbolKind::PayloadField => "payload_field",
        }
    }
}

/// Whether some file of the compilation needed syntax recovery, or a module could not be
/// loaded — so what was lost could be a use, a caller or a declaration. Such a compilation
/// has no packet, and no semantic snapshot (N28).
pub(crate) fn incomplete(s: &Snapshot) -> bool {
    !s.analysis.syntax_errors.is_empty()
        || s.analysis
            .diagnostics
            .iter()
            .any(|d| d.code == nazm_diag::codes::MODULE_NOT_FOUND)
}

/// The outline symbol of a function, record or enum target, and the file it is in — `None`
/// for anything else, or a definition with no source an editor can open.
fn target_symbol(
    s: &Snapshot,
    entity: ReferenceTarget,
) -> Option<(FileId, crate::symbols::Symbol)> {
    if !matches!(
        entity,
        ReferenceTarget::Function(_) | ReferenceTarget::Record(_) | ReferenceTarget::Enum(_)
    ) {
        return None;
    }
    let name = s.index.declaration(entity)?;
    if !matches!(s.files.get(name.file.index())?, FileKey::Path(_)) {
        return None;
    }
    let symbol = crate::symbols::declared_in(s, name.file)
        .into_iter()
        .find(|sym| sym.target == entity)?;
    Some((name.file, symbol))
}

/// A function's signature, a record's fields or an enum's variants, by the renderers other
/// answers use.
pub(crate) fn shape(
    res: &nazm_core::Resolution,
    entity: ReferenceTarget,
) -> (
    Option<String>,
    Option<Vec<ContextField>>,
    Option<Vec<ContextVariant>>,
) {
    let field = |m: crate::structure::Member| ContextField {
        name: m.name,
        ty: m.ty.map(|t| res.types().show(t)).unwrap_or_default(),
    };
    match entity {
        ReferenceTarget::Function(id) => (
            crate::Signature::of_function(res, id).map(|sig| sig.render(res, None).label),
            None,
            None,
        ),
        ReferenceTarget::Record(record) => (
            None,
            Some(
                members(res, StructureSite::Member { record })
                    .into_iter()
                    .map(field)
                    .collect(),
            ),
            None,
        ),
        ReferenceTarget::Enum(enumeration) => (
            None,
            None,
            Some(
                members(res, StructureSite::Variant { enumeration })
                    .into_iter()
                    .filter_map(|v| {
                        let ReferenceTarget::Variant(variant) = v.target else {
                            return None;
                        };
                        Some(ContextVariant {
                            name: v.name,
                            fields: members(
                                res,
                                StructureSite::PayloadLabel {
                                    variant,
                                    pattern: false,
                                },
                            )
                            .into_iter()
                            .map(field)
                            .collect(),
                        })
                    })
                    .collect(),
            ),
        ),
        _ => (None, None, None),
    }
}

/// The durable identity of an entity: its definition's key, and which member.
pub(crate) fn identity_of(s: &Snapshot, t: ReferenceTarget) -> Identity {
    let t_kind = t;
    let res = &s.analysis.resolution;
    let module_key = |m| s.graph.key(m).cloned();
    let key = |d: Option<DefKey>| d.map(|k| k.to_string());
    let record = |id| {
        let def = res.records().get(id)?;
        let module: ModuleKey = module_key(def.module)?;
        res.record_key(id, &module)
    };
    let enumeration = |id| {
        let def = res.enums().get(id)?;
        let module: ModuleKey = module_key(def.module)?;
        res.enum_key(id, &module)
    };
    let variant_name = |e, v: u32| {
        res.enums()
            .get(e)
            .and_then(|d| d.variants.get(v as usize))
            .map(|v| v.name.clone())
    };
    match t {
        ReferenceTarget::Function(id) => Identity {
            definition: key(res
                .function(id)
                .and_then(|f| module_key(f.module))
                .and_then(|m| res.def_key(id, &m))),
            member: None,
        },
        ReferenceTarget::Record(id) => Identity {
            definition: key(record(id)),
            member: None,
        },
        ReferenceTarget::Enum(id) => Identity {
            definition: key(enumeration(id)),
            member: None,
        },
        ReferenceTarget::Field(f) => Identity {
            definition: key(record(f.record)),
            member: res
                .records()
                .get(f.record)
                .and_then(|d| d.fields.get(f.field as usize))
                .map(|x| x.name.clone()),
        },
        ReferenceTarget::Variant(v) => Identity {
            definition: key(enumeration(v.enumeration)),
            member: variant_name(v.enumeration, v.variant),
        },
        ReferenceTarget::PayloadField(p) => Identity {
            definition: key(enumeration(p.enumeration)),
            member: variant_name(p.enumeration, p.variant).and_then(|v| {
                let field = res
                    .enums()
                    .get(p.enumeration)?
                    .variants
                    .get(p.variant as usize)?
                    .fields
                    .get(p.field as usize)?
                    .name
                    .clone();
                Some(format!("{v}.{field}"))
            }),
        },
        ReferenceTarget::Local(_) => Identity {
            definition: None,
            member: None,
        },
        // N78: a trait by its key; a trait method as its trait's key and its name.
        ReferenceTarget::Trait(t) | ReferenceTarget::TraitMethod(t, _) => {
            let d = res.traits().get(t);
            let definition = d.and_then(|d| {
                let module: ModuleKey = module_key(d.module)?;
                Some(DefKey::new(module, DefKind::Trait, d.name.clone()).to_string())
            });
            let member = match t_kind {
                ReferenceTarget::TraitMethod(_, m) => d
                    .and_then(|d| d.methods.get(m as usize))
                    .map(|m| m.name.clone()),
                _ => None,
            };
            Identity { definition, member }
        }
    }
}

/// How links are ordered and grouped: kind, then where the entity is declared, then its
/// identity — never its name alone.
type LinkKey = (u8, Option<SourceSpan>, Identity);

fn link_key(s: &Snapshot, paths: &Paths, t: ReferenceTarget) -> LinkKey {
    let rank = match t {
        ReferenceTarget::Function(_) => 0,
        ReferenceTarget::Record(_) => 1,
        ReferenceTarget::Enum(_) => 2,
        ReferenceTarget::Field(_) => 3,
        ReferenceTarget::Variant(_) => 4,
        ReferenceTarget::PayloadField(_) => 5,
        ReferenceTarget::Local(_) => 6,
        ReferenceTarget::Trait(_) => 7,
        ReferenceTarget::TraitMethod(..) => 8,
    };
    let declaration = s.index.declaration(t).and_then(|d| paths.span(d));
    (rank, declaration, identity_of(s, t))
}

fn link(
    s: &Snapshot,
    paths: &Paths,
    t: ReferenceTarget,
    occurrences: Vec<Occurrence>,
) -> Option<ContextLink> {
    // A link is a place to ask about next, so an entity with no declaration in a file of the
    // compilation — the core prelude's — is no link: it shaped the checking, and the packet
    // does not pretend it has a source.
    let declaration = s.index.declaration(t).and_then(|d| paths.span(d))?;
    let identity = identity_of(s, t);
    let name = identity.member.clone().unwrap_or_else(|| name_of(s, t));
    Some(ContextLink {
        kind: match t {
            ReferenceTarget::Function(_) => "function",
            ReferenceTarget::Record(_) => "record",
            ReferenceTarget::Enum(_) => "enum",
            ReferenceTarget::Field(_) => "field",
            ReferenceTarget::Variant(_) => "variant",
            ReferenceTarget::PayloadField(_) => "payload_field",
            ReferenceTarget::Local(_) => "local",
            ReferenceTarget::Trait(_) => "trait",
            ReferenceTarget::TraitMethod(..) => "trait_method",
        },
        name,
        declaration,
        identity,
        occurrences,
    })
}

/// A definition's declared name, read from the definition tables.
fn name_of(s: &Snapshot, t: ReferenceTarget) -> String {
    let res = &s.analysis.resolution;
    match t {
        ReferenceTarget::Function(id) => res.function(id).map(|f| f.name.clone()),
        ReferenceTarget::Record(id) => res.records().get(id).map(|r| r.name.clone()),
        ReferenceTarget::Enum(id) => res.enums().get(id).map(|e| e.name.clone()),
        _ => None,
    }
    .unwrap_or_default()
}

/// A function at the other end of calls, as a packet links it.
fn call(
    s: &Snapshot,
    paths: &Paths,
    f: &HierarchyFunction,
    calls: &[Location],
) -> Option<ContextCall> {
    let file = s
        .files
        .iter()
        .position(|k| *k == FileKey::Path(f.locator.file.clone()))
        .map(FileId::from_index)?;
    let at = Span::new(file, f.locator.name_start, f.locator.name_end);
    let id = match s.index.target(at)? {
        ReferenceTarget::Function(id) => id,
        _ => return None,
    };
    Some(ContextCall {
        identity: identity_of(s, ReferenceTarget::Function(id)),
        name: f.name.clone(),
        declaration: paths.span(at)?,
        calls: calls.iter().filter_map(|l| paths.location(s, l)).collect(),
    })
}

/// Paths relative to the compilation's source root, for every file it loaded.
pub(crate) struct Paths {
    files: Vec<Option<String>>,
}

impl Paths {
    pub(crate) fn of(s: &Snapshot, root: &Path, source_root: Option<&Path>) -> Self {
        let base = crate::root::decide(root, source_root)
            .ok()
            .map(|r| identity(r.path()));
        let files = s
            .files
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let FileKey::Path(p) = k else { return None };
                // The module's durable key where it has one: exactly the source-root-relative
                // spelling. Otherwise the relative path from the source root.
                let module = s.graph.module_of(FileId::from_index(i));
                if let Some(key) = module.and_then(|m| s.graph.key(m)) {
                    return Some(key.as_str().to_owned());
                }
                base.as_ref().and_then(|b| relative(b, p))
            })
            .collect();
        Self { files }
    }

    pub(crate) fn file(&self, f: FileId) -> Option<String> {
        self.files.get(f.index()).cloned().flatten()
    }

    pub(crate) fn span(&self, at: Span) -> Option<SourceSpan> {
        Some(SourceSpan {
            file: self.file(at.file)?,
            start: at.start,
            end: at.end,
        })
    }

    fn location(&self, s: &Snapshot, l: &Location) -> Option<SourceSpan> {
        let key = FileKey::Path(identity(&l.path));
        let f = s.files.iter().position(|k| *k == key)?;
        self.span(Span::new(FileId::from_index(f), l.start, l.end))
    }
}

/// `path` relative to `base`, with `..` where it is not under it; `/`-separated.
fn relative(base: &Path, path: &Path) -> Option<String> {
    let b: Vec<Component<'_>> = base.components().collect();
    let p: Vec<Component<'_>> = path.components().collect();
    let common = b.iter().zip(&p).take_while(|(x, y)| x == y).count();
    if common == 0 {
        return None;
    }
    let mut out = PathBuf::new();
    for _ in common..b.len() {
        out.push("..");
    }
    for c in p.iter().skip(common) {
        out.push(c.as_os_str());
    }
    let text = out.to_str()?.replace('\\', "/");
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_outside_the_root_is_relative_with_parents() {
        assert_eq!(
            relative(Path::new("/a/b"), Path::new("/a/b/c/d.nz")).as_deref(),
            Some("c/d.nz")
        );
        assert_eq!(
            relative(Path::new("/a/b"), Path::new("/a/x.nz")).as_deref(),
            Some("../x.nz")
        );
    }

    /// The structural half of "only what the compiler knows": this module never looks a
    /// definition up by its spelling, never reads a file, scans a directory, asks git, or
    /// stamps a time, a random value or a process id into a packet.
    #[test]
    fn a_packet_is_never_found_by_spelling_or_from_the_environment() {
        let source = include_str!("context.rs");
        let code: String = source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or_default()
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                !t.starts_with("//") && !t.starts_with("///")
            })
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in [
            ".definition(",
            "by_module",
            "read_to_string",
            "read_dir",
            "fs::",
            "Disk",
            "Command",
            "SystemTime",
            "Instant",
            "process::id",
            "rand",
            "HashMap",
            "HashSet",
            "git",
            "find(\"",
            "contains(\"",
        ] {
            assert!(
                !code.contains(forbidden),
                "`{forbidden}` in context: a packet would not be the compiler's"
            );
        }
    }
}
