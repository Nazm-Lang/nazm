//! Gate 2: attributes and conditional compilation.
//!
//! `docs/general-purpose.md` §16 and §17, `docs/spec.md` *Attributes* and *Conditional
//! compilation*. What this holds: a declaration whose `@cfg` does not hold is not there — not
//! resolved, not checked, not a duplicate — in every implementation; the build's target and
//! profiles decide, and a check filed under one configuration is not reused under another; the
//! vocabulary is closed and refused by name (`N0619`); a malformed attribute is one error; and
//! the formatter's layout.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::differential;

use differential::{agrees, check_codes, codes, nazm};

/// The host's operating system as `@cfg(os = …)` names it, another one, and a triple for it.
fn host_and_other() -> (&'static str, &'static str, &'static str) {
    if cfg!(target_os = "macos") {
        ("macos", "linux", "x86_64-unknown-linux-gnu")
    } else if cfg!(windows) {
        ("windows", "linux", "x86_64-unknown-linux-gnu")
    } else {
        ("linux", "macos", "aarch64-apple-darwin")
    }
}

#[test]
fn the_declaration_whose_condition_holds_is_the_one_every_implementation_runs() {
    let (host, other, _) = host_and_other();
    let src = format!(
        "@cfg(os = \"{host}\")\nfn place() -> Str {{ \"{host}\" }}\n\n\
         @cfg(os = \"{other}\")\nfn place() -> Str {{ \"{other}\" }}\n\n\
         @cfg(not(os = \"{host}\"))\nfn place() -> Str {{ \"not here\" }}\n\n\
         @cfg(all(os = \"{host}\", not(os = \"{other}\"), any(arch = \"nowhere\", target = \"nowhere\")))\n\
         fn never() -> Int {{ 1 }}\n\n\
         @cfg(any(arch = \"nowhere\", os = \"{host}\"))\nfn never() -> Int {{ 0 }}\n\n\
         fn main(out: OutCap) -> Int {{\n    print(str_concat(place(), \"\\n\"));\n    never()\n}}\n"
    );
    agrees(&src, &format!("{host}\n0"));
}

#[test]
fn an_impl_left_out_takes_its_methods_and_is_not_a_second_impl() {
    let src = "trait Show { fn show(self: Self) -> Int; }\n\n\
               @cfg(os = \"nowhere\")\nimpl Show for Int { fn show(self: Int) -> Int { \"never checked\" } }\n\n\
               @cfg(not(os = \"nowhere\"))\nimpl Show for Int { fn show(self: Int) -> Int { self + 2 } }\n\n\
               fn main(out: OutCap) -> Int {\n    print(str_concat(int_to_str(5.show()), \"\\n\"));\n    0\n}\n";
    agrees(src, "7\n0");
}

#[test]
fn code_only_another_target_compiles_is_not_checked_until_built_for_it() {
    let (_, other, triple) = host_and_other();
    // Ill-typed on purpose: only a build for `other` reaches it.
    let src = format!(
        "@cfg(os = \"{other}\")\nfn f() -> Int {{ \"not an Int\" }}\n\n\
         @cfg(not(os = \"{other}\"))\nfn f() -> Int {{ 0 }}\n\n\
         fn main() -> Int {{ f() }}\n"
    );
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), &src).unwrap();
    let here = nazm(d.path(), &["check", "prog.nz"]);
    assert_eq!(here.code, Some(0), "{}", here.stderr);
    let there = nazm(
        d.path(),
        &["build", "--target", triple, "--objects", "objs", "prog.nz"],
    );
    assert_ne!(there.code, Some(0), "{}", there.stderr);
    assert!(there.stderr.contains("N0300"), "{}", there.stderr);
}

#[test]
fn the_profiles_held_decide_and_a_check_is_not_reused_across_them() {
    // Ill-typed only under `general`: the check that passes without the profile is cached, and
    // the one with it must not reuse it.
    let src = "@cfg(profile = \"general\")\nfn f() -> Int { \"held\" }\n\n\
               @cfg(not(profile = \"general\"))\nfn f() -> Int { 0 }\n\n\
               fn main() -> Int { f() }\n";
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let plain = nazm(d.path(), &["check", "prog.nz"]);
    assert_eq!(plain.code, Some(0), "{}", plain.stderr);
    let held = nazm(d.path(), &["check", "--profile", "general", "prog.nz"]);
    assert_ne!(held.code, Some(0), "{}", held.stderr);
    assert!(held.stderr.contains("N0300"), "{}", held.stderr);
    let run = nazm(
        d.path(),
        &["run", "--interpret", "--profile", "general", "prog.nz"],
    );
    assert!(run.stderr.contains("N0300"), "{}", run.stderr);
    let again = nazm(d.path(), &["check", "prog.nz"]);
    assert_eq!(again.code, Some(0), "{}", again.stderr);
}

#[test]
fn every_attribute_outside_the_vocabulary_or_its_place_is_refused_by_name() {
    for (src, why) in [
        ("@bogus\nfn f() -> Int { 0 }\nfn main() -> Int { 0 }\n", "unknown"),
        ("@test\n@test\nfn f() -> Result[Int, Str] { Result[Int, Str].Ok(value: 0) }\nfn main() -> Int { 0 }\n", "twice"),
        ("@test\nstruct P { x: Int, }\nfn main() -> Int { 0 }\n", "test on a type"),
        ("@bench\nconst K: Int = 1;\nfn main() -> Int { 0 }\n", "bench on a constant"),
        ("@test(often)\nfn f() -> Result[Int, Str] { Result[Int, Str].Ok(value: 0) }\nfn main() -> Int { 0 }\n", "test's argument"),
        ("@fuzz(often)\nfn f(input: Bytes) -> Result[Int, Str] { Result[Int, Str].Ok(value: 0) }\nfn main() -> Int { 0 }\n", "fuzz's argument"),
        (
            "trait T { fn t(self: Self) -> Int; }\n@deprecated\nimpl T for Int { fn t(self: Int) -> Int { 0 } }\nfn main() -> Int { 0 }\n",
            "deprecated on an impl",
        ),
        ("@deprecated(why)\npub fn f() -> Int { 0 }\nfn main() -> Int { 0 }\n", "deprecated's argument"),
        ("@deprecated\nfn f() -> Int { 0 }\nfn main() -> Int { 0 }\n", "deprecated on a private function"),
        ("@deprecated\nconst K: Int = 1;\nfn main() -> Int { K }\n", "deprecated on a constant"),
        ("@deprecated\npub trait T { fn t(self: Self) -> Int; }\nfn main() -> Int { 0 }\n", "deprecated on a trait"),
        ("@cfg\nfn f() -> Int { 0 }\nfn main() -> Int { 0 }\n", "no condition"),
        ("@cfg(os = \"linux\", arch = \"x86_64\")\nfn f() -> Int { 0 }\nfn main() -> Int { 0 }\n", "two conditions"),
        ("@cfg(colour = \"red\")\nfn f() -> Int { 0 }\nfn main() -> Int { 0 }\n", "an unknown key"),
        ("@cfg(either(os = \"linux\"))\nfn f() -> Int { 0 }\nfn main() -> Int { 0 }\n", "an unknown combiner"),
        ("@cfg(not(os = \"a\", os = \"b\"))\nfn f() -> Int { 0 }\nfn main() -> Int { 0 }\n", "not of two"),
        ("@cfg(linux)\nfn f() -> Int { 0 }\nfn main() -> Int { 0 }\n", "a bare word"),
    ] {
        assert_eq!(codes(src), ["N0619"], "{why}");
    }
}

#[test]
fn a_malformed_attribute_is_one_syntax_error_and_an_attribute_before_a_use_is_refused() {
    assert_eq!(
        codes("@cfg(os = 1)\nfn f() -> Int { 0 }\nfn main() -> Int { 0 }\n"),
        ["N0100"]
    );
    assert_eq!(
        codes("@cfg(all(os = \"a\" os = \"b\"))\nfn f() -> Int { 0 }\nfn main() -> Int { 0 }\n"),
        ["N0100"]
    );
    assert_eq!(
        codes("@\nfn f() -> Int { 0 }\nfn main() -> Int { 0 }\n"),
        ["N0100"]
    );
    assert_eq!(
        check_codes(&[
            (
                "prog.nz",
                "@cfg(os = \"linux\")\nuse \"other.nz\";\nfn main() -> Int { 0 }\n"
            ),
            ("other.nz", "pub fn g() -> Int { 0 }\n"),
        ]),
        ["N0100"]
    );
}

#[test]
fn the_formatter_puts_each_attribute_on_its_own_line() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "@ test @cfg( all( os = \"linux\" , not( arch=\"x86_64\" ) ) ) fn f() -> Int { 0 }\n\
         @deprecated( \"use f\" )\nfn g() -> Int { 0 }\nfn main() -> Int { 0 }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["fmt", "prog.nz"]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let text = std::fs::read_to_string(d.path().join("prog.nz")).unwrap();
    assert_eq!(
        text,
        "@test\n@cfg(all(os = \"linux\", not(arch = \"x86_64\")))\nfn f() -> Int { 0 }\n\
         @deprecated(\"use f\")\nfn g() -> Int { 0 }\nfn main() -> Int { 0 }\n"
    );
    let again = nazm(d.path(), &["fmt", "--check", "prog.nz"]);
    assert_eq!(again.code, Some(0), "{}", again.stdout);
}

/// G2-C1: `@deprecated` is metadata-only — no diagnostic reports a use — and `nazm doc` publishes
/// it on the `pub` function, `struct` or `enum` it marks, with its reason or none, in the JSON and
/// the Markdown alike.
#[test]
fn deprecation_is_metadata_the_api_documentation_publishes() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("lib.nz"),
        "@deprecated(\"use `area` instead\")\npub fn size(n: Int) -> Int { n * n }\n\n\
         pub fn area(n: Int) -> Int { n * n }\n\n\
         @deprecated\npub struct Old { x: Int, }\n\n\
         @deprecated(\"one shape now\")\npub enum Shape { Square, }\n\n\
         fn main() -> Int { size(2) }\n",
    )
    .unwrap();
    let checked = nazm(d.path(), &["check", "lib.nz"]);
    assert_eq!(checked.code, Some(0), "{}", checked.stderr);
    // A use of `size` is reported nowhere: no warning severity exists in 1.0.
    assert!(checked.stderr.is_empty(), "{}", checked.stderr);
    let r = nazm(d.path(), &["doc", "lib.nz"]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let doc: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
    let items: Vec<&serde_json::Value> = doc["modules"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["items"].as_array().unwrap())
        .collect();
    let by = |name: &str| {
        *items
            .iter()
            .find(|i| i["name"] == name)
            .unwrap_or_else(|| panic!("{name}: {doc}"))
    };
    assert_eq!(by("size")["deprecated"], "use `area` instead");
    assert_eq!(by("Old")["deprecated"], "");
    assert_eq!(by("Shape")["deprecated"], "one shape now");
    assert!(by("area").get("deprecated").is_none(), "{doc}");
    let out = d.path().join("api");
    let w = nazm(d.path(), &["doc", "lib.nz", "-o", out.to_str().unwrap()]);
    assert_eq!(w.code, Some(0), "{}", w.stderr);
    let md = std::fs::read_to_string(out.join("api.md")).unwrap();
    assert!(md.contains("**Deprecated:** use `area` instead"), "{md}");
    assert!(md.contains("**Deprecated.**"), "{md}");
}
