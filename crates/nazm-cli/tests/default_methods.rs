//! R1b, L11a: default trait methods (`docs/spec.md`, *Traits and methods*, **Default bodies**).
//!
//! A trait method may end in a body an impl that writes none gets. The body is checked once, as a
//! function generic over `Self`, and each call — direct, UFCS, or through a bound — runs it at the
//! receiver's type. The tests ask every implementation for what the rules work out by hand with
//! nothing live, build a trait and its impl in different modules, and hold a default body to its
//! signature.

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
        ("interpreter", &["run", "prog.nz"][..]),
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
        let exe = d.path().join(label.replace([' ', '-'], ""));
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

const DEFAULTS: &str = "trait Describe {
    fn name(self: Self) -> Str;
    fn describe(self: Self) -> Str { str_concat(\"<\", str_concat(self.name(), \">\")) }
    fn twice(self: Self) -> Str { str_concat(self.describe(), self.describe()) }
    fn shout(self: Self, out: OutCap) -> Int ! {io} { print(str_concat(self.name(), \"!\\n\")) }
}

struct Dog { n: Int, }
struct Cat { n: Int, }

impl Describe for Dog {
    fn name(self: Dog) -> Str { \"dog\" }
}
// An impl may still define a method its trait gives a body: it replaces it for this type.
impl Describe for Cat {
    fn name(self: Cat) -> Str { \"cat\" }
    fn describe(self: Cat) -> Str { \"a cat\" }
}

fn through[T: Describe](x: T) -> Str { x.twice() }

fn show(out: OutCap, s: Str) -> Int ! {io} { print(str_concat(s, \"\\n\")) }

fn main(out: OutCap) -> Int ! {io} {
    let d = Dog(n: 1);
    let c = Cat(n: 2);
    show(out, d.describe());
    show(out, c.describe());
    show(out, through(d));
    show(out, through(c));
    show(out, Describe.twice(d));
    d.shout(out);
    0
}
";

#[test]
fn a_default_body_means_the_same_everywhere_and_is_reclaimed() {
    for (label, r) in everywhere(DEFAULTS) {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert!(
            r.stdout
                .starts_with("<dog>\na cat\n<dog><dog>\na cata cat\n<dog><dog>\ndog!\n"),
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
fn a_default_body_crosses_modules_to_an_impl_in_another() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("t.nz"),
        "pub trait Named {\n    fn name(self: Self) -> Str;\n    \
         fn greet(self: Self) -> Str { str_concat(\"hi \", self.name()) }\n}\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join("p.nz"),
        "use \"t.nz\";\npub struct P { x: Int, }\n\
         impl Named for P { fn name(self: P) -> Str { \"p\" } }\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join("main.nz"),
        "use \"t.nz\";\nuse \"p.nz\";\nfn g[T: Named](x: T) -> Str { x.greet() }\n\
         fn main(out: OutCap) -> Int ! {io} { let p = P(x: 1); \
         print(str_concat(p.greet(), g(p))); 0 }\n",
    )
    .unwrap();
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["run", "main.nz"]),
        "nazm run",
    );
    assert!(r.stdout.starts_with("hi phi p"), "{r:?}");
    for backend in ["llvm", "cranelift"] {
        let exe = d.path().join(backend);
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
        assert!(r.stdout.starts_with("hi phi p"), "{backend}: {r:?}");
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

#[test]
fn a_default_body_is_checked_once_and_held_to_its_signature() {
    // Once, whatever number of impls use it.
    assert_eq!(
        codes(
            "trait T { fn n(self: Self) -> Int; fn bad(self: Self) -> Int { self.n() + \"x\" } }\n\
             struct A { x: Int, }\nstruct B { x: Int, }\n\
             impl T for A { fn n(self: A) -> Int { 1 } }\nimpl T for B { fn n(self: B) -> Int { 2 } }\n\
             fn main() -> Int { 0 }\n"
        ),
        ["N0304"]
    );
    // Generic over `Self`: nothing is known of it but the trait.
    assert_eq!(
        codes(
            "trait T { fn n(self: Self) -> Int; fn len(self: Self) -> Int { str_len(self) } }\n\
             fn main() -> Int { 0 }\n"
        ),
        ["N0300"]
    );
    // Its effects are its signature's: none written is none.
    assert_eq!(
        codes(
            "trait T { fn n(self: Self) -> Int; \
             fn say(self: Self, out: OutCap) -> Int { print(\"x\") } }\n\
             fn main() -> Int { 0 }\n"
        ),
        ["N0366"]
    );
    // A method without a default body is still required.
    assert_eq!(
        codes(
            "trait T { fn n(self: Self) -> Int; fn m(self: Self) -> Int { 0 } }\n\
             struct A { x: Int, }\nimpl T for A { fn m(self: A) -> Int { 1 } }\n\
             fn main() -> Int { 0 }\n"
        ),
        ["N0602"]
    );
}
