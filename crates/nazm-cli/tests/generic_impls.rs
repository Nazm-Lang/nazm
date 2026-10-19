//! R1b, L11b: impls of generic types (`docs/user/spec.md`, *Traits and methods*, **Impls of generic
//! types**).
//!
//! `impl[T: Show] Show for Maybe[T]` implements a trait for every instance whose arguments meet
//! the impl's requirements. The tests ask every implementation for what the rules work out by
//! hand — direct calls, UFCS, through a bound, nested instances, a default body at an instance —
//! with nothing live; build one across modules; and hold the header's form, the requirements,
//! coherence and the orphan rule.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::{run_bounded, Ran};

use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// The program under the interpreter and the LIR oracle, and built and run by each backend,
/// each with the memory report asked for.
fn everywhere(src: &str) -> Vec<(String, Ran)> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let mut out = Vec::new();
    for (label, args) in [
        ("interpreter", &["run", "--interpret", "prog.nz"][..]),
        ("lir oracle", &["lir", "--run", "prog.nz"][..]),
    ] {
        out.push((
            label.to_owned(),
            run_bounded(
                Command::new(NAZM)
                    .current_dir(d.path())
                    .args(args)
                    .env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
        ));
    }
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = d
            .path()
            .join(crate::common::exe_name(label.replace([' ', '-'], "")));
        let b = run_bounded(
            Command::new(NAZM).current_dir(d.path()).args([
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ]),
            "nazm build",
        );
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        out.push((
            label.to_owned(),
            run_bounded(
                common::bounded_exe(&exe).env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
        ));
    }
    out
}

/// The memory report's line, which every implementation but the LIR oracle writes when asked.
fn report(r: &Ran) -> String {
    r.stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .unwrap_or_else(|| panic!("no memory report: {r:?}"))
        .to_owned()
}

fn nothing_live(line: &str) -> bool {
    line.split_whitespace()
        .filter(|w| w.starts_with("live="))
        .all(|w| w == "live=0")
}

const GENERIC: &str = "trait Show {
    fn show(self: Self) -> Str;
    fn loud(self: Self) -> Str { str_concat(self.show(), \"!\") }
}

struct Pair[A, B] { a: A, b: B, }
enum Maybe[T] { No, Yes(v: T), }

impl Show for Int { fn show(self: Int) -> Str { num_to_str(self) } }
impl Show for Str { fn show(self: Str) -> Str { self } }

impl[A: Show, B: Show] Show for Pair[A, B] {
    fn show(self: Self) -> Str {
        str_concat(\"(\", str_concat(self.a.show(), str_concat(\", \", str_concat(self.b.show(), \")\"))))
    }
}

impl[T: Show] Show for Maybe[T] {
    fn show(self: Maybe[T]) -> Str {
        match self { Maybe.No() => \"no\", Maybe.Yes(v: v) => str_concat(\"yes \", v.show()), }
    }
}

fn twice[T: Show](x: T) -> Str { str_concat(x.show(), x.loud()) }
fn none() -> Maybe[Int] { Maybe[Int].No() }
fn line(out: OutCap, s: Str) -> Int ! {io} { print(str_concat(s, \"\\n\")) }

fn main(out: OutCap) -> Int ! {io} {
    let p = Pair[Int, Str](a: 1, b: \"x\");
    line(out, p.show());
    line(out, p.loud());
    line(out, twice(p));
    let m = Maybe[Pair[Maybe[Int], Int]].Yes(v: Pair[Maybe[Int], Int](a: Maybe[Int].Yes(v: 7), b: 2));
    line(out, twice(m));
    line(out, Show.show(none()));
    0
}
";

#[test]
fn an_impl_of_a_generic_type_means_the_same_everywhere_and_is_reclaimed() {
    for (label, r) in everywhere(GENERIC) {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert!(
            r.stdout
                .starts_with("(1, x)\n(1, x)!\n(1, x)(1, x)!\nyes (yes 7, 2)yes (yes 7, 2)!\nno\n"),
            "{label}: {}",
            r.stdout
        );
        if label != "lir oracle" {
            let line = report(&r);
            assert!(nothing_live(&line), "{label}: {line}");
        }
    }
}

#[test]
fn an_impl_of_a_generic_type_crosses_modules() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("show.nz"),
        "pub trait Show { fn show(self: Self) -> Str; }\n\
         impl Show for Int { fn show(self: Int) -> Str { num_to_str(self) } }\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join("wrap.nz"),
        "use \"show.nz\";\npub struct W[T] { v: T, }\n\
         pub fn wrap(n: Int) -> W[Int] ! {} { W[Int](v: n) }\n\
         impl[T: Show] Show for W[T] { fn show(self: W[T]) -> Str { str_concat(\"w\", self.v.show()) } }\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join("main.nz"),
        "use \"show.nz\";\nuse \"wrap.nz\";\nfn g[T: Show](x: T) -> Str { x.show() }\n\
         fn main(out: OutCap) -> Int ! {io} { let w = wrap(4); print(str_concat(w.show(), g(w))); 0 }\n",
    )
    .unwrap();
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["run", "--interpret", "main.nz"]),
        "nazm run",
    );
    assert!(r.stdout.starts_with("w4w4"), "{r:?}");
    for backend in ["llvm", "cranelift"] {
        let exe = d.path().join(crate::common::exe_name(backend));
        let b = run_bounded(
            Command::new(NAZM).current_dir(d.path()).args([
                "build",
                "--backend",
                backend,
                "main.nz",
                "-o",
                exe.to_str().unwrap(),
            ]),
            "nazm build",
        );
        assert_eq!(b.code, Some(0), "{backend}: {}", b.stderr);
        let r = run_bounded(&mut common::bounded_exe(&exe), backend);
        assert!(r.stdout.starts_with("w4w4"), "{backend}: {r:?}");
    }
}

/// `nazm check` on `src`: every error code it reports, in order.
fn codes(src: &str) -> Vec<String> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), src).unwrap();
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["check", "p.nz"]),
        "nazm check",
    );
    r.stderr
        .split("error[")
        .skip(1)
        .map(|s| s[..5].to_owned())
        .collect()
}

const HEAD: &str = "trait Show { fn show(self: Self) -> Str; }
struct P[A, B] { a: A, b: B, }
struct W[T] { v: T, }
impl Show for Int { fn show(self: Int) -> Str { \"i\" } }
";

#[test]
fn a_generic_impl_is_for_the_type_applied_to_its_parameters_and_holds_its_requirements() {
    let with = |rest: &str| codes(&format!("{HEAD}{rest}"));
    // The type applied to exactly the impl's parameters, in order.
    assert_eq!(
        with("impl[T] Show for P[T, Int] { fn show(self: P[T, Int]) -> Str { \"p\" } }\nfn main() -> Int { 0 }\n"),
        ["N0604"]
    );
    assert_eq!(
        with("impl[A, B] Show for P[B, A] { fn show(self: P[B, A]) -> Str { \"p\" } }\nfn main() -> Int { 0 }\n"),
        ["N0604"]
    );
    assert_eq!(
        with("impl[T] Show for Int { fn show(self: Int) -> Str { \"p\" } }\nfn main() -> Int { 0 }\n"),
        ["N0604"]
    );
    // An instance whose argument does not meet a requirement has no impl: by a call, and by a
    // bound.
    let req = "impl[T: Show] Show for W[T] { fn show(self: W[T]) -> Str { self.v.show() } }\n";
    assert_eq!(
        with(&format!(
            "{req}fn main() -> Int {{ let w = W[Str](v: \"x\"); str_len(w.show()) }}\n"
        )),
        ["N0605"]
    );
    assert_eq!(
        with(&format!(
            "{req}fn f[T: Show](x: T) -> Int {{ str_len(x.show()) }}\n\
             fn main() -> Int {{ f(W[Bool](v: true)) }}\n"
        )),
        ["N0364"]
    );
    // One impl of a trait per generic definition.
    assert!(with(&format!(
        "{req}impl[U] Show for W[U] {{ fn show(self: W[U]) -> Str {{ \"b\" }} }}\nfn main() -> Int {{ 0 }}\n"
    ))
    .contains(&"N0600".to_owned()));
}

#[test]
fn a_generic_impl_in_a_third_module_is_an_orphan() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("t.nz"),
        "pub trait Show { fn show(self: Self) -> Str; }\n",
    )
    .unwrap();
    std::fs::write(d.path().join("w.nz"), "pub struct W[T] { v: T, }\n").unwrap();
    std::fs::write(
        d.path().join("main.nz"),
        "use \"t.nz\";\nuse \"w.nz\";\n\
         impl[T] Show for W[T] { fn show(self: W[T]) -> Str { \"w\" } }\nfn main() -> Int { 0 }\n",
    )
    .unwrap();
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["check", "main.nz"]),
        "nazm check",
    );
    assert!(r.stderr.contains("error[N0601]"), "{r:?}");
}
