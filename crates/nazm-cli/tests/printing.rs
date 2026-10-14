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
        // `main` holds an `OutCap` it was not written (R1, 5b.1); a function of its that
        // holds none still may not print.
        (
            "fn say() -> Int { println(\"x\"); 0 }\nfn main() -> Int { say() }\n",
            "N0369",
            "printing without the authority to",
        ),
        // Nor may a `main` that writes an `IoCap` and so is given no `OutCap`, once it has
        // lost the `IoCap`'s binding.
        (
            "fn main(io: IoCap) -> Int { let io = 0; println(\"x\"); io }\n",
            "N0369",
            "a main that wrote its authority and shadowed it",
        ),
    ] {
        assert_eq!(differential::codes(src), [code], "{why}");
    }
}
