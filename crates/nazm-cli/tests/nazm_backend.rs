//! R9, D15: `nazm build --backend nazm`, Nazm's own code generator (`nzcg`, `toolchain/codegen/`).
//!
//! What only a whole build shows: that every program of the generator's own differential suite
//! (`toolchain/codegen/tests/run.sh`, here driving `--backend nazm` instead of substituting
//! objects) behaves exactly as the LLVM build — stdout, stderr, exit status and the runtime's
//! memory report; that the generator is told the target and whether the program has one thread,
//! and handed the positions with `--debug`; that a construct it refuses is `N0101`, named, with
//! nothing written and no fallback, and any other failure `N0900`; and that a generator that is
//! not there, or a target it does not carry, is refused before anything is compiled.
//!
//! The generator is built once per test process from `toolchain/codegen/` by this `nazm`
//! (`differential::nzcg`). macOS only before 1.0 (the generator's D13): elsewhere these tests
//! are ignored, and counted as such.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common::{self, run_bounded, run_within, Ran};
use crate::differential;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

/// `nazm ARGS` in `dir`, with `NAZM_NZCG` naming `nzcg`.
fn nazm_with(dir: &Path, nzcg: &Path, args: &[&str]) -> Ran {
    run_bounded(
        Command::new(NAZM)
            .current_dir(dir)
            .env(nazm_codegen_nazm::VARIABLE, nzcg)
            .args(args),
        "nazm",
    )
}

/// The `.nz` files directly in `dir`, sorted.
fn nz_in(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|d| {
            d.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "nz"))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Every `.nz` file under `dir`, sorted.
fn nz_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(d) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = d.filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            nz_under(&p, out);
        } else if p.extension().is_some_and(|x| x == "nz") {
            out.push(p);
        }
    }
}

/// The inputs of `toolchain/codegen/tests/run.sh`, in its order.
fn inputs() -> Vec<PathBuf> {
    let r = repo();
    let fib = r.join("bench/compare/nazm/fib.nz");
    let mut out = vec![fib.clone()];
    out.extend(nz_in(&r.join("tests/compat/v1/run")));
    if let Ok(d) = std::fs::read_dir(r.join("tests/compat/v1/run")) {
        let mut dirs: Vec<PathBuf> = d
            .filter_map(Result::ok)
            .map(|e| e.path().join("main.nz"))
            .filter(|p| p.is_file())
            .collect();
        dirs.sort();
        out.extend(dirs);
    }
    for d in [
        "tests/compat/v1/trap",
        "toolchain/codegen/tests/programs",
        "bench/compare/nazm",
        "bench/programs",
        "examples",
        "examples/tour",
        "examples/std",
        "crates/nazm-cli/tests/transfers",
    ] {
        out.extend(nz_in(&r.join(d)).into_iter().filter(|p| *p != fib));
    }
    nz_under(&r.join("corpus"), &mut out);
    out
}

/// What one input came to.
enum Outcome {
    Agree,
    Skipped(String),
    Differ(String),
}

/// One run, its memory report on, in `dir`, stdin empty. A program that runs until the stack is
/// exhausted (it says `nzcg: stack depth`) allocates as many times as its frames fit, which the
/// backends' frames differ in: each count of things allocated and reclaimed is then only
/// required to match the other, every `live` count still exactly (as `run.sh` compares).
fn ran(exe: &Path, dir: &Path, depth: bool) -> Option<Ran> {
    let mut c = Command::new(exe);
    c.current_dir(dir).env(nazm_lir::MEMORY_VARIABLE, "1");
    let mut r = run_within(&mut c, std::time::Duration::from_secs(120))?;
    if depth {
        r.stderr = r
            .stderr
            .lines()
            .map(|l| {
                let mut words: Vec<String> = l.split(' ').map(str::to_owned).collect();
                for k in 0..words.len().saturating_sub(1) {
                    let (a, b) = (&words[k], &words[k + 1]);
                    if let (Some(x), Some(y)) =
                        (a.strip_prefix("allocated="), b.strip_prefix("reclaimed="))
                    {
                        if x == y {
                            words[k] = "allocated=N".to_owned();
                            words[k + 1] = "reclaimed=N".to_owned();
                        }
                    }
                }
                words.join(" ")
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    Some(r)
}

/// Built for `target` when one is given (x86-64 runs under Rosetta), else for this host.
fn one(src: &Path, nzcg: &Path, work: &Path, target: Option<&str>) -> Outcome {
    let rel = src
        .strip_prefix(repo())
        .unwrap_or(src)
        .display()
        .to_string();
    let w = work.join(rel.replace(['/', '.'], "_"));
    std::fs::create_dir_all(&w).unwrap();
    let mut link: Vec<String> = Vec::new();
    let c = src.with_extension("c");
    if c.is_file() {
        let o = w.join("c.o");
        let mut cc = Command::new("cc");
        if target == Some("x86_64-apple-darwin") {
            cc.args(["-arch", "x86_64"]);
        }
        let r = run_bounded(cc.arg("-c").arg(&c).arg("-o").arg(&o), "cc");
        if r.code != Some(0) {
            return Outcome::Skipped(format!("{rel}: its C does not compile"));
        }
        link = vec!["--link".to_owned(), o.display().to_string()];
    }
    let build = |backend: &str, exe: &Path| {
        let mut args: Vec<String> = vec![
            "build".into(),
            "--no-cache".into(),
            "--backend".into(),
            backend.into(),
        ];
        if let Some(t) = target {
            args.extend(["--target".to_owned(), t.to_owned()]);
        }
        args.extend(link.iter().cloned());
        args.push(src.display().to_string());
        args.push("-o".into());
        args.push(exe.display().to_string());
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        nazm_with(&w, nzcg, &args)
    };
    let (rexe, gexe) = (w.join("ref"), w.join("gen"));
    let rb = build("llvm", &rexe);
    if rb.code != Some(0) {
        return Outcome::Skipped(format!("{rel}: the LLVM build fails"));
    }
    let gb = build("nazm", &gexe);
    if gb.code != Some(0) {
        return Outcome::Differ(format!(
            "{rel}: `--backend nazm` does not build it ({:?}): {}",
            gb.code,
            gb.stderr.lines().take(3).collect::<Vec<_>>().join(" | ")
        ));
    }
    let depth = std::fs::read_to_string(src).is_ok_and(|t| t.contains("nzcg: stack depth"));
    let (Some(a), Some(b)) = (ran(&rexe, &w, depth), ran(&gexe, &w, depth)) else {
        return Outcome::Differ(format!("{rel}: a run did not terminate"));
    };
    if a == b {
        let _ = std::fs::remove_dir_all(&w);
        Outcome::Agree
    } else {
        let mut what = Vec::new();
        if a.stdout != b.stdout {
            what.push("stdout".to_owned());
        }
        if a.stderr != b.stderr {
            what.push(format!("stderr ({:?} / {:?})", a.stderr, b.stderr));
        }
        if a.code != b.code {
            what.push(format!("status ({:?} / {:?})", a.code, b.code));
        }
        Outcome::Differ(format!("{rel}: {}", what.join(", ")))
    }
}

/// A program of tasks built by each backend runs under the pool on one, two and four workers and
/// under the thread model, and gives the LLVM build's answer every time (performance leadership
/// 6.2-6.4): tasks that allocate, recurse, park on channels deep in their frames (6.2's copied
/// stacks), spawn tasks of their own (6.3's stealing) and are preempted while they compute. The
/// argument block a generated `spawn` allocates is freed by the runtime's scope join, so it must
/// come from the allocator the runtime frees with (R1.5 3.1's small-block heap).
#[cfg_attr(not(target_os = "macos"), ignore = "macOS only before 1.0")]
#[test]
fn a_task_program_from_each_backend_runs_on_any_number_of_workers() {
    let nzcg = differential::nzcg();
    let d = tempfile::tempdir().unwrap();
    let src = "fn depth(n: Int) -> Int { if n == 0 { 0 } else { 1 + depth(n - 1) } }\n\
fn leaf(c: Chan, v: Int) -> Int { chan_send(c, v); 0 }\n\
fn work(t: SpawnCap, k: Int, inp: Chan, out: Chan) -> Int ! { spawn } {\n\
    let mut total = 0\n\
    let buf = ints_new()\n\
    let mut round = 0\n\
    while round < 40 {\n\
        let v = ints_new()\n\
        let mut i = 0\n\
        while i < 500 { ints_push(v, (i * k + round) % 97); i += 1 }\n\
        let mut s = \"\"\n\
        let mut j = 0\n\
        while j < 20 { s = s + int_to_str(ints_get(v, j * 7)); j += 1 }\n\
        total = (total + str_len(s) + ints_pop(v) + depth(50 + k)) % 1000000007\n\
        round += 1\n\
    }\n\
    if chan_recv(inp, buf) { total += ints_pop(buf) }\n\
    let side = chan_new(4)\n\
    scope { spawn leaf(side, k); spawn leaf(side, k + 1) }\n\
    if chan_recv(side, buf) { total += ints_pop(buf) }\n\
    if chan_recv(side, buf) { total += ints_pop(buf) }\n\
    chan_send(out, total)\n\
    0\n\
}\n\
fn main(io: IoCap, tasks: SpawnCap) {\n\
    let inp = chan_new(1)\n\
    let out = chan_new(64)\n\
    let mut sum = 0\n\
    scope {\n\
        let mut k = 0\n\
        while k < 12 { spawn work(tasks, k, inp, out); k += 1 }\n\
        let mut r = 0\n\
        while r < 12 { chan_send(inp, r * 1000); r += 1 }\n\
        let buf = ints_new()\n\
        let mut got = 0\n\
        while got < 12 { if chan_recv(out, buf) { sum = (sum + ints_pop(buf)) % 1000000007 }; got += 1 }\n\
    }\n\
    println(sum)\n\
}\n";
    std::fs::write(d.path().join("tasks.nz"), src).unwrap();
    let mut want: Option<String> = None;
    for backend in ["llvm", "cranelift", "nazm"] {
        let exe = crate::common::exe_name(format!("t-{backend}"));
        let b = nazm_with(
            d.path(),
            nzcg,
            &["build", "--backend", backend, "tasks.nz", "-o", &exe],
        );
        assert_eq!(b.code, Some(0), "{backend}: {}{}", b.stdout, b.stderr);
        for (var, val) in [
            ("NAZM_WORKERS", "1"),
            ("NAZM_WORKERS", "2"),
            ("NAZM_WORKERS", "4"),
            ("NAZM_SCHEDULER", "threads"),
        ] {
            let mut c = Command::new(d.path().join(&exe));
            c.current_dir(d.path())
                .env_remove("NAZM_SCHEDULER")
                .env(var, val);
            let r = run_within(&mut c, std::time::Duration::from_secs(120))
                .unwrap_or_else(|| panic!("{backend} {var}={val}: did not end"));
            assert_eq!(r.code, Some(0), "{backend} {var}={val}: {}", r.stderr);
            let w = want.get_or_insert_with(|| r.stdout.clone());
            assert_eq!(&r.stdout, w, "{backend} {var}={val}");
        }
    }
}

/// Performance leadership 6.3: Nazm's own code generator's code is preempted as LLVM's and
/// Cranelift's is — it writes its code in the generated code's section (`__TEXT,__nz_text`), so the
/// monitor's interrupt finds a looping task there and it yields. Two tasks loop until four short
/// ones queued behind them on the same two workers have run; without preemption they never start.
#[cfg_attr(not(target_os = "macos"), ignore = "macOS only before 1.0")]
#[test]
fn a_long_loop_from_nazms_own_code_generator_yields_its_worker() {
    let nzcg = differential::nzcg();
    let d = tempfile::tempdir().unwrap();
    let src = "fn spin(stop: Atomic) -> Int { let mut x = 0; while atomic_load(stop) == 0 { x = (x + 1) % 1000; } x }\n\
               fn short(stop: Atomic, done: Atomic) -> Int { atomic_add(done, 1); if atomic_load(done) == 4 { atomic_store(stop, 1); } 0 }\n\
               fn main(io: IoCap, tasks: SpawnCap) {\n\
               let stop = atomic_new(0); let done = atomic_new(0);\n\
               scope { spawn spin(stop); spawn spin(stop); spawn short(stop, done); spawn short(stop, done); spawn short(stop, done); spawn short(stop, done); }\n\
               println(atomic_load(done)) }\n";
    std::fs::write(d.path().join("starve.nz"), src).unwrap();
    let b = nazm_with(
        d.path(),
        nzcg,
        &["build", "--backend", "nazm", "starve.nz", "-o", "starve"],
    );
    assert_eq!(b.code, Some(0), "{}{}", b.stdout, b.stderr);
    let mut c = Command::new(d.path().join(crate::common::exe_name("starve")));
    c.current_dir(d.path())
        .env_remove("NAZM_SCHEDULER")
        .env("NAZM_WORKERS", "2");
    let r = run_within(&mut c, std::time::Duration::from_secs(60))
        .unwrap_or_else(|| panic!("the looping tasks were never preempted"));
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    assert!(r.stdout.starts_with("4\n"), "{}{}", r.stdout, r.stderr);
}

#[cfg_attr(not(target_os = "macos"), ignore = "macOS only before 1.0")]
#[test]
fn every_input_of_the_generators_suite_agrees_with_the_llvm_build() {
    let nzcg = differential::nzcg();
    let work = tempfile::tempdir().unwrap();
    let all = inputs();
    assert!(all.len() > 100, "{} inputs", all.len());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| loop {
                let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(src) = all.get(k) else { break };
                let o = one(src, nzcg, work.path(), None);
                results.lock().unwrap().push(o);
            });
        }
    });
    let results = results.into_inner().unwrap();
    let agree = results
        .iter()
        .filter(|o| matches!(o, Outcome::Agree))
        .count();
    let skipped: Vec<&String> = results
        .iter()
        .filter_map(|o| match o {
            Outcome::Skipped(s) => Some(s),
            _ => None,
        })
        .collect();
    let differ: Vec<&String> = results
        .iter()
        .filter_map(|o| match o {
            Outcome::Differ(s) => Some(s),
            _ => None,
        })
        .collect();
    eprintln!(
        "{agree} agree, {} differ, {} skipped: {skipped:?}",
        differ.len(),
        skipped.len()
    );
    assert!(differ.is_empty(), "{differ:#?}");
    // A skip is not agreement: every input builds with LLVM today.
    assert!(skipped.is_empty(), "{skipped:#?}");
}

/// The generator's own programs (`toolchain/codegen/tests/programs/`: every width, the failure
/// convention, C on both sides, and `guards.nz` — the remainder selected without its divisor
/// guard, 7.6) built for `x86_64-apple-darwin` and run under Rosetta: they behave as the LLVM
/// build for that target does. The suite above runs only this host's target.
#[cfg_attr(
    not(all(target_os = "macos", target_arch = "aarch64")),
    ignore = "x86-64 under Rosetta 2, on Apple silicon"
)]
#[test]
fn the_generators_own_programs_agree_with_llvm_on_x86_64_under_rosetta() {
    let rosetta = Command::new("arch")
        .args(["-x86_64", "/usr/bin/true"])
        .status()
        .is_ok_and(|s| s.success());
    if common::skip_without(rosetta, "no Rosetta 2 on this machine") {
        return;
    }
    let nzcg = differential::nzcg();
    let work = tempfile::tempdir().unwrap();
    let programs = nz_in(&repo().join("toolchain/codegen/tests/programs"));
    assert!(
        programs.iter().any(|p| p.ends_with("guards.nz")),
        "{programs:?}"
    );
    let mut differ = Vec::new();
    for src in &programs {
        match one(src, nzcg, work.path(), Some("x86_64-apple-darwin")) {
            Outcome::Agree => {}
            Outcome::Skipped(s) | Outcome::Differ(s) => differ.push(s),
        }
    }
    assert!(differ.is_empty(), "{differ:#?}");
}

/// The generator's own programs built for `x86_64-pc-windows-gnu` (its section 8): every unit
/// object is PE/COFF for AMD64 with a `.pdata` entry per function, and MinGW-w64 links each
/// program as its `link.txt` says, C beside it included. Compile- and link-verified only: running
/// them is `scripts/windows-host.sh` with `BACKEND=nazm`, on a Windows host no test reaches.
#[cfg_attr(not(target_os = "macos"), ignore = "macOS only before 1.0")]
#[test]
fn the_generators_own_programs_build_for_windows_and_link_with_mingw() {
    const GCC: &str = "x86_64-w64-mingw32-gcc";
    let mingw = Command::new(GCC)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if common::skip_without(mingw, "no MinGW-w64 gcc on this machine") {
        return;
    }
    let nzcg = differential::nzcg();
    let work = tempfile::tempdir().unwrap();
    let programs = nz_in(&repo().join("toolchain/codegen/tests/programs"));
    assert!(programs.len() > 15, "{programs:?}");
    let mut failed = Vec::new();
    for src in &programs {
        let name = src.file_stem().unwrap().to_string_lossy().into_owned();
        let w = work.path().join(&name);
        std::fs::create_dir_all(&w).unwrap();
        let mut args: Vec<String> = vec![
            "build".into(),
            "--no-cache".into(),
            "--backend".into(),
            "nazm".into(),
            "--target".into(),
            "x86_64-pc-windows-gnu".into(),
        ];
        let c = src.with_extension("c");
        if c.is_file() {
            let o = w.join("c.o");
            let r = run_bounded(Command::new(GCC).arg("-c").arg(&c).arg("-o").arg(&o), GCC);
            assert_eq!(r.code, Some(0), "{name}: {}", r.stderr);
            args.extend(["--link".to_owned(), o.display().to_string()]);
        }
        args.extend([
            "--objects".to_owned(),
            "o".to_owned(),
            src.display().to_string(),
        ]);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let b = nazm_with(&w, nzcg, &args);
        if b.code != Some(0) {
            failed.push(format!("{name}: does not build: {}", b.stderr));
            continue;
        }
        let o = w.join("o");
        let link = std::fs::read_to_string(o.join("link.txt")).unwrap();
        let field = |key: &str| {
            link.lines()
                .find_map(|l| l.strip_prefix(key))
                .unwrap_or("")
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let objects = field("objects ");
        for obj in objects.iter().filter(|f| {
            let s = f.trim_end_matches(".o");
            s.rsplit_once('-')
                .is_some_and(|(_, k)| k.starts_with(['m', 'g']))
        }) {
            let bytes = std::fs::read(o.join(obj)).unwrap();
            // IMAGE_FILE_MACHINE_AMD64, and a `.pdata` section name among the headers.
            let pdata = bytes
                .windows(8)
                .take(20 + 40 * 16)
                .any(|x| x == b".pdata\0\0");
            if bytes.get(..2) != Some(&[0x64, 0x86][..]) || !pdata {
                failed.push(format!("{name}: {obj} is not an AMD64 object with .pdata"));
            }
        }
        let r = run_bounded(
            Command::new(GCC)
                .current_dir(&o)
                .args(&objects)
                .args(field("libraries "))
                .arg("-o")
                .arg("p.exe"),
            GCC,
        );
        if r.code != Some(0) {
            failed.push(format!("{name}: does not link: {}", r.stderr));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// Each function's prologue as Windows's unwinder must read it, from its code (MinGW-w64
/// `objdump -d`): its start, the prologue's size, and its steps last first — `pc+0xOFF: push REG`
/// or `pc+0xOFF: alloc 0xN` — as `objdump -x` prints UNWIND_INFO.
fn prologues(dis: &str) -> Vec<(u64, u64, Vec<String>)> {
    let mut out = Vec::new();
    let mut insns: Vec<(u64, String)> = Vec::new();
    let flush = |insns: &mut Vec<(u64, String)>, out: &mut Vec<(u64, u64, Vec<String>)>| {
        let Some(&(start, _)) = insns.first() else {
            return;
        };
        let mut steps = Vec::new();
        let mut k = 0;
        let end = |k: usize| insns.get(k + 1).map_or(0, |n| n.0 - start);
        while let Some((_, text)) = insns.get(k) {
            let (op, args) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
            let args: Vec<&str> = args.trim().split(',').collect();
            match (op, args.as_slice()) {
                ("push", [r]) => steps.push(format!("pc+0x{:02x}: push {}", end(k), &r[1..])),
                ("mov", ["%rsp", "%rbp"]) => {}
                ("sub", [n, "%rsp"]) if n.starts_with('$') => {
                    steps.push(format!("pc+0x{:02x}: alloc {n}", end(k)));
                }
                ("mov", [n, "%eax"])
                    if insns.get(k + 2).is_some_and(|x| x.1.contains("%rax,%rsp")) =>
                {
                    k += 2;
                    steps.push(format!("pc+0x{:02x}: alloc {n}", end(k)));
                }
                _ => break,
            }
            k += 1;
        }
        let size = insns.get(k).map_or(0, |x| x.0 - start);
        steps.reverse();
        out.push((start, size, steps));
        insns.clear();
    };
    for line in dis.lines() {
        if line.ends_with(">:") {
            flush(&mut insns, &mut out);
        } else if let Some((a, t)) = line.trim_start().split_once(":\t") {
            if let Ok(a) = u64::from_str_radix(a, 16) {
                insns.push((a, t.to_owned()));
            }
        }
    }
    flush(&mut insns, &mut out);
    out
}

/// Each UNWIND_INFO MinGW-w64's `objdump -x` reads back: the function's start, the prologue's
/// size, and its codes as `prologues` writes them.
fn unwind_infos(dump: &str) -> Vec<(u64, u64, Vec<String>)> {
    let mut out: Vec<(u64, u64, Vec<String>)> = Vec::new();
    let mut on = false;
    for line in dump.lines() {
        if line.starts_with("Dump of .xdata") {
            on = true;
        } else if on && line.contains("(rva: ") {
            let begin = line.split("): ").nth(1).and_then(|r| r.split(" - ").next());
            let begin = begin.and_then(|b| u64::from_str_radix(b.trim(), 16).ok());
            out.push((begin.unwrap_or(u64::MAX), 0, Vec::new()));
        } else if let (true, Some(e)) = (on, out.last_mut()) {
            let l = line.trim();
            if let Some(s) = l.split("Prologue size: 0x").nth(1) {
                e.1 = u64::from_str_radix(s.split(',').next().unwrap_or(""), 16).unwrap_or(0);
            } else if l.starts_with("pc+") {
                let (at, what) = l.split_once(": ").unwrap_or((l, ""));
                if let Some(n) = what
                    .strip_prefix("alloc ")
                    .and_then(|w| w.split("rsp - ").nth(1))
                {
                    e.2.push(format!("{at}: alloc ${n}"));
                } else {
                    e.2.push(format!("{at}: {what}"));
                }
            } else if l.is_empty() || l.starts_with("Sections") {
                on = !l.starts_with("Sections");
            }
        }
    }
    out
}

/// Windows x64's convention and unwind tables in the objects the generator writes for
/// `x86_64-pc-windows-gnu` (its section 8), read from MinGW-w64's `objdump`: a callee takes its
/// first four arguments from rcx, xmm1 (a float in the second position), r8 and r9 and the fifth
/// from above the caller's 32 bytes of shadow space; no xmm6–xmm15 is touched (callee-saved
/// there); a function using rsi or rdi pushes it; a frame of a page or more is probed by
/// `___chkstk_ms`; and every function's UNWIND_INFO says exactly what its prologue does.
#[cfg_attr(not(target_os = "macos"), ignore = "macOS only before 1.0")]
#[test]
fn the_windows_objects_keep_the_win64_convention_and_describe_every_frame() {
    let objdump = Command::new("x86_64-w64-mingw32-objdump")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if common::skip_without(objdump, "no MinGW-w64 objdump on this machine") {
        return;
    }
    let nzcg = differential::nzcg();
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("conv.nz"),
        "fn five(a: Int, b: Float64, c: Int, d: Int, e: Int) -> Int {\n    \
         let x = b + 1.5\n    if x > 2.0 { a + c + d + e } else { a - c }\n}\n\n\
         fn big_frame(n: Int) -> Int {\n    let a: [Int; 2000] = [0; 2000]\n    \
         let mut b = a\n    let mut i = 0\n    while i < 2000 {\n        b[i] = i * n\n        \
         i += 1\n    }\n    b[1999] + b[3] + a[5]\n}\n\n\
         fn main() -> Int {\n    five(1, 2.5, 3, 4, 5) + big_frame(7) - 20\n}\n",
    )
    .unwrap();
    let r = nazm_with(
        d.path(),
        nzcg,
        &[
            "build",
            "--no-cache",
            "--backend",
            "nazm",
            "--target",
            "x86_64-pc-windows-gnu",
            "--objects",
            "o",
            "conv.nz",
        ],
    );
    assert_eq!(r.code, Some(0), "{r:?}");
    let obj = d.path().join("o/00-m0.o");
    let dump = |flag: &str| {
        let r = run_bounded(
            Command::new("x86_64-w64-mingw32-objdump")
                .args([flag, "--no-show-raw-insn"])
                .arg(&obj),
            "objdump",
        );
        assert_eq!(r.code, Some(0), "{r:?}");
        r.stdout
    };
    let dis = dump("-dr");
    let five: String = dis
        .split("<nz.m.conv_2Enz.five>:")
        .nth(1)
        .unwrap()
        .lines()
        .take(18)
        .collect::<Vec<_>>()
        .join("\n");
    for want in [
        "mov    %rcx,",
        "movq   %xmm1,%rax",
        "mov    %r8,",
        "mov    %r9,",
        "mov    0x30(%rbp),%rax",
    ] {
        assert!(
            five.contains(want),
            "`{want}` not in five's prologue:\n{five}"
        );
    }
    for x in 6..16 {
        assert!(
            !dis.contains(&format!("%xmm{x}")),
            "xmm{x} is touched:\n{dis}"
        );
    }
    for f in dis.split(">:\n").skip(1) {
        let body = f.split("\n\n").next().unwrap_or(f);
        for r in ["%rsi", "%rdi"] {
            if body.contains(r) {
                let prologue: String = body.lines().take(10).collect::<Vec<_>>().join("\n");
                assert!(
                    prologue.contains(&format!("push   {r}")),
                    "{r} used, not saved:\n{body}"
                );
            }
        }
    }
    // `big_frame`'s array lives in a frame of its own (`nz.l.frame.N`), probed page by page.
    assert!(
        dis.contains("IMAGE_REL_AMD64_REL32\t___chkstk_ms"),
        "no frame is probed:\n{dis}"
    );
    // Each RUNTIME_FUNCTION: its start and end in `.text`, its UNWIND_INFO in `.xdata`.
    let rel = dump("-r");
    let pdata: Vec<&str> = rel
        .split("RELOCATION RECORDS FOR [.pdata]:")
        .nth(1)
        .unwrap_or("")
        .lines()
        .filter(|l| l.contains("IMAGE_REL_AMD64_ADDR32NB"))
        .collect();
    assert!(pdata.len() >= 12, "{rel}");
    for l in &pdata {
        let at = u64::from_str_radix(l.split_whitespace().next().unwrap(), 16).unwrap();
        let to = if at % 12 == 8 { ".xdata" } else { ".text$nM" };
        assert!(l.trim_end().ends_with(to), "{l}");
    }
    let code = prologues(&dump("-d"));
    let infos = unwind_infos(&dump("-x"));
    assert!(code.len() >= 4, "{code:?}");
    assert_eq!(code.len(), infos.len(), "{code:?}\n{infos:?}");
    for p in &code {
        assert!(infos.contains(p), "{p:?} not among {infos:#?}");
    }
}

/// A little-endian `u32` or `u64` of a Mach-O object at `at`.
fn le(b: &[u8], at: usize, n: usize) -> u64 {
    b[at..at + n]
        .iter()
        .rev()
        .fold(0, |v, x| (v << 8) | u64::from(*x))
}

/// [`le`] as an index into the object.
fn us(b: &[u8], at: usize, n: usize) -> usize {
    usize::try_from(le(b, at, n)).unwrap()
}

/// What a Mach-O object of the generator's says about its LSDAs: the header's flags, every
/// address a compact unwind entry names as its LSDA, and the addresses of the symbols in
/// `__TEXT,__gcc_except_tab`.
fn lsdas(o: &[u8]) -> (u64, Vec<u64>, Vec<u64>) {
    let flags = le(o, 24, 4);
    let (mut at, mut sect_no) = (32, 0u64);
    let (mut lsda_sect, mut entries, mut symbols) = (0, Vec::new(), Vec::new());
    for _ in 0..le(o, 16, 4) {
        let (cmd, size) = (le(o, at, 4), us(o, at + 4, 4));
        if cmd == 0x19 {
            for s in 0..us(o, at + 64, 4) {
                let h = at + 72 + 80 * s;
                sect_no += 1;
                let name = String::from_utf8_lossy(&o[h..h + 16])
                    .trim_end_matches('\0')
                    .to_owned();
                let (len, off) = (us(o, h + 40, 8), us(o, h + 48, 4));
                if name == "__gcc_except_tab" {
                    lsda_sect = sect_no;
                } else if name == "__compact_unwind" {
                    for e in (off..off + len).step_by(32) {
                        if le(o, e + 12, 4) & 0x4000_0000 != 0 {
                            entries.push(le(o, e + 24, 8));
                        }
                    }
                }
            }
        } else if cmd == 0x2 {
            let (symoff, nsyms) = (us(o, at + 8, 4), us(o, at + 12, 4));
            for k in 0..nsyms {
                let n = symoff + 16 * k;
                if o[n + 4] & 0x0E == 0x0E && u64::from(o[n + 5]) == lsda_sect {
                    symbols.push(le(o, n + 8, 8));
                }
            }
        }
        at += size;
    }
    (flags, entries, symbols)
}

/// Each LSDA the generator writes for Mach-O begins an atom of its own (2026-10-16): the object
/// says `MH_SUBSECTIONS_VIA_SYMBOLS`, as LLVM's do, and a symbol stands at every LSDA a compact
/// unwind entry names. Without them `__gcc_except_tab` was one atom, and the linker on GitHub's
/// macOS 26 runner tied every function to that atom's start, the first function's LSDA: no
/// landing pad was found below it and every failure aborted. The linker here resolves the
/// address inside the atom, so only the object can show it.
#[cfg_attr(not(target_os = "macos"), ignore = "macOS only before 1.0")]
#[test]
fn every_lsda_of_a_mach_o_object_is_an_atom_of_its_own() {
    let nzcg = differential::nzcg();
    let src = repo().join("toolchain/codegen/tests/programs/fail_deep.nz");
    for target in ["aarch64-apple-darwin", "x86_64-apple-darwin"] {
        let d = tempfile::tempdir().unwrap();
        std::fs::copy(&src, d.path().join("prog.nz")).unwrap();
        let r = nazm_with(
            d.path(),
            nzcg,
            &[
                "build",
                "--no-cache",
                "--backend",
                "nazm",
                "--target",
                target,
                "--objects",
                "o",
                "prog.nz",
            ],
        );
        assert_eq!(r.code, Some(0), "{target}: {r:?}");
        let (flags, entries, symbols) = lsdas(&std::fs::read(d.path().join("o/00-m0.o")).unwrap());
        assert_eq!(
            flags & 0x2000,
            0x2000,
            "{target}: no MH_SUBSECTIONS_VIA_SYMBOLS"
        );
        assert!(entries.len() >= 2, "{target}: {entries:?}");
        for e in &entries {
            assert!(
                symbols.contains(e),
                "{target}: no symbol at the LSDA {e:#x}: {symbols:x?}"
            );
        }
    }
}

/// A stand-in generator: a shell script that records its arguments and its input, then runs the
/// real one, or does what `body` says.
fn fake(dir: &Path, body: &str) -> PathBuf {
    let p = dir.join("nzcg-fake");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    p
}

/// A fake that records its arguments (`args.txt`) and its LIR (`unit.lir`, the last unit's),
/// then runs the real generator.
fn recording(dir: &Path) -> PathBuf {
    let real = differential::nzcg();
    let log = dir.join("args.txt");
    let lir = dir.join("unit.lir");
    fake(
        dir,
        &format!(
            "echo \"$@\" >> '{}'\nfor a; do last2=$last1; last1=$a; done\ncp \"$last2\" '{}'\nexec '{}' \"$@\"",
            log.display(),
            lir.display(),
            real.display()
        ),
    )
}

#[cfg_attr(not(target_os = "macos"), ignore = "macOS only before 1.0")]
#[test]
fn the_generator_is_told_the_target_and_whether_the_program_has_one_thread() {
    let d = tempfile::tempdir().unwrap();
    let one = "fn main(out: OutCap) -> Int ! {io} {\n    println(7)\n    0\n}\n";
    let many = "fn work(n: Int) -> Int { n * 2 }\n\
                fn main(tasks: SpawnCap, out: OutCap) -> Int {\n    \
                scope { spawn work(3); }\n    println(7)\n    0\n}\n";
    for (name, src, threads) in [("one.nz", one, false), ("many.nz", many, true)] {
        std::fs::write(d.path().join(name), src).unwrap();
        let _ = std::fs::remove_file(d.path().join("args.txt"));
        let nz = recording(d.path());
        let host = nazm_lir::backend::Target::host().unwrap();
        let r = nazm_with(
            d.path(),
            &nz,
            &["build", "--no-cache", "--backend", "nazm", name, "-o", "p"],
        );
        assert_eq!(r.code, Some(0), "{name}: {}", r.stderr);
        let args = std::fs::read_to_string(d.path().join("args.txt")).unwrap();
        assert!(
            args.lines()
                .all(|l| l.starts_with(&format!("--target {} ", host.triple))),
            "{name}: {args}"
        );
        assert_eq!(
            args.contains("--one-thread"),
            !threads,
            "{name}: one thread is {}: {args}",
            !threads
        );
        let p = run_bounded(
            &mut common::bounded_exe(&d.path().join(crate::common::exe_name("p"))),
            "p",
        );
        // `main`'s result is printed after what it printed.
        assert_eq!((p.code, p.stdout.as_str()), (Some(0), "7\n0\n"), "{name}");
    }
}

#[cfg_attr(not(target_os = "macos"), ignore = "macOS only before 1.0")]
#[test]
fn a_debug_build_hands_the_generator_its_positions_and_their_files() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn add(a: Int, b: Int) -> Int { a + b }\n\nfn main() -> Int { add(1, 2) }\n",
    )
    .unwrap();
    let nz = recording(d.path());
    for debug in [false, true] {
        let mut args = vec!["build", "--no-cache", "--backend", "nazm"];
        if debug {
            args.push("--debug");
        }
        args.extend(["prog.nz", "-o", "p"]);
        let r = nazm_with(d.path(), &nz, &args);
        assert_eq!(r.code, Some(0), "{}", r.stderr);
        let lir = std::fs::read_to_string(d.path().join("unit.lir")).unwrap();
        assert_eq!(lir.contains("\n; file "), debug, "{lir}");
        assert_eq!(lir.contains("  loc "), debug, "{lir}");
        let p = run_bounded(
            &mut common::bounded_exe(&d.path().join(crate::common::exe_name("p"))),
            "p",
        );
        assert_eq!((p.code, p.stdout.as_str()), (Some(0), "3\n"));
    }
}

#[cfg_attr(not(target_os = "macos"), ignore = "macOS only before 1.0")]
#[test]
fn a_refused_construct_is_n0101_by_name_and_nothing_is_written_or_fallen_back_to() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), "fn main() -> Int { 0 }\n").unwrap();
    let nz = fake(
        d.path(),
        "echo 'nzcg: refused: the instruction `frob i64`, in `main` of prog.nz' >&2\n\
         echo 'nzcg: refused: the terminator `jump.far`, in `main` of prog.nz' >&2\nexit 3",
    );
    let r = nazm_with(
        d.path(),
        &nz,
        &[
            "build",
            "--no-cache",
            "--backend",
            "nazm",
            "prog.nz",
            "-o",
            "p",
        ],
    );
    assert_eq!(r.code, Some(1), "{r:?}");
    assert_eq!(r.stderr.matches("[N0101]").count(), 2, "{}", r.stderr);
    assert!(
        r.stderr.contains("the instruction `frob i64`"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr.contains("the terminator `jump.far`"),
        "{}",
        r.stderr
    );
    assert!(!r.stderr.contains("N0900"), "{}", r.stderr);
    assert!(
        !d.path().join(crate::common::exe_name("p")).exists(),
        "no fallback: nothing is built"
    );
}

#[cfg_attr(not(target_os = "macos"), ignore = "macOS only before 1.0")]
#[test]
fn any_other_failure_of_the_generator_is_a_compiler_defect_with_its_words() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), "fn main() -> Int { 0 }\n").unwrap();
    let nz = fake(d.path(), "echo 'index out of bounds in sel.nz' >&2\nexit 2");
    let r = nazm_with(
        d.path(),
        &nz,
        &[
            "build",
            "--no-cache",
            "--backend",
            "nazm",
            "prog.nz",
            "-o",
            "p",
        ],
    );
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(r.stderr.contains("N0900"), "{}", r.stderr);
    assert!(
        r.stderr.contains("index out of bounds in sel.nz"),
        "{}",
        r.stderr
    );
    assert!(!r.stderr.contains("N0101"), "{}", r.stderr);
    assert!(!d.path().join(crate::common::exe_name("p")).exists());
}

#[test]
fn a_missing_generator_or_a_target_it_does_not_carry_is_refused_before_compiling() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), "fn main() -> Int { 0 }\n").unwrap();
    let missing = d.path().join("no-such-nzcg");
    if cfg!(target_os = "macos") {
        let r = nazm_with(
            d.path(),
            &missing,
            &[
                "build",
                "--no-cache",
                "--backend",
                "nazm",
                "prog.nz",
                "-o",
                "p",
            ],
        );
        assert_eq!(r.code, Some(4), "{r:?}");
        assert!(r.stderr.contains("no-such-nzcg"), "{}", r.stderr);
    }
    let r = nazm_with(
        d.path(),
        &missing,
        &[
            "build",
            "--no-cache",
            "--backend",
            "nazm",
            "--target",
            "aarch64-unknown-linux-gnu",
            "--objects",
            "o",
            "prog.nz",
        ],
    );
    assert_eq!(r.code, Some(5), "{r:?}");
    assert!(
        r.stderr
            .contains("`--backend nazm` builds for aarch64-apple-darwin"),
        "{}",
        r.stderr
    );
    assert!(!d.path().join("o").exists());
}

/// R1.5 7.2: `scripts/host-archive.sh` writes the same bytes for the same files, whenever it runs —
/// entries sorted, every time `SOURCE_DATE_EPOCH`, owner 0, a gzip header with no name or time —
/// so a release archive built twice compares equal (`scripts/reproducible-check.sh`, in CI).
/// Packaged under a triple with no generator, so it runs on every host.
#[test]
fn the_host_archive_is_the_same_bytes_when_packaged_twice() {
    // The contained image has no python3, which the archive's writer needs. Said, not hidden:
    // the test is skipped there and runs on every host and in CI.
    if !Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("SKIPPED: python3 is not on PATH, so host-archive.sh cannot write the archive");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let mut bytes = Vec::new();
    for n in 0..2 {
        if n == 1 {
            // Past the second the first run's files were staged in.
            std::thread::sleep(std::time::Duration::from_millis(1100));
        }
        let out = d.path().join(format!("archives-{n}"));
        let r = run_bounded(
            Command::new("sh")
                .arg(repo().join("scripts/host-archive.sh"))
                .args(["x86_64-unknown-linux-gnu", NAZM])
                .arg(&out)
                .env("SOURCE_DATE_EPOCH", "1760000000"),
            "host-archive.sh",
        );
        assert_eq!(r.code, Some(0), "{r:?}");
        bytes.push(std::fs::read(out.join("nazm-x86_64-unknown-linux-gnu.tar.gz")).unwrap());
    }
    assert!(bytes[0] == bytes[1], "the two archives differ");
}

/// The packaged layout (task 7.8): `scripts/host-archive.sh` builds `nzcg` with the `nazm` it
/// packages and installs it as `libexec/nazm/nzcg`, where the unpacked `bin/nazm` finds it with
/// `NAZM_NZCG` unset — and nowhere else: moved away, the build is refused naming that path. The
/// archive passes `scripts/clean-install-test.sh`, which builds with `--backend nazm` too.
#[cfg_attr(not(target_os = "macos"), ignore = "macOS only before 1.0")]
#[test]
fn the_packaged_toolchain_finds_its_generator_beside_nazm_without_the_variable() {
    let triple = if cfg!(target_arch = "aarch64") {
        "aarch64-apple-darwin"
    } else {
        "x86_64-apple-darwin"
    };
    let d = tempfile::tempdir().unwrap();
    let out = d.path().join("archives");
    let r = run_bounded(
        Command::new("sh")
            .arg(repo().join("scripts/host-archive.sh"))
            .args([triple, NAZM])
            .arg(&out),
        "host-archive.sh",
    );
    assert_eq!(r.code, Some(0), "{r:?}");
    let archive = out.join(format!("nazm-{triple}.tar.gz"));

    let r = run_bounded(
        Command::new("sh")
            .arg(repo().join("scripts/clean-install-test.sh"))
            .arg(&archive)
            .env_remove(nazm_codegen_nazm::VARIABLE)
            .env_remove(nazm_lir::RESULT_VARIABLE),
        "clean-install-test.sh",
    );
    assert_eq!(r.code, Some(0), "{r:?}");
    assert!(r.stdout.contains("clean-install: ok"), "{}", r.stdout);

    let unpacked = d.path().join("unpacked");
    std::fs::create_dir_all(&unpacked).unwrap();
    let r = run_bounded(
        Command::new("tar")
            .arg("-xzf")
            .arg(&archive)
            .arg("-C")
            .arg(&unpacked),
        "tar",
    );
    assert_eq!(r.code, Some(0), "{r:?}");
    let top = unpacked.join(format!("nazm-{triple}"));
    let nzcg = top.join("libexec/nazm/nzcg");
    assert!(nzcg.is_file(), "the archive carries no {}", nzcg.display());
    let install = std::fs::read_to_string(top.join("INSTALL.txt")).unwrap();
    assert!(install.contains("libexec/nazm/nzcg"), "{install}");

    let work = d.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(
        work.join("prog.nz"),
        "fn main() {\n    println(\"from nzcg\")\n}\n",
    )
    .unwrap();
    let build = |exe: &Path| {
        run_bounded(
            Command::new(top.join("bin/nazm"))
                .current_dir(&work)
                .env_remove(nazm_codegen_nazm::VARIABLE)
                .args(["build", "--no-cache", "--backend", "nazm", "prog.nz", "-o"])
                .arg(exe),
            "nazm build --backend nazm",
        )
    };
    let r = build(&work.join(crate::common::exe_name("p")));
    assert_eq!(r.code, Some(0), "{r:?}");
    let r = run_bounded(
        Command::new(work.join(crate::common::exe_name("p"))).env_remove(nazm_lir::RESULT_VARIABLE),
        "the program",
    );
    assert_eq!(
        (r.code, r.stdout.as_str()),
        (Some(0), "from nzcg\n"),
        "{r:?}"
    );

    // Moved away, it is not found anywhere else: the refusal names the installed path.
    std::fs::rename(&nzcg, top.join("nzcg-moved")).unwrap();
    let r = build(&work.join(crate::common::exe_name("q")));
    assert_eq!(r.code, Some(4), "{r:?}");
    assert!(r.stderr.contains("libexec/nazm/nzcg"), "{}", r.stderr);
    assert!(!work.join(crate::common::exe_name("q")).exists());
}
