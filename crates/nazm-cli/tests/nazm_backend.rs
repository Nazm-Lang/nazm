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

fn one(src: &Path, nzcg: &Path, work: &Path) -> Outcome {
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
        let r = run_bounded(Command::new("cc").arg("-c").arg(&c).arg("-o").arg(&o), "cc");
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
                let o = one(src, nzcg, work.path());
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
        let p = run_bounded(&mut common::bounded_exe(&d.path().join("p")), "p");
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
        let p = run_bounded(&mut common::bounded_exe(&d.path().join("p")), "p");
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
        !d.path().join("p").exists(),
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
    assert!(!d.path().join("p").exists());
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
    let r = build(&work.join("p"));
    assert_eq!(r.code, Some(0), "{r:?}");
    let r = run_bounded(
        Command::new(work.join("p")).env_remove(nazm_lir::RESULT_VARIABLE),
        "the program",
    );
    assert_eq!(
        (r.code, r.stdout.as_str()),
        (Some(0), "from nzcg\n"),
        "{r:?}"
    );

    // Moved away, it is not found anywhere else: the refusal names the installed path.
    std::fs::rename(&nzcg, top.join("nzcg-moved")).unwrap();
    let r = build(&work.join("q"));
    assert_eq!(r.code, Some(4), "{r:?}");
    assert!(r.stderr.contains("libexec/nazm/nzcg"), "{}", r.stderr);
    assert!(!work.join("q").exists());
}
