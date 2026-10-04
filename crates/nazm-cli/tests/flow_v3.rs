//! N80: provenance v3 — container cells, indirect targets, local shapes, one policy engine.
//!
//! `docs/architecture.md` §7.81. What the commands show, each in both directions: a container
//! carries what any container of its type was given, and nothing from another type; a receive
//! and a select move a channel's cell, and a select's index carries nothing; generic code is the
//! wild cell; a call through a function value or a bound carries what its candidates return, and
//! a closure's parameters and captures what they are handed; fields and variants are apart
//! within a body and whole across a call; an export to C is handed anything; `cyber`'s
//! `checked-paths` is the language's rule in every function; `nazm explain-flow` prints each
//! origin's chain as `nazm.flow/2`; and nothing that runs changes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn program(src: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), src).unwrap();
    d
}

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// The codes `nazm check` reports, in order; empty when it accepts.
fn codes(src: &str, extra: &[&str]) -> Vec<String> {
    let d = program(src);
    let mut args = vec!["check", "p.nz", "--json"];
    args.extend_from_slice(extra);
    let o = nazm(d.path(), &args);
    let found: Vec<String> = String::from_utf8_lossy(&o.stderr)
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|d| d["code"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(o.status.success(), found.is_empty(), "{}", text(&o));
    found
}

fn accepted(src: &str) {
    assert!(codes(src, &[]).is_empty(), "{src}");
}

fn refused(src: &str) {
    assert_eq!(codes(src, &[]), ["N0372"], "{src}");
}

/// The human `nazm check` output, for a refused program.
fn explained(src: &str) -> String {
    let d = program(src);
    text(&nazm(d.path(), &["check", "p.nz"]))
}

const MAIN: &str = "fn main(io: IoCap) -> Int ! { io } {\n";

#[test]
fn a_container_carries_what_any_container_of_its_type_was_given() {
    accepted(&format!(
        "{MAIN} let xs = strs_new(); strs_push(xs, arg(1)); write_file(strs_get(xs, 0), \"x\")\n}}\n"
    ));
    refused(&format!(
        "{MAIN} let xs = strs_new(); strs_push(xs, read_file(\"c\")); write_file(strs_get(xs, 0), \"x\")\n}}\n"
    ));
    // Another container of the same type, written anywhere, is the same cell…
    let other = "fn fill(io: IoCap) -> Int ! { io } { let ys = strs_new(); strs_push(ys, read_file(\"c\")); 0 }\n\
                 fn main(io: IoCap) -> Int ! { io } { let xs = strs_new(); strs_push(xs, arg(1)); fill(io); write_file(strs_get(xs, 0), \"x\") }\n";
    refused(other);
    let why = explained(other);
    assert!(
        why.contains("in `fill`: data from the contents of a file is written into a `Strs` here"),
        "{why}"
    );
    // …and a container of another type is not.
    accepted(
        "fn fill(io: IoCap) -> Int ! { io } { let ys = vec_new[Int](); vec_push(ys, str_len(read_file(\"c\"))); 0 }\n\
         fn main(io: IoCap) -> Int ! { io } { let xs = vec_new[Str](); vec_push(xs, arg(1)); fill(io); write_file(vec_get(xs, 0), \"x\") }\n",
    );
    // Set, pop, length and join read and write the same cell.
    refused(&format!(
        "{MAIN} let xs = strs_new(); strs_push(xs, \"a\"); strs_set(xs, 0, read_file(\"c\")); write_file(strs_pop(xs), \"x\")\n}}\n"
    ));
    refused(&format!(
        "{MAIN} let xs = strs_new(); strs_push(xs, read_file(\"c\")); write_file(str_join(xs, \"/\"), \"x\")\n}}\n"
    ));
    refused(&format!(
        "{MAIN} let xs = ints_new(); ints_push(xs, str_len(read_file(\"c\"))); write_file(int_to_str(ints_len(xs)), \"x\")\n}}\n"
    ));
}

#[test]
fn a_receive_and_a_select_move_a_channels_cell_and_an_index_carries_nothing() {
    refused(&format!(
        "{MAIN} let c = chan_new_of[Str](4); chan_send_of(c, read_file(\"c\")); let out = vec_new[Str](); let got = chan_recv_of(c, out); write_file(vec_get(out, 0), \"x\")\n}}\n"
    ));
    accepted(&format!(
        "{MAIN} let c = chan_new_of[Str](4); chan_send_of(c, arg(1)); let out = vec_new[Str](); let got = chan_recv_of(c, out); write_file(vec_get(out, 0), \"x\")\n}}\n"
    ));
    let select = |path: &str| {
        format!(
            "{MAIN} let c = chan_new_of[Str](4); chan_send_of(c, read_file(\"c\")); let cs = vec_new[Chan[Str]](); vec_push(cs, c); let out = vec_new[Str](); let i = chan_select_of(cs, out); write_file({path}, \"x\")\n}}\n"
        )
    };
    refused(&select("vec_get(out, 0)"));
    // Which channel was ready is control, as a condition is: it carries nothing.
    accepted(&select("int_to_str(i)"));
    // The legacy `Chan` moves into an `Ints`.
    refused(&format!(
        "{MAIN} let c = chan_new(1); chan_send(c, str_len(read_file(\"c\"))); let b = ints_new(); let got = chan_recv(c, b); write_file(int_to_str(ints_pop(b)), \"x\")\n}}\n"
    ));
}

#[test]
fn a_container_in_generic_code_is_every_container() {
    let src = "fn put[T](v: Vec[T], x: T) -> Int ! {} { vec_push(v, x); 0 }\n\
               fn main(io: IoCap) -> Int ! { io } { let ps = vec_new[Str](); let qs = vec_new[Int](); put(ps, arg(1)); put(qs, str_len(read_file(\"c\"))); write_file(vec_get(ps, 0), \"x\") }\n";
    refused(src);
    let why = explained(src);
    assert!(why.contains("a container in generic code"), "{why}");
    assert!(why.contains("`put` is called here"), "{why}");
    // With nothing restricted written anywhere, generic code adds nothing.
    accepted(
        "fn put[T](v: Vec[T], x: T) -> Int ! {} { vec_push(v, x); 0 }\n\
         fn main(io: IoCap) -> Int ! { io } { let ps = vec_new[Str](); put(ps, arg(1)); write_file(vec_get(ps, 0), \"x\") }\n",
    );
}

#[test]
fn a_call_through_a_value_carries_what_its_candidates_return() {
    // A named function as a value, and a closure, of the call's type.
    refused(
        "fn reader(io: IoCap) -> Str ! { io } { read_file(\"c\") }\n\
         fn main(io: IoCap) -> Int ! { io } { let f = reader; write_file(f(io), \"x\") }\n",
    );
    accepted(
        "fn named(io: IoCap) -> Str ! { io } { \"out.txt\" }\n\
         fn main(io: IoCap) -> Int ! { io } { let f = named; write_file(f(io), \"x\") }\n",
    );
    // A function value of another type is not a candidate.
    accepted(
        "fn reader(n: Int) -> Str ! {} { int_to_str(n) }\n\
         fn bad(io: IoCap) -> Str ! { io } { read_file(\"c\") }\n\
         fn main(io: IoCap) -> Int ! { io } { let g = bad; let f = reader; write_file(f(1), \"x\") }\n",
    );
    // A pure value where an effectful one is expected (N79) is still a candidate.
    refused(
        "fn apply(f: fn(Str) -> Str ! { io }, s: Str) -> Str ! { io } { f(s) }\n\
         fn same(s: Str) -> Str ! {} { s }\n\
         fn main(io: IoCap) -> Int ! { io } { write_file(apply(same, read_file(\"c\")), \"x\") }\n",
    );
}

#[test]
fn a_closures_parameters_and_captures_are_what_it_is_handed() {
    let apply = "fn apply(f: fn(Str) -> Int ! { io }, s: Str) -> Int ! { io } { f(s) }\n";
    accepted(&format!(
        "{apply}fn main(io: IoCap) -> Int ! {{ io }} {{ let w = fn(p: Str) -> Int ! {{ io }} {{ write_file(p, \"x\") }}; apply(w, arg(1)) }}\n"
    ));
    refused(&format!(
        "{apply}fn main(io: IoCap) -> Int ! {{ io }} {{ let w = fn(p: Str) -> Int ! {{ io }} {{ write_file(p, \"x\") }}; apply(w, read_file(\"c\")) }}\n"
    ));
    accepted(&format!(
        "{MAIN} let p = arg(1); let w = fn(d: Str) -> Int ! {{ io }} {{ write_file(p, d) }}; w(read_file(\"c\"))\n}}\n"
    ));
    let captured = format!(
        "{MAIN} let p = read_file(\"c\"); let w = fn(d: Str) -> Int ! {{ io }} {{ write_file(p, d) }}; w(\"x\")\n}}\n"
    );
    refused(&captured);
    let why = explained(&captured);
    assert!(
        why.contains("this closure writes to a path made from what it captures, `p`"),
        "{why}"
    );
    // A closure that returns what it captured hands it to whoever calls through the value.
    refused(&format!(
        "{MAIN} let p = read_file(\"c\"); let g = fn(n: Int) -> Str ! {{}} {{ p }}; write_file(g(1), \"x\")\n}}\n"
    ));
    accepted(&format!(
        "{MAIN} let p = arg(1); let g = fn(n: Int) -> Str ! {{}} {{ p }}; write_file(g(1), \"x\")\n}}\n"
    ));
    // What a closure's parameter is handed reaches its sinks as solved (N80): the call
    // through the value that hands it a file's contents is on the closure's output.
    let d = program(&format!(
        "{MAIN} let e = fn(t: Str) -> Int ! {{ io }} {{ print(t) }}; e(read_file(\"c\"))\n}}\n"
    ));
    let o = nazm(d.path(), &["explain-flow", "--json", "p.nz"]);
    let flow: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let shown = flow["sinks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["function"] == "main#0")
        .unwrap();
    assert_eq!(shown["origins"], serde_json::json!(["file"]), "{flow}");
    // A capture of a parameter is what the maker's callers hand it.
    refused(
        "fn make(io: IoCap, p: Str) -> Int ! { io } { let w = fn(d: Str) -> Int ! { io } { write_file(p, d) }; w(\"x\") }\n\
         fn main(io: IoCap) -> Int ! { io } { make(io, read_file(\"c\")) }\n",
    );
    accepted(
        "fn make(io: IoCap, p: Str) -> Int ! { io } { let w = fn(d: Str) -> Int ! { io } { write_file(p, d) }; w(\"x\") }\n\
         fn main(io: IoCap) -> Int ! { io } { make(io, arg(1)) }\n",
    );
}

#[test]
fn fields_and_variants_are_apart_within_a_body_and_whole_across_a_call() {
    let job = "struct Job { path: Str, body: Str }\n";
    accepted(&format!(
        "{job}{MAIN} let j = Job(path: arg(1), body: read_file(\"c\")); write_file(j.path, j.body)\n}}\n"
    ));
    refused(&format!(
        "{job}{MAIN} let j = Job(path: read_file(\"c\"), body: arg(1)); write_file(j.path, j.body)\n}}\n"
    ));
    // Across a call the record is one value: its fields are all of it.
    refused(&format!(
        "{job}fn path_of(j: Job) -> Str ! {{}} {{ j.path }}\n{MAIN} let j = Job(path: arg(1), body: read_file(\"c\")); write_file(path_of(j), \"x\")\n}}\n"
    ));
    // A field written through a place makes the binding whole.
    refused(&format!(
        "{job}{MAIN} let mut j = Job(path: arg(1), body: \"b\"); j.body = read_file(\"c\"); write_file(j.path, \"x\")\n}}\n"
    ));
    let src = "enum Src { Arg(path: Str), File(text: Str) }\n";
    accepted(&format!(
        "{src}{MAIN} let s = Src.Arg(path: arg(1)); let t = Src.File(text: read_file(\"c\")); match s {{ Src.Arg(path: p) => write_file(p, \"x\"), Src.File(text: q) => 0 }}\n}}\n"
    ));
    refused(&format!(
        "{src}{MAIN} let mut s = Src.Arg(path: arg(1)); s = Src.File(text: read_file(\"c\")); match s {{ Src.Arg(path: p) => 0, Src.File(text: q) => write_file(q, \"x\") }}\n}}\n"
    ));
    // `?` reads the payload, not the error.
    accepted(
        "fn pick(io: IoCap) -> Result[Str, Str] ! { io } { let mut r = Result[Str, Str].Ok(value: arg(1)); r = Result[Str, Str].Err(error: read_file(\"c\")); let p = r?; Result[Str, Str].Ok(value: p) }\n\
         fn main(io: IoCap) -> Int ! { io } { match pick(io) { Result.Ok(value: p) => write_file(p, \"x\"), Result.Err(error: e) => 0 } }\n",
    );
}

#[test]
fn a_call_through_a_bound_reaches_every_impl_of_its_method() {
    let head = "trait Src { fn get(self: Self) -> Str; }\nstruct A { n: Int }\nstruct B { n: Int }\n\
                impl Src for A { fn get(self: A) -> Str ! {} { \"a\" } }\n\
                fn fill(io: IoCap) -> Int ! { io } { let ys = strs_new(); strs_push(ys, read_file(\"c\")); 0 }\n\
                fn path[T: Src](x: T) -> Str ! {} { x.get() }\n\
                fn main(io: IoCap) -> Int ! { io } { fill(io); write_file(path(A(n: 1)), \"x\") }\n";
    accepted(&format!(
        "{head}impl Src for B {{ fn get(self: B) -> Str ! {{}} {{ \"b\" }} }}\n"
    ));
    // An impl that returns what a `Strs` holds is a candidate of every call through the
    // bound, whichever instance runs: stated, and conservative.
    refused(&format!(
        "{head}impl Src for B {{ fn get(self: B) -> Str ! {{}} {{ let xs = strs_new(); strs_get(xs, 0) }} }}\n"
    ));
}

#[test]
fn a_function_exported_to_c_is_handed_anything() {
    // An export is pure, so it has no sink of its own; what C hands it reaches the program
    // through what it writes, here an `Ints`, which every `Ints` read then carries.
    let src = "pub extern \"C\" fn kept(x: Int) -> Int ! {} = \"kept\" { let xs = ints_new(); ints_push(xs, x); 0 }\n\
               fn main(io: IoCap) -> Int ! { io } { let ys = ints_new(); ints_push(ys, 1); print(int_to_str(ints_get(ys, 0))); 0 }\n";
    let d = program(src);
    let o = nazm(d.path(), &["explain-flow", "--json", "p.nz"]);
    assert!(o.status.success(), "{}", text(&o));
    let flow: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let main = flow["sinks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["function"] == "main")
        .unwrap();
    assert_eq!(main["origins"], serde_json::json!(["unknown"]), "{flow}");
    let notes: Vec<&str> = main["traces"][0]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["note"].as_str().unwrap())
        .collect();
    assert!(
        notes
            .iter()
            .any(|n| n.contains("exported to C, which may call it with anything")),
        "{notes:?}"
    );
}

#[test]
fn checked_paths_is_the_languages_rule_in_every_function() {
    // An undeclared helper is N37's bridge: the language does not check its own writes…
    let src = "fn save(p: Str, io: IoCap) -> Int { write_file(p, \"x\") }\n\
               fn main(io: IoCap) -> Int { save(read_file(\"c\"), io) }\n";
    accepted(src);
    // …and `cyber`'s `checked-paths` does, with the chain.
    let d = program(src);
    let o = nazm(d.path(), &["check", "--profile", "cyber", "p.nz"]);
    let t = text(&o);
    assert!(!o.status.success());
    assert!(t.contains("rule `checked-paths`"), "{t}");
    assert!(
        t.contains("this path comes from the contents of a file"),
        "{t}"
    );
    assert!(t.contains("`save` is called here"), "{t}");
    assert!(t.contains("a file's contents are read here"), "{t}");
    // A path from the command line passes it.
    let d = program(
        "fn save(p: Str, io: IoCap) -> Int { write_file(p, \"x\") }\n\
         fn main(io: IoCap) -> Int { save(arg(1), io) }\n",
    );
    let o = nazm(
        d.path(),
        &["check", "--profile", "cyber", "--profile-report", "p.nz"],
    );
    let report = String::from_utf8_lossy(&o.stdout);
    assert!(report.contains("\"checked-paths\""), "{report}");
    assert!(!text(&o).contains("rule `checked-paths`"), "{}", text(&o));
}

#[test]
fn explain_flow_prints_each_origins_chain_from_where_it_entered() {
    let src = "fn show(s: Str, out: OutCap) -> Int { print(s) }\n\
               fn main(io: IoCap) -> Int ! { io } { let xs = strs_new(); strs_push(xs, read_file(\"c\")); show(strs_get(xs, 0), io); print(arg(0)); 0 }\n";
    let d = program(src);
    let o = nazm(d.path(), &["explain-flow", "--json", "p.nz"]);
    assert!(o.status.success(), "{}", text(&o));
    let flow: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(flow["schema"], "nazm.flow/2");
    let show = flow["sinks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["function"] == "show")
        .unwrap();
    assert_eq!(show["origins"], serde_json::json!(["file"]));
    let notes: Vec<&str> = show["traces"][0]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["note"].as_str().unwrap())
        .collect();
    // From the source to the sink, every hop in order.
    assert_eq!(
        notes.first().copied(),
        Some("a file's contents are read here"),
        "{notes:?}"
    );
    let pos = |needle: &str| {
        notes
            .iter()
            .position(|n| n.contains(needle))
            .unwrap_or_else(|| panic!("{needle}: {notes:?}"))
    };
    assert!(
        pos("written into a `Strs`") < pos("read out of a `Strs`"),
        "{notes:?}"
    );
    assert!(
        pos("read out of a `Strs`") < pos("`show` is called here"),
        "{notes:?}"
    );
    assert!(
        notes.last().unwrap().contains("standard output"),
        "{notes:?}"
    );
    // Every step names a place.
    for st in show["traces"][0]["steps"].as_array().unwrap() {
        assert!(st["at"]["line"].as_u64().is_some(), "{st}");
    }
    // Deterministic: the same bytes twice.
    let again = nazm(d.path(), &["explain-flow", "--json", "p.nz"]);
    assert_eq!(o.stdout, again.stdout);
    // And the human form shows the chain under the sink.
    let human = text(&nazm(d.path(), &["explain-flow", "p.nz"]));
    assert!(human.contains("    file:"), "{human}");
    assert!(human.contains("a file's contents are read here"), "{human}");
}

#[test]
fn nothing_that_runs_changes() {
    // Provenance is erased before Core IR: an accepted program behaves the same in the
    // interpreter and natively, as it did before N80 made it acceptable.
    let src = "struct Job { path: Str, body: Str }\n\
               fn main(io: IoCap) -> Int ! { io } {\n\
                   let xs = strs_new(); strs_push(xs, \"out\");\n\
                   let j = Job(path: strs_get(xs, 0), body: \"b\");\n\
                   let f = fn(s: Str) -> Int ! {} { str_len(s) };\n\
                   print(j.path); f(j.body)\n\
               }\n";
    let d = program(src);
    let run = nazm(d.path(), &["run", "p.nz"]);
    assert!(run.status.success(), "{}", text(&run));
    let o = nazm(
        d.path(),
        &["build", "--backend", "cranelift", "p.nz", "-o", "p"],
    );
    assert!(o.status.success(), "{}", text(&o));
    let native = Command::new(d.path().join("p")).output().unwrap();
    assert_eq!(run.stdout, native.stdout);
    assert_eq!(String::from_utf8_lossy(&run.stdout), "out1\n");
}
