//! N40: MIR — the one lowering from Core IR, its validator, its digest, and the native backend
//! reading it.
//!
//! Three kinds of test. *Positive*: programs over every construct lower to MIR the validator
//! accepts, and the compiled executable agrees with the interpreter, memory report included.
//! *Negative*: a valid MIR program, broken one invariant at a time, is refused by the validator
//! for that invariant — the validator's teeth, since the lowering never produces these.
//! *Laws and scale*: what the executable digest does and does not see, determinism, and lowering
//! time over large functions, large programs and deep nesting.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use nazm_mir::{BlockId, LocalId, Operand, Program, Rvalue, Statement, StatementKind, Terminator};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

/// Lower a one-file program to MIR in process. Loading parses on the calling thread, so the
/// whole of it runs on the interpreter's stack, as every driver does.
fn mir_of_file(path: &Path) -> Program {
    let dir = path.parent().unwrap().to_path_buf();
    nazm_core::eval::on_interpreter_stack(0, || {
        let loaded = nazm_service::load::load(path, &dir).expect("load");
        nazm_core::on_lowered(
            &loaded.map,
            &loaded.graph,
            nazm_core::Entry::Absent,
            |c, r| nazm_mir::lower(c, r, &loaded.graph),
        )
        .unwrap_or_else(|d| panic!("{d:?}"))
    })
    .expect("a thread")
}

fn mir_of(src: &str) -> Program {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("prog.nz");
    std::fs::write(&path, src).unwrap();
    mir_of_file(&path)
}

fn valid(src: &str) -> Program {
    let p = mir_of(src);
    let broken = nazm_mir::verify(&p);
    assert!(broken.is_empty(), "{broken:#?}\n{}", print(&p));
    p
}

fn print(p: &Program) -> String {
    nazm_mir::print(
        p,
        &nazm_mir::Options {
            span: None,
            compact: false,
        },
    )
}

fn function<'a>(p: &'a Program, name: &str) -> &'a nazm_mir::Function {
    p.functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no `{name}`"))
}

fn function_mut<'a>(p: &'a mut Program, name: &str) -> &'a mut nazm_mir::Function {
    p.functions
        .iter_mut()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no `{name}`"))
}

struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Build natively and run with the memory report on; run interpreted with it on too.
fn both(src: &str) -> (Ran, Ran) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("prog.nz");
    std::fs::write(&path, src).unwrap();
    let exe = dir.path().join("prog");
    let built = Command::new(NAZM)
        .args(["build", path.to_str().unwrap(), "-o"])
        .arg(&exe)
        .output()
        .expect("build");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let native = common::bounded_exe(&exe)
        .env(nazm_lir::MEMORY_VARIABLE, "1")
        .output()
        .expect("run exe");
    let interp = Command::new(NAZM)
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("run");
    (
        Ran {
            code: native.status.code(),
            stdout: String::from_utf8_lossy(&native.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&native.stderr).into_owned(),
        },
        Ran {
            code: interp.status.code(),
            stdout: String::from_utf8_lossy(&interp.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&interp.stderr).into_owned(),
        },
    )
}

/// The native run's memory report says everything allocated was reclaimed.
fn balanced(stderr: &str) {
    let line = stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .unwrap_or_else(|| panic!("no memory report in {stderr:?}"));
    for part in ["sequences", "strings", "channels"] {
        let after = line.split(part).nth(1).expect(part);
        let num = |key: &str| -> i64 {
            after
                .split(key)
                .nth(1)
                .and_then(|r| r.split_whitespace().next())
                .and_then(|n| n.parse().ok())
                .unwrap_or(-1)
        };
        assert_eq!(num("live="), 0, "{part} still live: {line}");
        assert_eq!(num("allocated="), num("reclaimed="), "{part}: {line}");
    }
}

fn agree(src: &str) {
    let (native, interp) = both(src);
    assert_eq!(
        native.code, interp.code,
        "status: {} / {}",
        native.stderr, interp.stderr
    );
    assert_eq!(native.stdout, interp.stdout, "stdout");
    if native.code == Some(0) {
        balanced(&native.stderr);
    }
}

// --- positive: every construct lowers, validates, and runs as the interpreter does ------------

const EVERYTHING: &str = r#"
struct Pair { name: Str, n: Int, }
enum Shape { Circle(r: Int), Named(title: Str, sides: Int), Empty }

fn area(s: Shape) -> Int {
    match s {
        Shape.Circle(r: r) => 3 * r * r,
        Shape.Named(title: t, sides: n) => str_len(t) + n,
        Shape.Empty() => 0,
    }
}

fn parse(s: Str) -> Result[Int, Str] {
    if str_len(s) == 0 { return Result[Int, Str].Err(error: "empty"); }
    Result[Int, Str].Ok(value: str_len(s))
}

fn twice(s: Str) -> Result[Int, Str] {
    let a = parse(s)?;
    let b = parse(str_concat(s, s))?;
    Result[Int, Str].Ok(value: a + b)
}

fn loops(n: Int) -> Int {
    let mut total = 0;
    let mut i = 0;
    while i < n {
        i = i + 1;
        let text = int_to_str(i);
        if i % 3 == 0 { continue; }
        if i > 20 { break; }
        let mut j = 0;
        while j < i {
            j = j + 1;
            if j == 4 { break; }
            total = total + str_len(text);
        }
    }
    total
}

fn main() -> Int {
    let p = Pair(name: "abc", n: 4);
    let q = Pair(n: p.n + 1, name: str_concat(p.name, "d"));
    let v = if q.n > 3 { str_len(q.name) } else { 0 };
    let shapes = vec_new[Shape]();
    vec_push(shapes, Shape.Circle(r: 2));
    vec_push(shapes, Shape.Named(title: "tri", sides: 3));
    vec_push(shapes, Shape.Empty());
    let mut k = 0;
    let mut sum = 0;
    while k < vec_len(shapes) { sum = sum + area(vec_get(shapes, k)); k = k + 1; }
    let r = match twice("xy") { Result.Ok(value: x) => x, Result.Err(error: e) => str_len(e) };
    let e = match twice("") { Result.Ok(value: x) => x, Result.Err(error: e) => str_len(e) + 100 };
    let same = if q == Pair(name: "abcd", n: 5) { 1000 } else { 0 };
    v + sum + r + e + loops(30) + same
}
"#;

#[test]
fn every_construct_lowers_to_mir_the_validator_accepts() {
    let p = valid(EVERYTHING);
    let main = function(&p, "main");
    // A real CFG: a loop's back edge, a switch per `match`, and failure edges.
    assert!(main.blocks.len() > 10, "{}", print(&p));
    assert!(
        p.functions.iter().any(|f| f
            .blocks
            .iter()
            .any(|b| matches!(b.term, Terminator::Switch { .. }))),
        "a match lowers to a switch"
    );
    // `?` is gone: `twice` has two switches (one per `?`) whose error arms return.
    let twice = function(&p, "twice");
    let switches = twice
        .blocks
        .iter()
        .filter(|b| matches!(b.term, Terminator::Switch { .. }))
        .count();
    assert_eq!(switches, 2, "{}", print(&p));
    let returns = twice
        .blocks
        .iter()
        .filter(|b| matches!(b.term, Terminator::Return(_)))
        .count();
    assert_eq!(returns, 3, "two error returns and the tail");
}

#[test]
fn every_construct_runs_natively_as_it_is_interpreted_and_reclaims_everything() {
    agree(EVERYTHING);
}

#[test]
fn cleanup_on_every_exit_is_balanced() {
    // A managed value live across each kind of exit: return from inside nested regions,
    // break and continue out of inner loops, `?` out of an initialiser, and a match arm that
    // returns.
    agree(
        r#"
struct Box { s: Str, t: Str, }
fn inner(k: Int) -> Result[Int, Str] {
    if k == 3 { return Result[Int, Str].Err(error: int_to_str(k)); }
    Result[Int, Str].Ok(value: k)
}
fn build(k: Int) -> Result[Box, Str] {
    let b = Box(s: int_to_str(k), t: int_to_str(inner(k)?));
    Result[Box, Str].Ok(value: b)
}
fn early(n: Int) -> Int {
    let a = int_to_str(n);
    let mut i = 0;
    while i < 10 {
        let x = str_concat(a, "x");
        i = i + 1;
        if i == 2 { continue; }
        if i == n {
            let y = str_concat(x, "y");
            return str_len(y);
        }
        if i == 7 { break; }
    }
    str_len(a)
}
fn main() -> Int {
    let mut total = 0;
    let mut k = 0;
    while k < 6 {
        total = total + early(k);
        let r = match build(k) { Result.Ok(value: b) => str_len(b.s) + str_len(b.t), Result.Err(error: e) => str_len(e) };
        total = total + r;
        k = k + 1;
    }
    total
}
"#,
    );
}

#[test]
fn a_field_write_moves_its_value_and_releases_the_old_one() {
    // `SetField` moves a temporary into the field: the temporary is emptied, the old field's
    // value released, and nothing is released twice — including `p.name = p.name`.
    agree(
        r#"
struct Person { name: Str, age: Int, }
struct Pair { left: Person, right: Person, }
fn main() -> Int {
    let mut p = Person(name: int_to_str(1), age: 3);
    let mut q = Pair(left: Person(name: "a", age: 1), right: Person(name: int_to_str(2), age: 2));
    let mut i = 0;
    while i < 30 {
        p.name = str_concat(p.name, "x");
        p.name = p.name;
        q.left.name = int_to_str(i);
        q.right = Person(name: str_concat(q.left.name, p.name), age: i);
        i = i + 1;
    }
    str_len(p.name) + str_len(q.right.name) + q.right.age
}
"#,
    );
}

#[test]
fn a_failure_after_moves_drops_and_field_writes_reclaims_each_value_once() {
    // The shared unwind block drops every managed local, trusting that a moved, dropped or
    // field-written local was emptied. A failure after each of those — a reassignment (a
    // move), a region's end (a drop) and a field write — must reclaim every value exactly
    // once, and report the failure the interpreter reports.
    let src = r#"
struct P { name: Str, n: Int, }
fn main() -> Int {
    let mut p = P(name: int_to_str(1), n: 0);
    let mut s = int_to_str(7);
    let mut i = 0;
    while i < 5 {
        let t = int_to_str(i);
        s = str_concat(s, t);
        p.name = str_concat(p.name, "x");
        i = i + 1;
    }
    let z = i - 5;
    str_len(s) / z
}
"#;
    let (native, interp) = both(src);
    assert_eq!(native.code, Some(2), "{}", native.stderr);
    assert_eq!(native.code, interp.code);
    assert!(native.stderr.contains("N0401"), "{}", native.stderr);
    balanced(&native.stderr);
}

#[test]
fn every_move_and_drop_leaves_its_slot_holding_the_zero() {
    // The shared unwind block drops every managed local, which is correct only because a
    // local that has been moved from or dropped holds the representation's zero again. A
    // double release that follows from breaking this lands on freed memory, which no counter
    // can see — so this checks the invariant itself: for every managed slot of `main`, one
    // zero stored in the entry block and exactly one more for each MIR `drop` and `move` of it.
    let src = r#"
struct P { name: Str, n: Int, }
fn main() -> Int {
    let mut p = P(name: int_to_str(1), n: 0);
    let mut s = int_to_str(7);
    let mut i = 0;
    while i < 5 {
        let t = int_to_str(i);
        s = str_concat(s, t);
        p.name = str_concat(p.name, "x");
        i = i + 1;
    }
    str_len(s) + str_len(p.name)
}
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("prog.nz");
    std::fs::write(&path, src).unwrap();
    let exe = dir.path().join("prog");
    let built = Command::new(NAZM)
        .args(["build", "--emit-ir", path.to_str().unwrap(), "-o"])
        .arg(&exe)
        .output()
        .expect("build");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let ir = std::fs::read_to_string(dir.path().join("prog.m0.ll")).expect("unit IR");
    // `main`'s body: past its signature and any function attributes (Q1's stack probes).
    let body: Vec<&str> = ir
        .split("main() ")
        .nth(1)
        .and_then(|after| after.split_once('{'))
        .and_then(|(_, rest)| rest.split_once('\n'))
        .map(|(_, body)| body)
        .expect("main")
        .lines()
        .take_while(|l| l.trim() != "}")
        .collect();
    let mir = Command::new(NAZM)
        .args(["mir", "--json", path.to_str().unwrap()])
        .output()
        .expect("mir");
    let mir: serde_json::Value = serde_json::from_slice(&mir.stdout).expect("json");
    let main = mir
        .get("functions")
        .and_then(serde_json::Value::as_array)
        .and_then(|fs| fs.iter().find(|f| f.get("name") == Some(&"main".into())))
        .expect("main's MIR");
    let lines: Vec<String> = main
        .get("blocks")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|b| {
            b.get("stmts")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|s| s.as_str().map(str::to_owned))
        .collect();
    // Since N105 a value held in memory lives in a LIR slot, annotated with the MIR local it
    // holds (`name : Type`), and emptied by zeroing its bytes: one `memset` of the slot.
    let names: std::collections::BTreeMap<String, String> = main
        .get("locals")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|l| {
            Some((
                l.get("local")?.as_str()?.to_owned(),
                format!("{} : {}", l.get("name")?.as_str()?, l.get("ty")?.as_str()?),
            ))
        })
        .collect();
    let slot_of = |name: &str| -> Option<String> {
        body.iter().find_map(|l| {
            let (code, note) = l.split_once(';')?;
            (note.trim() == name && code.contains(" = alloca ["))
                .then(|| code.trim().split(' ').next().map(str::to_owned))
                .flatten()
        })
    };
    let zero_store = |slot: &str| {
        body.iter()
            .filter(|l| l.contains(&format!("@llvm.memset.p0.i64(ptr {slot}, i8 0,")))
            .count()
    };
    let mut checked = 0;
    for (local, name) in &names {
        let Some(slot) = slot_of(name) else { continue };
        if zero_store(&slot) == 0 {
            continue;
        }
        let empties = lines
            .iter()
            .map(|l| {
                let drops = usize::from(l == &format!("drop {local}"));
                let moves = l
                    .match_indices(&format!("move {local}"))
                    .filter(|(i, _)| {
                        let after = &l[i + 5 + local.len()..];
                        !after.starts_with(|c: char| c.is_ascii_digit())
                    })
                    .count();
                drops + moves
            })
            .sum::<usize>();
        assert_eq!(
            zero_store(&slot),
            1 + empties,
            "{local} ({name}, {slot}): one zero at entry and one per drop or move ({empties})\n{}",
            body.join("\n")
        );
        checked += 1;
    }
    assert!(checked >= 5, "only {checked} managed slots checked");
}

#[test]
fn a_pattern_binding_holds_its_own_reference_to_a_heap_payload() {
    // A binding reads its payload with a reference of its own: the binding is dropped at its
    // arm's end and the scrutinee at the join, and each gives back one reference.
    agree(
        r#"
enum Tok { Word(text: Str), Pair(a: Str, b: Str), Nothing }
fn size(t: Tok) -> Int {
    match t {
        Tok.Word(text: w) => str_len(w),
        Tok.Pair(a: x, b: y) => str_len(str_concat(x, y)),
        Tok.Nothing() => 0,
    }
}
fn main() -> Int {
    let mut total = 0;
    let mut i = 0;
    while i < 20 {
        total = total + size(Tok.Word(text: int_to_str(i)));
        total = total + size(Tok.Pair(a: int_to_str(i), b: str_concat(int_to_str(i), "z")));
        let kept = Tok.Word(text: int_to_str(i * 3));
        total = total + match kept { Tok.Word(text: w) => str_len(w), Tok.Pair(a: x, b: _) => str_len(x), Tok.Nothing() => 0 };
        i = i + 1;
    }
    total
}
"#,
    );
}

#[test]
fn concurrency_lowers_to_scope_primitives_and_every_exit_joins() {
    let src = r#"
fn worker(c: Chan, n: Int) -> Int { chan_send(c, n * 2); 0 }
fn main(tasks: SpawnCap) -> Int {
    let c = chan_new(8);
    let mut total = 0;
    scope {
        spawn worker(c, 1);
        spawn worker(c, 2);
    }
    let got = ints_new();
    while chan_recv(c, got) { if ints_len(got) == 2 { chan_close(c); } }
    let mut i = 0;
    while i < ints_len(got) { total = total + ints_get(got, i); i = i + 1; }
    total
}
"#;
    let p = valid(src);
    let main = function(&p, "main");
    let opens = main
        .blocks
        .iter()
        .flat_map(|b| &b.stmts)
        .filter(|s| matches!(s.kind, StatementKind::ScopeOpen { .. }))
        .count();
    let joins = main
        .blocks
        .iter()
        .flat_map(|b| &b.stmts)
        .filter(|s| matches!(s.kind, StatementKind::ScopeJoin { .. }))
        .count();
    assert_eq!(opens, 1);
    // The closing brace's join, and the failure path's (a spawn can fail).
    assert!(joins >= 2, "{}", print(&p));
    agree(src);
}

#[test]
fn a_function_in_a_call_cycle_checks_its_stack_first_and_no_other_does() {
    // N49: MIR places the check, so both backends emit the same thing from it. First in
    // every function of a cycle, direct or mutual; nowhere in a function outside one; and a
    // `spawn` closes no cycle, because a task has its own stack.
    let src = "fn f(n: Int) -> Int { if n == 0 { 0 } else { f(n - 1) + 1 } }\n\
               fn a(n: Int) -> Int { if n == 0 { 0 } else { b(n - 1) } }\n\
               fn b(n: Int) -> Int { a(n) }\n\
               fn leaf(n: Int) -> Int { n + 1 }\n\
               fn job(c: Chan) -> Int { chan_send(c, 1); 0 }\n\
               fn again(c: Chan, t: SpawnCap) -> Int { scope { spawn job(c); } 0 }\n\
               fn w(n: Int, tasks: SpawnCap) -> Int { if n <= 0 { 0 } else { scope { spawn w(n - 1, tasks); } 0 } }\n\
               fn main(t: SpawnCap) -> Int { let c = chan_new(2); again(c, t); f(3) + a(2) + leaf(1) + w(2, t) }\n";
    let p = valid(src);
    let first_is_check = |name: &str| {
        let f = function(&p, name);
        let entry = f.blocks.first().expect("an entry block");
        matches!(
            entry.stmts.first().map(|s| &s.kind),
            Some(StatementKind::StackCheck { .. })
        )
    };
    let checks = |name: &str| {
        function(&p, name)
            .blocks
            .iter()
            .flat_map(|b| &b.stmts)
            .filter(|s| matches!(s.kind, StatementKind::StackCheck { .. }))
            .count()
    };
    for name in ["f", "a", "b"] {
        assert!(
            first_is_check(name),
            "{name} is in a cycle and checks first"
        );
        assert_eq!(checks(name), 1, "{name}: once");
    }
    for name in ["leaf", "job", "again", "w", "main"] {
        assert_eq!(checks(name), 0, "{name} is in no call cycle");
    }
}

#[test]
fn generic_functions_are_instances_in_mir() {
    let p = valid(
        "fn id[T](x: T) -> T { x }\nfn main() -> Int { str_len(id[Str](\"ab\")) + id[Int](3) }\n",
    );
    let instances: Vec<_> = p
        .functions
        .iter()
        .filter(|f| !f.type_args.is_empty())
        .collect();
    assert_eq!(instances.len(), 2, "{}", print(&p));
    // No generic definition appears as a function of its own.
    assert!(p
        .functions
        .iter()
        .all(|f| f.name != "id" || !f.type_args.is_empty()));
}

#[test]
fn the_printed_mir_is_the_same_bytes_in_every_process() {
    let file = repo().join("examples/pipeline.nz");
    let run = || {
        Command::new(NAZM)
            .args(["mir", "--json"])
            .arg(&file)
            .output()
            .expect("mir")
    };
    let first = run();
    assert!(first.status.success(), "{first:?}");
    let text = String::from_utf8(first.stdout.clone()).unwrap();
    assert!(
        text.starts_with("{\"schema\": \"nazm.mir/1\"")
            || text.starts_with("{\"schema\":\"nazm.mir/1\""),
        "{text}"
    );
    assert_eq!(text.trim_end().lines().count(), 1);
    for _ in 0..2 {
        assert_eq!(run().stdout, first.stdout);
    }
    let pretty = Command::new(NAZM).arg("mir").arg(&file).output().unwrap();
    let a: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    let b: serde_json::Value = serde_json::from_slice(&pretty.stdout).unwrap();
    assert_eq!(a, b);
}

#[test]
fn lowering_twice_gives_the_same_mir() {
    let file = repo().join("compiler/emit.nz");
    let a = mir_of_file(&file);
    let b = mir_of_file(&file);
    assert_eq!(a, b);
    assert!(nazm_mir::verify(&a).is_empty());
    assert_eq!(nazm_mir::verify(&a), nazm_mir::verify(&a), "idempotent");
}

#[test]
fn a_program_that_does_not_check_prints_no_mir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("prog.nz");
    std::fs::write(&path, "fn main() -> Int { nope }\n").unwrap();
    let out = Command::new(NAZM).arg("mir").arg(&path).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}

// --- negative: the validator's teeth ---------------------------------------------------------

const SMALL: &str = r#"
enum E { A(s: Str), B }
fn g(x: Int) -> Int { x + 1 }
fn pick(e: E) -> Int { match e { E.A(s: s) => str_len(s), E.B() => 0 } }
fn main() -> Int {
    let s = int_to_str(g(4));
    let n = pick(E.A(s: s));
    scope { }
    n
}
"#;

fn refused(p: &Program, needle: &str) {
    let broken = nazm_mir::verify(p);
    assert!(
        broken.iter().any(|e| e.contains(needle)),
        "expected a refusal mentioning {needle:?}, got {broken:#?}"
    );
}

fn first_stmt_where(
    f: &mut nazm_mir::Function,
    pick: impl Fn(&StatementKind) -> bool,
) -> &mut Statement {
    f.blocks
        .iter_mut()
        .flat_map(|b| b.stmts.iter_mut())
        .find(|s| pick(&s.kind))
        .expect("a statement to break")
}

#[test]
fn the_validator_refuses_a_jump_to_a_block_that_does_not_exist() {
    let mut p = valid(SMALL);
    let f = function_mut(&mut p, "main");
    let last = f.blocks.len();
    if let Some(b) = f.blocks.first_mut() {
        b.term = Terminator::Goto(BlockId(u32::try_from(last + 5).unwrap()));
    }
    refused(&p, "does not exist");
}

#[test]
fn the_validator_refuses_a_local_of_the_wrong_type() {
    let mut p = valid(SMALL);
    let f = function_mut(&mut p, "g");
    let s = first_stmt_where(f, |k| matches!(k, StatementKind::Assign { .. }));
    if let StatementKind::Assign { value, .. } = &mut s.kind {
        *value = Rvalue::Use(Operand::Const(nazm_mir::Const::Bool(true)));
    }
    refused(&p, "another type");
}

#[test]
fn the_validator_refuses_a_call_with_the_wrong_arguments() {
    let mut p = valid(SMALL);
    let f = function_mut(&mut p, "main");
    let s = first_stmt_where(f, |k| {
        matches!(
            k,
            StatementKind::Assign {
                value: Rvalue::Call { .. },
                ..
            }
        )
    });
    if let StatementKind::Assign {
        value: Rvalue::Call { args, .. },
        ..
    } = &mut s.kind
    {
        args.push(Operand::Const(nazm_mir::Const::Int(1)));
    }
    refused(&p, "arguments");
}

#[test]
fn the_validator_refuses_a_payload_read_outside_its_arm() {
    let mut p = valid(SMALL);
    let f = function_mut(&mut p, "pick");
    // Move the arm's binding into the entry block, before the switch.
    let (bi, si) = f
        .blocks
        .iter()
        .enumerate()
        .find_map(|(bi, b)| {
            b.stmts
                .iter()
                .position(|s| {
                    matches!(
                        s.kind,
                        StatementKind::Assign {
                            value: Rvalue::Payload { .. },
                            ..
                        }
                    )
                })
                .map(|si| (bi, si))
        })
        .expect("a payload read");
    let s = f.blocks[bi].stmts.remove(si);
    f.blocks[0].stmts.push(s);
    refused(&p, "outside the arm");
}

#[test]
fn the_validator_refuses_a_primitive_with_a_bad_signature() {
    let mut p = valid(SMALL);
    let f = function_mut(&mut p, "main");
    let s = first_stmt_where(f, |k| {
        matches!(
            k,
            StatementKind::Assign {
                value: Rvalue::Primitive { .. },
                ..
            }
        )
    });
    if let StatementKind::Assign {
        value: Rvalue::Primitive { args, .. },
        ..
    } = &mut s.kind
    {
        args.clear();
    }
    refused(&p, "arguments where it takes");
}

#[test]
fn the_validator_refuses_a_leak_at_return() {
    let mut p = valid(SMALL);
    let f = function_mut(&mut p, "main");
    // Drop the first `drop`: something owned reaches the `return`.
    for b in &mut f.blocks {
        if let Some(pos) = b
            .stmts
            .iter()
            .position(|s| matches!(s.kind, StatementKind::Drop(_)))
        {
            b.stmts.remove(pos);
            break;
        }
    }
    refused(&p, "a leak");
}

#[test]
fn the_validator_refuses_a_use_after_move() {
    let mut p = valid(SMALL);
    let f = function_mut(&mut p, "main");
    // Duplicate the first move: the second reads an emptied local.
    let (bi, si) = f
        .blocks
        .iter()
        .enumerate()
        .find_map(|(bi, b)| {
            b.stmts
                .iter()
                .position(|s| {
                    matches!(
                        s.kind,
                        StatementKind::Assign {
                            value: Rvalue::Variant { .. },
                            ..
                        }
                    )
                })
                .map(|si| (bi, si))
        })
        .expect("a variant built from moved temporaries");
    let again = f.blocks[bi].stmts[si].clone();
    f.blocks[bi].stmts.insert(si + 1, again);
    refused(&p, "empty");
}

#[test]
fn the_validator_refuses_a_fallible_operation_without_a_failure_edge() {
    let mut p = valid(SMALL);
    let f = function_mut(&mut p, "g");
    let s = first_stmt_where(f, |k| {
        matches!(
            k,
            StatementKind::Assign {
                value: Rvalue::Arith(..),
                ..
            }
        )
    });
    if let StatementKind::Assign { unwind, .. } = &mut s.kind {
        *unwind = None;
    }
    refused(&p, "no failure edge");
}

#[test]
fn the_validator_refuses_a_switch_that_misses_a_variant() {
    let mut p = valid(SMALL);
    let f = function_mut(&mut p, "pick");
    for b in &mut f.blocks {
        if let Terminator::Switch { arms, .. } = &mut b.term {
            arms.pop();
        }
    }
    refused(&p, "exactly once");
}

#[test]
fn the_validator_refuses_a_scope_left_without_its_join() {
    let mut p = valid(SMALL);
    let f = function_mut(&mut p, "main");
    for b in &mut f.blocks {
        b.stmts
            .retain(|s| !matches!(s.kind, StatementKind::ScopeJoin { .. }));
    }
    refused(&p, "a leak");
}

#[test]
fn the_validator_refuses_dropping_a_parameter() {
    let mut p = valid(SMALL);
    let f = function_mut(&mut p, "pick");
    f.blocks[0].stmts.insert(
        0,
        Statement {
            kind: StatementKind::Drop(LocalId(0)),
            at: nazm_span::Span::synthetic(),
        },
    );
    refused(&p, "parameter");
}

// --- digests ----------------------------------------------------------------------------------

fn digest_of(src: &str, name: &str) -> String {
    let p = valid(src);
    nazm_mir::digest::executable(&p, function(&p, name))
}

#[test]
fn formatting_comments_and_local_names_leave_the_executable_digest_alone() {
    let a = digest_of(
        "fn f(x: Int) -> Int { let y = x + 1; y * 2 }\nfn main() -> Int { f(1) }\n",
        "f",
    );
    let b = digest_of(
        "// a comment\nfn f(x: Int) -> Int {\n    let renamed   = x + 1;\n    renamed * 2\n}\n\nfn main() -> Int { f(1) }\n",
        "f",
    );
    assert_eq!(a, b);
}

#[test]
fn an_executable_change_moves_the_digest() {
    let a = digest_of(
        "fn f(x: Int) -> Int { x + 1 }\nfn main() -> Int { f(1) }\n",
        "f",
    );
    let b = digest_of(
        "fn f(x: Int) -> Int { x + 2 }\nfn main() -> Int { f(1) }\n",
        "f",
    );
    let c = digest_of(
        "fn f(x: Int) -> Int { x - 1 }\nfn main() -> Int { f(1) }\n",
        "f",
    );
    assert_ne!(a, b);
    assert_ne!(a, c);
}

#[test]
fn an_effect_only_change_leaves_the_executable_digest_alone() {
    // The same body, with and without a declared effect contract.
    let a = digest_of(
        "fn f(x: Int) -> Int { x + 1 }\nfn main() -> Int { f(1) }\n",
        "f",
    );
    let b = digest_of(
        "fn f(x: Int) -> Int ! {} { x + 1 }\nfn main() -> Int { f(1) }\n",
        "f",
    );
    let c = digest_of(
        "fn f(x: Int) -> Int ! { io } { x + 1 }\nfn main() -> Int { f(1) }\n",
        "f",
    );
    assert_eq!(a, b, "effect metadata is not executable");
    assert_eq!(a, c, "a wider declared contract is not executable either");
}

#[test]
fn a_capability_parameter_change_moves_the_digest() {
    let a = digest_of(
        "fn f(io: IoCap, x: Int) -> Int { x }\nfn main(io: IoCap) -> Int { f(io, 1) }\n",
        "f",
    );
    let b = digest_of(
        "fn f(sp: SpawnCap, x: Int) -> Int { x }\nfn main(io: IoCap, sp: SpawnCap) -> Int { f(sp, 1) }\n",
        "f",
    );
    assert_ne!(
        a, b,
        "a parameter's type is part of the executable signature"
    );
}

#[test]
fn an_unrelated_function_leaves_a_digest_alone() {
    let a = digest_of(
        "fn f(x: Int) -> Int { x + 1 }\nfn main() -> Int { f(1) }\n",
        "f",
    );
    let b = digest_of(
        "fn f(x: Int) -> Int { x + 1 }\nfn other() -> Int { 7 }\nfn main() -> Int { f(1) + other() }\n",
        "f",
    );
    assert_eq!(a, b);
}

// --- scale -------------------------------------------------------------------------------------

fn time(f: impl FnOnce()) -> f64 {
    let t = std::time::Instant::now();
    f();
    t.elapsed().as_secs_f64()
}

/// Lowering to MIR and validating it, minus checking and Core IR: the MIR share of the cost.
fn mir_share(src: &str) -> f64 {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("prog.nz");
    std::fs::write(&path, src).unwrap();
    let loaded = nazm_core::eval::on_interpreter_stack(0, || {
        nazm_service::load::load(&path, dir.path()).expect("load")
    })
    .expect("a thread");
    nazm_core::on_lowered(
        &loaded.map,
        &loaded.graph,
        nazm_core::Entry::Absent,
        |c, r| {
            let mut best = f64::MAX;
            for _ in 0..3 {
                best = best.min(time(|| {
                    let p = nazm_mir::lower(c, r, &loaded.graph).unwrap();
                    assert!(nazm_mir::verify(&p).is_empty());
                }));
            }
            Ok(best)
        },
    )
    .unwrap()
}

fn large_function(n: usize) -> String {
    let mut s = String::from("fn big(k: Int) -> Int {\n    let mut total = 0;\n");
    for i in 0..n {
        s.push_str(&format!(
            "    let s{i} = int_to_str(k + {i});\n    if str_len(s{i}) > {m} {{ total = total + 1; }} else {{ total = total + g({i}); }}\n    let mut j{i} = 0;\n    while j{i} < 2 {{ j{i} = j{i} + 1; if j{i} == 5 {{ break; }} }}\n",
            m = i % 3
        ));
    }
    s.push_str("    total\n}\nfn g(x: Int) -> Int { x }\nfn main() -> Int { big(1) }\n");
    s
}

#[test]
fn a_large_function_lowers_in_time_proportional_to_its_size() {
    let small = mir_share(&large_function(250));
    let large = mir_share(&large_function(1000));
    // Four times the input: allow a generous constant for noise, and refuse anything that
    // looks quadratic.
    assert!(
        large < small * 4.0 * 3.0 + 0.05,
        "250 → {small:.4}s, 1000 → {large:.4}s"
    );
    let p = mir_of(&large_function(1000));
    let big = function(&p, "big");
    println!(
        "large function: {} blocks, {} statements, {} locals; MIR {small:.4}s at 250, {large:.4}s at 1000",
        big.blocks.len(),
        big.blocks.iter().map(|b| b.stmts.len()).sum::<usize>(),
        big.locals.len()
    );
}

#[test]
fn a_large_program_lowers_in_time_proportional_to_its_size() {
    let program = |n: usize| {
        let mut s = String::new();
        for i in 0..n {
            s.push_str(&format!(
                "fn f{i}(x: Int) -> Int {{ if x > {i} {{ x }} else {{ f{}(x + 1) }} }}\n",
                i + 1
            ));
        }
        s.push_str(&format!(
            "fn f{n}(x: Int) -> Int {{ x }}\nfn main() -> Int {{ f0(0) }}\n"
        ));
        s
    };
    let small = mir_share(&program(750));
    let large = mir_share(&program(3000));
    assert!(
        large < small * 4.0 * 3.0 + 0.05,
        "750 → {small:.4}s, 3000 → {large:.4}s"
    );
}

#[test]
fn deep_nesting_lowers_without_new_recursion_limits() {
    // As deep as the parser's own bound comfortably allows: nested blocks, ifs and loops.
    let depth = 60;
    let mut s = String::from("fn main() -> Int {\n    let mut t = 0;\n");
    for i in 0..depth {
        s.push_str(&format!(
            "    if t < {} {{ while t < {} {{ t = t + 1;\n",
            i + 1000,
            i + 1
        ));
    }
    for _ in 0..depth {
        s.push_str("    } }\n");
    }
    s.push_str("    t\n}\n");
    let p = valid(&s);
    assert!(function(&p, "main").blocks.len() > depth * 3);
    agree(&s);
}

#[test]
fn a_large_match_lowers_linearly() {
    let variants = 200;
    let mut s = String::from("enum Big { ");
    for i in 0..variants {
        s.push_str(&format!("V{i}(x: Int), "));
    }
    s.push_str("}\nfn score(b: Big) -> Int { match b { ");
    for i in 0..variants {
        s.push_str(&format!("Big.V{i}(x: x) => x + {i}, "));
    }
    s.push_str("} }\nfn main() -> Int { score(Big.V7(x: 1)) }\n");
    let p = valid(&s);
    let f = function(&p, "score");
    // One arm block per variant plus a small constant: linear, not quadratic.
    assert!(
        f.blocks.len() < variants * 2 + 10,
        "{} blocks",
        f.blocks.len()
    );
    agree(&s);
}

#[test]
fn many_branches_over_managed_values_reclaim_everything() {
    let mut s = String::from("fn main() -> Int {\n    let mut total = 0;\n    let mut i = 0;\n    while i < 50 {\n        let a = int_to_str(i);\n");
    for k in 0..40 {
        s.push_str(&format!(
            "        let b{k} = if i % {m} == 0 {{ str_concat(a, \"x\") }} else {{ int_to_str({k}) }};\n        if str_len(b{k}) == 99 {{ return 1; }}\n        total = total + str_len(b{k});\n",
            m = k % 5 + 2
        ));
    }
    s.push_str("        i = i + 1;\n    }\n    total\n}\n");
    agree(&s);
}

#[test]
fn many_nested_early_returns_neither_duplicate_nor_omit_cleanup() {
    let mut s = String::from("fn step(k: Int) -> Result[Int, Str] { if k == 17 { Result[Int, Str].Err(error: int_to_str(k)) } else { Result[Int, Str].Ok(value: k) } }\nfn chain(k: Int) -> Result[Int, Str] {\n    let s = int_to_str(k);\n");
    for i in 0..30 {
        s.push_str(&format!("    let v{i} = step(k + {i})?;\n"));
    }
    s.push_str("    Result[Int, Str].Ok(value: str_len(s) + v0 + v29)\n}\nfn main() -> Int {\n    let mut t = 0;\n    let mut k = 0;\n    while k < 20 { t = t + match chain(k) { Result.Ok(value: v) => v, Result.Err(error: e) => str_len(e) }; k = k + 1; }\n    t\n}\n");
    agree(&s);
}

/// Representative sizes, for the record (`docs/performance.md`, N40).
#[test]
#[ignore = "a measurement, not a check: run with --ignored --nocapture"]
fn what_mir_looks_like_on_the_compiler() {
    let p = mir_of_file(&repo().join("compiler/emit.nz"));
    let blocks: usize = p.functions.iter().map(|f| f.blocks.len()).sum();
    let stmts: usize = p
        .functions
        .iter()
        .flat_map(|f| &f.blocks)
        .map(|b| b.stmts.len())
        .sum();
    let locals: usize = p.functions.iter().map(|f| f.locals.len()).sum();
    let largest = p.functions.iter().max_by_key(|f| f.blocks.len()).unwrap();
    println!(
        "compiler/emit.nz: {} functions, {blocks} blocks, {stmts} statements, {locals} locals; \
         largest `{}`: {} blocks, {} locals",
        p.functions.len(),
        largest.name,
        largest.blocks.len(),
        largest.locals.len()
    );
}
