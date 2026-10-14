//! MIR as text: one canonical rendering of each statement and terminator, shared by the printed
//! form and the digest so the two cannot describe a program differently.
//!
//! Everything is named durably — a type by its canonical key, a callee by its identity, a local
//! as `_N`, a block as `bbN` — and nothing session-local or positional (a span, a file index, a
//! display name) appears.

use nazm_sema::Type;

use crate::ir::{
    Const, FieldRef, FuncId, Identity, LocalTy, Operand, Program, Rvalue, StatementKind, Terminator,
};
use crate::names::owner_text;

/// A type, canonically.
#[must_use]
pub fn ty(program: &Program, t: Type) -> String {
    match t {
        Type::Record(id) => program
            .types
            .record(id)
            .map_or_else(|| "?record".to_owned(), |r| r.key.clone()),
        Type::Enum(id) => program
            .types
            .enumeration(id)
            .map_or_else(|| "?enum".to_owned(), |e| e.key.clone()),
        Type::Vec(v) | Type::Box(v) | Type::List(v) | Type::Cell(v) => {
            let kind = match t {
                Type::Box(_) => "Box",
                Type::List(_) => "List",
                Type::Cell(_) => "Cell",
                _ => "Vec",
            };
            program.types.vec(v).map_or_else(
                || format!("{kind}(?)"),
                |i| format!("{kind}({})", i.elem_key),
            )
        }
        Type::ChanOf(c) => program
            .types
            .chan(c)
            .map_or_else(|| "Chan(?)".to_owned(), |i| format!("Chan({})", i.elem_key)),
        Type::Param(_) => "?param".to_owned(),
        other => other.builtin_name().unwrap_or("?").to_owned(),
    }
}

#[must_use]
pub fn local_ty(program: &Program, t: LocalTy) -> String {
    match t {
        LocalTy::Value(v) => ty(program, v),
        LocalTy::Tasks => "Tasks".to_owned(),
    }
}

#[must_use]
pub fn identity(i: &Identity) -> String {
    if i.args.is_empty() {
        owner_text(&i.owner)
    } else {
        format!("{}[{}]", owner_text(&i.owner), i.args.join(","))
    }
}

fn callee(program: &Program, f: FuncId) -> String {
    program
        .function(f)
        .map_or_else(|| "?fn".to_owned(), |g| identity(&g.identity))
}

fn constant(c: &Const) -> String {
    match c {
        Const::Int(v) => v.to_string(),
        Const::Num { kind, bits } => format!(
            "{}:{}",
            nazm_sema::numeric::to_str(nazm_sema::numeric::NumVal {
                kind: *kind,
                bits: *bits
            }),
            kind.name()
        ),
        Const::Bool(v) => v.to_string(),
        Const::Str(b) => {
            let mut s = String::from("b\"");
            for byte in b {
                if byte.is_ascii_graphic() && *byte != b'"' && *byte != b'\\' || *byte == b' ' {
                    s.push(char::from(*byte));
                } else {
                    s.push_str(&format!("\\x{byte:02x}"));
                }
            }
            s.push('"');
            s
        }
        Const::Cell { name, .. } => format!("static {name}"),
    }
}

fn operand(o: &Operand) -> String {
    match o {
        Operand::Local(l) => format!("_{}", l.0),
        Operand::Const(c) => constant(c),
    }
}

fn operands(os: &[Operand]) -> String {
    os.iter().map(operand).collect::<Vec<_>>().join(", ")
}

fn field(program: &Program, f: FieldRef) -> String {
    let name = program
        .types
        .record(f.record)
        .and_then(|r| r.fields.get(f.field as usize))
        .map_or("?", |x| x.name.as_str());
    format!("{}.{name}", ty(program, Type::Record(f.record)))
}

fn variant_name(program: &Program, e: nazm_sema::EnumId, v: u32) -> String {
    program
        .types
        .enumeration(e)
        .and_then(|x| x.variants.get(v as usize))
        .map_or_else(|| "?".to_owned(), |x| x.name.clone())
}

#[must_use]
pub fn rvalue(program: &Program, v: &Rvalue) -> String {
    match v {
        Rvalue::Use(o) => operand(o),
        Rvalue::Copy(l) => format!("copy _{}", l.0),
        Rvalue::Move(l) => format!("move _{}", l.0),
        Rvalue::Neg(o) => format!("neg {}", operand(o)),
        Rvalue::Arith(op, a, b) => format!("{} {} {}", operand(a), op.symbol(), operand(b)),
        Rvalue::Bits(op, a, b) => format!("{} {} {}", operand(a), op.symbol(), operand(b)),
        Rvalue::Complement(o) => format!("~{}", operand(o)),
        Rvalue::Convert {
            mode,
            to,
            operand: o,
        } => format!(
            "convert.{} {} to {}",
            nazm_cir::digest::convert_name(*mode),
            operand(o),
            ty(program, *to)
        ),
        Rvalue::Fits { to, operand: o } => format!("fits {} in {}", operand(o), ty(program, *to)),
        Rvalue::Array { elems } => format!("[{}]", operands(elems)),
        Rvalue::ArrayRepeat { value, count } => format!("[{}; {count}]", operand(value)),
        Rvalue::Index { base, index } => format!("{}[{}]", operand(base), operand(index)),
        Rvalue::Compare {
            op,
            law,
            operand: t,
            lhs,
            rhs,
        } => format!(
            "{} {} {} ({} on {})",
            operand(lhs),
            op.symbol(),
            operand(rhs),
            law.name(),
            ty(program, *t)
        ),
        Rvalue::Hash { operand: t, value } => {
            format!("hash {} ({})", operand(value), ty(program, *t))
        }
        Rvalue::Call { callee: c, args } => {
            format!("call {}({})", callee(program, *c), operands(args))
        }
        Rvalue::CallIndirect { callee: c, args } => {
            format!("call_indirect {}({})", operand(c), operands(args))
        }
        Rvalue::Closure { func, captures } => {
            format!("closure {}[{}]", callee(program, *func), operands(captures))
        }
        Rvalue::Capture { env, index } => format!("capture _{}.{index}", env.0),
        Rvalue::Primitive { op, elem, args } => match elem {
            Some(e) => format!("{}[{}]({})", op.name(), ty(program, *e), operands(args)),
            None => format!("{}({})", op.name(), operands(args)),
        },
        Rvalue::Record { record, fields } => {
            let inner: Vec<String> = fields
                .iter()
                .map(|(f, l)| {
                    let name = program
                        .types
                        .record(*record)
                        .and_then(|r| r.fields.get(*f as usize))
                        .map_or("?", |x| x.name.as_str());
                    format!("{name}: move _{}", l.0)
                })
                .collect();
            format!(
                "{} {{ {} }}",
                ty(program, Type::Record(*record)),
                inner.join(", ")
            )
        }
        Rvalue::Variant {
            enumeration,
            variant,
            fields,
        } => {
            let names = program
                .types
                .enumeration(*enumeration)
                .and_then(|e| e.variants.get(*variant as usize));
            let inner: Vec<String> = fields
                .iter()
                .map(|(f, l)| {
                    let name = names
                        .and_then(|v| v.fields.get(*f as usize))
                        .map_or("?", |x| x.name.as_str());
                    format!("{name}: move _{}", l.0)
                })
                .collect();
            format!(
                "{}.{}({})",
                ty(program, Type::Enum(*enumeration)),
                variant_name(program, *enumeration, *variant),
                inner.join(", ")
            )
        }
        Rvalue::Field { base, field: f } => format!("{} . {}", operand(base), field(program, *f)),
        Rvalue::FieldBorrow { base, field: f } => {
            format!("borrow {} . {}", operand(base), field(program, *f))
        }
        Rvalue::Payload {
            base,
            enumeration,
            variant,
            field: f,
        } => {
            let name = program
                .types
                .enumeration(*enumeration)
                .and_then(|e| e.variants.get(*variant as usize))
                .and_then(|v| v.fields.get(*f as usize))
                .map_or("?", |x| x.name.as_str());
            format!(
                "payload _{} as {}.{} . {name}",
                base.0,
                ty(program, Type::Enum(*enumeration)),
                variant_name(program, *enumeration, *variant)
            )
        }
    }
}

#[must_use]
pub fn statement(program: &Program, s: &StatementKind) -> String {
    let edge = |u: Option<crate::ir::BlockId>| {
        u.map_or_else(String::new, |b| format!(" unwind bb{}", b.0))
    };
    match s {
        StatementKind::Assign { dst, value, unwind } => {
            format!("_{} = {}{}", dst.0, rvalue(program, value), edge(*unwind))
        }
        StatementKind::SetField { base, path, value } => {
            let p: Vec<String> = path.iter().map(|f| field(program, *f)).collect();
            format!("_{}.[{}] = move _{}", base.0, p.join(" / "), value.0)
        }
        StatementKind::SetElement {
            base,
            path,
            value,
            unwind,
        } => {
            let p: Vec<String> = path
                .iter()
                .map(|s| match s {
                    crate::ir::PathRef::Field(f) => field(program, *f),
                    crate::ir::PathRef::Index { index, len } => format!("[_{} of {len}]", index.0),
                })
                .collect();
            format!(
                "_{}.[{}] = _{}{}",
                base.0,
                p.join(" / "),
                value.0,
                edge(Some(*unwind))
            )
        }
        StatementKind::Drop(l) => format!("drop _{}", l.0),
        StatementKind::ScopeOpen { list, unwind } => {
            format!("_{} = scope_open(){}", list.0, edge(Some(*unwind)))
        }
        StatementKind::Spawn {
            list,
            callee: c,
            args,
            unwind,
            ..
        } => format!(
            "spawn _{} {}({}){}",
            list.0,
            callee(program, *c),
            operands(args),
            edge(Some(*unwind))
        ),
        StatementKind::ScopeJoin { list } => format!("scope_join _{}", list.0),
        StatementKind::CheckFailed { unwind } => format!("check_failed{}", edge(Some(*unwind))),
        StatementKind::StackCheck { unwind } => format!("stack_check{}", edge(Some(*unwind))),
    }
}

#[must_use]
pub fn terminator(program: &Program, t: &Terminator) -> String {
    match t {
        Terminator::Goto(b) => format!("goto bb{}", b.0),
        Terminator::Branch {
            cond,
            then,
            otherwise,
        } => format!("branch {} bb{} bb{}", operand(cond), then.0, otherwise.0),
        Terminator::Switch {
            scrutinee,
            enumeration,
            arms,
        } => {
            let a: Vec<String> = arms
                .iter()
                .map(|(v, b)| format!("{} => bb{}", variant_name(program, *enumeration, *v), b.0))
                .collect();
            format!("switch _{} [{}]", scrutinee.0, a.join(", "))
        }
        Terminator::Return(l) => format!("return move _{}", l.0),
        Terminator::Unwind => "unwind".to_owned(),
        Terminator::Exit(o) => format!("exit {}", operand(o)),
    }
}
