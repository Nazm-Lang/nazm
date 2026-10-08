//! Contracts (N69, `docs/architecture.md` §7.71): what a module that is a contract offers, read off
//! the checker's resolution and Core IR — its state, its entrypoints, and each entrypoint's read and
//! write sets over the state.
//!
//! A read or write set is conservative: a field the entrypoint projects from its state parameter is
//! read; a field of a `State(…)` it builds is written unless its initialiser is that same field of
//! the parameter; and a use of the state the analysis does not see through — the parameter passed
//! to a helper, bound to another name, or a state built anywhere but in the result — makes the whole
//! state *may* read or *may* write. Never an omission.

use std::collections::BTreeSet;

use nazm_core::cir::ir::{Block, Expr, ExprKind, Function, LocalId, Program, Stmt};
use nazm_core::sema::{ModuleId, RecordId, Resolution, Type, Visibility};

/// One entrypoint's facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entrypoint {
    pub name: String,
    /// The arguments after `s` and `caller`, by name.
    pub args: Vec<String>,
    /// Fields read, by name, or `None` for "may read every field".
    pub reads: Option<BTreeSet<String>>,
    /// Fields written, by name, or `None` for "may write every field".
    pub writes: Option<BTreeSet<String>>,
    /// Whether `caller` appears in a condition of the entrypoint's own body (N72): a write the
    /// signer could be refused is one the body decides on by who signed. A helper given `caller` is
    /// not looked into, so it does not count.
    pub caller_checked: bool,
    /// The entrypoint's name, for a witness.
    pub at: nazm_span::Span,
}

/// A contract's shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    pub state: RecordId,
    /// The state's fields in declaration order.
    pub fields: Vec<String>,
    pub entrypoints: Vec<Entrypoint>,
    pub conserved: bool,
}

/// Why a module is not a contract.
pub type NotAContract = String;

/// The contract `root` declares, or why it is not one.
///
/// # Errors
///
/// The module has no `State` record of `Int`s and `Bool`s, no `fn init() -> State`, or no
/// entrypoint.
pub fn of(
    program: &Program,
    res: &Resolution,
    root: Option<ModuleId>,
) -> Result<Contract, NotAContract> {
    let records = res.records();
    let state = records
        .all()
        .iter()
        .position(|r| r.name == "State" && Some(r.module) == root)
        .map(RecordId::from_index)
        .ok_or("a contract declares a record `State`")?;
    let def = records
        .get(state)
        .ok_or("a contract declares a record `State`")?;
    if def.fields.is_empty()
        || !def
            .fields
            .iter()
            .all(|f| matches!(f.ty, Type::Int | Type::Bool))
    {
        return Err("a contract's `State` has `Int` and `Bool` fields, at least one".to_owned());
    }
    let fields: Vec<String> = def.fields.iter().map(|f| f.name.clone()).collect();
    let types = res.types();
    let state_ty = Type::Record(state);
    let mine = |f: &&Function| Some(f.module) == root && f.foreign.is_none();
    let init = program
        .functions
        .iter()
        .filter(mine)
        .any(|f| f.name == "init" && f.params == 0 && f.ret == state_ty);
    if !init {
        return Err("a contract declares `fn init() -> State`".to_owned());
    }
    let public = |name: &str| {
        res.functions().iter().any(|f| {
            f.name == name && Some(f.module) == root && f.visibility == Visibility::Exported
        })
    };
    let mut entrypoints = Vec::new();
    for f in program.functions.iter().filter(mine) {
        if !public(&f.name) || f.params < 2 {
            continue;
        }
        let ty = |i: u32| f.local(LocalId(i)).map(|l| l.ty);
        let shaped = ty(0) == Some(state_ty)
            && ty(1) == Some(Type::Int)
            && (2..f.params).all(|i| ty(i) == Some(Type::Int))
            && types.show(f.ret) == "Result[State, Int]";
        if !shaped {
            continue;
        }
        let args = (2..f.params)
            .filter_map(|i| f.local(LocalId(i)).map(|l| l.name.clone()))
            .collect();
        let mut sets = Sets {
            caller: LocalId(1),
            caller_checked: false,
            state: LocalId(0),
            fields: &fields,
            state_record: state,
            reads: Some(BTreeSet::new()),
            writes: Some(BTreeSet::new()),
        };
        sets.block(&f.body, true);
        entrypoints.push(Entrypoint {
            name: f.name.clone(),
            args,
            reads: sets.reads,
            writes: sets.writes,
            caller_checked: sets.caller_checked,
            at: f.name_span,
        });
    }
    if entrypoints.is_empty() {
        return Err(
            "a contract has an entrypoint: `pub fn name(s: State, caller: Int, …: Int) -> Result[State, Int]`"
                .to_owned(),
        );
    }
    let conserved = program
        .functions
        .iter()
        .filter(mine)
        .any(|f| f.name == "conserved" && f.params == 1 && f.ret == Type::Int);
    Ok(Contract {
        state,
        fields,
        entrypoints,
        conserved,
    })
}

struct Sets<'a> {
    caller: LocalId,
    caller_checked: bool,
    state: LocalId,
    fields: &'a [String],
    state_record: RecordId,
    reads: Option<BTreeSet<String>>,
    writes: Option<BTreeSet<String>>,
}

/// Whether `l` is read anywhere in `e`, its regions' values included.
fn mentions(e: &Expr, l: LocalId) -> bool {
    match &e.kind {
        ExprKind::Local(x) => *x == l,
        ExprKind::Neg(x) | ExprKind::Field { base: x, .. } => mentions(x, l),
        ExprKind::Arith { lhs, rhs, .. } | ExprKind::Compare { lhs, rhs, .. } => {
            mentions(lhs, l) || mentions(rhs, l)
        }
        ExprKind::If {
            cond,
            then,
            otherwise,
        } => mentions(cond, l) || block_mentions(then, l) || block_mentions(otherwise, l),
        ExprKind::Block(b) => block_mentions(b, l),
        // A value passed to a call is not a decision the entrypoint made on it.
        _ => false,
    }
}

fn block_mentions(b: &Block, l: LocalId) -> bool {
    b.tail.as_ref().is_some_and(|t| mentions(t, l))
}

impl Sets<'_> {
    fn condition(&mut self, cond: &Expr) {
        if mentions(cond, self.caller) {
            self.caller_checked = true;
        }
    }

    fn read(&mut self, field: u32) {
        if let (Some(r), Some(n)) = (self.reads.as_mut(), self.fields.get(field as usize)) {
            r.insert(n.clone());
        }
    }

    fn write(&mut self, field: u32) {
        if let (Some(w), Some(n)) = (self.writes.as_mut(), self.fields.get(field as usize)) {
            w.insert(n.clone());
        }
    }

    fn block(&mut self, b: &Block, tail_is_result: bool) {
        for s in &b.stmts {
            self.stmt(s);
        }
        if let Some(t) = &b.tail {
            self.expr(t, tail_is_result);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let { value, .. } | Stmt::Eval(value) => self.expr(value, false),
            Stmt::Assign { local, value, .. } => {
                // `s` rebound inside the entrypoint: what it holds at the return is not followed.
                if *local == self.state {
                    self.reads = None;
                    self.writes = None;
                }
                self.expr(value, false);
            }
            Stmt::Return { value, .. } => self.expr(value, true),
            Stmt::If {
                cond,
                then,
                otherwise,
                ..
            } => {
                self.condition(cond);
                self.expr(cond, false);
                self.block(then, false);
                if let Some(o) = otherwise {
                    self.block(o, false);
                }
            }
            Stmt::Loop { cond, body, .. } => {
                self.condition(cond);
                self.expr(cond, false);
                self.block(body, false);
            }
            Stmt::Block(b) | Stmt::Scope { body: b, .. } => self.block(b, false),
            Stmt::Match(m) => {
                self.expr(&m.scrutinee, false);
                for a in &m.arms {
                    self.block(&a.body, false);
                }
            }
            Stmt::Spawn { args, .. } => {
                for a in args {
                    self.expr(a, false);
                }
            }
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
        }
    }

    /// `result` is whether `e` is (part of) what the entrypoint returns.
    fn expr(&mut self, e: &Expr, result: bool) {
        // Returned as the new state, anything but the state itself, a `State(…)` literal, an
        // `Ok(…)`/`Err(…)` around one, or a branch, block or match whose arms are those, is a
        // value the analysis does not see into — a helper's result, a trait method's, a field of
        // a wrapper, an element of an array — so it may write every field. Until Q1 only a
        // returned local said so, and the rest read as writing nothing (Q1-C-03).
        if result {
            let seen = match &e.kind {
                ExprKind::Local(_)
                | ExprKind::Variant { .. }
                | ExprKind::If { .. }
                | ExprKind::Block(_)
                | ExprKind::Match(_) => true,
                ExprKind::Record { record, .. } => *record == self.state_record,
                _ => false,
            };
            if !seen {
                self.writes = None;
            }
        }
        match &e.kind {
            // A projection of the state parameter: one field read.
            ExprKind::Field { base, field, .. } if matches!(base.kind, ExprKind::Local(l) if l == self.state) =>
            {
                self.read(*field);
            }
            // The state parameter used whole, anywhere but as the base of a projection: the
            // analysis cannot see what is done with it.
            // Returned as it is: nothing read and nothing written. Used whole anywhere else — handed
            // to a helper, bound to another name — the analysis cannot see which fields are read;
            // nothing can be written through it, a record being a value.
            ExprKind::Local(l) if *l == self.state => {
                if !result {
                    self.reads = None;
                }
            }
            // Some other value returned as the new state: what it holds is not seen.
            ExprKind::Local(_) if result => self.writes = None,
            ExprKind::Record { record, fields, .. } if *record == self.state_record => {
                for init in fields {
                    let passthrough = matches!(
                        &init.value.kind,
                        ExprKind::Field { base, field, .. }
                            if *field == init.field && matches!(base.kind, ExprKind::Local(l) if l == self.state)
                    );
                    if result {
                        if passthrough {
                            // `field: s.field` keeps the field: neither read nor written.
                            continue;
                        }
                        self.write(init.field);
                    }
                    // A state built anywhere but in the result is a value like any other; it is
                    // written only if it is returned, which the `Local` arm sees.
                    self.expr(&init.value, false);
                }
            }
            ExprKind::Variant {
                variant, fields, ..
            } => {
                for init in fields {
                    // `Ok(value: State(…))` is the new state; `Err(error: code)` is not one.
                    self.expr(&init.value, result && *variant == 0);
                }
            }
            ExprKind::Field { base, .. } => self.expr(base, false),
            ExprKind::Neg(x) => self.expr(x, false),
            ExprKind::Arith { lhs, rhs, .. } | ExprKind::Compare { lhs, rhs, .. } => {
                self.expr(lhs, false);
                self.expr(rhs, false);
            }
            ExprKind::Call { args, .. } | ExprKind::Builtin { args, .. } => {
                for a in args {
                    self.expr(a, false);
                }
            }
            ExprKind::CallIndirect { callee, args, .. } => {
                self.expr(callee, false);
                for a in args {
                    self.expr(a, false);
                }
            }
            // A call through a bound (N78): which function runs is the instance's, as an
            // indirect call's is the value's.
            ExprKind::CallTrait { args, .. } => {
                for a in args {
                    self.expr(a, false);
                }
            }
            ExprKind::Record { fields, .. } => {
                for init in fields {
                    self.expr(&init.value, false);
                }
            }
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.condition(cond);
                self.expr(cond, false);
                self.block(then, result);
                self.block(otherwise, result);
            }
            ExprKind::Block(b) => self.block(b, result),
            ExprKind::Match(m) => {
                self.expr(&m.scrutinee, false);
                for a in &m.arms {
                    self.block(&a.body, result);
                }
            }
            ExprKind::Closure { .. } => {
                // A closure may capture the state; which fields it reads is not seen.
                self.reads = None;
            }
            ExprKind::Const(_) | ExprKind::Local(_) | ExprKind::FnValue(_) => {}
            ExprKind::Complement(x)
            | ExprKind::Convert { operand: x, .. }
            | ExprKind::Fits { operand: x, .. }
            | ExprKind::ArrayRepeat { value: x, .. } => self.expr(x, false),
            ExprKind::Array { elems } => {
                for x in elems {
                    self.expr(x, false);
                }
            }
            ExprKind::Index { base, index } => {
                self.expr(base, false);
                self.expr(index, false);
            }
            ExprKind::Bits { lhs, rhs, .. } => {
                self.expr(lhs, false);
                self.expr(rhs, false);
            }
        }
    }
}
