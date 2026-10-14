//! N41: the Cranelift backend, behind the backend boundary.
//!
//! What only a whole build shows: that a program compiled by Cranelift behaves exactly as the
//! LLVM backend's and the interpreter's do — stdout, stderr, exit status and the runtime's
//! memory report — including every trap; that the same MIR gives the same object bytes; that
//! an unchanged unit's object is reused by its key without being generated, and a changed one
//! is not; that a target other than the host is refused rather than built for the host; and that
//! choosing Cranelift never quietly produces LLVM text for a unit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use nazm_lir::backend::{Backend, Code, Target};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

#[derive(Debug, PartialEq, Eq)]
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn build(path: &Path, backend: &str, exe: &Path, extra: &[&str]) -> std::process::Output {
    Command::new(NAZM)
        .args(["build", "--backend", backend])
        .args(extra)
        .arg(path)
        .arg("-o")
        .arg(exe)
        .output()
        .expect("build")
}

/// How long a compiled program may run. Every program here finishes in well under a second; a
/// miscompiled one that deadlocks fails by name instead of holding the suite until a harness
/// deadline.
const DEADLINE: Duration = Duration::from_secs(30);

/// Run a command, killing and reaping it if it outlives [`DEADLINE`].
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

fn run_exe(exe: &Path) -> Ran {
    run_bounded(
        common::bounded_exe(exe).env(nazm_lir::MEMORY_VARIABLE, "1"),
        &exe.display().to_string(),
    )
}

/// Build a file with both backends and run both, with the memory report; and interpret it.
fn three(path: &Path) -> (Ran, Ran, Ran) {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut out = Vec::new();
    for backend in ["llvm", "cranelift"] {
        let exe = dir.path().join(backend);
        let b = build(path, backend, &exe, &["--no-cache"]);
        assert!(
            b.status.success(),
            "{backend}: {}",
            String::from_utf8_lossy(&b.stderr)
        );
        out.push(run_exe(&exe));
    }
    let interp = run_bounded(
        Command::new(NAZM).arg("run").arg("--interpret").arg(path),
        "the interpreter",
    );
    let c = out.pop().unwrap();
    let l = out.pop().unwrap();
    (l, c, interp)
}

fn agree_src(src: &str) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("prog.nz");
    std::fs::write(&path, src).unwrap();
    let (llvm, clif, interp) = three(&path);
    assert_eq!(clif, llvm, "Cranelift and LLVM disagree");
    assert_eq!(clif.code, interp.code, "status against the interpreter");
    assert_eq!(clif.stdout, interp.stdout, "stdout against the interpreter");
}

#[test]
fn every_example_compiled_by_cranelift_agrees_with_llvm_and_the_interpreter() {
    for example in [
        "examples/sum.nz",
        "examples/max.nz",
        "examples/primes.nz",
        "examples/search.nz",
        "examples/pipeline.nz",
        "examples/worker-pool.nz",
    ] {
        let (llvm, clif, interp) = three(&repo().join(example));
        assert_eq!(clif, llvm, "{example}");
        assert_eq!(clif.stdout, interp.stdout, "{example}");
        assert_eq!(clif.code, interp.code, "{example}");
    }
}

#[test]
fn signed_arithmetic_and_its_edges_agree() {
    agree_src(
        r#"
fn main() -> Int {
    let min = -9223372036854775807 - 1;
    let a = min % -1;
    let b = -7 / 2;
    let c = -7 % 2;
    let d = 7 % -2;
    let e = if -1 < 0 { 1 } else { 0 };
    let f = if min < 0 { 10 } else { 0 };
    let g = 9223372036854775807 - 9223372036854775806;
    a + b + c + d + e + f + g
}
"#,
    );
}

#[test]
fn every_trap_is_the_same_trap() {
    for body in [
        "let x = 9223372036854775807; x + 1",
        "let x = -9223372036854775807 - 1; x - 1",
        "let x = 4611686018427387904; x * 2",
        "let x = -9223372036854775807 - 1; -x",
        "let x = 0; 1 / x",
        "let x = 0; 1 % x",
        "let x = -9223372036854775807 - 1; x / -1",
        "str_byte(\"ab\", 2)",
        "str_len(str_slice(\"abc\", 2, 1))",
        "ints_get(ints_new(), 0)",
        "ints_pop(ints_new())",
        "str_len(str_from_byte(256))",
        "let c = chan_new(0); 0",
    ] {
        let src = format!("fn main() -> Int {{\n    {body}\n}}\n");
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("prog.nz");
        std::fs::write(&path, &src).unwrap();
        let (llvm, clif, interp) = three(&path);
        assert_eq!(clif, llvm, "{body}");
        assert_eq!(clif.code, Some(2), "{body}: {}", clif.stderr);
        assert_eq!(interp.code, Some(2), "{body}");
        // The same code as the interpreter's diagnostic, which is rendered with its source.
        let code = |s: &str| {
            s.split("error[")
                .nth(1)
                .and_then(|r| r.split(']').next())
                .map(str::to_owned)
        };
        assert!(code(&clif.stderr).is_some(), "{body}: {}", clif.stderr);
        assert_eq!(code(&clif.stderr), code(&interp.stderr), "{body}");
    }
}

#[test]
fn records_enums_matches_and_managed_values_agree() {
    agree_src(
        r#"
struct Pair { name: Str, n: Int, flag: Bool, }
enum Shape { Circle(r: Int), Named(title: Str, sides: Int), Empty }
struct Nest { inner: Pair, shape: Shape, }
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
fn make(i: Int) -> Nest {
    Nest(inner: Pair(name: int_to_str(i), n: i, flag: i % 2 == 0), shape: Shape.Named(title: str_concat("t", int_to_str(i)), sides: i))
}
fn main() -> Int {
    let mut total = 0;
    let shapes = vec_new[Shape]();
    let names = strs_new();
    let mut i = 0;
    while i < 30 {
        vec_push(shapes, Shape.Circle(r: i));
        vec_push(shapes, Shape.Named(title: int_to_str(i), sides: 3));
        vec_push(shapes, Shape.Empty());
        strs_push(names, str_concat("n", int_to_str(i)));
        let m = make(i);
        let copy = m;
        total = total + str_len(copy.inner.name) + area(m.shape);
        if copy.inner.flag { total = total + 1; }
        if m == make(i) { total = total + 100; }
        if m != make(i + 1) { total = total + 1000; }
        i = i + 1;
    }
    let mut k = 0;
    while k < vec_len(shapes) { total = total + area(vec_get(shapes, k)); k = k + 1; }
    vec_set(shapes, 0, Shape.Named(title: "replaced", sides: 9));
    let last = vec_pop(shapes);
    total = total + area(last) + area(vec_get(shapes, 0));
    strs_set(names, 1, "x");
    total = total + str_len(str_join(names, ","));
    total = total + match twice("xy") { Result.Ok(value: x) => x, Result.Err(error: e) => str_len(e) };
    total = total + match twice("") { Result.Ok(value: x) => x, Result.Err(error: e) => str_len(e) + 7 };
    total
}
"#,
    );
}

#[test]
fn cleanup_on_every_exit_and_on_failure_agrees() {
    agree_src(
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
        if i == n { return str_len(x); }
        if i == 7 { break; }
    }
    str_len(a)
}
fn main() -> Int {
    let mut total = 0;
    let mut k = 0;
    while k < 6 {
        total = total + early(k);
        total = total + match build(k) { Result.Ok(value: b) => str_len(b.s) + str_len(b.t), Result.Err(error: e) => str_len(e) };
        k = k + 1;
    }
    let z = k - 6;
    total / z
}
"#,
    );
}

#[test]
fn recursion_compiles_under_both_backends_and_agrees() {
    // Refused by both until N49; both now emit the stack check MIR places first in each
    // function of a call cycle (`docs/spec.md`, *Native recursion*).
    agree_src(
        "fn f(n: Int) -> Int { if n == 0 { 0 } else { f(n - 1) + 1 } }\n\
         fn even(n: Int) -> Bool { if n == 0 { true } else { odd(n - 1) } }\n\
         fn odd(n: Int) -> Bool { if n == 0 { false } else { even(n - 1) } }\n\
         fn main() -> Int { if even(10) { f(30) } else { 0 } }\n",
    );
}

#[test]
fn a_target_the_host_cannot_link_is_refused_without_objects_and_nothing_is_written() {
    // Until N57 every other target was refused; since, one the host has no linker for is.
    let host = Target::host().unwrap();
    let foreign = if host.triple.starts_with("aarch64") {
        "x86_64-unknown-linux-gnu"
    } else {
        "aarch64-unknown-linux-gnu"
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = dir.path().join("prog");
    // A host with no linker for the target is the machine's (4); a triple that is no target is
    // the command line's (5) — the v1 exit contract (Q1-C1).
    for (triple, says, status) in [
        (foreign, "has no linker for", 4),
        ("riscv64gc-unknown-linux-gnu", "not a target", 5),
    ] {
        for backend in ["llvm", "cranelift"] {
            let b = build(
                &repo().join("examples/sum.nz"),
                backend,
                &exe,
                &["--target", triple],
            );
            assert_eq!(b.status.code(), Some(status), "{triple} {backend}");
            let stderr = String::from_utf8_lossy(&b.stderr);
            assert!(stderr.contains(says), "{stderr}");
            assert!(!exe.exists());
        }
    }
    // The host, named, is the host.
    let b = build(
        &repo().join("examples/sum.nz"),
        "cranelift",
        &exe,
        &["--target", &host.triple],
    );
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
}

#[test]
fn choosing_cranelift_writes_no_llvm_text_for_any_unit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = dir.path().join("prog");
    let b = build(
        &repo().join("examples/pipeline.nz"),
        "cranelift",
        &exe,
        &["--emit-ir", "--no-cache"],
    );
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    let mut ll: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".ll"))
        .collect();
    ll.sort();
    // Only the runtime, the platform entry, the task pool's switch (N83), the target's
    // `errno` (N85), its `os` unit (Gate 2) and its `stack` unit (Gate 3) are LLVM text; every
    // unit is an object.
    assert_eq!(
        ll,
        [
            "prog.entry.ll",
            "prog.errno.ll",
            "prog.os.ll",
            "prog.runtime.ll",
            "prog.stack.ll",
            "prog.switch.ll"
        ],
        "{ll:?}"
    );
}

/// The native program for a source, as a backend sees it.
fn native_of(
    path: &Path,
) -> (
    nazm_lir::Native,
    nazm_span::SourceMap,
    nazm_core::sema::ModuleGraph,
) {
    let dir = path.parent().unwrap().to_path_buf();
    nazm_core::eval::on_interpreter_stack(0, || {
        let loaded = nazm_service::load::load(path, &dir).expect("load");
        let names = nazm_lir::SourceNames::of(&loaded.map, &loaded.graph);
        let native = nazm_core::on_lowered(
            &loaded.map,
            &loaded.graph,
            nazm_core::Entry::Required,
            |c, r| {
                let p = nazm_mir::lower(c, r, &loaded.graph)?;
                assert!(nazm_mir::verify(&p).is_empty());
                nazm_lir::lower(p, &names).map_err(|e| e.diagnostics)
            },
        )
        .unwrap();
        (native, loaded.map, loaded.graph)
    })
    .expect("a thread")
}

#[test]
fn the_same_mir_gives_the_same_object_bytes() {
    let path = repo().join("compiler/emit.nz");
    let (native, map, graph) = native_of(&path);
    let names = nazm_lir::SourceNames::of(&map, &graph);
    let clif = nazm_codegen_clif::Cranelift::new(Target::host().unwrap(), 0);
    let objects = |_: ()| -> Vec<Vec<u8>> {
        clif.emit(&native, &names)
            .unwrap()
            .units
            .into_iter()
            .map(|(_, g)| match g.code {
                Code::Object(b) => b,
                Code::Llvm(_) => panic!("Cranelift produced LLVM text"),
            })
            .collect()
    };
    let a = objects(());
    let b = objects(());
    assert!(!a.is_empty());
    assert_eq!(a, b, "two generations of one MIR differ");
}

#[test]
fn a_cache_key_moves_with_every_setting_that_changes_the_code_and_only_those() {
    let path = repo().join("examples/pipeline.nz");
    let (native, map, graph) = native_of(&path);
    let names = nazm_lir::SourceNames::of(&map, &graph);
    let host = Target::host().unwrap();
    let keys = |b: &dyn Backend| -> Vec<Option<String>> {
        b.plan(&native, &names).into_iter().map(|p| p.key).collect()
    };
    let o0 = nazm_codegen_clif::Cranelift::new(host.clone(), 0);
    let o0b = nazm_codegen_clif::Cranelift::new(host.clone(), 0);
    let o2 = nazm_codegen_clif::Cranelift::new(host.clone(), 2);
    assert_eq!(keys(&o0), keys(&o0b), "the same settings, the same keys");
    assert!(keys(&o0).iter().all(Option::is_some));
    let (a, b) = (keys(&o0), keys(&o2));
    assert!(
        a.iter().zip(&b).all(|(x, y)| x != y),
        "an optimisation level is in every key"
    );
    assert!(
        o0.version().contains("cranelift-0.136.1"),
        "{}",
        o0.version()
    );
    assert!(o0.version().contains("verify=true"));
    assert!(o0.version().contains("runtime-abi="));
    // LLVM text keys itself by its bytes.
    let llvm = nazm_lir::backend::LlvmText::new(host);
    assert!(keys(&llvm).iter().all(Option::is_none));
}

fn project(dir: &Path, files: &[(&str, &str)]) {
    std::fs::write(dir.join("nazm.root"), "").unwrap();
    for (name, src) in files {
        std::fs::write(dir.join(name), src).unwrap();
    }
}

fn report(dir: &Path) -> serde_json::Value {
    let out = Command::new(NAZM)
        .current_dir(dir)
        .args([
            "build",
            "--backend",
            "cranelift",
            "--build-report",
            "main.nz",
            "-o",
            "prog",
        ])
        .output()
        .expect("build");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let line = String::from_utf8_lossy(&out.stdout)
        .lines()
        .find(|l| l.starts_with('{'))
        .unwrap()
        .to_owned();
    serde_json::from_str(&line).unwrap()
}

#[test]
fn an_unchanged_unit_is_reused_by_its_key_and_a_changed_one_is_regenerated() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(
        dir.path(),
        &[
            (
                "main.nz",
                "use \"lib.nz\";\nfn main() -> Int { twice(21) }\n",
            ),
            ("lib.nz", "pub fn twice(n: Int) -> Int { n * 2 }\n"),
        ],
    );
    let first = report(dir.path());
    assert_eq!(first["native"]["reused"], 0, "{first}");
    let second = report(dir.path());
    // Both Cranelift units come from the store; only the runtime and the entry are LLVM text,
    // and those are reused by clang's cache as before.
    let units = second["native"]["units"].as_u64().unwrap();
    assert_eq!(second["native"]["reused"].as_u64(), Some(units), "{second}");
    // Change the library's body only: its unit is regenerated, the caller's is not.
    std::fs::write(
        dir.path().join("lib.nz"),
        "pub fn twice(n: Int) -> Int { n + n }\n",
    )
    .unwrap();
    let third = report(dir.path());
    assert_eq!(third["native"]["compiled"], 1, "{third}");
    assert_eq!(
        third["native"]["reused"].as_u64(),
        Some(units - 1),
        "{third}"
    );
    let ran = Command::new(dir.path().join("prog")).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&ran.stdout).trim(), "42");
}
