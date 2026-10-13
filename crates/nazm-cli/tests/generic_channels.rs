//! N53: generic channels.
//!
//! `docs/architecture.md` §7.55 and the spec section *Generic channels*. What only a whole run
//! shows: that a `Chan[T]` of strings, records and enums means the same in the interpreter and
//! in both native backends — the same stdout, exit status and memory report — that a value still
//! queued when the last reference goes is released exactly once, that a send after close is
//! refused and its value released by the sender, and that an element which cannot cross into a
//! task is refused before anything runs.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;
use crate::differential;

use common::Ran;
use differential::{agrees, agrees_on, codes, nazm};

// --- values across tasks ------------------------------------------------------------------

#[test]
fn records_and_strings_cross_tasks_through_typed_channels_everywhere() {
    agrees(
        r#"
struct Job { name: Str, size: Int, }

fn worker(jobs: Chan[Job], done: Chan[Str]) -> Int ! {} {
    let got = vec_new[Job]();
    while chan_recv_of(jobs, got) {
        let j = vec_pop(got);
        chan_send_of(done, str_concat(j.name, int_to_str(j.size)));
    }
    chan_close_of(done)
}

fn main(io: IoCap, sp: SpawnCap) -> Int ! { io, spawn } {
    let jobs = chan_new_of[Job](2);
    let done = chan_new_of[Str](4);
    scope {
        spawn worker(jobs, done);
        chan_send_of(jobs, Job(name: str_concat("a", "b"), size: 1));
        chan_send_of(jobs, Job(name: "cd", size: 2));
        chan_close_of(jobs);
    }
    let out = vec_new[Str]();
    while chan_recv_of(done, out) { print(vec_pop(out)); print(";"); }
    0
}
"#,
        "ab1;cd2;0",
    );
}

#[test]
fn an_enum_with_a_payload_and_plain_values_cross_a_channel() {
    agrees(
        r#"
enum Msg { Word(text: Str), Stop, }

fn main(out: OutCap) -> Int {
    let c = chan_new_of[Msg](3);
    chan_send_of(c, Msg.Word(text: str_concat("he", "y")));
    chan_send_of(c, Msg.Stop());
    chan_close_of(c);
    let got = vec_new[Msg]();
    let mut n = 0;
    while chan_recv_of(c, got) {
        match vec_pop(got) {
            Msg.Word(text: w) => { print(w); n = n + 1; },
            Msg.Stop() => { print("."); n = n + 10; },
        }
    }
    let b = chan_new_of[Bool](1);
    chan_send_of(b, true);
    let bs = vec_new[Bool]();
    if chan_recv_of(b, bs) { if vec_pop(bs) { n = n + 100; } }
    print(int_to_str(n));
    0
}
"#,
        "hey.1110",
    );
}

// --- lifetimes ----------------------------------------------------------------------------

#[test]
fn a_value_still_queued_when_the_channel_goes_is_released_once() {
    let line = agrees(
        r#"
fn main() -> Int {
    let c = chan_new_of[Str](3);
    chan_send_of(c, str_concat("left", "over"));
    chan_send_of(c, int_to_str(42));
    0
}
"#,
        "0",
    );
    assert!(line.contains("channels allocated=1"), "{line}");
}

#[test]
fn a_queued_string_outlives_the_binding_it_was_sent_from() {
    // The channel holds its own reference: the sender's binding goes at the end of `put`,
    // the freed-size allocations after it would reuse a string the channel did not hold,
    // and the value received must still be the one sent.
    agrees(
        r#"
fn put(c: Chan[Str]) -> Bool {
    let s = str_concat("ab", "cd");
    chan_send_of(c, s)
}

fn main(out: OutCap) -> Int {
    let c = chan_new_of[Str](1);
    put(c);
    let junk = vec_new[Str]();
    let mut i = 0;
    while i < 64 {
        vec_push(junk, str_concat("zz", "zz"));
        i = i + 1;
    }
    let got = vec_new[Str]();
    if chan_recv_of(c, got) { print(vec_pop(got)); }
    0
}
"#,
        "abcd0",
    );
}

#[test]
fn a_send_after_close_is_refused_and_its_value_released() {
    agrees(
        r#"
fn main(out: OutCap) -> Int {
    let c = chan_new_of[Str](2);
    chan_close_of(c);
    let sent = chan_send_of(c, str_concat("never", "queued"));
    let got = vec_new[Str]();
    let r = chan_recv_of(c, got);
    if sent || r { print("wrong"); }
    print(int_to_str(vec_len(got)));
    0
}
"#,
        "00",
    );
}

// --- across modules ---------------------------------------------------------------------

#[test]
fn a_channel_type_crosses_a_module_boundary_and_its_interface() {
    let lib = "pub struct Item { tag: Str, }\n\
               pub fn put(c: Chan[Item], s: Str) -> Bool ! {} { chan_send_of(c, Item(tag: s)) }\n";
    let main = "use \"lib.nz\";\n\
                fn main(out: OutCap) -> Int {\n    let c = chan_new_of[Item](1);\n    \
                put(c, str_concat(\"x\", \"y\"));\n    let v = vec_new[Item]();\n    \
                if chan_recv_of(c, v) { print(vec_pop(v).tag); }\n    0\n}\n";
    agrees_on(&[("main.nz", main), ("lib.nz", lib)], "xy0", 0);

    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("nazm.root"), "").unwrap();
    std::fs::write(d.path().join("main.nz"), main).unwrap();
    std::fs::write(d.path().join("lib.nz"), lib).unwrap();
    let report = |r: &Ran| -> serde_json::Value {
        r.stdout
            .lines()
            .find(|l| l.contains("nazm.check-report/1"))
            .map(|l| serde_json::from_str(l).unwrap())
            .unwrap_or_else(|| panic!("no cache report: {r:?}"))
    };
    let first = nazm(d.path(), &["check", "main.nz", "--cache-report", "--json"]);
    assert_eq!(first.code, Some(0), "{first:?}");
    assert_eq!(report(&first)["declined"].as_u64(), Some(0), "{first:?}");
    let again = nazm(d.path(), &["check", "main.nz", "--cache-report", "--json"]);
    assert_eq!(again.code, Some(0), "{again:?}");
    let r = report(&again);
    assert_eq!(r["checked"].as_u64(), Some(0), "{r}");
    assert!(r["reused"].as_u64().unwrap() >= 2, "{r}");

    // An edited `main` is checked again against the library's interface as read back — so the
    // channel type that comes back must be the channel type that went in.
    std::fs::write(d.path().join("main.nz"), main.replace("\"x\"", "\"z\"")).unwrap();
    let edited = nazm(d.path(), &["check", "main.nz", "--cache-report", "--json"]);
    assert_eq!(edited.code, Some(0), "{edited:?}");
    let r = report(&edited);
    assert_eq!(r["checked"].as_u64(), Some(1), "{r}");
    let wrong =
        "use \"lib.nz\";\nfn main() -> Int { let c = chan_new_of[Str](1); put(c, \"q\"); 0 }\n";
    std::fs::write(d.path().join("main.nz"), wrong).unwrap();
    let refused = nazm(d.path(), &["check", "main.nz", "--cache-report", "--json"]);
    assert_eq!(
        refused.code,
        Some(1),
        "a `Chan[Str]` passed for a `Chan[Item]`: {refused:?}"
    );
}

// --- facts ------------------------------------------------------------------------------

#[test]
fn a_typed_channel_s_operations_are_resource_sites() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn main() -> Int {\n    let c = chan_new_of[Str](1);\n    \
         chan_send_of(c, \"a\");\n    let v = vec_new[Str]();\n    \
         if chan_recv_of(c, v) { 1 } else { 0 }\n}\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["explain-cost", "prog.nz", "--json"]);
    assert_eq!(r.code, Some(0), "{r:?}");
    let v: serde_json::Value = serde_json::from_str(r.stdout.trim()).unwrap();
    let totals = &v["totals"];
    assert_eq!(totals["channel"]["at-most-once"].as_u64(), Some(3), "{v}");
    assert_eq!(totals["block"]["at-most-once"].as_u64(), Some(2), "{v}");
    assert!(
        totals["allocation"]["at-most-once"].as_u64().unwrap() >= 3,
        "{v}"
    );
}

// --- refusals ---------------------------------------------------------------------------

#[test]
fn an_element_that_cannot_cross_into_a_task_is_refused() {
    assert_eq!(
        codes("fn main() -> Int { let c = chan_new_of[Ints](1); 0 }\n"),
        ["N0390"]
    );
    assert_eq!(
        codes("fn f(c: Chan[Vec[Int]]) -> Int { 0 }\nfn main() -> Int { 0 }\n"),
        ["N0390"]
    );
    assert_eq!(
        codes(
            "struct Bag { xs: Ints, }\n\
             fn main() -> Int { let c = chan_new_of[Bag](1); 0 }\n"
        ),
        ["N0390"]
    );
    // A closure may hold storage it shares with its maker: it does not cross either.
    assert_eq!(
        codes("fn f(c: Chan[fn(Int) -> Int]) -> Int { 0 }\nfn main() -> Int { 0 }\n"),
        ["N0390"]
    );
    // Strings, records of plain values and enums do.
    assert_eq!(
        codes(
            "struct P { a: Str, b: Int, }\nenum E { A(s: Str), B, }\n\
             fn f(c: Chan[P], d: Chan[E], e: Chan[Str], g: Chan[Bool]) -> Int { 0 }\n\
             fn main() -> Int { 0 }\n"
        ),
        Vec::<String>::new()
    );
}

#[test]
fn a_typed_channel_carries_only_its_element() {
    assert!(
        !codes("fn main() -> Int { let c = chan_new_of[Str](1); chan_send_of(c, 3); 0 }\n")
            .is_empty()
    );
    assert!(!codes("fn f(c: Chan[Int, Int]) -> Int { 0 }\nfn main() -> Int { 0 }\n").is_empty());
    // A vector of the element is not a channel of it, to a generic built-in either.
    assert!(
        !codes("fn main() -> Int { let v = vec_new[Int](); chan_send_of(v, 1); 0 }\n").is_empty()
    );
}
