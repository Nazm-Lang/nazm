//! Each function's executable digest.
//!
//! BLAKE3 over the canonical text of what a backend reads from the function: its identity's
//! type arguments, its parameter, local and result types, and every block — named durably, with
//! no span, no display name, no effect metadata and no session-local number except MIR's own
//! local and block numbering, which is a deterministic function of the program
//! (`docs/architecture.md` §7.42).
//!
//! What moves it and what does not, by construction: a change to what executes moves it; a
//! change to formatting, comments, spans, names of locals or the effect contract does not; a
//! capability parameter's kind moves it, because it is part of the signature's types.
//! Provenance cannot reach it — MIR carries none.

use std::fmt::Write as _;

use crate::ir::{Function, Program};
use crate::text;

/// A function's executable digest, as 64 lowercase hex digits.
#[must_use]
pub fn executable(program: &Program, f: &Function) -> String {
    let mut h = blake3::Hasher::new();
    h.update(canonical(program, f).as_bytes());
    h.finalize().to_hex().to_string()
}

/// The canonical text the digest is taken over.
#[must_use]
pub fn canonical(program: &Program, f: &Function) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "nazm.mir/1 function");
    if let Some(sym) = &f.foreign {
        let _ = writeln!(s, "foreign {sym}");
    }
    if let Some(sym) = &f.export {
        let _ = writeln!(s, "export {sym}");
    }
    let args: Vec<String> = f.type_args.iter().map(|t| text::ty(program, *t)).collect();
    let _ = writeln!(s, "args [{}]", args.join(","));
    let _ = writeln!(s, "params {}", f.params);
    let _ = writeln!(s, "ret {}", text::ty(program, f.ret));
    for (i, l) in f.locals.iter().enumerate() {
        let _ = writeln!(s, "local _{i} {}", text::local_ty(program, l.ty));
    }
    for (i, b) in f.blocks.iter().enumerate() {
        let _ = writeln!(s, "bb{i}");
        for st in &b.stmts {
            let _ = writeln!(s, "  {}", text::statement(program, &st.kind));
        }
        let _ = writeln!(s, "  {}", text::terminator(program, &b.term));
    }
    s
}
