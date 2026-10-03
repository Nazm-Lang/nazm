//! Applied types: interning, substitution, and the instances a compilation creates.
//!
//! # Why an applied record is an entry of its own
//!
//! `Box[Int]` has a field `value: Int`, and `Box[Str]` has `value: Str`. Everything that
//! reads a record — copy and drop derivation, layout, task safety, construction, field
//! projection — reads *fields*, and each of those consumers was written, tested and
//! mutation-tested against a table entry with concrete fields. So an application is
//! **interned as an entry of that table** whose fields are the definition's after
//! substitution, done once, here. The consumers do not learn that generics exist, and
//! there is no second copy of any of them that does.
//!
//! What makes that sound is interning: one definition applied to one argument list is one
//! id, so `==` on [`Type`] stays the language's type equality. Two modules that write
//! `Pair[Int, Str]` for the same `Pair` get the same id; two `Pair` definitions do not.
//!
//! # Completion is deferred, and iterative
//!
//! An instance's fields come from its definition's, and while field types are being
//! resolved the definition may not have any yet — a field may name a generic type declared
//! later, or in another module. So an instance is interned at once and **completed later**,
//! from a worklist, when [`UserTypes::settle`] is called. Completion substitutes, which can
//! intern more instances, which join the same worklist: no step recurses into another
//! completion, so nothing here puts the depth of a type family on the host's stack.
//!
//! A definition that expands without bound — `Strange[T] { next: Vec[Strange[Vec[T]]] }` —
//! is refused by the ownership rule (`N0359`) and would otherwise make this list grow for
//! ever. It cannot: an instance nested deeper than [`HARD_NESTING`] is interned and never
//! completed, and there is a total [`INSTANCE_BUDGET`]. Both are the last line, not the
//! rule; the rule is the cycle check, which reports first.

use std::collections::HashMap;

use crate::enums::EnumId;
use crate::record::RecordId;
use crate::types::{ChanId, FnSignature, FnTypeId, ParamOwner, Type, TypeParam, VecId};
use crate::udt::{TypeNode, UserTypes};

/// Which definition an applied entry applies, and to what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance<Id> {
    pub template: Id,
    pub args: Vec<Type>,
}

/// How deeply one type term may nest before an instance of it is left uncompleted.
///
/// Well past [`crate::MAX_TYPE_DEPTH`], which is the language's bound and is reported; this
/// one exists only so that a refused definition cannot keep the checker busy.
pub const HARD_NESTING: usize = 256;

/// How many applied entries one compilation may create before completion stops.
pub const INSTANCE_BUDGET: usize = 100_000;

/// The generic state of one compilation's types.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Generics {
    /// `Vec` element types, by [`VecId`].
    vecs: Vec<Type>,
    vec_ids: HashMap<Type, VecId>,
    /// Generic channel element types, by [`ChanId`] (N53).
    chans: Vec<Type>,
    chan_ids: HashMap<Type, ChanId>,
    /// Function types, by [`FnTypeId`] (N50).
    fns: Vec<FnSignature>,
    fn_ids: HashMap<FnSignature, FnTypeId>,
    /// Instances interned and not yet given their fields.
    pending: Vec<TypeNode>,
    /// While set, completion waits: field types are still being resolved.
    deferred: bool,
    /// While set, the worklist is being drained, and an instance interned by a completion
    /// joins it rather than starting a second drain inside the first.
    settling: bool,
    /// Every type parameter's name, by owner, for messages only.
    names: HashMap<ParamOwner, Vec<String>>,
    /// Every effect parameter's name, by owner, for messages only (N51).
    effect_names: HashMap<ParamOwner, String>,
    /// Instances interned in total, against [`INSTANCE_BUDGET`].
    created: usize,
    /// The type parameters whose declaration requires equality — `T` in
    /// `fn same[T: Equality]`. N14. Keyed by the parameter itself, its owner and position,
    /// never by its name: a requirement belongs to the position, as the parameter does.
    ///
    /// This is the whole of the assumption environment. A parameter appears only inside its
    /// own declaration, so one set for the compilation answers "what may this body assume
    /// about `T`" for every body at once.
    equality: std::collections::BTreeSet<TypeParam>,
}

impl UserTypes {
    /// `Vec[elem]`, interned.
    pub fn vec_of(&mut self, elem: Type) -> Type {
        let g = self.generics_mut();
        if let Some(id) = g.vec_ids.get(&elem) {
            return Type::Vec(*id);
        }
        let id = VecId::from_index(g.vecs.len());
        g.vecs.push(elem);
        g.vec_ids.insert(elem, id);
        Type::Vec(id)
    }

    /// `fn(params) -> ret ! {effects}`, interned (N50). A signature whose effects name no
    /// effect parameter has no `row`, whatever it was given (N51), so one type is one id.
    pub fn fn_of(&mut self, mut sig: FnSignature) -> Type {
        if !sig.effects.has_param() {
            sig.row = None;
        }
        let g = self.generics_mut();
        if let Some(id) = g.fn_ids.get(&sig) {
            return Type::Fn(*id);
        }
        let id = FnTypeId::from_index(g.fns.len());
        g.fns.push(sig.clone());
        g.fn_ids.insert(sig, id);
        Type::Fn(id)
    }

    /// What a function type is (N50).
    #[must_use]
    pub fn fn_signature(&self, id: FnTypeId) -> Option<&FnSignature> {
        self.generics().fns.get(id.index())
    }

    /// `Chan[elem]`, interned (N53).
    pub fn chan_of(&mut self, elem: Type) -> Type {
        let g = self.generics_mut();
        if let Some(id) = g.chan_ids.get(&elem) {
            return Type::ChanOf(*id);
        }
        let id = ChanId::from_index(g.chans.len());
        g.chans.push(elem);
        g.chan_ids.insert(elem, id);
        Type::ChanOf(id)
    }

    /// `Chan[elem]` if it has been interned already. Never creates one.
    #[must_use]
    pub fn find_chan(&self, elem: Type) -> Option<Type> {
        self.generics()
            .chan_ids
            .get(&elem)
            .map(|id| Type::ChanOf(*id))
    }

    /// The element type of a generic channel (N53).
    #[must_use]
    pub fn chan_element(&self, id: ChanId) -> Option<Type> {
        self.generics().chans.get(id.index()).copied()
    }

    /// `Vec[elem]` if it has been interned already. Never creates one.
    #[must_use]
    pub fn find_vec(&self, elem: Type) -> Option<Type> {
        self.generics().vec_ids.get(&elem).map(|id| Type::Vec(*id))
    }

    /// The element type of a `Vec`.
    #[must_use]
    pub fn vec_element(&self, id: VecId) -> Option<Type> {
        self.generics().vecs.get(id.index()).copied()
    }

    /// How many `Vec` types this compilation has interned.
    #[must_use]
    pub fn vec_count(&self) -> usize {
        self.generics().vecs.len()
    }

    /// Record a function's effect parameter's name, for messages (N51).
    pub fn name_effect_param(&mut self, owner: ParamOwner, name: String) {
        self.generics_mut().effect_names.insert(owner, name);
    }

    /// An effect parameter's name as written, or `*` where none was recorded.
    #[must_use]
    pub fn effect_param_name(&self, owner: Option<ParamOwner>) -> String {
        owner
            .and_then(|o| self.generics().effect_names.get(&o).cloned())
            .unwrap_or_else(|| "*".to_owned())
    }

    /// `t` with `owner`'s effect parameter, wherever a function type in it names it, bound to
    /// `binding` (N51). A binding that itself names an effect parameter names `caller`'s:
    /// the only parameter a binding can carry is the one in scope where the call is.
    pub fn bind_effects(
        &mut self,
        t: Type,
        owner: ParamOwner,
        binding: crate::EffectSet,
        caller: Option<ParamOwner>,
    ) -> Type {
        let Type::Fn(f) = t else {
            return t;
        };
        let Some(sig) = self.fn_signature(f).cloned() else {
            return t;
        };
        let params = sig
            .params
            .iter()
            .map(|p| self.bind_effects(*p, owner, binding, caller))
            .collect();
        let ret = self.bind_effects(sig.ret, owner, binding, caller);
        let (effects, row) = if sig.row == Some(owner) {
            let effects = sig.effects.bound(binding);
            (effects, binding.has_param().then_some(caller).flatten())
        } else {
            (sig.effects, sig.row)
        };
        self.fn_of(FnSignature {
            params,
            ret,
            effects,
            row,
        })
    }

    /// Whether a function type in `t` names `owner`'s effect parameter (N51).
    #[must_use]
    pub fn names_effect_param(&self, t: Type, owner: ParamOwner) -> bool {
        let mut work = vec![t];
        while let Some(t) = work.pop() {
            if let Type::Fn(f) = t {
                if let Some(sig) = self.fn_signature(f) {
                    if sig.row == Some(owner) {
                        return true;
                    }
                    work.extend(sig.params.iter().copied());
                    work.push(sig.ret);
                }
            }
        }
        false
    }

    /// Whether two types are one up to the effects of the function types in them (N51):
    /// what MIR compares, since effects mean nothing below the checker and an effect
    /// parameter makes no instance.
    #[must_use]
    pub fn same_shape(&self, a: Type, b: Type) -> bool {
        if a == b {
            return true;
        }
        match (a, b) {
            (Type::Fn(x), Type::Fn(y)) => match (self.fn_signature(x), self.fn_signature(y)) {
                (Some(p), Some(q)) => {
                    p.params.len() == q.params.len()
                        && p.params
                            .iter()
                            .zip(&q.params)
                            .all(|(a, b)| self.same_shape(*a, *b))
                        && self.same_shape(p.ret, q.ret)
                }
                _ => false,
            },
            // A capability of any kind is one word below the checker (N79): which authority
            // it is, is the checker's question, settled before Core IR.
            (Type::Cap(_), Type::Cap(_)) => true,
            _ => false,
        }
    }

    /// Record the names of one declaration's type parameters, for messages.
    pub fn name_params(&mut self, owner: ParamOwner, names: Vec<String>) {
        self.generics_mut().names.insert(owner, names);
    }

    /// Record that a type parameter's declaration requires equality. N14.
    pub fn require_equality(&mut self, p: TypeParam) {
        self.generics_mut().equality.insert(p);
    }

    /// Whether a type parameter's declaration requires equality — and so whether, inside
    /// that declaration, it has it. N14.
    #[must_use]
    pub fn requires_equality(&self, p: TypeParam) -> bool {
        self.generics().equality.contains(&p)
    }

    /// How many type parameters an owner declared, as recorded by [`Self::name_params`].
    #[must_use]
    pub fn arity(&self, owner: ParamOwner) -> usize {
        self.generics().names.get(&owner).map_or(0, Vec::len)
    }

    /// A parameter's name as written, or its position where none was recorded — a
    /// dependency read from an interface, which persists positions and not names.
    #[must_use]
    pub fn param_name(&self, p: TypeParam) -> String {
        self.generics()
            .names
            .get(&p.owner)
            .and_then(|n| n.get(p.index as usize))
            .cloned()
            .unwrap_or_else(|| format!("type parameter {}", p.index + 1))
    }

    /// Hold completion until [`Self::settle`], while field types are being resolved.
    pub fn defer(&mut self) {
        self.generics_mut().deferred = true;
    }

    /// Complete every pending instance, and every instance completing them creates.
    ///
    /// A worklist, so completion never recurses into completion.
    pub fn settle(&mut self) {
        let g = self.generics_mut();
        g.deferred = false;
        if g.settling {
            return;
        }
        g.settling = true;
        while let Some(node) = self.generics_mut().pending.pop() {
            self.complete_instance(node);
        }
        self.generics_mut().settling = false;
    }

    /// `template[args]` for a record, interned.
    pub fn apply_record(&mut self, template: RecordId, args: Vec<Type>) -> Type {
        if let Some(id) = self.records().instance(template, &args) {
            return Type::Record(id);
        }
        let Some(def) = self.records().get(template).cloned() else {
            return Type::Int;
        };
        let id = self
            .records_mut()
            .declare_instance(crate::record::RecordDef {
                name: def.name.clone(),
                module: def.module,
                visibility: def.visibility,
                name_span: def.name_span,
                fields: Vec::new(),
                layout: Vec::new(),
                decl: def.decl,
                params: Vec::new(),
                instance: Some(Instance { template, args }),
                repr: def.repr,
            });
        self.enqueue(TypeNode::Record(id));
        Type::Record(id)
    }

    /// `template[args]` for an enum, interned.
    pub fn apply_enum(&mut self, template: EnumId, args: Vec<Type>) -> Type {
        if let Some(id) = self.enums().instance(template, &args) {
            return Type::Enum(id);
        }
        let Some(def) = self.enums().get(template).cloned() else {
            return Type::Int;
        };
        let id = self.enums_mut().declare_instance(crate::enums::EnumDef {
            name: def.name.clone(),
            module: def.module,
            visibility: def.visibility,
            name_span: def.name_span,
            variants: Vec::new(),
            order: Vec::new(),
            decl: def.decl,
            params: Vec::new(),
            instance: Some(Instance { template, args }),
        });
        self.enqueue(TypeNode::Enum(id));
        Type::Enum(id)
    }

    fn enqueue(&mut self, node: TypeNode) {
        let g = self.generics_mut();
        g.created += 1;
        g.pending.push(node);
        if !g.deferred && !g.settling {
            self.settle();
        }
    }

    /// Give an instance its fields or variants: the definition's, substituted.
    fn complete_instance(&mut self, node: TypeNode) {
        if self.generics().created > INSTANCE_BUDGET {
            return;
        }
        if self.nesting(node.ty()) > HARD_NESTING {
            return;
        }
        match node {
            TypeNode::Record(id) => {
                let Some(inst) = self.records().get(id).and_then(|d| d.instance.clone()) else {
                    return;
                };
                let Some(template) = self.records().get(inst.template).cloned() else {
                    return;
                };
                let owner = ParamOwner::Record(inst.template);
                let fields = template
                    .fields
                    .iter()
                    .map(|f| {
                        let mut f = f.clone();
                        f.ty = self.substitute(f.ty, owner, &inst.args);
                        f
                    })
                    .collect();
                self.records_mut().complete(id, fields);
            }
            TypeNode::Enum(id) => {
                let Some(inst) = self.enums().get(id).and_then(|d| d.instance.clone()) else {
                    return;
                };
                let Some(template) = self.enums().get(inst.template).cloned() else {
                    return;
                };
                let owner = ParamOwner::Enum(inst.template);
                let variants = template
                    .variants
                    .iter()
                    .map(|v| {
                        let mut v = v.clone();
                        for f in &mut v.fields {
                            f.ty = self.substitute(f.ty, owner, &inst.args);
                        }
                        v
                    })
                    .collect();
                self.enums_mut().complete(id, variants);
            }
        }
    }

    /// `t` with every parameter of `owner` replaced by the matching argument.
    ///
    /// Parameters of any other owner are left alone: substituting a callee's parameters in
    /// its signature must not touch the caller's own `T`, which may be one of the
    /// arguments. Interns whatever it builds.
    pub fn substitute(&mut self, t: Type, owner: ParamOwner, args: &[Type]) -> Type {
        match t {
            Type::Param(p) if p.owner == owner => {
                args.get(p.index as usize).copied().unwrap_or(Type::Int)
            }
            Type::Vec(v) => {
                let Some(elem) = self.vec_element(v) else {
                    return t;
                };
                if !self.mentions(elem, owner) {
                    return t;
                }
                let elem = self.substitute(elem, owner, args);
                self.vec_of(elem)
            }
            Type::ChanOf(c) => {
                let Some(elem) = self.chan_element(c) else {
                    return t;
                };
                if !self.mentions(elem, owner) {
                    return t;
                }
                let elem = self.substitute(elem, owner, args);
                self.chan_of(elem)
            }
            Type::Fn(f) => {
                let Some(sig) = self.fn_signature(f).cloned() else {
                    return t;
                };
                if !self.mentions(t, owner) {
                    return t;
                }
                let params = sig
                    .params
                    .iter()
                    .map(|p| self.substitute(*p, owner, args))
                    .collect();
                let ret = self.substitute(sig.ret, owner, args);
                self.fn_of(FnSignature {
                    params,
                    ret,
                    effects: sig.effects,
                    row: sig.row,
                })
            }
            Type::Record(id) => {
                let Some(inst) = self.records().get(id).and_then(|d| d.instance.clone()) else {
                    return t;
                };
                if !inst.args.iter().any(|a| self.mentions(*a, owner)) {
                    return t;
                }
                let args: Vec<Type> = inst
                    .args
                    .iter()
                    .map(|a| self.substitute(*a, owner, args))
                    .collect();
                self.apply_record(inst.template, args)
            }
            Type::Enum(id) => {
                let Some(inst) = self.enums().get(id).and_then(|d| d.instance.clone()) else {
                    return t;
                };
                if !inst.args.iter().any(|a| self.mentions(*a, owner)) {
                    return t;
                }
                let args: Vec<Type> = inst
                    .args
                    .iter()
                    .map(|a| self.substitute(*a, owner, args))
                    .collect();
                self.apply_enum(inst.template, args)
            }
            other => other,
        }
    }

    /// [`Self::substitute`], answered from what has already been interned. `None` when the
    /// answer would need a type nobody has created — which, for the native backend, means
    /// the instance planner missed one, and is reported as the compiler bug it is.
    #[must_use]
    pub fn substituted(&self, t: Type, owner: ParamOwner, args: &[Type]) -> Option<Type> {
        match t {
            Type::Param(p) if p.owner == owner => args.get(p.index as usize).copied(),
            Type::Vec(v) => {
                let elem = self.vec_element(v)?;
                if !self.mentions(elem, owner) {
                    return Some(t);
                }
                let elem = self.substituted(elem, owner, args)?;
                self.find_vec(elem)
            }
            Type::ChanOf(c) => {
                let elem = self.chan_element(c)?;
                if !self.mentions(elem, owner) {
                    return Some(t);
                }
                let elem = self.substituted(elem, owner, args)?;
                self.find_chan(elem)
            }
            Type::Fn(f) => {
                let sig = self.fn_signature(f)?.clone();
                if !self.mentions(t, owner) {
                    return Some(t);
                }
                let params: Option<Vec<Type>> = sig
                    .params
                    .iter()
                    .map(|p| self.substituted(*p, owner, args))
                    .collect();
                let want = FnSignature {
                    params: params?,
                    ret: self.substituted(sig.ret, owner, args)?,
                    effects: sig.effects,
                    row: sig.row,
                };
                self.generics().fn_ids.get(&want).map(|id| Type::Fn(*id))
            }
            Type::Record(id) => {
                let inst = self.records().get(id)?.instance.clone();
                let Some(inst) = inst else { return Some(t) };
                if !inst.args.iter().any(|a| self.mentions(*a, owner)) {
                    return Some(t);
                }
                let args: Option<Vec<Type>> = inst
                    .args
                    .iter()
                    .map(|a| self.substituted(*a, owner, args))
                    .collect();
                self.records()
                    .instance(inst.template, &args?)
                    .map(Type::Record)
            }
            Type::Enum(id) => {
                let inst = self.enums().get(id)?.instance.clone();
                let Some(inst) = inst else { return Some(t) };
                if !inst.args.iter().any(|a| self.mentions(*a, owner)) {
                    return Some(t);
                }
                let args: Option<Vec<Type>> = inst
                    .args
                    .iter()
                    .map(|a| self.substituted(*a, owner, args))
                    .collect();
                self.enums().instance(inst.template, &args?).map(Type::Enum)
            }
            other => Some(other),
        }
    }

    /// Whether `t` mentions a parameter of `owner`, anywhere.
    #[must_use]
    pub fn mentions(&self, t: Type, owner: ParamOwner) -> bool {
        self.any_param(t, &|p| p.owner == owner)
    }

    /// Whether `t` mentions any type parameter at all — whether it is concrete.
    #[must_use]
    pub fn is_concrete(&self, t: Type) -> bool {
        !self.any_param(t, &|_| true)
    }

    fn any_param(&self, t: Type, want: &dyn Fn(TypeParam) -> bool) -> bool {
        let mut work = vec![t];
        while let Some(t) = work.pop() {
            match t {
                Type::Param(p) => {
                    if want(p) {
                        return true;
                    }
                }
                Type::Vec(v) => work.extend(self.vec_element(v)),
                Type::ChanOf(c) => work.extend(self.chan_element(c)),
                Type::Record(_) | Type::Enum(_) | Type::Fn(_) => {
                    work.extend(self.type_args(t).iter().copied());
                }
                _ => {}
            }
        }
        false
    }

    /// The arguments an applied type was applied to; empty for anything else.
    #[must_use]
    pub fn type_args(&self, t: Type) -> Vec<Type> {
        match t {
            Type::Record(id) => self
                .records()
                .get(id)
                .and_then(|d| d.instance.as_ref())
                .map(|i| i.args.clone())
                .unwrap_or_default(),
            Type::Enum(id) => self
                .enums()
                .get(id)
                .and_then(|d| d.instance.as_ref())
                .map(|i| i.args.clone())
                .unwrap_or_default(),
            Type::Vec(v) => self.vec_element(v).into_iter().collect(),
            Type::ChanOf(c) => self.chan_element(c).into_iter().collect(),
            // A function type's parts: its parameters, then its result (N50).
            Type::Fn(f) => self
                .fn_signature(f)
                .map(|s| s.params.iter().copied().chain([s.ret]).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// The definition an applied type applies, or the type's own node when it is not an
    /// application. `None` for a built-in, a `Vec` or a parameter.
    #[must_use]
    pub fn definition_of(&self, t: Type) -> Option<TypeNode> {
        match t {
            Type::Record(id) => Some(TypeNode::Record(
                self.records()
                    .get(id)
                    .and_then(|d| d.instance.as_ref())
                    .map_or(id, |i| i.template),
            )),
            Type::Enum(id) => Some(TypeNode::Enum(
                self.enums()
                    .get(id)
                    .and_then(|d| d.instance.as_ref())
                    .map_or(id, |i| i.template),
            )),
            _ => None,
        }
    }

    /// How many type parameters a user-defined definition declares.
    #[must_use]
    pub fn declared_params(&self, node: TypeNode) -> usize {
        match node {
            TypeNode::Record(id) => self.records().get(id).map_or(0, |d| d.params.len()),
            TypeNode::Enum(id) => self.enums().get(id).map_or(0, |d| d.params.len()),
        }
    }

    /// How deeply a type term nests: `Int` is 1, `Vec[Int]` is 2, `Pair[Int, Vec[Int]]` is 3.
    ///
    /// Iterative, with an explicit stack of (type, depth).
    #[must_use]
    pub fn nesting(&self, t: Type) -> usize {
        let mut deepest = 0;
        let mut work = vec![(t, 1usize)];
        while let Some((t, d)) = work.pop() {
            deepest = deepest.max(d);
            if d > HARD_NESTING {
                return d;
            }
            for a in self.type_args(t) {
                work.push((a, d + 1));
            }
        }
        deepest
    }
}

impl Generics {
    /// How many instances have been interned.
    #[must_use]
    pub fn created(&self) -> usize {
        self.created
    }
}
