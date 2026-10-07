//! The user-defined types of one compilation, and every property derived from them.
//!
//! # Why the derivations left the tables
//!
//! N9 put `needs_cleanup`, `task_safety`, `layout_cycle` and `depth` on `Records`, which
//! was right while a record could only contain built-ins and records. N10 makes a record
//! field able to be an enum and an enum payload able to be a record, so **every one of
//! those walks crosses both tables** and none of them can be answered by either alone.
//!
//! That is what earns this type. It is not a "generic user type": a record and an enum stay
//! two definitions with two tables and two meanings, and the difference between them —
//! *every field lives* against *one variant's fields live* — is written out in each
//! derivation rather than hidden behind a `kind` field. What is shared is the **graph**:
//! both kinds contain other types by value, and the walk over that containment is one walk.

use crate::enums::{EnumId, Enums};
use crate::record::{Records, TaskUnsafe};
use crate::types::Type;

/// How deeply user-defined types may contain one another by value.
///
/// A bound rather than a stack: computing a layout, a copy or a drop walks the containment
/// graph, and `docs/architecture.md`'s rule about recursion with the shape of the input
/// applies to *type* structure too. Cycles are already refused
/// ([`UserTypes::layout_cycle`]), so the graph is finite — but finite is not bounded, and a
/// thousand-deep chain would still overflow whichever consumer recursed first.
///
/// 32 rather than the parser's 512, and the two measure different things: that one bounds
/// how deeply one *expression* nests, which a generator reaches easily, while this bounds
/// how deeply a *type* contains types. Each level here multiplies the copy and drop code
/// generated for every value of the outer type, so the cost is in the emitted program
/// rather than in the compiler's stack. Nothing hand-written comes near it, and a program
/// that does is refused by name rather than by a crash.
pub const MAX_TYPE_DEPTH: usize = 32;

/// One node of the inline-containment graph: a record definition or an enum definition.
///
/// A node rather than a `Type` because only user-defined types can *be* contained in the
/// sense this graph means. A built-in is a leaf and has no edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypeNode {
    Record(crate::record::RecordId),
    Enum(EnumId),
}

impl TypeNode {
    /// The type that names this definition.
    #[must_use]
    pub fn ty(self) -> Type {
        match self {
            Self::Record(id) => Type::Record(id),
            Self::Enum(id) => Type::Enum(id),
        }
    }

    /// The node a type names, or `None` for a built-in.
    #[must_use]
    pub fn of(t: Type) -> Option<Self> {
        match t {
            Type::Record(id) => Some(Self::Record(id)),
            Type::Enum(id) => Some(Self::Enum(id)),
            _ => None,
        }
    }
}

/// Which field of which definition closes a containment cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CycleEdge {
    pub from: TypeNode,
    /// The variant the field belongs to, in declaration order. `None` for a record field.
    pub variant: Option<u32>,
    /// The field's index in its owner's declaration order.
    pub field: u32,
}

/// Why a set of definitions cannot be laid out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutCycle {
    /// The definitions in the cycle, starting and ending at the same one.
    pub through: Vec<TypeNode>,
    /// The field that closes it.
    pub via: CycleEdge,
}

/// Why a type has no built-in equality, and which component is responsible. N13.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoEquality {
    /// The field path from the outer type to the component, by name — a variant named
    /// on the way, as [`TaskUnsafe`](crate::TaskUnsafe)'s is: `["inner", "Values", "items"]`.
    /// Empty when the type itself has none.
    pub path: Vec<String>,
    /// The component's type: a sequence, a `Vec`, a `Chan` or a type parameter.
    pub ty: Type,
}

/// Every record and enum declared in one compilation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserTypes {
    records: Records,
    enums: Enums,
    /// `Vec` types, pending instances and parameter names. N11; see [`crate::generic`].
    generics: crate::generic::Generics,
}

impl UserTypes {
    pub(crate) fn generics(&self) -> &crate::generic::Generics {
        &self.generics
    }

    pub(crate) fn generics_mut(&mut self) -> &mut crate::generic::Generics {
        &mut self.generics
    }

    /// How many applied records and enums this compilation has interned, for evidence.
    #[must_use]
    pub fn instances_created(&self) -> usize {
        self.generics.created()
    }

    #[must_use]
    pub fn records(&self) -> &Records {
        &self.records
    }

    pub fn records_mut(&mut self) -> &mut Records {
        &mut self.records
    }

    #[must_use]
    pub fn enums(&self) -> &Enums {
        &self.enums
    }

    pub fn enums_mut(&mut self) -> &mut Enums {
        &mut self.enums
    }

    /// How many nodes the containment graph has, for a visited set sized once.
    #[must_use]
    fn nodes(&self) -> usize {
        self.records.len() + self.enums.len()
    }

    /// A node's index in a dense visited set. Records first, then enums.
    #[must_use]
    fn slot(&self, node: TypeNode) -> usize {
        match node {
            TypeNode::Record(id) => id.index(),
            TypeNode::Enum(id) => self.records.len() + id.index(),
        }
    }

    /// Every type this definition contains by value, with where it was written.
    ///
    /// `(variant, field, type)` — `variant` is `None` for a record. This is the one place
    /// the two kinds are unified, because it is the one thing they genuinely share: both
    /// contain other types inline, and the containment graph does not care which shape
    /// produced an edge. What they do *with* those fields is different at every call site
    /// below and is written out there.
    fn contained(&self, node: TypeNode) -> Vec<(Option<u32>, u32, Type)> {
        let mut out = Vec::new();
        match node {
            TypeNode::Record(id) => {
                if let Some(def) = self.records.get(id) {
                    for (i, f) in def.fields.iter().enumerate() {
                        // An array field contains its elements inline (Gate 2).
                        let ty = self.strip_array(f.ty);
                        out.push((None, u32::try_from(i).unwrap_or(u32::MAX), ty));
                    }
                }
            }
            TypeNode::Enum(id) => {
                if let Some(def) = self.enums.get(id) {
                    for (vi, v) in def.variants.iter().enumerate() {
                        let vi = u32::try_from(vi).unwrap_or(u32::MAX);
                        for (i, f) in v.fields.iter().enumerate() {
                            let ty = self.strip_array(f.ty);
                            out.push((Some(vi), u32::try_from(i).unwrap_or(u32::MAX), ty));
                        }
                    }
                }
            }
        }
        out
    }

    /// How a type is spelled for a reader.
    ///
    /// Built-ins spell themselves; a user type spells its declared name. Not an identity —
    /// two modules may each declare `Thing` and this returns `Thing` for both, which is
    /// correct for a message and wrong for anything that compares. Comparison is
    /// [`Type`]'s own `Eq`, over ids.
    ///
    /// An application spells its arguments — `Pair[Int, Str]` — and a parameter its
    /// declared name, which is presentation: `T` in one message and `T` in another may be
    /// two different parameters, and the id says which.
    #[must_use]
    pub fn show(&self, t: Type) -> String {
        let args = self.type_args(t);
        let shown = |me: &Self| -> String {
            args.iter()
                .map(|a| me.show(*a))
                .collect::<Vec<_>>()
                .join(", ")
        };
        match t {
            Type::Record(id) => {
                let name = self
                    .records
                    .get(id)
                    .map_or("<unknown record>", |r| r.name.as_str());
                if args.is_empty() {
                    name.to_owned()
                } else {
                    format!("{name}[{}]", shown(self))
                }
            }
            Type::Enum(id) => {
                let name = self
                    .enums
                    .get(id)
                    .map_or("<unknown enum>", |e| e.name.as_str());
                if args.is_empty() {
                    name.to_owned()
                } else {
                    format!("{name}[{}]", shown(self))
                }
            }
            Type::Vec(_) => format!("Vec[{}]", shown(self)),
            Type::Array(a) => match self.array_info(a) {
                Some((e, n)) => format!("[{}; {n}]", self.show(e)),
                None => "[?; ?]".to_owned(),
            },
            Type::ChanOf(_) => format!("Chan[{}]", shown(self)),
            Type::Fn(f) => match self.fn_signature(f) {
                Some(sig) => {
                    let params: Vec<String> = sig.params.iter().map(|p| self.show(*p)).collect();
                    let effects = if sig.effects.is_empty() {
                        String::new()
                    } else {
                        // The effect parameter by its declared name (N51).
                        let mut names: Vec<String> = sig
                            .effects
                            .named()
                            .names()
                            .into_iter()
                            .map(str::to_owned)
                            .collect();
                        if sig.effects.has_param() {
                            names.push(self.effect_param_name(sig.row));
                        }
                        format!(" ! {{ {} }}", names.join(", "))
                    };
                    format!(
                        "fn({}) -> {}{effects}",
                        params.join(", "),
                        self.show(sig.ret)
                    )
                }
                None => "<unknown function type>".to_owned(),
            },
            Type::Param(p) => self.param_name(p),
            other => other.builtin_name().unwrap_or("<unknown type>").to_owned(),
        }
    }

    /// Whether a value of this type owns anything that has to be released.
    ///
    /// A record needs cleanup when **some field** does; an enum when **some field of some
    /// variant** does, because any of its variants may be the active one at runtime. Both
    /// recursively.
    ///
    /// Iterative rather than recursive, with an explicit worklist, because the depth is a
    /// property of the program.
    ///
    /// A `Vec` always does: its storage is its own. A type parameter is answered `true`,
    /// because nothing is known about it — no concrete question ever reaches here with
    /// one, since the backend asks only about instances.
    #[must_use]
    pub fn needs_cleanup(&self, t: Type) -> bool {
        let t = self.strip_array(t);
        let Some(root) = TypeNode::of(t) else {
            return !matches!(t, Type::Int | Type::Bool | Type::Num(_));
        };
        let mut seen = vec![false; self.nodes()];
        let mut work = vec![root];
        while let Some(node) = work.pop() {
            let at = self.slot(node);
            if seen.get(at).copied().unwrap_or(true) {
                continue;
            }
            if let Some(s) = seen.get_mut(at) {
                *s = true;
            }
            for (_, _, ty) in self.contained(node) {
                match ty {
                    Type::Int | Type::Bool | Type::Num(_) => {}
                    Type::Record(_) | Type::Enum(_) => {
                        if let Some(inner) = TypeNode::of(ty) {
                            work.push(inner);
                        }
                    }
                    // Str, Ints, Strs, Chan: every one of them owns something since N8.
                    _ => return true,
                }
            }
        }
        false
    }

    /// The first counted handle in `t` whose elements can be, or contain, a function value —
    /// or `None` when a value of `t` can hold no function value behind one (Gate 1-C1).
    ///
    /// A closure's environment owns what it captured, and it is the one node of the ownership
    /// graph a definition does not show: a function type says nothing of what its closure
    /// holds. An environment never changes after it is made and holds only older values, so
    /// it can come to own itself only by capturing a container a later store puts it into — a
    /// container whose elements can be function values. This finds such a container in a
    /// captured binding's type: a `Vec` or channel element that is a function type, or reaches
    /// one through record and enum fields or further handles, at any depth of `t`. The answer
    /// is the outermost such handle, for the message. Every owning container is asked the
    /// same way, by its element type, so one added later is covered by being one.
    ///
    /// An explicit worklist over `(type, under a handle)`, with each pair visited once, so a
    /// containment cycle being refused in the same run cannot make it loop.
    #[must_use]
    pub fn function_holder(&self, t: Type) -> Option<Type> {
        let mut seen: Vec<(Type, bool)> = Vec::new();
        let mut work: Vec<(Type, Option<Type>)> = vec![(self.strip_array(t), None)];
        while let Some((ty, holder)) = work.pop() {
            if seen.contains(&(ty, holder.is_some())) {
                continue;
            }
            seen.push((ty, holder.is_some()));
            match ty {
                Type::Fn(_) => {
                    if holder.is_some() {
                        return holder;
                    }
                }
                Type::Vec(v) => {
                    if let Some(e) = self.vec_element(v) {
                        work.push((e, holder.or(Some(ty))));
                    }
                }
                Type::ChanOf(c) => {
                    if let Some(e) = self.chan_element(c) {
                        work.push((e, holder.or(Some(ty))));
                    }
                }
                Type::Record(_) | Type::Enum(_) => {
                    if let Some(node) = TypeNode::of(ty) {
                        for (_, _, f) in self.contained(node) {
                            work.push((f, holder));
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Why `[elem; len]` is not an array the language has, if it is not (Gate 2): its elements
    /// must be plain data — numbers, `Bool`, and records, enums and arrays of them, which own
    /// nothing — and the whole at most [`MAX_ARRAY_BYTES`] by [`Self::data_bytes`].
    #[must_use]
    pub fn array_problem(&self, elem: Type, len: u32) -> Option<ArrayProblem> {
        if len == 0 {
            return Some(ArrayProblem::Empty);
        }
        if let Some(owner) = self.not_plain(elem) {
            return Some(ArrayProblem::NotPlain(owner));
        }
        let bytes = self.data_bytes(elem).saturating_mul(u64::from(len));
        (bytes > MAX_ARRAY_BYTES).then_some(ArrayProblem::TooLarge(bytes))
    }

    /// The first component of `t` that is not plain data, or `None` when `t` is plain: a
    /// number, `Bool`, or a record, enum or array of them — something that owns nothing and
    /// is copied by its bytes.
    #[must_use]
    pub fn not_plain(&self, t: Type) -> Option<Type> {
        let mut seen: Vec<Type> = Vec::new();
        let mut work = vec![self.strip_array(t)];
        while let Some(ty) = work.pop() {
            if seen.contains(&ty) {
                continue;
            }
            seen.push(ty);
            match ty {
                Type::Int | Type::Bool | Type::Num(_) => {}
                Type::Record(_) | Type::Enum(_) => {
                    let Some(node) = TypeNode::of(ty) else {
                        return Some(ty);
                    };
                    if self.opaque(node) {
                        return Some(ty);
                    }
                    for (_, _, f) in self.contained(node) {
                        work.push(f);
                    }
                }
                other => return Some(other),
            }
        }
        None
    }

    /// The bytes a plain value of `t` holds, counted without padding: a number its width, a
    /// `Bool` one, a record its fields, an enum eight for its discriminant and its largest
    /// variant, an array its elements. The bound arrays are held to is stated in these units.
    #[must_use]
    pub fn data_bytes(&self, t: Type) -> u64 {
        self.data_bytes_within(t, 0)
    }

    fn data_bytes_within(&self, t: Type, depth: usize) -> u64 {
        if depth > crate::MAX_TYPE_DEPTH * 4 {
            return 0;
        }
        match t {
            Type::Int => 8,
            Type::Bool => 1,
            Type::Num(k) => u64::from(k.bytes()),
            Type::Array(a) => self.array_info(a).map_or(0, |(e, n)| {
                self.data_bytes_within(e, depth + 1)
                    .saturating_mul(u64::from(n))
            }),
            Type::Record(id) => self.records().get(id).map_or(0, |r| {
                r.fields
                    .iter()
                    .map(|f| self.data_bytes_within(f.ty, depth + 1))
                    .sum()
            }),
            Type::Enum(id) => {
                8 + self.enums().get(id).map_or(0, |e| {
                    e.variants
                        .iter()
                        .map(|v| {
                            v.fields
                                .iter()
                                .map(|f| self.data_bytes_within(f.ty, depth + 1))
                                .sum::<u64>()
                        })
                        .max()
                        .unwrap_or(0)
                })
            }
            _ => 8,
        }
    }

    /// Whether this definition is an opaque C handle (N85).
    fn opaque(&self, node: TypeNode) -> bool {
        match node {
            TypeNode::Record(id) => self
                .records()
                .get(id)
                .is_some_and(|d| d.repr == crate::record::CRepr::Opaque),
            TypeNode::Enum(_) => false,
        }
    }

    /// Whether a value of this type may be passed to a task, and if not, why.
    ///
    /// `Ok(())` or the **field path** that makes it illegal. A record inherits the answer
    /// from its fields; an enum from **every** field of **every** variant, because task
    /// safety is a property of the type and a value's active variant is not known where a
    /// `spawn` is checked. `docs/spec.md` states that asymmetry and why there is no
    /// flow-sensitive escape.
    ///
    /// The path names the variant it went through: `work.Values.items` rather than
    /// `work.items`, so a reader can find the declaration the answer came from.
    ///
    /// Depth-first over the declaration order, so the path reported is the first one a
    /// reader would find.
    ///
    /// # Errors
    ///
    /// [`TaskUnsafe`] naming the field path and the type that cannot cross.
    pub fn task_safety(&self, t: Type) -> Result<(), TaskUnsafe> {
        let t = self.strip_array(t);
        let Some(root) = TypeNode::of(t) else {
            return if t.may_cross_into_a_task() {
                Ok(())
            } else {
                Err(TaskUnsafe {
                    path: Vec::new(),
                    ty: t,
                })
            };
        };
        // A C handle never crosses (N85): nothing says which C objects are thread-safe.
        if self.opaque(root) {
            return Err(TaskUnsafe {
                path: Vec::new(),
                ty: t,
            });
        }
        // An explicit stack of (node, next edge to look at), so the walk is bounded by the
        // heap rather than by the host's frame size. `path` mirrors it, and each frame
        // records how many components it pushed so unwinding is exact.
        let mut stack: Vec<(TypeNode, usize, usize)> = vec![(root, 0, 0)];
        let mut path: Vec<String> = Vec::new();
        while let Some((node, at, pushed)) = stack.pop() {
            let edges = self.contained(node);
            let Some((variant, field, ty)) = edges.get(at).copied() else {
                for _ in 0..pushed {
                    path.pop();
                }
                continue;
            };
            stack.push((node, at + 1, pushed));
            let names = self.names_of(node, variant, field);
            match ty {
                Type::Record(_) | Type::Enum(_) => {
                    let n = names.len();
                    path.extend(names);
                    if let Some(inner) = TypeNode::of(ty) {
                        if self.opaque(inner) {
                            return Err(TaskUnsafe { path, ty });
                        }
                        stack.push((inner, 0, n));
                    }
                }
                other if !other.may_cross_into_a_task() => {
                    path.extend(names);
                    return Err(TaskUnsafe { path, ty: other });
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Whether built-in `==` and `!=` are defined for this type, and if not, why.
    ///
    /// N13. `Ok(())` or the **field path** to the first component that has no equality.
    /// `Int`, `Bool` and `Str` have their primitive equality; a sequence, a `Vec`, a
    /// `Chan` and a type parameter have none. A record has equality when **every field**
    /// does, and an enum when **every payload field of every variant** does — the static
    /// rule [`Self::task_safety`] states for the same reason: `==` is checked where the
    /// active variant is not known, and a guarantee about a type is about all of its
    /// values. An applied type is answered over its fields *after* substitution, which is
    /// what an instance's entry holds, so `Box[Int]` has equality and `Box[Vec[Int]]` and
    /// `Box[T]` do not. `Option` and `Result` are ordinary enums here and nothing names
    /// them.
    ///
    /// Independent of every other derived property on purpose: a `Str` needs cleanup and
    /// compares; a `Chan` may cross into a task and does not compare.
    ///
    /// Depth-first over declaration order, so the path reported is the first one a reader
    /// would find. A definition already visited is not walked again, which bounds the walk
    /// by the number of definitions even in a program whose containment cycle is being
    /// refused in the same run.
    ///
    /// # Errors
    ///
    /// [`NoEquality`] naming the field path and the type that has no equality.
    pub fn equality(&self, t: Type) -> Result<(), NoEquality> {
        let t = self.strip_array(t);
        // A type parameter has equality exactly when its declaration requires it (N14):
        // that is what the body may assume, and every argument is checked against it where
        // the definition is used. One rule, so `T`, `Box[T]` and `Result[T, E]` all answer
        // from it and nothing about bounds is written anywhere else.
        let leaf = |t: Type| match t {
            // Every numeric type has equality (Gate 2); a float's is IEEE 754's.
            // `Text` is equal by its bytes, as `Str` is; a `Bytes` is a mutable handle and,
            // like a `Vec`, has none.
            Type::Int | Type::Bool | Type::Str | Type::Text | Type::Num(_) => true,
            Type::Param(p) => self.requires_equality(p),
            _ => false,
        };
        let Some(root) = TypeNode::of(t) else {
            return if leaf(t) {
                Ok(())
            } else {
                Err(NoEquality {
                    path: Vec::new(),
                    ty: t,
                })
            };
        };
        // A C handle has no equality (N85): a pointer's identity is C's to define.
        if self.opaque(root) {
            return Err(NoEquality {
                path: Vec::new(),
                ty: t,
            });
        }
        let mut seen = vec![false; self.nodes()];
        let mut stack: Vec<(TypeNode, usize, usize)> = vec![(root, 0, 0)];
        let mut path: Vec<String> = Vec::new();
        if let Some(s) = seen.get_mut(self.slot(root)) {
            *s = true;
        }
        while let Some((node, at, pushed)) = stack.pop() {
            let edges = self.contained(node);
            let Some((variant, field, ty)) = edges.get(at).copied() else {
                for _ in 0..pushed {
                    path.pop();
                }
                continue;
            };
            stack.push((node, at + 1, pushed));
            match TypeNode::of(ty) {
                Some(inner) if self.opaque(inner) => {
                    path.extend(self.names_of(node, variant, field));
                    return Err(NoEquality { path, ty });
                }
                Some(inner) => {
                    let at = self.slot(inner);
                    if seen.get(at).copied().unwrap_or(true) {
                        continue;
                    }
                    if let Some(s) = seen.get_mut(at) {
                        *s = true;
                    }
                    let names = self.names_of(node, variant, field);
                    let n = names.len();
                    path.extend(names);
                    stack.push((inner, 0, n));
                }
                None if leaf(ty) => {}
                None => {
                    path.extend(self.names_of(node, variant, field));
                    return Err(NoEquality { path, ty });
                }
            }
        }
        Ok(())
    }

    /// The path components one containment edge contributes: `["field"]` for a record,
    /// `["Variant", "field"]` for an enum payload.
    fn names_of(&self, node: TypeNode, variant: Option<u32>, field: u32) -> Vec<String> {
        match (node, variant) {
            (TypeNode::Record(id), _) => self
                .records
                .get(id)
                .and_then(|d| d.fields.get(field as usize))
                .map(|f| vec![f.name.clone()])
                .unwrap_or_default(),
            (TypeNode::Enum(id), Some(v)) => self
                .enums
                .get(id)
                .and_then(|d| d.variants.get(v as usize))
                .and_then(|vd| {
                    vd.fields
                        .get(field as usize)
                        .map(|f| vec![vd.name.clone(), f.name.clone()])
                })
                .unwrap_or_default(),
            (TypeNode::Enum(_), None) => Vec::new(),
        }
    }

    /// The inline-containment cycle this definition is part of, if it is part of one.
    ///
    /// A record contains its fields by value and an enum contains its active payload by
    /// value, so a chain of either that returns to where it started describes a value that
    /// contains itself — no finite layout exists. `docs/spec.md` refuses it as an
    /// impossible layout rather than as a memory-safety problem, and the rule is stated
    /// over inline containment so that a future indirection can break a cycle without this
    /// rule having to change.
    ///
    /// One graph over both kinds: N9's version walked records only, and an `A → B.V → A`
    /// cycle through an enum would have run forever.
    ///
    /// Iterative depth-first with an explicit stack.
    #[must_use]
    pub fn layout_cycle(&self, start: TypeNode) -> Option<LayoutCycle> {
        // 0 unvisited, 1 on the current path, 2 finished.
        let mut state = vec![0_u8; self.nodes()];
        let mut path: Vec<TypeNode> = Vec::new();
        let mut stack: Vec<(TypeNode, usize)> = vec![(start, 0)];
        while let Some((node, at)) = stack.pop() {
            if at == 0 {
                match state.get(self.slot(node)).copied() {
                    Some(1) => {
                        // Found it: the cycle is the suffix of `path` from this node on.
                        let from = path.iter().position(|p| *p == node)?;
                        let mut through: Vec<TypeNode> = path.get(from..)?.to_vec();
                        let last = *through.last()?;
                        let (variant, field, _) = self
                            .contained(last)
                            .into_iter()
                            .find(|(_, _, ty)| TypeNode::of(*ty) == Some(node))?;
                        through.push(node);
                        return Some(LayoutCycle {
                            through,
                            via: CycleEdge {
                                from: last,
                                variant,
                                field,
                            },
                        });
                    }
                    Some(2) => continue,
                    _ => {}
                }
                if let Some(s) = state.get_mut(self.slot(node)) {
                    *s = 1;
                }
                path.push(node);
            }
            let edges = self.contained(node);
            let next = edges
                .iter()
                .enumerate()
                .skip(at)
                .find_map(|(i, (_, _, ty))| TypeNode::of(*ty).map(|n| (i, n)));
            match next {
                Some((i, inner)) => {
                    stack.push((node, i + 1));
                    stack.push((inner, 0));
                }
                None => {
                    if let Some(s) = state.get_mut(self.slot(node)) {
                        *s = 2;
                    }
                    path.pop();
                }
            }
        }
        None
    }

    /// How deeply this type nests user-defined types, counting itself as 1.
    ///
    /// `None` when the chain is longer than [`MAX_TYPE_DEPTH`]. Only meaningful once
    /// [`Self::layout_cycle`] has found nothing — a cycle has no depth, and this would walk
    /// it forever. The caller checks cycles first, which is the order `docs/spec.md`'s
    /// declaration stages are written in.
    #[must_use]
    pub fn depth(&self, t: Type) -> Option<usize> {
        let Some(root) = TypeNode::of(t) else {
            return Some(0);
        };
        let mut stack: Vec<(TypeNode, usize)> = vec![(root, 0)];
        let mut deepest = 1;
        while let Some((node, at)) = stack.pop() {
            let edges = self.contained(node);
            let next = edges
                .iter()
                .enumerate()
                .skip(at)
                .find_map(|(i, (_, _, ty))| TypeNode::of(*ty).map(|n| (i, n)));
            if let Some((i, inner)) = next {
                stack.push((node, i + 1));
                stack.push((inner, 0));
                deepest = deepest.max(stack.len());
                if deepest > MAX_TYPE_DEPTH {
                    return None;
                }
            }
        }
        Some(deepest)
    }
}

/// How a containment edge reaches what it contains.
///
/// The distinction N11 adds to N9's graph. A field holds its value **inline**, so a cycle
/// of inline edges has no finite layout. A `Vec` element is held **through a counted
/// handle**: the layout is finite, and a cycle through one is a value that can own itself,
/// which reference counting never reclaims. Both are refused, for different reasons and with
/// different codes, because the remediation differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    Inline,
    Counted,
}

/// A cycle in the ownership graph of definitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipCycle {
    /// [`Reach::Inline`] when every edge is inline — an impossible layout, `N0336`; else
    /// [`Reach::Counted`], an ownership cycle, `N0359`.
    pub reach: Reach,
    /// The definitions around the cycle, starting and ending at the same one.
    pub through: Vec<TypeNode>,
    /// The field each hop leaves through: `hops[i]` leaves `through[i]`.
    pub hops: Vec<CycleEdge>,
}

const INLINE: u8 = 1;
const COUNTED: u8 = 2;

/// How a use through `outer` composes with a use of kind `inner`: inline only when both
/// are, and counted as soon as either is.
fn compose(outer: u8, inner: u8) -> u8 {
    let mut out = 0;
    if outer & INLINE != 0 && inner & INLINE != 0 {
        out |= INLINE;
    }
    if (outer & COUNTED != 0 && inner != 0) || (inner & COUNTED != 0 && outer != 0) {
        out |= COUNTED;
    }
    out
}

/// The most an array may hold, in [`UserTypes::data_bytes`]: 64 KiB, which a task's stack holds
/// four times over (Gate 2). A larger buffer is a `Vec` or `Bytes`.
pub const MAX_ARRAY_BYTES: u64 = 65_536;

/// Why an array type is refused (Gate 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrayProblem {
    /// No elements.
    Empty,
    /// An element type that owns something, by the first component that does.
    NotPlain(Type),
    /// Over [`MAX_ARRAY_BYTES`], by how many bytes it would hold.
    TooLarge(u64),
}

/// One edge of the ownership graph: a definition, how it is reached, and from which field.
#[derive(Debug, Clone, Copy)]
struct Owns {
    to: TypeNode,
    reach: u8,
    via: CycleEdge,
}

impl UserTypes {
    /// Every definition as written — not an applied instance of one.
    fn definitions(&self) -> Vec<TypeNode> {
        let records = self
            .records
            .all()
            .iter()
            .enumerate()
            .filter(|(_, d)| d.instance.is_none())
            .map(|(i, _)| TypeNode::Record(crate::record::RecordId::from_index(i)));
        let enums = self
            .enums
            .all()
            .iter()
            .enumerate()
            .filter(|(_, d)| d.instance.is_none())
            .map(|(i, _)| TypeNode::Enum(EnumId::from_index(i)));
        records.chain(enums).collect()
    }

    fn owner_of(node: TypeNode) -> crate::types::ParamOwner {
        match node {
            TypeNode::Record(id) => crate::types::ParamOwner::Record(id),
            TypeNode::Enum(id) => crate::types::ParamOwner::Enum(id),
        }
    }

    /// How `t` reaches a value of parameter `p`: inline, through a counted handle, both, or
    /// not at all. `uses` is how each generic definition uses each of its parameters.
    fn reaches_param(
        &self,
        t: Type,
        p: crate::types::TypeParam,
        uses: &std::collections::HashMap<(TypeNode, u32), u8>,
    ) -> u8 {
        match t {
            Type::Param(q) if q == p => INLINE,
            Type::Vec(v) => match self.vec_element(v) {
                Some(e) if self.reaches_param(e, p, uses) != 0 => COUNTED,
                _ => 0,
            },
            // A channel holds what it queues behind a counted handle, as a `Vec` does (N53).
            Type::ChanOf(c) => match self.chan_element(c) {
                Some(e) if self.reaches_param(e, p, uses) != 0 => COUNTED,
                _ => 0,
            },
            Type::Record(_) | Type::Enum(_) => {
                let Some(template) = self.definition_of(t) else {
                    return 0;
                };
                let mut out = 0;
                for (k, a) in self.type_args(t).into_iter().enumerate() {
                    let inner = self.reaches_param(a, p, uses);
                    if inner == 0 {
                        continue;
                    }
                    let used = uses
                        .get(&(template, u32::try_from(k).unwrap_or(u32::MAX)))
                        .copied()
                        .unwrap_or(0);
                    out |= compose(used, inner);
                }
                out
            }
            _ => 0,
        }
    }

    /// How each generic definition uses each of its parameters, to a fixed point.
    ///
    /// A parameter no field mentions is used not at all, and contributes no edge: `Tag[T] {
    /// n: Int }` applied to anything owns nothing of it. Monotone — each round only adds
    /// kinds — so it settles in at most two rounds per parameter.
    fn parameter_uses(&self) -> std::collections::HashMap<(TypeNode, u32), u8> {
        let mut uses: std::collections::HashMap<(TypeNode, u32), u8> =
            std::collections::HashMap::new();
        let generic: Vec<TypeNode> = self
            .definitions()
            .into_iter()
            .filter(|n| self.declared_params(*n) > 0)
            .collect();
        loop {
            let mut changed = false;
            for node in &generic {
                for i in 0..self.declared_params(*node) {
                    let index = u32::try_from(i).unwrap_or(u32::MAX);
                    let p = crate::types::TypeParam {
                        owner: Self::owner_of(*node),
                        index,
                    };
                    let mut used = 0;
                    for (_, _, ty) in self.contained(*node) {
                        used |= self.reaches_param(ty, p, &uses);
                    }
                    let entry = uses.entry((*node, index)).or_insert(0);
                    if *entry | used != *entry {
                        *entry |= used;
                        changed = true;
                    }
                }
            }
            if !changed {
                return uses;
            }
        }
    }

    /// Every definition a type mentions, and how.
    fn owned_by(
        &self,
        t: Type,
        reach: u8,
        via: CycleEdge,
        uses: &std::collections::HashMap<(TypeNode, u32), u8>,
        out: &mut Vec<Owns>,
    ) {
        match t {
            Type::Vec(v) => {
                if let Some(e) = self.vec_element(v) {
                    self.owned_by(e, COUNTED, via, uses, out);
                }
            }
            Type::ChanOf(c) => {
                if let Some(e) = self.chan_element(c) {
                    self.owned_by(e, COUNTED, via, uses, out);
                }
            }
            Type::Record(_) | Type::Enum(_) => {
                let Some(to) = self.definition_of(t) else {
                    return;
                };
                out.push(Owns { to, reach, via });
                for (k, a) in self.type_args(t).into_iter().enumerate() {
                    let used = uses
                        .get(&(to, u32::try_from(k).unwrap_or(u32::MAX)))
                        .copied()
                        .unwrap_or(0);
                    let through = compose(used, reach);
                    for bit in [INLINE, COUNTED] {
                        if through & bit != 0 {
                            self.owned_by(a, bit, via, uses, out);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn ownership_edges(
        &self,
        node: TypeNode,
        uses: &std::collections::HashMap<(TypeNode, u32), u8>,
    ) -> Vec<Owns> {
        let mut out = Vec::new();
        for (variant, field, ty) in self.contained(node) {
            let via = CycleEdge {
                from: node,
                variant,
                field,
            };
            self.owned_by(ty, INLINE, via, uses, &mut out);
        }
        out
    }

    /// Every cycle in the ownership graph of this compilation's definitions.
    ///
    /// Two passes. The first follows inline edges only and finds the impossible layouts;
    /// the second follows every edge, stays out of the definitions the first reported, and
    /// finds the cycles that run through a counted handle. One cycle is reported once, and a
    /// definition already in one is not reported again.
    ///
    /// Iterative depth-first search with an explicit stack, as every walk here is.
    #[must_use]
    pub fn ownership_cycles(&self) -> Vec<OwnershipCycle> {
        let uses = self.parameter_uses();
        let nodes = self.definitions();
        let edges: std::collections::HashMap<TypeNode, Vec<Owns>> = nodes
            .iter()
            .map(|n| (*n, self.ownership_edges(*n, &uses)))
            .collect();
        let mut out: Vec<OwnershipCycle> = Vec::new();
        let mut reported: Vec<TypeNode> = Vec::new();
        for pass in [INLINE, INLINE | COUNTED] {
            for start in &nodes {
                if reported.contains(start) {
                    continue;
                }
                if let Some(cycle) = find_cycle(*start, &edges, pass, &reported) {
                    reported.extend(cycle.through.iter().copied());
                    out.push(cycle);
                }
            }
        }
        out
    }
}

/// A cycle reachable from `start` over edges whose reach is in `allowed`, avoiding
/// `excluded`, if there is one.
fn find_cycle(
    start: TypeNode,
    edges: &std::collections::HashMap<TypeNode, Vec<Owns>>,
    allowed: u8,
    excluded: &[TypeNode],
) -> Option<OwnershipCycle> {
    // Each frame: a node, the next edge of it to try, and the edge that entered it.
    let mut stack: Vec<(TypeNode, usize, Option<Owns>)> = vec![(start, 0, None)];
    let mut done: Vec<TypeNode> = Vec::new();
    while let Some((node, at, _)) = stack.last().copied() {
        let next = edges.get(&node).and_then(|e| e.get(at)).copied();
        let Some(edge) = next else {
            done.push(node);
            stack.pop();
            continue;
        };
        if let Some(top) = stack.last_mut() {
            top.1 += 1;
        }
        if edge.reach & allowed == 0 || excluded.contains(&edge.to) || done.contains(&edge.to) {
            continue;
        }
        if let Some(from) = stack.iter().position(|(n, _, _)| *n == edge.to) {
            // The suffix of the path from where it re-enters, closed by this edge.
            let frames = stack.get(from..)?;
            let mut through: Vec<TypeNode> = frames.iter().map(|(n, _, _)| *n).collect();
            let mut hops: Vec<CycleEdge> = frames
                .iter()
                .skip(1)
                .filter_map(|(_, _, entered)| entered.map(|e| e.via))
                .collect();
            hops.push(edge.via);
            through.push(edge.to);
            let mut reach = if edge.reach == INLINE {
                Reach::Inline
            } else {
                Reach::Counted
            };
            if frames
                .iter()
                .skip(1)
                .any(|(_, _, e)| e.is_some_and(|e| e.reach != INLINE))
            {
                reach = Reach::Counted;
            }
            return Some(OwnershipCycle {
                reach,
                through,
                hops,
            });
        }
        stack.push((edge.to, 0, Some(edge)));
    }
    None
}
