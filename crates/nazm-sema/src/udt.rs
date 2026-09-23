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

/// Every record and enum declared in one compilation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserTypes {
    records: Records,
    enums: Enums,
}

impl UserTypes {
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
                        out.push((None, u32::try_from(i).unwrap_or(u32::MAX), f.ty));
                    }
                }
            }
            TypeNode::Enum(id) => {
                if let Some(def) = self.enums.get(id) {
                    for (vi, v) in def.variants.iter().enumerate() {
                        let vi = u32::try_from(vi).unwrap_or(u32::MAX);
                        for (i, f) in v.fields.iter().enumerate() {
                            out.push((Some(vi), u32::try_from(i).unwrap_or(u32::MAX), f.ty));
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
    #[must_use]
    pub fn show(&self, t: Type) -> &str {
        match t {
            Type::Record(id) => self
                .records
                .get(id)
                .map_or("<unknown record>", |r| r.name.as_str()),
            Type::Enum(id) => self
                .enums
                .get(id)
                .map_or("<unknown enum>", |e| e.name.as_str()),
            other => other.builtin_name().unwrap_or("<unknown type>"),
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
    #[must_use]
    pub fn needs_cleanup(&self, t: Type) -> bool {
        let Some(root) = TypeNode::of(t) else {
            return !matches!(t, Type::Int | Type::Bool);
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
                    Type::Int | Type::Bool => {}
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
