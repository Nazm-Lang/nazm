//! N73: semantic differential fuzzing — generated programs, one answer from every tier.
//!
//! `docs/architecture.md` §7.75. A seeded generator writes well-typed programs that terminate by
//! construction: `Int` arithmetic of every operator (overflow and division by zero included, from
//! edge literals), comparisons in conditions, `let`, `if`, bounded `while`, calls between generated
//! functions (only to earlier ones, so no recursion), and a record. Each is run by the interpreter
//! and built by LLVM at `-O0` and `-O2` and by Cranelift; every tier must print the same value, or
//! fail with the same code at the same place. A disagreement is shrunk — lines removed while it still
//! type-checks and still disagrees — and printed for the regression corpus (`tests/fuzz-corpus`),
//! which every run replays first. `NAZM_FUZZ_SEED` and `NAZM_FUZZ_CASES` choose another run.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }

    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[usize::try_from(self.below(u64::try_from(xs.len()).unwrap())).unwrap()]
    }
}

struct Gen {
    rng: Rng,
}

impl Gen {
    fn literal(&mut self) -> String {
        match self.rng.below(10) {
            0 => "9223372036854775807".to_owned(),
            1 => "(0 - 9223372036854775807 - 1)".to_owned(),
            2 => "3037000500".to_owned(),
            3 => "-1".to_owned(),
            4 => "0".to_owned(),
            _ => format!("{}", i64::try_from(self.rng.below(41)).unwrap() - 20),
        }
    }

    fn int(&mut self, vars: &[String], depth: u32, callable: usize) -> String {
        if depth == 0 || self.rng.below(3) == 0 {
            return if !vars.is_empty() && self.rng.below(3) != 0 {
                self.rng.pick(vars).clone()
            } else {
                self.literal()
            };
        }
        match self.rng.below(8) {
            0..=3 => {
                let op = *self.rng.pick(&["+", "-", "*", "/", "%", "+", "-"]);
                format!(
                    "({} {op} {})",
                    self.int(vars, depth - 1, callable),
                    self.int(vars, depth - 1, callable)
                )
            }
            4 => format!("(0 - {})", self.int(vars, depth - 1, callable)),
            5 => format!(
                "(if {} {{ {} }} else {{ {} }})",
                self.cond(vars, depth - 1, callable),
                self.int(vars, depth - 1, callable),
                self.int(vars, depth - 1, callable)
            ),
            6 if callable > 0 => {
                let f = self.rng.below(u64::try_from(callable).unwrap());
                format!(
                    "f{f}({}, {})",
                    self.int(vars, depth - 1, callable),
                    self.int(vars, depth - 1, callable)
                )
            }
            _ => format!(
                "P(a: {}, b: {}).a",
                self.int(vars, depth - 1, callable),
                self.int(vars, depth - 1, callable)
            ),
        }
    }

    fn cond(&mut self, vars: &[String], depth: u32, callable: usize) -> String {
        let op = *self.rng.pick(&["<", "<=", ">", ">=", "==", "!="]);
        let c = format!(
            "{} {op} {}",
            self.int(vars, depth, callable),
            self.int(vars, depth, callable)
        );
        match self.rng.below(5) {
            0 => format!("({c} && {})", self.cond(vars, 0, callable)),
            1 => format!("({c} || {})", self.cond(vars, 0, callable)),
            _ => c,
        }
    }

    /// One function body: a statement per line, so a line can be removed when shrinking.
    fn body(&mut self, params: &[String], callable: usize) -> String {
        let mut vars: Vec<String> = params.to_vec();
        let mut out = String::new();
        for k in 0..1 + self.rng.below(5) {
            let v = format!("v{k}");
            match self.rng.below(4) {
                0 => {
                    let _ = writeln!(out, "    let mut {v} = {};", self.int(&vars, 2, callable));
                    let bound = 1 + self.rng.below(6);
                    let _ = writeln!(
                        out,
                        "    let mut i{k} = 0; while i{k} < {bound} {{ {v} = {v} + {}; i{k} = i{k} + 1; }}",
                        self.int(&vars, 1, callable)
                    );
                }
                1 => {
                    let _ = writeln!(out, "    let mut {v} = {};", self.int(&vars, 2, callable));
                    let _ = writeln!(
                        out,
                        "    if {} {{ {v} = {}; }} else {{ {v} = {}; }}",
                        self.cond(&vars, 1, callable),
                        self.int(&vars, 1, callable),
                        self.int(&vars, 1, callable)
                    );
                }
                _ => {
                    let _ = writeln!(out, "    let {v} = {};", self.int(&vars, 3, callable));
                }
            }
            vars.push(v);
        }
        // Bound first: a tail beginning with `-` or `(` after an `if … else …` statement would
        // continue that statement as a binary expression (`(if …) - 17`), which the language
        // parses so and the checker refuses.
        let _ = writeln!(out, "    let result = {};", self.int(&vars, 2, callable));
        let _ = writeln!(out, "    result");
        out
    }

    fn program(&mut self) -> String {
        let mut out = String::from("struct P { a: Int, b: Int }\n");
        let n = 1 + usize::try_from(self.rng.below(3)).unwrap();
        for f in 0..n {
            let _ = writeln!(out, "fn f{f}(x: Int, y: Int) -> Int {{");
            out.push_str(&self.body(&["x".to_owned(), "y".to_owned()], f));
            out.push_str("}\n");
        }
        out.push_str("fn main() -> Int {\n");
        out.push_str(&self.body(&[], n));
        out.push_str("}\n");
        out
    }
}

/// A tier's name and its `(stdout, exit status, the failure line without the file name)`.
type Tiers = Vec<(String, (String, Option<i32>, String))>;

/// What each tier did with one program.
fn tiers(dir: &Path, src: &str) -> Tiers {
    std::fs::write(dir.join("prog.nz"), src).unwrap();
    let failure = |stderr: &[u8]| {
        String::from_utf8_lossy(stderr)
            .lines()
            .find(|l| l.contains("error[N04"))
            .map(|l| l.trim_start_matches("nazm: ").to_owned())
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    let run = Command::new(NAZM)
        .current_dir(dir)
        .args(["run", "prog.nz"])
        .output()
        .unwrap();
    // The interpreter names the place on the next line; the native tiers on the same one.
    let place = String::from_utf8_lossy(&run.stderr)
        .lines()
        .find(|l| l.trim_start().starts_with("--> "))
        .map(|l| l.trim_start().trim_start_matches("--> ").to_owned())
        .unwrap_or_default();
    let line = failure(&run.stderr);
    let interpreted = if line.is_empty() {
        String::new()
    } else {
        format!("{line} at {place}")
    };
    out.push((
        "run".to_owned(),
        (
            String::from_utf8_lossy(&run.stdout).into_owned(),
            run.status.code(),
            interpreted,
        ),
    ));
    for (backend, opt) in [("llvm", "0"), ("llvm", "2"), ("cranelift", "2")] {
        let exe = dir.join(format!("p-{backend}-{opt}"));
        let b = Command::new(NAZM)
            .current_dir(dir)
            .args([
                "build",
                "--backend",
                backend,
                "--opt-level",
                opt,
                "prog.nz",
                "-o",
            ])
            .arg(&exe)
            .output()
            .unwrap();
        if !b.status.success() {
            out.push((
                format!("{backend} -O{opt}"),
                (
                    "build failed".to_owned(),
                    b.status.code(),
                    failure(&b.stderr),
                ),
            ));
            continue;
        }
        let o = Command::new(&exe).output().unwrap();
        out.push((
            format!("{backend} -O{opt}"),
            (
                String::from_utf8_lossy(&o.stdout).into_owned(),
                o.status.code(),
                failure(&o.stderr),
            ),
        ));
    }
    out
}

fn agrees(results: &Tiers) -> bool {
    let first = &results[0].1;
    // A program the interpreter refuses to check is not a case.
    results.iter().all(|(_, r)| r == first)
}

fn checks(dir: &Path, src: &str) -> bool {
    std::fs::write(dir.join("prog.nz"), src).unwrap();
    Command::new(NAZM)
        .current_dir(dir)
        .args(["check", "prog.nz"])
        .output()
        .unwrap()
        .status
        .success()
}

/// Remove lines while the program still checks and the tiers still disagree.
fn shrink(dir: &Path, src: &str) -> String {
    let mut lines: Vec<String> = src.lines().map(str::to_owned).collect();
    let mut i = 0;
    while i < lines.len() {
        let mut candidate = lines.clone();
        candidate.remove(i);
        let text = candidate.join("\n") + "\n";
        if checks(dir, &text) && !agrees(&tiers(dir, &text)) {
            lines = candidate;
        } else {
            i += 1;
        }
    }
    lines.join("\n") + "\n"
}

fn check_one(dir: &Path, src: &str, label: &str) {
    if !checks(dir, src) {
        let why = Command::new(NAZM)
            .current_dir(dir)
            .args(["check", "prog.nz"])
            .output()
            .unwrap();
        panic!(
            "{label}: the generator wrote a program that does not check:\n{src}\n{}",
            String::from_utf8_lossy(&why.stderr)
        );
    }
    let results = tiers(dir, src);
    if !agrees(&results) {
        let small = shrink(dir, src);
        let shrunk = tiers(dir, &small);
        panic!(
            "{label}: the tiers disagree. Shrunk, for tests/fuzz-corpus:\n---\n{small}---\n{:#?}",
            shrunk
        );
    }
}

#[test]
fn every_tier_gives_a_generated_program_the_same_answer() {
    let d = tempfile::tempdir().unwrap();
    // The regression corpus first.
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fuzz-corpus");
    let mut files: Vec<_> = std::fs::read_dir(&corpus)
        .map(|e| e.flatten().map(|f| f.path()).collect())
        .unwrap_or_default();
    files.sort();
    for f in files
        .iter()
        .filter(|f| f.extension().is_some_and(|x| x == "nz"))
    {
        check_one(
            d.path(),
            &std::fs::read_to_string(f).unwrap(),
            &f.display().to_string(),
        );
    }
    let seed: u64 = std::env::var("NAZM_FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x6e61_7a6d_0073);
    let cases: u64 = std::env::var("NAZM_FUZZ_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(24);
    let mut g = Gen { rng: Rng(seed | 1) };
    let mut failing = 0;
    for case in 0..cases {
        let src = g.program();
        check_one(d.path(), &src, &format!("seed {seed:#x} case {case}"));
        let r = tiers(d.path(), &src);
        if r[0].1 .1 == Some(2) {
            failing += 1;
        }
    }
    // The generator reaches failures as well as values: the edge literals see to it.
    assert!(
        cases < 10 || failing > 0,
        "no generated program failed: the edges are not reached"
    );
}
