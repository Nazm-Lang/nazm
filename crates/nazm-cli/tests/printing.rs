//! R1, 5b.4 and 5b.13 (`docs/spec.md`, *Output*): `print` and `eprint` of any value an
//! interpolation's hole takes, and `println` and `eprintln`, which add a newline.
//!
//! The same bytes on each stream in every implementation, nothing live; what has no text is
//! refused by name, and `println` cannot be redefined.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::differential;

#[test]
fn every_kind_of_value_prints_its_text_everywhere() {
    let runs = differential::everywhere(&[(
        "prog.nz",
        "use \"@std/show\"

struct P { x: Int }

impl Show for P { fn show(self: P) -> Str { f\"P({self.x})\" } }

fn main(out: OutCap) -> Int ! {io} {
    let s = \"str\"
    println(\"hello\")
    println(42)
    println(0 - 7)
    println(3.5)
    println(true)
    println(s)
    println(P(x: 7))
    println(UInt8(200))
    print(5)
    print(\" \")
    print(s)
    println()
    eprintln(\"to stderr\")
    eprint(9)
    eprint(\"\\n\")
    0
}
",
    )]);
    let first = differential::memory_line(&runs[0].1);
    for (label, r) in &runs {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert_eq!(
            r.stdout, "hello\n42\n-7\n3.5\ntrue\nstr\nP(7)\n200\n5 str\n0\n",
            "{label}"
        );
        assert!(r.stderr.starts_with("to stderr\n9"), "{label}: {r:?}");
        let line = differential::memory_line(r);
        assert_eq!(line, first, "{label}");
        assert!(
            line.split_whitespace()
                .filter(|w| w.starts_with("live="))
                .all(|w| w == "live=0"),
            "{label}: {line}"
        );
    }
}

#[test]
fn what_has_no_text_or_takes_the_name_is_refused() {
    for (src, code, why) in [
        (
            "struct Q { y: Int }\nfn main(out: OutCap) -> Int ! {io} { println(Q(y: 1)); 0 }\n",
            "N0300",
            "a record with no `Show`",
        ),
        (
            "fn println(s: Str) -> Int { 0 }\nfn main() -> Int { 0 }\n",
            "N0203",
            "a function named `println`",
        ),
    ] {
        assert_eq!(differential::codes(src), [code], "{why}");
    }
    // `main` holds an `OutCap` it was not written (R1, 5b.1), and since D8 (6.7) so does every
    // function of the program itself: a function of its passed none prints, and so does a `main`
    // that wrote an `IoCap` and then lost the `IoCap`'s binding.
    for (src, why) in [
        (
            "fn say() -> Int { println(\"x\"); 0 }\nfn main() -> Int { say() }\n",
            "printing in the program's own function",
        ),
        (
            "fn main(io: IoCap) -> Int { let io = 0; println(\"x\"); io }\n",
            "a main that wrote its authority and shadowed it",
        ),
    ] {
        assert!(differential::codes(src).is_empty(), "{why}");
    }
    // That `main` is still given no `OutCap` beside the `IoCap` it wrote: its authority is
    // the `IoCap` alone, and none is implicit.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("p.nz"),
        "fn main(io: IoCap) -> Int { let io = 0; println(\"x\"); io }\n",
    )
    .unwrap();
    let r = differential::nazm(d.path(), &["inspect", "p.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
    let doc: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
    let caps: Vec<(&str, bool)> = doc["authority"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["capability"].as_str().unwrap(),
                a["implicit"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(caps, [("IoCap", false)], "{}", r.stdout);
    // A package's function that holds none still may not print: printing without the
    // authority to is refused there, in its own module.
    let d = tempfile::tempdir().unwrap();
    for (rel, text) in [
        (
            "lib/nazm.toml",
            "schema = \"nazm.package/1\"\n[package]\nname = \"lib\"\nversion = \"1.0.0\"\n",
        ),
        (
            "lib/src/say.nz",
            "pub fn say() -> Int { println(\"x\"); 0 }\n",
        ),
        (
            "app/nazm.toml",
            "schema = \"nazm.package/1\"\n[package]\nname = \"app\"\nversion = \"1.0.0\"\n\
             main = \"main.nz\"\n[dependencies]\nlib = { path = \"../lib\", version = \"1.0.0\" }\n",
        ),
        (
            "app/src/main.nz",
            "use \"lib:say.nz\";\nfn main() -> Int { say() }\n",
        ),
    ] {
        let p = d.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    let r = differential::nazm(d.path(), &["check", "app"]);
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(
        r.stderr.contains("N0369") && r.stderr.contains("`say` holds no `OutCap`"),
        "printing without the authority to: {r:?}"
    );
}
