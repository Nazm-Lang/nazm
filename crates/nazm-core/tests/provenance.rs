//! N38: provenance — where values come from, and the one flow v1 restricts.
//!
//! `docs/spec.md`, *Provenance*, is the law. What these tests hold:
//!
//! * only the outside-world built-ins that bring data in are sources — the command line, a
//!   file's contents and existence — and `main`'s capabilities are the runtime's authority;
//!   output is not input, and a literal is local;
//! * explicit data flow carries provenance, through bindings, operators, calls, records,
//!   variants, `Result` and `?`, and a join is a set union, in any order;
//! * a function's summary is what its result carries and which parameters it passes on: a
//!   constant-returning function passes on nothing, and recursion reaches a fixed point;
//! * a condition contributes nothing, and a value read out of a sequence or a channel is
//!   `unknown`;
//! * provenance is not an effect and not authority;
//! * in a function that declares its effects, the path `write_file` writes to may not come
//!   from a file's contents or from storage the checker does not follow (`N0372`), directly
//!   or through any function — declared or not — that writes to a path it is given.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use nazm_core::check_source;
use nazm_core::sema::{EffectSet, Origin, OriginSet, Summary};
use nazm_diag::codes;

fn analyse<T: Send + 'static>(
    src: &str,
    f: impl FnOnce(&nazm_core::sema::resolve::Resolution, nazm_core::sema::module::ModuleId) -> T
        + Send
        + 'static,
) -> T {
    let src = src.to_owned();
    std::thread::Builder::new()
        .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
        .spawn(move || {
            let (map, graph) = nazm_core::one_file(&src);
            let a = nazm_core::analyse(&map, &graph, nazm_core::Entry::Absent);
            f(&a.resolution, graph.root())
        })
        .unwrap()
        .join()
        .unwrap()
}

fn summary(src: &str, name: &str) -> Summary {
    let name = name.to_owned();
    analyse(src, move |res, root| {
        res.functions()
            .iter()
            .find(|f| f.module == root && f.name == name)
            .and_then(|f| f.provenance.clone())
            .unwrap_or_else(|| panic!("no summary for {name}"))
    })
}

fn origins(src: &str, name: &str) -> Vec<&'static str> {
    summary(src, name).origins.names()
}

fn inferred(src: &str, name: &str) -> EffectSet {
    let name = name.to_owned();
    analyse(src, move |res, root| {
        res.functions()
            .iter()
            .find(|f| f.module == root && f.name == name)
            .and_then(|f| f.inferred_effects)
            .unwrap()
    })
}

fn accepted(src: &str) {
    let d = check_source(src);
    assert!(d.is_empty(), "{d:#?}\n{src}");
}

fn at(src: &str, needle: &str) -> (u32, u32) {
    let s = src.find(needle).unwrap_or_else(|| panic!("no {needle:?}"));
    (
        u32::try_from(s).unwrap(),
        u32::try_from(s + needle.len()).unwrap(),
    )
}

/// Exactly the one diagnostic, `N0372`, with its primary span at `span`.
fn restricted_at(src: &str, span: (u32, u32)) -> nazm_diag::Diagnostic {
    let d = check_source(src);
    assert_eq!(d.len(), 1, "{d:#?}\n{src}");
    let d = d.into_iter().next().unwrap();
    assert_eq!(d.code, codes::RESTRICTED_FLOW, "{d:#?}");
    assert_eq!((d.primary.span.start, d.primary.span.end), span, "{d:#?}");
    assert!(d.fixes.is_empty(), "there is nothing safe to rewrite");
    d
}

const MAIN: &str = "fn main() -> Int { 0 }\n";

#[test]
fn the_sources_are_the_built_ins_that_bring_data_in_and_nothing_else() {
    let src = format!(
        "fn a(io: IoCap) -> Str {{ arg(0) }}\n\
         fn n(io: IoCap) -> Int {{ args_count() }}\n\
         fn r(io: IoCap) -> Str {{ read_file(\"f\") }}\n\
         fn e(io: IoCap) -> Bool {{ file_exists(\"f\") }}\n\
         fn p(out: OutCap) -> Int {{ print(\"x\") }}\n\
         fn lit() -> Str {{ \"x\" }}\n\
         fn num() -> Int {{ 1 + 2 * 3 }}\n{MAIN}"
    );
    assert_eq!(origins(&src, "a"), ["argument"]);
    assert_eq!(origins(&src, "n"), ["argument"]);
    assert_eq!(origins(&src, "r"), ["file"]);
    assert_eq!(origins(&src, "e"), ["file"]);
    // Output is not input: what `print` returns is its argument's length, a literal's.
    assert!(origins(&src, "p").is_empty());
    assert!(origins(&src, "lit").is_empty());
    assert!(origins(&src, "num").is_empty());
}

#[test]
fn explicit_data_flow_carries_provenance_and_a_join_is_a_union_in_any_order() {
    let src = format!(
        "struct Pair {{ a: Str, b: Str, }}\n\
         enum Got {{ Some(v: Str), None, }}\n\
         fn both(io: IoCap) -> Str {{ str_concat(arg(0), read_file(\"f\")) }}\n\
         fn back(io: IoCap) -> Str {{ str_concat(read_file(\"f\"), arg(0)) }}\n\
         fn len(io: IoCap) -> Int {{ str_len(read_file(\"f\")) + 1 }}\n\
         fn right(io: IoCap) -> Int {{ 1 + str_len(arg(0)) }}\n\
         fn text(io: IoCap) -> Str {{ int_to_str(args_count()) }}\n\
         fn field(io: IoCap) -> Str {{ let p = Pair(a: \"x\", b: read_file(\"f\")); p.a }}\n\
         fn other(io: IoCap) -> Str {{ let p = Pair(a: \"x\", b: read_file(\"f\")); p.b }}\n\
         fn handed(p: Pair) -> Str {{ p.a }}\n\
         fn payload(io: IoCap) -> Str {{ match Got.Some(v: arg(1)) {{ Got.Some(v: s) => s, Got.None() => \"\", }} }}\n\
         fn mutated(io: IoCap) -> Str {{ let mut x = \"local\"; x = read_file(\"f\"); x }}\n\
         fn twice(io: IoCap) -> Str {{ let mut x = arg(0); x = read_file(\"f\"); x }}\n\
         fn looped(io: IoCap) -> Str {{ let mut x = \"\"; let mut y = \"\"; let mut i = 0; while i < 3 {{ x = y; y = arg(0); i = i + 1; }} x }}\n\
         fn half(n: Int) -> Result[Int, Str] {{ Result[Int, Str].Ok(value: n / 2) }}\n\
         fn prop(io: IoCap) -> Result[Int, Str] {{ let h = half(str_len(arg(0)))?; Result[Int, Str].Ok(value: h) }}\n{MAIN}"
    );
    assert_eq!(origins(&src, "both"), ["argument", "file"]);
    assert_eq!(
        summary(&src, "both"),
        summary(&src, "back"),
        "order is not provenance"
    );
    assert_eq!(
        origins(&src, "len"),
        ["file"],
        "a value computed from input is from it"
    );
    assert_eq!(
        origins(&src, "right"),
        ["argument"],
        "an operator joins both operands"
    );
    assert_eq!(origins(&src, "text"), ["argument"]);
    // Within a body, a record built there keeps each field apart (N80); one handed in is a
    // whole value, whose fields are all of it.
    assert!(origins(&src, "field").is_empty());
    assert_eq!(origins(&src, "other"), ["file"]);
    assert_eq!(summary(&src, "handed").params, [0]);
    assert_eq!(origins(&src, "payload"), ["argument"]);
    assert_eq!(origins(&src, "prop"), ["argument"]);
    // A binding carries everything assigned to it, loops included.
    assert_eq!(origins(&src, "mutated"), ["file"]);
    assert_eq!(
        origins(&src, "twice"),
        ["argument", "file"],
        "every assignment, not the first"
    );
    assert_eq!(origins(&src, "looped"), ["argument"]);
}

#[test]
fn a_summary_passes_on_only_what_the_result_is_computed_from() {
    let src = format!(
        "fn id(x: Str) -> Str {{ x }}\n\
         fn len(x: Str, io: IoCap) -> Int {{ str_len(x) }}\n\
         fn one(x: Str) -> Int {{ 1 }}\n\
         fn second(a: Str, b: Str) -> Str {{ b }}\n\
         fn pick(c: Bool, a: Str, b: Str) -> Str {{ if c {{ a }} else {{ b }} }}\n\
         fn use_id(io: IoCap) -> Str {{ id(read_file(\"f\")) }}\n\
         fn use_one(io: IoCap) -> Int {{ one(read_file(\"f\")) }}\n\
         fn use_second(io: IoCap) -> Str {{ second(read_file(\"f\"), arg(0)) }}\n\
         fn guarded(io: IoCap) -> Str {{ if file_exists(\"f\") {{ \"yes\" }} else {{ \"no\" }} }}\n{MAIN}"
    );
    assert_eq!(summary(&src, "id").params, [0]);
    assert_eq!(summary(&src, "len").params, [0]);
    // A constant is local whatever it was given: no parameter is passed on.
    let one = summary(&src, "one");
    assert!(one.origins.is_empty() && one.params.is_empty() && one.reaches.is_empty());
    // And what it is handed is recorded where it was handed, not in what it returns.
    assert_eq!(one.receives, [OriginSet::of(Origin::File)]);
    assert_eq!(summary(&src, "second").params, [1]);
    // A branch's value is its arms'; the condition contributes nothing (explicit flow).
    assert_eq!(summary(&src, "pick").params, [1, 2]);
    assert!(origins(&src, "guarded").is_empty());
    assert_eq!(origins(&src, "use_id"), ["file"]);
    assert!(origins(&src, "use_one").is_empty(), "no over-tainting");
    assert_eq!(origins(&src, "use_second"), ["argument"]);
}

#[test]
fn recursion_and_mutual_recursion_reach_the_least_fixed_point_in_any_order() {
    let src = format!(
        "fn count(x: Str, n: Int) -> Str {{ if n == 0 {{ x }} else {{ count(x, n - 1) }} }}\n\
         fn ping(x: Str, n: Int, io: IoCap) -> Str {{ if n == 0 {{ \"done\" }} else {{ pong(x, n - 1, io) }} }}\n\
         fn pong(x: Str, n: Int, io: IoCap) -> Str {{ if n == 7 {{ read_file(x) }} else {{ ping(x, n - 1, io) }} }}\n\
         fn spin(n: Int) -> Int {{ if n == 0 {{ 0 }} else {{ spin(n - 1) }} }}\n\
         fn top(io: IoCap) -> Str {{ count(arg(0), 3) }}\n{MAIN}"
    );
    assert_eq!(summary(&src, "count").params, [0]);
    assert_eq!(origins(&src, "ping"), ["file"]);
    assert_eq!(
        summary(&src, "ping").params,
        [0],
        "`read_file`'s path is passed on too"
    );
    assert_eq!(origins(&src, "pong"), ["file"]);
    let spin = summary(&src, "spin");
    assert!(
        spin.origins.is_empty() && spin.params.is_empty() && spin.receives == [OriginSet::EMPTY],
        "recursion adds nothing: {spin:?}"
    );
    assert_eq!(origins(&src, "top"), ["argument"]);
    let mut lines: Vec<&str> = src.lines().collect();
    lines.reverse();
    let reversed = lines.join("\n");
    for f in ["count", "ping", "pong", "spin", "top"] {
        assert_eq!(summary(&src, f), summary(&reversed, f), "{f}");
    }
}

#[test]
fn a_container_carries_what_any_container_of_its_type_was_given() {
    // Containers are followed by cell (N80): each read carries what was written into any
    // container of its type, and nothing else.
    let src = format!(
        "fn from_seq() -> Str {{ let xs = strs_new(); strs_push(xs, \"lit\"); strs_get(xs, 0) }}\n\
         fn from_vec(io: IoCap) -> Int {{ let v = vec_new[Int](); vec_push(v, 1); vec_len(v) }}\n\
         fn from_chan() -> Int {{ let c = chan_new(1); chan_send(c, 1); let b = ints_new(); chan_recv(c, b); ints_pop(b) }}\n\
         fn received() -> Bool {{ let c = chan_new(1); chan_send(c, 1); let b = ints_new(); chan_recv(c, b) }}\n{MAIN}"
    );
    for f in ["from_seq", "from_vec", "from_chan", "received"] {
        assert!(origins(&src, f).is_empty(), "{f}: {:?}", origins(&src, f));
    }
    // One write of a file's contents anywhere reaches every read of that type — and only
    // that type: a `Vec[Int]` filled from the command line stays the command line's.
    let src = format!(
        "fn fill(io: IoCap) -> Int {{ let ys = strs_new(); strs_push(ys, read_file(\"f\")); 0 }}\n\
         fn from_seq() -> Str {{ let xs = strs_new(); strs_push(xs, \"lit\"); strs_get(xs, 0) }}\n\
         fn from_vec(io: IoCap) -> Int {{ let v = vec_new[Int](); vec_push(v, str_len(arg(0))); vec_get(v, 0) }}\n\
         fn through_chan(io: IoCap) -> Int {{ let c = chan_new(1); chan_send(c, str_len(read_file(\"f\"))); let b = ints_new(); chan_recv(c, b); ints_pop(b) }}\n{MAIN}"
    );
    assert_eq!(origins(&src, "from_seq"), ["file"]);
    assert_eq!(origins(&src, "from_vec"), ["argument"]);
    // A receive moves the channel's cell into the `Ints`.
    assert_eq!(origins(&src, "through_chan"), ["file"]);
    // A container in generic code may be any container: a write there reaches every cell.
    let src = format!(
        "fn put[T](v: Vec[T], x: T) -> Int {{ vec_push(v, x); 0 }}\n\
         fn fill(io: IoCap) -> Int {{ let w = vec_new[Int](); put(w, str_len(read_file(\"f\"))) }}\n\
         fn from_vec(io: IoCap) -> Str {{ let v = vec_new[Str](); vec_push(v, \"lit\"); vec_get(v, 0) }}\n{MAIN}"
    );
    assert_eq!(origins(&src, "from_vec"), ["file"]);
}

#[test]
fn authority_comes_from_main_and_survives_passing_storage_and_tasks() {
    let src = "struct Ctx { io: IoCap, n: Int, }\n\
        fn keep(io: IoCap) -> IoCap ! {} { io }\n\
        fn boxed(io: IoCap) -> Ctx ! {} { Ctx(io: keep(io), n: 1) }\n\
        fn worker(io: IoCap, c: Chan) -> Int ! { io } { print(\"t\"); 0 }\n\
        fn run(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } { let c = chan_new(1); scope { spawn worker(io, c); } 0 }\n\
        fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } { let k = boxed(io); run(k.io, t) }\n";
    accepted(src);
    assert_eq!(summary(src, "keep").params, [0]);
    assert_eq!(summary(src, "boxed").params, [0]);
    // `main`'s parameters are the runtime's: it has no caller to pass anything on from.
    assert!(summary(src, "main").params.is_empty());
    // What each parameter is handed: the root authority, passed through `keep`, stored in
    // a record, read back out of it and captured by a `spawn` — never reset on the way.
    let authority = OriginSet::of(Origin::Authority);
    assert_eq!(summary(src, "keep").receives, [authority]);
    assert_eq!(summary(src, "boxed").receives, [authority]);
    assert_eq!(summary(src, "run").receives, [authority, authority]);
    assert_eq!(
        summary(src, "worker").receives,
        [authority, OriginSet::EMPTY],
        "the task's capability is the spawner's, and its channel is local"
    );
    // Passing and storing authority is not exercising it: `keep` and `boxed` stay pure.
    assert_eq!(inferred(src, "keep"), EffectSet::EMPTY);
    assert_eq!(inferred(src, "boxed"), EffectSet::EMPTY);
    // Knowing a value came from authority grants none: a capability still in a record is
    // not held (N37).
    let src = "struct Ctx { io: IoCap, }\n\
        fn f(c: Ctx) -> Int ! { io } { print(\"x\") }\n\
        fn main(io: IoCap) -> Int { f(Ctx(io: io)) }\n";
    let d = check_source(src);
    assert_eq!(d.len(), 1, "{d:#?}");
    assert_eq!(d[0].code, codes::MISSING_AUTHORITY);
}

#[test]
fn provenance_is_not_an_effect_and_pure_code_may_compute_over_input() {
    let src = "fn shout(s: Str) -> Str ! {} { str_concat(s, \"!\") }\n\
        fn size(s: Str) -> Int ! {} { str_len(shout(s)) }\n\
        fn main(io: IoCap) -> Int ! { io } { size(read_file(\"in\")) }\n";
    accepted(src);
    assert_eq!(inferred(src, "shout"), EffectSet::EMPTY);
    assert_eq!(inferred(src, "size"), EffectSet::EMPTY);
    assert_eq!(origins(src, "main"), ["file"]);
    // And external provenance never stands in for a missing effect or authority.
    let src = "fn f(s: Str) -> Int ! {} { print(s) }\nfn main(io: IoCap) -> Int { f(arg(0)) }\n";
    let d = check_source(src);
    assert!(d.iter().all(|d| d.code != codes::RESTRICTED_FLOW), "{d:#?}");
    assert!(d.iter().any(|d| d.code == codes::MISSING_AUTHORITY));
}

#[test]
fn a_path_from_a_files_contents_is_refused_with_its_witness() {
    let src = "fn target(io: IoCap) -> Str ! { io } { read_file(\"manifest\") }\n\
        fn main(io: IoCap) -> Int ! { io } { write_file(target(io), \"x\"); 0 }\n";
    let d = restricted_at(src, at(src, "write_file"));
    assert!(d.message.contains("contents of a file"), "{}", d.message);
    let hops: Vec<(u32, u32)> = d
        .secondary
        .iter()
        .map(|l| (l.span.start, l.span.end))
        .collect();
    let call = at(src, "target(io), ").0;
    assert!(hops.contains(&(call, call + 6)), "{hops:?}");
    assert!(hops.contains(&at(src, "read_file")), "{hops:?}");
    // A container is followed by its cell (N80): what was written into it is what a read
    // carries, so the command line through a `Strs` is allowed…
    let src = "fn main(io: IoCap) -> Int ! { io } { let xs = strs_new(); strs_push(xs, arg(0)); write_file(strs_get(xs, 0), \"x\"); 0 }\n";
    accepted(src);
    // …and a file's contents through one are refused, with the read, the write into the cell
    // and the file read on the chain.
    let src = "fn main(io: IoCap) -> Int ! { io } { let xs = strs_new(); strs_push(xs, read_file(\"f\")); write_file(strs_get(xs, 0), \"x\"); 0 }\n";
    let d = restricted_at(src, at(src, "write_file"));
    assert!(d.message.contains("contents of a file"), "{}", d.message);
    let hops: Vec<(u32, u32)> = d
        .secondary
        .iter()
        .map(|l| (l.span.start, l.span.end))
        .collect();
    assert!(hops.contains(&at(src, "strs_get")), "{hops:?}");
    assert!(hops.contains(&at(src, "strs_push")), "{hops:?}");
    assert!(hops.contains(&at(src, "read_file")), "{hops:?}");
}

#[test]
fn a_path_from_the_command_line_or_the_program_is_allowed_and_nothing_is_over_tainted() {
    let src = "fn out(s: Str) -> Str ! {} { \"out.txt\" }\n\
        fn name(s: Str) -> Str ! {} { str_concat(s, \".o\") }\n\
        fn main(io: IoCap) -> Int ! { io } {\n\
            let text = read_file(\"in\");\n\
            write_file(name(arg(0)), text);\n\
            write_file(out(text), text);\n\
            if file_exists(\"lock\") { write_file(\"a\", text) } else { write_file(\"b\", text) };\n\
            0\n\
        }\n";
    accepted(src);
}

#[test]
fn a_function_that_writes_to_a_path_it_is_given_carries_the_rule_to_its_callers() {
    // Declared, undeclared, several calls deep, and through a `spawn`: the call that hands
    // over the file's contents is refused, and the chain to the sink is shown.
    for (helper, call) in [
        ("fn save(io: IoCap, p: Str) -> Int ! { io } { write_file(p, \"x\") }", "save(io, read_file(\"cfg\"))"),
        ("fn save(io: IoCap, p: Str) -> Int { write_file(p, \"x\") }", "save(io, read_file(\"cfg\"))"),
        (
            "fn inner(p: Str, io: IoCap) -> Int { write_file(str_concat(p, \"/x\"), \"x\") }\nfn save(io: IoCap, p: Str) -> Int ! { io } { inner(p, io) }",
            "save(io, read_file(\"cfg\"))",
        ),
    ] {
        let src = format!("{helper}\nfn main(io: IoCap) -> Int ! {{ io }} {{ {call}; 0 }}\n");
        let s = at(&src, call).0;
        let d = restricted_at(&src, (s, s + 4));
        assert!(!d.secondary.is_empty(), "{d:#?}");
    }
    let src = "fn w(p: Str, c: Chan) -> Int ! { io } { write_file(p, \"x\") }\n\
        fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } { let c = chan_new(1); scope { spawn w(read_file(\"cfg\"), c); } 0 }\n";
    let d = check_source(src);
    assert!(d.iter().any(|d| d.code == codes::RESTRICTED_FLOW), "{d:#?}");
    // The parameter a helper writes to is the one that matters.
    let src = "fn save(io: IoCap, p: Str, data: Str) -> Int ! { io } { write_file(p, data) }\n\
        fn main(io: IoCap) -> Int ! { io } { save(io, arg(0), read_file(\"in\")) }\n";
    accepted(src);
}

#[test]
fn ambient_code_is_the_bridge_and_is_not_checked_but_cannot_launder() {
    // A program that declares nothing is accepted as before.
    accepted("fn main(io: IoCap) -> Int { write_file(read_file(\"cfg\"), \"x\"); 0 }\n");
    // A declared function that lets an undeclared one choose from a file is refused where
    // it hands the file's contents over, not excused because the helper declares nothing.
    let src = "fn helper(p: Str, io: IoCap) -> Int { write_file(p, \"x\") }\n\
        fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } { helper(read_file(\"cfg\"), io); 0 }\n";
    let s = at(src, "helper(read").0;
    restricted_at(src, (s, s + 6));
    // And through a declared function in between, which hands its own parameter on to it.
    let src = "fn helper(p: Str, io: IoCap) -> Int { write_file(p, \"x\") }\n\
        fn mid(io: IoCap, t: SpawnCap, p: Str) -> Int ! { io, spawn } { helper(p, io) }\n\
        fn main(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } { mid(io, t, read_file(\"cfg\")) }\n";
    let s = at(src, "mid(io, t, read").0;
    restricted_at(src, (s, s + 3));
}

#[test]
fn provenance_errors_are_their_own_and_the_order_is_fixed() {
    // Missing authority, a missing effect and a restricted path in one body: three codes,
    // authority first as N37 fixed, then the effect, then the flow.
    let src =
        "fn f() -> Int ! {} { write_file(read_file(\"a\"), \"x\") }\nfn main() -> Int { f() }\n";
    let d: Vec<String> = check_source(src)
        .into_iter()
        .map(|d| d.code.as_str().to_owned())
        .collect();
    assert_eq!(d, ["N0369", "N0366", "N0372"]);
    assert_eq!(check_source(src), check_source(src));
}

#[test]
fn a_large_program_settles_quickly_and_the_same_every_time() {
    let mut src = String::new();
    let n = 2000;
    for i in 0..n {
        src.push_str(&format!(
            "fn c{i}(p: Str) -> Str {{ c{}(str_concat(p, \"x\")) }}\n",
            i + 1
        ));
    }
    src.push_str(&format!("fn c{n}(p: Str) -> Str {{ p }}\n"));
    let ring = 500;
    for i in 0..ring {
        let next = (i + 1) % ring;
        let body = if i == 250 {
            format!("if true {{ read_file(\"r\") }} else {{ r{next}(p, io) }}")
        } else {
            format!("r{next}(p, io)")
        };
        // Each holds the authority the ring's one read needs (N104).
        src.push_str(&format!("fn r{i}(p: Str, io: IoCap) -> Str {{ {body} }}\n"));
    }
    src.push_str("fn main(io: IoCap) -> Int ! { io } { write_file(c0(arg(0)), \"a\"); write_file(r0(\"x\", io), \"b\"); 0 }\n");
    let start = std::time::Instant::now();
    let d = check_source(&src);
    assert!(start.elapsed().as_secs() < 60, "{:?}", start.elapsed());
    // The chain passes the command line through 2,000 calls and stays allowed; the ring
    // reaches a file somewhere and is refused.
    assert_eq!(d.len(), 1, "{d:#?}");
    assert_eq!(d[0].code, codes::RESTRICTED_FLOW);
    assert_eq!(d[0].primary.span.start, at(&src, "write_file(r0").0);
    assert_eq!(summary(&src, "c0").params, [0]);
    assert_eq!(summary(&src, "r7").origins, OriginSet::of(Origin::File));
    assert_eq!(check_source(&src), d);
}

#[test]
fn a_path_from_where_the_checker_cannot_follow_is_refused_too() {
    // `Unknown` — a device register or C's `errno` — is restricted like a file's contents: the
    // checker cannot say where it came from, so it may not name a path (in a function that
    // declares its effects: the language rule's scope). N108 found the
    // restriction's `Unknown` half held by no program.
    let src = "fn main(io: IoCap, f: ForeignCap) -> Int ! { io, foreign } { write_file(int_to_str(c_errno()), \"x\"); 0 }\n";
    let d = restricted_at(src, at(src, "write_file"));
    assert_eq!(d.code, codes::RESTRICTED_FLOW, "{}", d.message);
    // The same program with a command-line argument is allowed.
    accepted("fn main(io: IoCap) -> Int ! { io } { write_file(arg(0), \"x\"); 0 }\n");
}
