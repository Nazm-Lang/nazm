//! The coverage-guided fuzzer (N93, `docs/architecture.md` §7.94).
//!
//! `nazm-fuzz TARGET --seconds N [--seed S] [--corpus DIR]… [--out DIR] [--minimize]`
//!
//! Inputs are kept when they reach an edge of the compiler no earlier input reached — edge
//! coverage from LLVM's SanitizerCoverage, counted by `cov.c` — and mutated from there: byte
//! edits, tokens of the language inserted, whole lines duplicated, deleted, swapped or spliced
//! from another input, names renamed. A panic is a defect: the input is shrunk by lines and
//! written to `OUT/crashes/`. An input that runs past its time limit is written, as it was, to
//! `OUT/hangs/` (v1, Q1-F-01: hangs were counted and nothing kept). `--minimize` writes the smallest set of kept inputs that reaches
//! every edge reached, which is the persistent corpus `fuzz/corpus/TARGET/` is made from.
//!
//! The targets are the compiler's own entry points, called in process:
//! `front` (parse and check), `lower` (check, Core IR, MIR, and MIR's validator — which must
//! accept every program the checker accepts), `run` (the interpreter, with its step budgets), `lir`
//! (N105: the instruction-level LIR both backends translate, which must validate, run on LIR's own
//! interpreter without a defect), and `manifest` (a package manifest and the version requirements
//! in it).

#![allow(unsafe_code)]

use std::collections::BTreeSet;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

unsafe extern "C" {
    fn nz_cov_reset();
    fn nz_cov_fresh() -> usize;
    fn nz_cov_keep();
    fn nz_cov_hit(i: u32) -> i32;
    fn nz_cov_guards() -> usize;
    fn nz_cov_seen() -> usize;
}

// The coverage counters are plain C globals the callbacks write; reading them is the only
// `unsafe` here, and nothing else touches them.
fn reset() {
    unsafe { nz_cov_reset() }
}
fn fresh() -> usize {
    unsafe { nz_cov_fresh() }
}
fn keep() {
    unsafe { nz_cov_keep() }
}
fn hit(i: u32) -> bool {
    unsafe { nz_cov_hit(i) != 0 }
}
fn guards() -> usize {
    unsafe { nz_cov_guards() }
}
fn seen() -> usize {
    unsafe { nz_cov_seen() }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Front,
    Lower,
    Run,
    Manifest,
    /// N105: a checked program's instruction-level LIR validates, and LIR's interpreter — the
    /// oracle both backends are held to — finds no defect running it.
    Lir,
}

impl Target {
    fn by_name(n: &str) -> Option<Self> {
        Some(match n {
            "front" => Self::Front,
            "lower" => Self::Lower,
            "run" => Self::Run,
            "manifest" => Self::Manifest,
            "lir" => Self::Lir,
            _ => return None,
        })
    }
}

/// How one execution ended.
enum Ended {
    Done,
    Panicked(String),
    /// Still running after [`HANG`]: a program may wait forever — Nazm does not promise deadlock
    /// freedom — so this is not a defect; the input is not kept and its thread is abandoned.
    Hung,
}

const HANG: Duration = Duration::from_secs(2);
/// After this many hangs a target stops: each leaves a blocked thread behind.
const MAX_HANGS: usize = 50;

/// One execution, bounded in time for the interpreter, whose programs may block forever.
fn exec_bounded(target: Target, input: &str) -> Ended {
    if target != Target::Run {
        return match exec(target, input) {
            Ok(()) => Ended::Done,
            Err(m) => Ended::Panicked(m),
        };
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let owned = input.to_owned();
    std::thread::spawn(move || {
        let _ = tx.send(exec(target, &owned));
    });
    match rx.recv_timeout(HANG) {
        Ok(Ok(())) => Ended::Done,
        Ok(Err(m)) => Ended::Panicked(m),
        Err(_) => Ended::Hung,
    }
}

/// One execution: `Err` with the panic's message when the compiler panicked.
fn exec(target: Target, input: &str) -> Result<(), String> {
    let run = AssertUnwindSafe(|| match target {
        Target::Front => {
            let _ = nazm_core::check_source(input);
        }
        Target::Lower => {
            let (map, graph) = nazm_core::one_file(input);
            let g = &graph;
            let _ = nazm_core::on_lowered(&map, &graph, nazm_core::Entry::Absent, |core, res| {
                let mir = nazm_mir::lower(core, res, g)?;
                let problems = nazm_mir::verify(&mir);
                assert!(
                    problems.is_empty(),
                    "MIR lowered from a checked program does not verify: {problems:?}"
                );
                Ok(())
            });
        }
        Target::Run => {
            let limits = nazm_core::eval::Limits {
                max_iterations: 20_000,
                max_depth: 64,
            };
            let _ = nazm_core::run_with_limits(input, limits);
        }
        Target::Lir => {
            let (map, graph) = nazm_core::one_file(input);
            let g = &graph;
            let names = nazm_lir::SourceNames::of(&map, &graph);
            let _ = nazm_core::on_lowered(&map, &graph, nazm_core::Entry::Required, |core, res| {
                let mir = nazm_mir::lower(core, res, g)?;
                // Outside the native subset is a refusal, not a defect.
                let Ok(native) = nazm_lir::lower(mir, &names) else {
                    return Ok(());
                };
                let mut modules = Vec::new();
                for (_, u, _) in nazm_lir::backend::units(&native) {
                    let m = nazm_lir::op::lower_unit(
                        &native,
                        u,
                        &names,
                        nazm_lir::abi::DataLayout::LP64,
                        false,
                        false,
                    )?;
                    if let Err(e) = nazm_lir::op::validate::module(&m) {
                        panic!("LIR lowered from a checked program does not validate: {e:?}");
                    }
                    modules.push(m);
                }
                let entry = native.entry.as_str().to_owned();
                if let Err(nazm_lir::op::interp::Stop::Defect(why)) =
                    nazm_lir::op::interp::run(&modules, &entry, native.entry_roots, &[], 200_000)
                {
                    panic!("the LIR interpreter found a defect: {why}");
                }
                Ok(())
            });
        }
        Target::Manifest => {
            let _ = nazm_package::Manifest::parse(input, "nazm.toml");
            for line in input.lines() {
                let _ = nazm_package::VersionReq::parse(line.trim());
            }
        }
    });
    std::panic::catch_unwind(run).map_err(|p| {
        p.downcast_ref::<String>()
            .cloned()
            .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_else(|| "a panic with no message".to_owned())
    })
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*: seeded, the same on every host.
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

/// Tokens of the language, and of a manifest, inserted whole: structure a byte flip rarely makes.
const TOKENS: &[&str] = &[
    "fn ",
    "let ",
    "let mut ",
    "if ",
    "else ",
    "while ",
    "match ",
    "return ",
    "break;",
    "continue;",
    "struct ",
    "enum ",
    "trait ",
    "impl ",
    "pub ",
    "use \"",
    "scope { ",
    "spawn ",
    "requires ",
    "ensures ",
    "result",
    "! {}",
    "! { io }",
    "-> Int",
    "-> Bool",
    "-> Str",
    ": Int",
    "(",
    ")",
    "{",
    "}",
    "[",
    "]",
    "<",
    ">",
    ",",
    ";",
    ":",
    "::",
    "=>",
    "->",
    "=",
    "==",
    "!=",
    "<=",
    ">=",
    "&&",
    "||",
    "!",
    "+",
    "-",
    "*",
    "/",
    "%",
    "?",
    "\"",
    "0",
    "1",
    "-1",
    "9223372036854775807",
    "true",
    "false",
    "Int",
    "Bool",
    "Str",
    "Vec[Int]",
    "Option[Int]",
    "Result[Int, Str]",
    "Some(",
    "None",
    "Ok(",
    "Err(",
    "|x| ",
    "ints_new()",
    "str_len(",
    "chan_new_of[Int](1)",
    "main",
    "\n",
    "  ",
    "[package]\n",
    "name = ",
    "version = \"1.0.0\"\n",
    "[dependencies]\n",
    "^1.2.3",
    "~0.1.0",
    "=2.0.0",
    "{ path = \"../x\" }",
    "schema = \"nazm.package/1\"\n",
];

fn mutate(rng: &mut Rng, input: &str, others: &[String]) -> String {
    let mut lines: Vec<String> = input.lines().map(str::to_owned).collect();
    let mut bytes: Vec<u8> = input.as_bytes().to_vec();
    let mut by_line = false;
    for _ in 0..=rng.below(4) {
        match rng.below(9) {
            // Byte edits.
            0 if !bytes.is_empty() => {
                let i = rng.below(bytes.len());
                bytes[i] = (rng.next() & 0x7f) as u8;
            }
            1 if !bytes.is_empty() => {
                let i = rng.below(bytes.len());
                let n = 1 + rng.below(8.min(bytes.len() - i));
                bytes.drain(i..i + n);
            }
            2 | 3 => {
                let i = rng.below(bytes.len() + 1);
                let t = TOKENS[rng.below(TOKENS.len())];
                bytes.splice(i..i, t.bytes());
            }
            // Line edits: whole statements and declarations move.
            4 if !lines.is_empty() => {
                by_line = true;
                let i = rng.below(lines.len());
                let l = lines[i].clone();
                lines.insert(rng.below(lines.len() + 1), l);
            }
            5 if !lines.is_empty() => {
                by_line = true;
                lines.remove(rng.below(lines.len()));
            }
            6 if lines.len() > 1 => {
                by_line = true;
                let (a, b) = (rng.below(lines.len()), rng.below(lines.len()));
                lines.swap(a, b);
            }
            7 if !others.is_empty() => {
                by_line = true;
                let other = &others[rng.below(others.len())];
                let theirs: Vec<&str> = other.lines().collect();
                if !theirs.is_empty() {
                    let from = rng.below(theirs.len());
                    let n = 1 + rng.below(6.min(theirs.len() - from));
                    let at = rng.below(lines.len() + 1);
                    for (k, l) in theirs[from..from + n].iter().enumerate() {
                        lines.insert(at + k, (*l).to_owned());
                    }
                }
            }
            // A name renamed everywhere: binding and use change together.
            8 => {
                by_line = true;
                let text = lines.join("\n");
                let words: Vec<&str> = text
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .filter(|w| w.len() > 1 && w.starts_with(|c: char| c.is_ascii_lowercase()))
                    .collect();
                if !words.is_empty() {
                    let w = words[rng.below(words.len())].to_owned();
                    let to = ["x", "main", "result", "Int", "fn", "value"][rng.below(6)];
                    lines = text.replace(&w, to).lines().map(str::to_owned).collect();
                }
            }
            _ => {}
        }
        if by_line {
            bytes = lines.join("\n").into_bytes();
            by_line = false;
        } else {
            lines = String::from_utf8_lossy(&bytes)
                .lines()
                .map(str::to_owned)
                .collect();
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The smallest prefix-free set of lines that still panics, one line at a time.
fn shrink(target: Target, input: &str) -> String {
    let mut lines: Vec<&str> = input.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let mut fewer = lines.clone();
        fewer.remove(i);
        if matches!(exec_bounded(target, &fewer.join("\n")), Ended::Panicked(_)) {
            lines = fewer;
        } else {
            i += 1;
        }
    }
    lines.join("\n")
}

/// Keep an input that hung (v1, Q1-F-01): `OUT/hangs/NAME.txt`, once per input. Not shrunk —
/// shrinking re-runs it, and each run of a hang leaves a blocked thread behind.
fn keep_hang(out: &Path, input: &str, kept: &mut BTreeSet<String>) {
    let name = name_of(input);
    if kept.insert(name.clone()) {
        let dir = out.join("hangs");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(format!("{name}.txt")), input);
        eprintln!("hang {name}");
    }
}

fn name_of(text: &str) -> String {
    // FNV-1a: a stable file name for an input.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    format!("{h:016x}")
}

fn seeds(dirs: &[PathBuf], target: Target) -> Vec<String> {
    let wanted = |p: &Path| match target {
        Target::Manifest => p.extension().is_some_and(|e| e == "toml" || e == "txt"),
        _ => p.extension().is_some_and(|e| e == "nz" || e == "txt"),
    };
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = dirs.to_vec();
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                stack.push(p);
            } else if wanted(&p) {
                if let Ok(t) = std::fs::read_to_string(&p) {
                    if t.len() <= 16 * 1024 {
                        out.push(t);
                    }
                }
            }
        }
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(target) = args.first().and_then(|a| Target::by_name(a)) else {
        eprintln!("nazm-fuzz: TARGET is one of front, lower, run, manifest");
        std::process::exit(2);
    };
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let seconds: u64 = value("--seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let seed: u64 = value("--seed")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x5eed);
    let out = PathBuf::from(value("--out").unwrap_or_else(|| "fuzz-out".to_owned()));
    let corpora: Vec<PathBuf> = args
        .windows(2)
        .filter(|w| w[0] == "--corpus")
        .map(|w| PathBuf::from(&w[1]))
        .collect();
    let minimize = args.iter().any(|a| a == "--minimize");
    // A panic is the fuzzer's finding, reported once by it, not printed by every hook.
    std::panic::set_hook(Box::new(|_| {}));

    let mut queue: Vec<String> = Vec::new();
    let mut crashes: BTreeSet<String> = BTreeSet::new();
    let _ = std::fs::create_dir_all(out.join("crashes"));
    let record = |input: &str, message: &str, crashes: &mut BTreeSet<String>| {
        let small = shrink(target, input);
        let name = name_of(&small);
        if crashes.insert(name.clone()) {
            let _ = std::fs::write(out.join("crashes").join(format!("{name}.txt")), &small);
            eprintln!("crash {name}: {}", message.lines().next().unwrap_or(""));
        }
    };
    let mut execs: u64 = 0;
    let mut hangs: usize = 0;
    let mut hung: BTreeSet<String> = BTreeSet::new();
    // Every seed runs; one that reaches a new edge is kept. An empty input always is.
    let mut initial = seeds(&corpora, target);
    initial.insert(0, String::new());
    for s in initial {
        reset();
        execs += 1;
        match exec_bounded(target, &s) {
            Ended::Panicked(m) => record(&s, &m, &mut crashes),
            Ended::Hung => {
                hangs += 1;
                keep_hang(&out, &s, &mut hung);
            }
            Ended::Done if fresh() > 0 => {
                keep();
                queue.push(s);
            }
            Ended::Done => {}
        }
    }
    let seeded = queue.len();
    let mut rng = Rng(seed | 1);
    let start = Instant::now();
    let budget = Duration::from_secs(seconds);
    while start.elapsed() < budget && !queue.is_empty() && hangs < MAX_HANGS {
        let parent = &queue[rng.below(queue.len())];
        let child = mutate(&mut rng, parent, &queue);
        if child.len() > 64 * 1024 {
            continue;
        }
        reset();
        execs += 1;
        match exec_bounded(target, &child) {
            Ended::Panicked(m) => record(&child, &m, &mut crashes),
            Ended::Hung => {
                hangs += 1;
                keep_hang(&out, &child, &mut hung);
            }
            Ended::Done if fresh() > 0 => {
                keep();
                queue.push(child);
            }
            Ended::Done => {}
        }
    }
    let elapsed = start.elapsed().as_secs_f64();
    if minimize {
        // Greedy set cover over the edges reached: the largest contribution first, ties to the
        // smaller input, until every reached edge is covered.
        let n = u32::try_from(guards()).unwrap_or(u32::MAX);
        let edges: Vec<BTreeSet<u32>> = queue
            .iter()
            .map(|q| {
                reset();
                let _ = exec_bounded(target, q);
                (1..=n).filter(|&i| hit(i)).collect()
            })
            .collect();
        let mut covered: BTreeSet<u32> = BTreeSet::new();
        let mut chosen: Vec<usize> = Vec::new();
        loop {
            let best = (0..queue.len())
                .filter(|i| !chosen.contains(i))
                .map(|i| {
                    (
                        edges[i].difference(&covered).count(),
                        std::cmp::Reverse(queue[i].len()),
                        i,
                    )
                })
                .max();
            match best {
                Some((gain, _, i)) if gain > 0 => {
                    covered.extend(edges[i].iter().copied());
                    chosen.push(i);
                }
                _ => break,
            }
        }
        let dir = out.join("min");
        let _ = std::fs::create_dir_all(&dir);
        for i in &chosen {
            let q = &queue[*i];
            let _ = std::fs::write(dir.join(format!("{}.txt", name_of(q))), q);
        }
        eprintln!(
            "minimized: {} inputs cover {} edges",
            chosen.len(),
            covered.len()
        );
    }
    // On standard error: standard output is the interpreted programs', and is discarded.
    eprintln!(
        "{{\"schema\": \"nazm.fuzz/1\", \"target\": \"{}\", \"seconds\": {elapsed:.1}, \"seed\": {seed}, \"execs\": {execs}, \"guards\": {}, \"covered\": {}, \"seeded\": {seeded}, \"kept\": {}, \"hangs\": {hangs}, \"crashes\": {}}}",
        args[0],
        guards(),
        seen(),
        queue.len(),
        crashes.len()
    );
    if !crashes.is_empty() {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    /// Q1-F-01: a hung input is kept under `hangs/`, as it was, once however often it hangs.
    #[test]
    fn a_hang_is_kept_once_and_unchanged() {
        let out = std::env::temp_dir().join(format!("nazm-fuzz-hangs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let mut kept = std::collections::BTreeSet::new();
        let input = "fn main() -> Int { while true { } 0 }\n";
        super::keep_hang(&out, input, &mut kept);
        super::keep_hang(&out, input, &mut kept);
        super::keep_hang(&out, "another", &mut kept);
        let files: Vec<_> = std::fs::read_dir(out.join("hangs"))
            .map(|d| d.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        assert_eq!(files.len(), 2, "{files:?}");
        let name = super::name_of(input);
        let text = std::fs::read_to_string(out.join("hangs").join(format!("{name}.txt")));
        assert_eq!(text.ok().as_deref(), Some(input));
        let _ = std::fs::remove_dir_all(&out);
    }
}
