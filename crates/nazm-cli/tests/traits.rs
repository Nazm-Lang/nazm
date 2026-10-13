//! Traits and methods (N78, `docs/architecture.md` §7.79), held across the interpreter,
//! LLVM at -O0 and -O2, and Cranelift, and every refusal by its code.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::{run_bounded, Ran};

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

/// The program at `dir/main` under the interpreter, then built and run by each backend at
/// each optimisation level, with the memory report on.
fn everywhere(dir: &Path, main: &str) -> Vec<(String, Ran)> {
    let mut out = vec![("interpreter".to_owned(), nazm(dir, &["run", main]))];
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = dir.join(label.replace([' ', '-'], ""));
        let b = nazm(
            dir,
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
                main,
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        let mut c = common::bounded_exe(&exe);
        c.env(nazm_lir::MEMORY_VARIABLE, "1");
        out.push((label.to_owned(), run_bounded(&mut c, label)));
    }
    out
}

/// Every implementation prints `expected` and exits with `code`; every native build
/// reclaims all it allocated.
fn agrees_on(files: &[(&str, &str)], expected: &str, code: i32) {
    let d = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let p = d.path().join(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, text).unwrap();
    }
    for (label, r) in everywhere(d.path(), files[0].0) {
        assert_eq!(r.code, Some(code), "{label}: {}", r.stderr);
        assert_eq!(r.stdout.trim_end(), expected, "{label}");
        if label != "interpreter" {
            let line = r
                .stderr
                .lines()
                .find(|l| l.starts_with("nazm-memory:"))
                .unwrap_or_else(|| panic!("{label}: no memory report: {}", r.stderr));
            for kind in ["sequences", "strings", "channels"] {
                let part = line.split(kind).nth(1).unwrap_or("");
                assert!(
                    part.contains("live=0"),
                    "{label}: {kind} not all reclaimed: {line}"
                );
            }
        }
    }
}

fn agrees(src: &str, expected: &str) {
    agrees_on(&[("prog.nz", src)], expected, 0);
}

/// The codes `nazm check --json` reports for the program, in order.
fn check_codes(files: &[(&str, &str)]) -> Vec<String> {
    let d = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(d.path().join(name), text).unwrap();
    }
    let r = nazm(d.path(), &["check", "--json", files[0].0]);
    r.stderr
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["code"].as_str().map(str::to_owned))
        .collect()
}

const SHOW: &str = "trait Show {\n    fn show(self: Self) -> Str;\n}\n";

// --- semantics, the same in every implementation ----------------------------------------

#[test]
fn a_method_call_on_a_record_an_enum_and_a_builtin_calls_its_impl() {
    let src = format!(
        "{SHOW}struct P {{ x: Int, }}\nenum L {{ Red, Green, }}\n\
         impl Show for P {{ fn show(self: P) -> Str {{ str_concat(\"P\", int_to_str(self.x)) }} }}\n\
         impl Show for L {{ fn show(self: Self) -> Str {{ match self {{ L.Red() => \"red\", L.Green() => \"green\", }} }} }}\n\
         impl Show for Int {{ fn show(self: Int) -> Str {{ str_concat(\"#\", int_to_str(self)) }} }}\n\
         fn main(out: OutCap) -> Int {{\n\
             print(str_concat(P(x: 4).show(), \"\\n\"));\n\
             print(str_concat(L.Green().show(), \"\\n\"));\n\
             let n = 7;\n\
             print(str_concat(n.show(), \"\\n\"));\n\
             print(str_concat(Show.show(n), \"\\n\"));\n\
             0\n\
         }}\n"
    );
    agrees(&src, "P4\ngreen\n#7\n#7\n0");
}

#[test]
fn a_call_through_a_bound_runs_the_instances_impl_through_two_generic_levels() {
    let src = format!(
        "{SHOW}struct P {{ x: Int, }}\n\
         impl Show for P {{ fn show(self: P) -> Str {{ int_to_str(self.x) }} }}\n\
         impl Show for Bool {{ fn show(self: Bool) -> Str {{ if self {{ \"yes\" }} else {{ \"no\" }} }} }}\n\
         fn twice[T: Show](x: T) -> Str {{ str_concat(x.show(), Show.show(x)) }}\n\
         fn outer[U: Show](u: U) -> Str {{ str_concat(\"<\", str_concat(twice(u), \">\")) }}\n\
         fn main(out: OutCap) -> Int {{\n\
             print(str_concat(outer(P(x: 3)), \"\\n\"));\n\
             print(str_concat(outer(true), \"\\n\"));\n\
             str_len(twice(false))\n\
         }}\n"
    );
    agrees_on(&[("prog.nz", &src)], "<33>\n<yesyes>\n4", 0);
}

#[test]
fn a_method_takes_arguments_and_returns_any_type() {
    let src =
        "trait Area { fn area(self: Self, scale: Int) -> Int; fn named(self: Self) -> Bool; }\n\
         struct Sq { s: Int, }\n\
         impl Area for Sq {\n\
             fn area(self: Sq, scale: Int) -> Int { self.s * self.s * scale }\n\
             fn named(self: Sq) -> Bool { self.s > 1 }\n\
         }\n\
         fn total[T: Area](a: T, b: T) -> Int { a.area(1) + b.area(2) }\n\
         fn main() -> Int { let q = Sq(s: 3); if q.named() { total(q, Sq(s: 2)) } else { 0 } }\n";
    agrees_on(&[("prog.nz", src)], "17", 0);
}

#[test]
fn a_standard_trait_shows_a_vector_of_a_programs_own_records() {
    let src = "use \"@std/show\";\n\
         struct P { x: Int, }\n\
         impl Show for P { fn show(self: P) -> Str { str_concat(\"P\", int_to_str(self.x)) } }\n\
         fn main(io: IoCap, tasks: SpawnCap) -> Int {\n\
             let v = vec_new[P]();\n\
             vec_push(v, P(x: 1));\n\
             vec_push(v, P(x: 2));\n\
             let w = vec_new[Int]();\n\
             vec_push(w, 3);\n\
             vec_push(w, 4);\n\
             print(str_concat(show_all(v, \", \"), \"\\n\"));\n\
             print(str_concat(show_all(w, \"|\"), \"\\n\"));\n\
             print(str_concat(true.show(), \"\\n\"));\n\
             0\n\
         }\n";
    agrees(src, "P1, P2\n3|4\ntrue\n0");
}

#[test]
fn traits_and_impls_cross_modules_and_aliases() {
    // The trait in one module, the type and its impl in another, the call in a third under an
    // alias.
    let shape = "pub trait Shape { fn sides(self: Self) -> Int; }\n";
    let tri = "use \"shape.nz\";\npub struct Tri { a: Int, }\n\
               impl Shape for Tri { fn sides(self: Tri) -> Int { 3 } }\n";
    let main = "use \"shape.nz\" as s;\nuse \"tri.nz\";\n\
                fn main() -> Int { let t = Tri(a: 1); t.sides() + s::Shape.sides(t) }\n";
    agrees_on(
        &[("main.nz", main), ("shape.nz", shape), ("tri.nz", tri)],
        "6",
        0,
    );
}

#[test]
fn an_imported_impls_method_is_checked_against_its_traits_effects() {
    // M1: a direct or UFCS call of a method whose impl is in another module was taken for a call
    // of an undeclared function, and every effect assumed of it — so a caller declaring `! {}`
    // was refused (`N0366`) for calling a method its trait declares pure. Its contract is its
    // trait method's, the set the impl is held to.
    let lib = "pub trait Show { fn show(self: Self) -> Str ! {}; }\n\
               pub trait Say { fn say(self: Self, out: OutCap) -> Int ! {io}; }\n\
               pub struct P { x: Int, }\n\
               impl Show for P { fn show(self: P) -> Str ! {} { int_to_str(self.x) } }\n\
               impl Show for Int { fn show(self: Int) -> Str ! {} { int_to_str(self) } }\n\
               impl Say for P { fn say(self: P, out: OutCap) -> Int ! {io} { print(\"P\\n\") } }\n";
    let main = "use \"lib.nz\";\n\
                fn direct(p: P) -> Str ! {} { p.show() }\n\
                fn named(p: P) -> Str ! {} { Show.show(p) }\n\
                fn builtin(n: Int) -> Str ! {} { n.show() }\n\
                fn main() -> Int { str_len(direct(P(x: 7))) + str_len(named(P(x: 10))) + str_len(builtin(123)) }\n";
    agrees_on(&[("main.nz", main), ("lib.nz", lib)], "6", 0);
    // Checked from the dependency's persisted interface too: the second check reads it.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("lib.nz"), lib).unwrap();
    std::fs::write(d.path().join("main.nz"), main).unwrap();
    for pass in ["cold", "cached"] {
        let r = nazm(d.path(), &["check", "main.nz"]);
        assert_eq!(r.code, Some(0), "{pass}: {}", r.stderr);
    }
    // And the contract is still enforced: a method its trait declares `! {io}` is not pure.
    let impure = "use \"lib.nz\";\n\
                  fn quiet(p: P, out: OutCap) -> Int ! {} { p.say(out) }\n\
                  fn main() -> Int { 0 }\n";
    assert_eq!(
        check_codes(&[("main.nz", impure), ("lib.nz", lib)]),
        ["N0366"]
    );
}

// --- refusals, each by its own code -------------------------------------------------------

#[test]
fn two_impls_of_one_trait_for_one_type_are_refused() {
    let src = format!(
        "{SHOW}struct P {{ x: Int, }}\n\
         impl Show for P {{ fn show(self: P) -> Str {{ \"a\" }} }}\n\
         impl Show for P {{ fn show(self: P) -> Str {{ \"b\" }} }}\n\
         fn main() -> Int {{ 0 }}\n"
    );
    assert!(check_codes(&[("prog.nz", &src)]).contains(&"N0600".to_owned()));
}

#[test]
fn an_impl_in_a_third_module_is_an_orphan() {
    let shape = "pub trait Shape { fn sides(self: Self) -> Int; }\n";
    let tri = "pub struct Tri { a: Int, }\n";
    let main = "use \"shape.nz\";\nuse \"tri.nz\";\n\
                impl Shape for Tri { fn sides(self: Tri) -> Int { 3 } }\n\
                fn main() -> Int { 0 }\n";
    assert_eq!(
        check_codes(&[("main.nz", main), ("shape.nz", shape), ("tri.nz", tri)]),
        ["N0601"]
    );
    // And for a built-in type, only the trait's module may.
    let main = "use \"shape.nz\";\nimpl Shape for Int { fn sides(self: Int) -> Int { self } }\n\
                fn main() -> Int { 0 }\n";
    assert_eq!(
        check_codes(&[("main.nz", main), ("shape.nz", shape)]),
        ["N0601"]
    );
}

#[test]
fn an_impl_must_define_exactly_its_traits_methods_with_its_signatures() {
    let two = "trait T { fn a(self: Self) -> Int; fn b(self: Self, n: Int) -> Int; }\nstruct P { x: Int, }\n";
    let missing =
        format!("{two}impl T for P {{ fn a(self: P) -> Int {{ 1 }} }}\nfn main() -> Int {{ 0 }}\n");
    assert_eq!(check_codes(&[("p.nz", &missing)]), ["N0602"]);
    let extra = format!(
        "{two}impl T for P {{ fn a(self: P) -> Int {{ 1 }} fn b(self: P, n: Int) -> Int {{ n }} fn c(self: P) -> Int {{ 0 }} }}\nfn main() -> Int {{ 0 }}\n"
    );
    assert_eq!(check_codes(&[("p.nz", &extra)]), ["N0603"]);
    let wrong = format!(
        "{two}impl T for P {{ fn a(self: P) -> Int {{ 1 }} fn b(self: P, n: Str) -> Int {{ 0 }} }}\nfn main() -> Int {{ 0 }}\n"
    );
    assert_eq!(check_codes(&[("p.nz", &wrong)]), ["N0603", "N0602"]);
    let effects = "trait T { fn a(self: Self) -> Int; }\nstruct P { x: Int, }\n\
                   impl T for P { fn a(self: P) -> Int ! {io} { print(\"x\"); 1 } }\nfn main() -> Int { 0 }\n";
    assert!(check_codes(&[("p.nz", effects)]).contains(&"N0603".to_owned()));
}

#[test]
fn an_impl_for_a_type_that_cannot_have_one_is_refused() {
    for ty in ["Vec[Int]", "Chan", "IoCap", "Box[Int]"] {
        let src = format!(
            "{SHOW}struct Box[T] {{ v: T, }}\nimpl Show for {ty} {{ fn show(self: Self) -> Str {{ \"\" }} }}\nfn main() -> Int {{ 0 }}\n"
        );
        assert_eq!(check_codes(&[("p.nz", &src)]), ["N0604"], "{ty}");
    }
}

#[test]
fn a_number_of_any_width_has_impls_and_its_methods_run_everywhere() {
    // Gate 2 §7: `@std/hash` implements `Hash` for every integer type, so an impl is for any
    // number, as one was for `Int` — called directly, by UFCS and through a bound.
    let src = format!(
        "{SHOW}impl Show for UInt8 {{ fn show(self: UInt8) -> Str {{ str_concat(\"u8:\", num_to_str(self)) }} }}\n\
         impl Show for Int32 {{ fn show(self: Int32) -> Str {{ str_concat(\"i32:\", num_to_str(self)) }} }}\n\
         impl Show for Float64 {{ fn show(self: Float64) -> Str {{ str_concat(\"f64:\", num_to_str(self)) }} }}\n\
         fn twice[T: Show](x: T) -> Str {{ str_concat(x.show(), x.show()) }}\n\
         fn main(out: OutCap) -> Int {{\n\
             let b = UInt8(200);\n\
             print(str_concat(b.show(), \"\\n\"));\n\
             print(str_concat(Show.show(Int32(0 - 5)), \"\\n\"));\n\
             print(str_concat(twice(1.5), \"\\n\"));\n\
             0\n\
         }}\n"
    );
    agrees(&src, "u8:200\ni32:-5\nf64:1.5f64:1.5\n0");
}

#[test]
fn a_method_no_trait_in_scope_gives_is_refused_and_two_are_ambiguous() {
    let src =
        format!("{SHOW}struct P {{ x: Int, }}\nfn main() -> Int {{ str_len(P(x: 1).show()) }}\n");
    assert_eq!(check_codes(&[("p.nz", &src)]), ["N0605"]);
    let two = "trait A { fn go(self: Self) -> Int; }\ntrait B { fn go(self: Self) -> Int; }\n\
               impl A for Int { fn go(self: Int) -> Int { 1 } }\nimpl B for Int { fn go(self: Int) -> Int { 2 } }\n";
    let src = format!("{two}fn main() -> Int {{ 3.go() }}\n");
    assert_eq!(check_codes(&[("p.nz", &src)]), ["N0606"]);
    // UFCS chooses.
    let src = format!("{two}fn main() -> Int {{ A.go(3) * 10 + B.go(3) }}\n");
    agrees_on(&[("prog.nz", &src)], "12", 0);
    // A type parameter has its bound's methods and no others.
    let src = format!("{SHOW}fn f[T](x: T) -> Str {{ x.show() }}\nfn main() -> Int {{ 0 }}\n");
    assert_eq!(check_codes(&[("p.nz", &src)]), ["N0605"]);
}

#[test]
fn an_impl_in_a_module_not_imported_is_not_in_scope() {
    // `main` reaches a `Tri` through `make.nz` without importing `tri.nz`, where the impl is:
    // the impl is not used, so a cached check of `main` never depends on it.
    let shape = "pub trait Shape { fn sides(self: Self) -> Int; }\n";
    let tri = "use \"shape.nz\";\npub struct Tri { a: Int, }\n\
               impl Shape for Tri { fn sides(self: Tri) -> Int { 3 } }\n";
    let make = "use \"tri.nz\";\npub fn make() -> Tri { Tri(a: 1) }\n";
    let main = "use \"shape.nz\";\nuse \"make.nz\";\nfn main() -> Int { make().sides() }\n";
    assert_eq!(
        check_codes(&[
            ("main.nz", main),
            ("shape.nz", shape),
            ("tri.nz", tri),
            ("make.nz", make)
        ]),
        ["N0605"]
    );
}

#[test]
fn an_impl_out_of_scope_is_not_a_candidate_even_when_another_trait_has_the_method() {
    // Two traits in scope declare `sides`, and both impls for `Tri` are in a module `main`
    // does not import: neither is usable, so this is no method, never an ambiguity between
    // impls the module cannot use.
    let shape = "pub trait Shape { fn sides(self: Self) -> Int; }\n";
    let poly = "pub trait Poly { fn sides(self: Self) -> Int; }\n";
    let tri = "use \"shape.nz\";\nuse \"poly.nz\";\npub struct Tri { a: Int, }\n\
               impl Shape for Tri { fn sides(self: Tri) -> Int { 3 } }\n\
               impl Poly for Tri { fn sides(self: Tri) -> Int { 4 } }\n";
    let make = "use \"tri.nz\";\npub fn make() -> Tri { Tri(a: 1) }\n";
    let main = "use \"shape.nz\";\nuse \"poly.nz\";\nuse \"make.nz\";\nfn main() -> Int { make().sides() }\n";
    assert_eq!(
        check_codes(&[
            ("main.nz", main),
            ("shape.nz", shape),
            ("poly.nz", poly),
            ("tri.nz", tri),
            ("make.nz", make)
        ]),
        ["N0605"]
    );
}

#[test]
fn an_enum_keeps_its_variant_calls_beside_an_imported_trait_of_its_name() {
    // A module may declare an enum `E` and import a trait `E`: `E.A()` is the variant.
    let t = "pub trait E { fn A(self: Self) -> Int; }\n";
    let main = "use \"t.nz\";\nenum E { A, B, }\n\
                fn main() -> Int { match E.A() { E.A() => 1, E.B() => 2, } }\n";
    agrees_on(&[("main.nz", main), ("t.nz", t)], "1", 0);
}

#[test]
fn a_trait_is_not_a_type_and_a_method_is_not_a_value() {
    let src = format!("{SHOW}fn f(s: Show) -> Int {{ 0 }}\nfn main() -> Int {{ 0 }}\n");
    assert_eq!(check_codes(&[("p.nz", &src)]), ["N0607"]);
    let src = format!(
        "{SHOW}impl Show for Int {{ fn show(self: Int) -> Str {{ \"\" }} }}\nfn main() -> Int {{ let f = 1.show; 0 }}\n"
    );
    assert!(check_codes(&[("p.nz", &src)]).contains(&"N0608".to_owned()));
}

#[test]
fn a_trait_method_must_take_self_first_and_may_have_a_default_body() {
    let src = "trait T { fn a(x: Int) -> Int; }\nfn main() -> Int { 0 }\n";
    assert_eq!(check_codes(&[("p.nz", src)]), ["N0609"]);
    // A default body since R1b (L11); until then a syntax error (`N0100`).
    let src = "trait T { fn a(self: Self) -> Int { 1 } }\nfn main() -> Int { 0 }\n";
    assert_eq!(check_codes(&[("p.nz", src)]), Vec::<String>::new());
    // Neither `;` nor a body is still one.
    let src = "trait T { fn a(self: Self) -> Int }\nfn main() -> Int { 0 }\n";
    assert_eq!(check_codes(&[("p.nz", src)]), ["N0100"]);
}

#[test]
fn a_bound_or_an_impl_naming_something_else_is_refused() {
    let src = "struct P { x: Int, }\nfn f[T: P](x: T) -> Int { 0 }\nfn main() -> Int { 0 }\n";
    assert_eq!(check_codes(&[("p.nz", src)]), ["N0365"]);
    let src = "struct P { x: Int, }\nimpl P for Int { fn a(self: Int) -> Int { 1 } }\nfn main() -> Int { 0 }\n";
    assert_eq!(check_codes(&[("p.nz", src)]), ["N0610"]);
    let src = "impl Nowhere for Int { fn a(self: Int) -> Int { 1 } }\nfn main() -> Int { 0 }\n";
    assert_eq!(check_codes(&[("p.nz", src)]), ["N0305"]);
}

#[test]
fn trait_is_a_keyword_now_and_an_enum_keeps_its_variant_calls() {
    let src = "fn trait() -> Int { 1 }\nfn main() -> Int { 0 }\n";
    assert!(!check_codes(&[("p.nz", src)]).is_empty());
    // `E.A()` is still a variant where `E` is an enum, even with a method `A` in scope.
    let src = "trait T { fn A(self: Self) -> Int; }\nenum E { A, B, }\n\
               impl T for E { fn A(self: E) -> Int { 9 } }\n\
               fn main() -> Int { match E.A() { E.A() => 1, E.B() => 2, } }\n";
    agrees_on(&[("prog.nz", src)], "1", 0);
}

// --- what the tools see ---------------------------------------------------------------

#[test]
fn a_method_call_refers_to_its_impls_function_and_a_bound_call_to_the_trait_method() {
    let src = format!(
        "{SHOW}struct P {{ x: Int, }}\n\
         impl Show for P {{ fn show(self: P) -> Str {{ \"p\" }} }}\n\
         fn f[T: Show](x: T) -> Str {{ x.show() }}\n\
         fn main() -> Int {{ str_len(P(x: 1).show()) + str_len(f(P(x: 2))) }}\n"
    );
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("nazm.root"), "").unwrap();
    std::fs::write(d.path().join("main.nz"), &src).unwrap();
    let units = nazm(d.path(), &["resolve", "main.nz"]);
    assert_eq!(units.code, Some(0), "{}", units.stderr);
    let unit: serde_json::Value =
        serde_json::from_str(units.stdout.lines().next().unwrap()).unwrap();
    let kinds: Vec<(String, String)> = unit["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                o["target"]["kind"].as_str().unwrap().to_owned(),
                format!(
                    "{}{}",
                    o["target"]["item"].as_str().unwrap(),
                    o["target"]["member"]
                        .as_str()
                        .map(|m| format!(".{m}"))
                        .unwrap_or_default()
                ),
            )
        })
        .collect();
    assert!(
        kinds.contains(&("function".to_owned(), "main.nz::fn Show/P/show".to_owned())),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&(
            "trait method".to_owned(),
            "main.nz::trait Show.show".to_owned()
        )),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&("trait".to_owned(), "main.nz::trait Show".to_owned())),
        "{kinds:?}"
    );
    let refs = nazm(d.path(), &["references", "main.nz::trait Show", "main.nz"]);
    let refs: serde_json::Value = serde_json::from_str(refs.stdout.trim()).unwrap();
    // The bound and the impl's header.
    assert_eq!(refs["uses"].as_array().unwrap().len(), 2, "{refs}");
}

#[test]
fn the_interface_carries_exported_traits_impls_and_bounds() {
    let src = "pub trait Show { fn show(self: Self) -> Str; }\npub struct P { x: Int, }\n\
               impl Show for P { fn show(self: P) -> Str { \"p\" } }\n\
               pub fn f[T: Show](x: T) -> Str { x.show() }\n";
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("lib.nz"), src).unwrap();
    let r = nazm(d.path(), &["interface", "lib.nz"]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let v: serde_json::Value = serde_json::from_str(r.stdout.trim()).unwrap();
    assert_eq!(v["schema"], "nazm.interface/13");
    assert_eq!(v["traits"][0]["def"], "lib.nz::trait Show");
    assert_eq!(v["impls"][0]["trait"], "lib.nz::trait Show");
    assert_eq!(v["impls"][0]["methods"][0], "lib.nz::fn Show/P/show");
    assert_eq!(
        v["exports"][0]["requires_traits"][0][1],
        "lib.nz::trait Show"
    );
    // Adding an impl moves the fingerprint, so an importer is checked again.
    let before = nazm(d.path(), &["interface", "--hash", "lib.nz"]).stdout;
    std::fs::write(
        d.path().join("lib.nz"),
        format!("{src}impl Show for Int {{ fn show(self: Int) -> Str {{ \"\" }} }}\n"),
    )
    .unwrap();
    let after = nazm(d.path(), &["interface", "--hash", "lib.nz"]).stdout;
    assert_ne!(before, after);
}

#[test]
fn a_bound_is_satisfied_by_an_impl_in_scope_or_by_the_same_bound_and_nothing_else() {
    // No impl for `P`: refused at the call, by the requirement's own code.
    let src = format!(
        "{SHOW}struct P {{ x: Int, }}\nfn f[T: Show](x: T) -> Str {{ x.show() }}\nfn main() -> Int {{ str_len(f(P(x: 1))) }}\n"
    );
    assert_eq!(check_codes(&[("p.nz", &src)]), ["N0364"]);
    // Forwarding a `T` needs the same bound on the caller's `T`.
    let src = format!(
        "{SHOW}fn f[T: Show](x: T) -> Str {{ x.show() }}\nfn g[U](u: U) -> Str {{ f(u) }}\nfn main() -> Int {{ 0 }}\n"
    );
    assert_eq!(check_codes(&[("p.nz", &src)]), ["N0364"]);
    let src = format!(
        "{SHOW}fn f[T: Show](x: T) -> Str {{ x.show() }}\nfn g[U: Show](u: U) -> Str {{ f(u) }}\nfn main() -> Int {{ 0 }}\n"
    );
    assert!(check_codes(&[("p.nz", &src)]).is_empty());
    // A written type argument is held to it too.
    let src = format!(
        "{SHOW}fn f[T: Show](x: T) -> Str {{ x.show() }}\nfn main() -> Int {{ str_len(f[Bool](true)) }}\n"
    );
    assert_eq!(check_codes(&[("p.nz", &src)]), ["N0364"]);
}
