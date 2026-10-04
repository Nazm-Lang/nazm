//! N52: resource facts, the sink registry and declassification.
//!
//! `docs/architecture.md` §7.54. That `nazm explain-cost` reports what MIR does — each site
//! with a count per call that is `at-most-once` or `unknown`, never a number — that every sink
//! class is solved through helpers, closures and modules, that declassification needs its
//! authority everywhere and is recorded, that profiles read all of it, and that the analysis
//! stays deterministic and bounded on a large call graph.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Every program here finishes in well under a second; one that does not fails by name.
const DEADLINE: Duration = Duration::from_secs(60);

#[derive(Debug, PartialEq, Eq)]
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run_bounded(cmd: &mut Command, what: &str) -> Ran {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let start = Instant::now();
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            let o = child.wait_with_output().expect("collect output");
            return Ran {
                code: o.status.code(),
                stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            };
        }
        if start.elapsed() >= DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{what} did not terminate within {DEADLINE:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

fn memory_line(r: &Ran) -> String {
    r.stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .unwrap_or_else(|| panic!("no memory report: {}", r.stderr))
        .to_owned()
}

/// The program `files[0]` under the interpreter and built and run by each backend at each
/// optimisation level, every one with the memory report on.
fn everywhere(files: &[(&str, &str)]) -> Vec<(String, Ran)> {
    let d = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(d.path().join(name), text).unwrap();
    }
    let main = files[0].0;
    let mut interpreted = Command::new(NAZM);
    interpreted
        .current_dir(d.path())
        .args(["run", main])
        .env(nazm_lir::MEMORY_VARIABLE, "1");
    let mut out = vec![(
        "interpreter".to_owned(),
        run_bounded(&mut interpreted, "nazm run"),
    )];
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = d.path().join(label.replace([' ', '-'], ""));
        let b = nazm(
            d.path(),
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

/// Every implementation prints `expected`, exits with `code`, writes the same memory report
/// — closure objects counted with everything else — and reclaims all it allocated.
fn agrees_on(files: &[(&str, &str)], expected: &str, code: i32) -> String {
    let runs = everywhere(files);
    let first = memory_line(&runs[0].1);
    for (label, r) in &runs {
        assert_eq!(r.code, Some(code), "{label}: {}", r.stderr);
        assert_eq!(r.stdout.trim_end(), expected, "{label}");
        let line = memory_line(r);
        assert_eq!(
            line, first,
            "{label} and the interpreter disagree about storage"
        );
        for kind in ["sequences", "strings", "channels", "closures"] {
            let part = line.split(kind).nth(1).unwrap_or("");
            let live = part.split_whitespace().nth(2).unwrap_or("");
            assert_eq!(live, "live=0", "{label}: {kind} not all reclaimed: {line}");
        }
    }
    first
}

fn agrees(src: &str, expected: &str) -> String {
    agrees_on(&[("prog.nz", src)], expected, 0)
}

fn check_codes(files: &[(&str, &str)]) -> Vec<String> {
    let d = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(d.path().join(name), text).unwrap();
    }
    let r = nazm(d.path(), &["check", "--json", files[0].0]);
    let mut codes: Vec<String> = Vec::new();
    for line in r.stdout.lines().chain(r.stderr.lines()) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(c) = v["code"].as_str() {
            codes.push(c.to_owned());
        }
        if let Some(ds) = v["diagnostics"].as_array() {
            for d in ds {
                if let Some(c) = d["code"].as_str() {
                    codes.push(c.to_owned());
                }
            }
        }
    }
    codes
}

fn codes(src: &str) -> Vec<String> {
    check_codes(&[("prog.nz", src)])
}

fn json_of(files: &[(&str, &str)], args: &[&str]) -> serde_json::Value {
    let d = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(d.path().join(name), text).unwrap();
    }
    let mut a: Vec<&str> = args.to_vec();
    a.push("--json");
    a.push(files[0].0);
    let r = nazm(d.path(), &a);
    assert_eq!(r.code, Some(0), "{r:?}");
    serde_json::from_str(r.stdout.trim()).unwrap_or_else(|e| panic!("{e}: {}", r.stdout))
}

fn sites_of<'a>(cost: &'a serde_json::Value, function: &str) -> &'a Vec<serde_json::Value> {
    cost["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| {
            f["identity"]
                .as_str()
                .is_some_and(|i| i.ends_with(&format!("::fn {function}")))
        })
        .unwrap_or_else(|| panic!("no `{function}` in {cost}"))["sites"]
        .as_array()
        .unwrap()
}

// --- resource facts -----------------------------------------------------------------------

const COSTLY: &str = r#"
fn work(c: Chan) -> Int ! {} { if chan_send(c, 1) { 1 } else { 0 } }
fn main(io: IoCap, sp: SpawnCap) -> Int ! { io, spawn } {
    let xs = ints_new();
    let mut i = 0;
    while i < 3 { ints_push(xs, i); i = i + 1; }
    let s = str_concat("a", "b");
    let t = s;
    let f = fn(x: Int) -> Int { x + str_len(t) };
    let c = chan_new(1);
    scope { spawn work(c); }
    let got = ints_new();
    if chan_recv(c, got) { print(int_to_str(ints_get(got, 0) + f(1))); }
    0
}
"#;

#[test]
fn explain_cost_reports_each_site_with_a_count_per_call() {
    let cost = json_of(&[("prog.nz", COSTLY)], &["explain-cost"]);
    assert_eq!(cost["schema"], "nazm.cost/1");
    let main = sites_of(&cost, "main");
    let find = |kind: &str, op: &str| {
        main.iter()
            .find(|s| s["kind"] == kind && s["op"] == op)
            .unwrap_or_else(|| panic!("no {kind} `{op}` in {main:?}"))
    };
    assert_eq!(find("allocation", "ints_new")["count"], "at-most-once");
    // Inside the loop: a count the compiler does not know, and never a number.
    assert_eq!(find("allocation", "ints_push")["count"], "unknown");
    assert_eq!(find("allocation", "str_concat")["count"], "at-most-once");
    assert_eq!(find("allocation", "closure")["count"], "at-most-once");
    assert_eq!(find("retain", "copy")["count"], "at-most-once");
    assert_eq!(find("channel", "chan_new")["count"], "at-most-once");
    assert_eq!(find("block", "chan_recv")["count"], "at-most-once");
    assert_eq!(find("task", "spawn")["count"], "at-most-once");
    assert_eq!(find("block", "join")["count"], "at-most-once");
    let work = sites_of(&cost, "work");
    assert!(
        work.iter()
            .any(|s| s["kind"] == "block" && s["op"] == "chan_send"),
        "{work:?}"
    );
    // Every count is one of the two words.
    for f in cost["functions"].as_array().unwrap() {
        for s in f["sites"].as_array().unwrap() {
            assert!(
                s["count"] == "at-most-once" || s["count"] == "unknown",
                "{s}"
            );
            // A source position, or a compiler-made failure path that says it is one.
            assert!(
                s["at"]["line"].as_u64().is_some() || s["on_failure"] == true,
                "{s}"
            );
        }
    }
    assert_eq!(
        cost["totals"]["allocation"]["unknown"], 1,
        "{}",
        cost["totals"]
    );
    // Deterministic: the same bytes twice.
    assert_eq!(cost, json_of(&[("prog.nz", COSTLY)], &["explain-cost"]));
}

#[test]
fn a_foreign_call_is_a_resource_site() {
    let src = "extern \"C\" fn labs(x: Int) -> Int = \"labs\";\n\
               fn main(f: ForeignCap) -> Int ! { foreign } { labs(0 - 3) }\n";
    let cost = json_of(&[("prog.nz", src)], &["explain-cost"]);
    let main = sites_of(&cost, "main");
    assert!(
        main.iter()
            .any(|s| s["kind"] == "foreign" && s["op"] == "labs"),
        "{main:?}"
    );
}

// --- the sink registry -------------------------------------------------------------------

#[test]
fn every_sink_class_is_solved_through_helpers_closures_and_containers() {
    let src = r#"
fn show(s: Str, out: OutCap) -> Int { print(s) }
fn main(io: IoCap) -> Int ! { io } {
    let secret = read_file("prog.nz");
    show(str_slice(secret, 0, 2), io);
    let names = strs_new();
    strs_push(names, "x");
    print(strs_get(names, 0));
    let echo = fn(t: Str) -> Int ! { io } { eprint(t) };
    echo("plain");
    write_file(str_concat("o", "ut.txt"), secret);
    print(arg(0));
    0
}
"#;
    let flow = json_of(&[("prog.nz", src)], &["explain-flow"]);
    assert_eq!(flow["schema"], "nazm.flow/2");
    let sinks = flow["sinks"].as_array().unwrap();
    let at = |class: &str, function: &str, origin: &str| {
        sinks.iter().any(|s| {
            s["class"] == class
                && s["function"] == function
                && s["origins"]
                    .as_array()
                    .is_some_and(|o| o.iter().any(|x| x == origin))
        })
    };
    // Through a helper's parameter, from its caller.
    assert!(at("output", "show", "file"), "{flow}");
    // Out of a container, and through a closure's parameter: each carries what was written
    // into its cell, or handed by the calls that may reach it (N80) — here, literals.
    assert!(
        !sinks.iter().any(|s| s["origins"]
            .as_array()
            .is_some_and(|o| o.iter().any(|x| x == "unknown"))),
        "{flow}"
    );
    assert!(
        sinks.iter().any(
            |s| s["function"] == "main#0" && s["origins"].as_array().is_some_and(Vec::is_empty)
        ),
        "{flow}"
    );
    // Each origin at a sink carries its chain, from where it entered to the sink (N80).
    let shown = sinks
        .iter()
        .find(|s| s["class"] == "output" && s["function"] == "show")
        .unwrap();
    let trace = &shown["traces"][0];
    assert_eq!(trace["origin"], "file", "{shown}");
    let steps = trace["steps"].as_array().unwrap();
    assert_eq!(
        steps.first().unwrap()["note"],
        "a file's contents are read here",
        "{shown}"
    );
    assert!(
        steps.last().unwrap()["note"]
            .as_str()
            .unwrap()
            .contains("standard output"),
        "{shown}"
    );
    assert!(
        steps.iter().any(|s| s["note"]
            .as_str()
            .unwrap()
            .contains("`show` is called here")),
        "{shown}"
    );
    assert!(at("file-data", "main", "file"), "{flow}");
    assert!(at("output", "main", "argument"), "{flow}");
    // The path is computed in the program: nothing restricted reaches it.
    assert!(
        sinks
            .iter()
            .any(|s| s["class"] == "path" && s["origins"].as_array().is_some_and(Vec::is_empty)),
        "{flow}"
    );
}

// --- declassification --------------------------------------------------------------------

#[test]
fn a_vouch_needs_its_authority_everywhere_and_is_recorded() {
    // Declared or not, a vouch needs a `VouchCap` held.
    assert_eq!(
        codes("fn main(io: IoCap) -> Int { let s = str_vouch(\"x\"); 0 }\n"),
        ["N0369"]
    );
    assert_eq!(
        codes("fn main(io: IoCap) -> Int ! {} { let s = str_vouch(\"x\"); 0 }\n"),
        ["N0369"]
    );
    // Neither the type nor the built-in can be redefined.
    assert_eq!(
        codes("struct VouchCap { x: Int, }\nfn main() -> Int { 0 }\n"),
        ["N0331"]
    );
    // It clears an origin, so the path rule no longer applies — and the clearing is evidence.
    let refused = "fn main(io: IoCap) -> Int ! { io } \
        { let p = read_file(\"prog.nz\"); write_file(p, \"x\") }\n";
    assert_eq!(codes(refused), ["N0372"]);
    let vouched = "fn main(io: IoCap, v: VouchCap) -> Int ! { io } \
        { let p = read_file(\"prog.nz\"); write_file(str_vouch(str_slice(p, 0, 0)), \"x\") }\n";
    assert_eq!(codes(vouched), Vec::<String>::new());
    let flow = json_of(&[("prog.nz", vouched)], &["explain-flow"]);
    assert!(
        flow["sinks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["class"] == "vouch"
                && s["origins"]
                    .as_array()
                    .is_some_and(|o| o.iter().any(|x| x == "file"))),
        "{flow}"
    );
}

#[test]
fn a_vouch_changes_nothing_at_run_time_everywhere() {
    agrees(
        r#"
fn tag(v: VouchCap) -> Str ! {} {
    let s = str_concat("to", "ken");
    str_vouch(str_slice(s, 0, 3))
}
fn main(io: IoCap, v: VouchCap) -> Int ! { io } {
    // The vouched value outlives the string it was cut from, and storage is allocated after:
    // a vouch that took no reference of its own would print what reused it.
    let t = tag(v);
    let other = str_concat("xx", "yyyy");
    print(t);
    print(other);
    0
}
"#,
        "tokxxyyyy0",
    );
}

// --- profiles ----------------------------------------------------------------------------

fn profile_refuses(src: &str, profile: &str, rule: &str) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let r = nazm(d.path(), &["check", "--profile", profile, "prog.nz"]);
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(
        r.stderr.contains("N0510") && r.stderr.contains(rule),
        "{r:?}"
    );
}

#[test]
fn profiles_read_resource_and_flow_facts() {
    profile_refuses(
        "fn main() -> Int ! {} { let xs = ints_new(); let mut i = 0; \
         while i < 3 { ints_push(xs, i); i = i + 1; } ints_len(xs) }\n",
        "embedded",
        "bounded-allocation",
    );
    profile_refuses(
        "fn main(io: IoCap) -> Int ! { io } { print(read_file(\"prog.nz\")) }\n",
        "cyber",
        "contents-stay-in-files",
    );
    profile_refuses(
        "fn main(io: IoCap, v: VouchCap) -> Int ! { io } { print(str_vouch(\"a\")) }\n",
        "critical",
        "no-declassification",
    );
    // A file's contents may go back to a file under `cyber`.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn main(io: IoCap) -> Int ! { io } { write_file(\"copy.txt\", read_file(\"prog.nz\")) }\n",
    )
    .unwrap();
    let r = nazm(
        d.path(),
        &["check", "--profile", "cyber", "--profile-report", "prog.nz"],
    );
    assert!(
        r.stdout.contains("\"contents-stay-in-files\"")
            && !r.stderr.contains("contents-stay-in-files"),
        "{r:?}"
    );
}

// --- bounded and deterministic -----------------------------------------------------------

#[test]
fn a_long_call_chain_is_solved_deterministically_and_in_bounded_time() {
    let n = 2000;
    let mut src = String::new();
    for i in 0..n {
        src.push_str(&format!(
            "fn f{i}(s: Str, out: OutCap) -> Int {{ f{}(str_concat(s, \"x\"), out) }}\n",
            i + 1
        ));
    }
    src.push_str(&format!("fn f{n}(s: Str, out: OutCap) -> Int {{ print(s) }}\n"));
    src.push_str("fn main(io: IoCap) -> Int ! { io } { f0(read_file(\"prog.nz\"), io) }\n");
    let start = Instant::now();
    let a = json_of(&[("prog.nz", &src)], &["explain-flow"]);
    let b = json_of(&[("prog.nz", &src)], &["explain-flow"]);
    assert!(
        start.elapsed() < Duration::from_secs(120),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(a, b);
    // The file's origin reaches the sink at the far end of the chain.
    let sinks = a["sinks"].as_array().unwrap();
    assert!(
        sinks.iter().any(|s| s["function"] == format!("f{n}")
            && s["origins"]
                .as_array()
                .is_some_and(|o| o.iter().any(|x| x == "file"))),
        "{a}"
    );
}

#[test]
fn a_cached_module_keeps_its_sink_classes() {
    // The library's sinks are read back from its cache entry on the second check: an output
    // sink that came back as a path sink would refuse what the first check accepted.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("nazm.root"), "").unwrap();
    std::fs::write(
        d.path().join("lib.nz"),
        "pub fn show(io: IoCap, s: Str) -> Int ! { io } { print(s) }\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join("main.nz"),
        "use \"lib.nz\";\nfn main(io: IoCap) -> Int ! { io } { show(io, read_file(\"lib.nz\")) }\n",
    )
    .unwrap();
    let first = nazm(d.path(), &["check", "main.nz"]);
    assert_eq!(first.code, Some(0), "{first:?}");
    let again = nazm(d.path(), &["check", "main.nz", "--cache-report", "--json"]);
    assert_eq!(again.code, Some(0), "{again:?}");
    assert!(again.stdout.contains("\"reused\":"), "{again:?}");
}
