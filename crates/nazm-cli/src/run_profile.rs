//! `nazm profile FILE` (N58, `docs/architecture.md` §7.60): build the program, run it once with
//! its memory and scheduler counters on, and print `nazm.profile/2` — counts of events, and
//! nothing the compiler measured about itself (that is `nazm.timings/1`).
//!
//! `--sample` (N91, §7.92) also samples the run: the program is built with debug information and
//! run under the host's sampler — macOS's `sample`, the one this host has — and each sample's
//! innermost frame is attributed to a Nazm function and `.nz` line, the runtime, foreign code
//! linked into the program, or the system, with the system's known waits counted as blocked.
//! The sampler is an observer outside the program; nothing in the program changes.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, ExitCode};
use std::time::Instant;

use serde_json::{json, Map, Value};

use crate::exit;

/// `key=value` pairs after `prefix` on the first line of `text` that starts with it.
fn counts(text: &str, prefix: &str) -> Vec<(String, u64)> {
    let Some(line) = text.lines().find(|l| l.starts_with(prefix)) else {
        return Vec::new();
    };
    line.split_whitespace()
        .filter_map(|w| {
            let (k, v) = w.split_once('=')?;
            Some((k.to_owned(), v.parse().ok()?))
        })
        .collect()
}

/// The sampler, and the waits in the system's libraries a sample's innermost frame names when
/// a thread is blocked rather than running: locks and condition variables, sleeps, waits for a
/// child, file descriptors or kernel events, and Mach messages.
const SAMPLER: &str = "/usr/bin/sample";
const WAITS: &[&str] = &[
    "__psynch_cvwait",
    "__psynch_mutexwait",
    "__ulock_wait",
    "__ulock_wait2",
    "__semwait_signal",
    "__select",
    "__wait4",
    "kevent",
    "kevent64",
    "kevent_id",
    "mach_msg2_trap",
    "mach_msg_trap",
    "nanosleep",
    "__read_nocancel",
    "read",
    "poll",
];

/// One frame of the sampler's call tree.
struct Frame {
    depth: usize,
    count: u64,
    symbol: String,
    image: String,
    /// `FILE:LINE`, when the sampler resolved one.
    place: Option<(String, u64)>,
    thread: bool,
}

/// The call tree of a `sample` report: each line after `Call graph:` up to the blank line.
fn frames(report: &str) -> Vec<Frame> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in report.lines() {
        if line.starts_with("Call graph:") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if line.trim().is_empty() {
            break;
        }
        // The count is the first digit after the tree's drawing characters; its column is the
        // frame's depth.
        let Some(depth) = line.find(|c: char| c.is_ascii_digit()) else {
            continue;
        };
        let rest = line.get(depth..).unwrap_or("");
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        let Ok(count) = digits.parse::<u64>() else {
            continue;
        };
        let rest = rest.get(digits.len()..).unwrap_or("").trim_start();
        if rest.starts_with("Thread_") {
            out.push(Frame {
                depth,
                count,
                symbol: String::new(),
                image: String::new(),
                place: None,
                thread: true,
            });
            continue;
        }
        let (symbol, after) = rest.split_once("  (in ").unwrap_or((rest, ""));
        let image = after.split(')').next().unwrap_or("").to_owned();
        let place = after.split_whitespace().last().and_then(|w| {
            let (f, l) = w.rsplit_once(':')?;
            f.ends_with(".nz").then_some(())?;
            Some((f.to_owned(), l.parse().ok()?))
        });
        out.push(Frame {
            depth,
            count,
            symbol: symbol.trim().to_owned(),
            image,
            place,
            thread: false,
        });
    }
    out
}

/// Each frame's own samples — its count less its children's — attributed by where it is.
fn attribute(report: &str, program: &str, interval_ms: u64) -> Value {
    let fs = frames(report);
    let mut nazm: BTreeMap<(String, String, String, u64), u64> = BTreeMap::new();
    let mut runtime: BTreeMap<String, u64> = BTreeMap::new();
    let mut foreign: BTreeMap<String, u64> = BTreeMap::new();
    let mut system: BTreeMap<(String, String), u64> = BTreeMap::new();
    let (mut total, mut blocked, mut unknown) = (0u64, 0u64, 0u64);
    for (i, f) in fs.iter().enumerate() {
        // Its direct children: the frames under it at the first one's column — siblings share
        // a column.
        let mut children = 0u64;
        let mut column = None;
        for c in fs.iter().skip(i + 1) {
            if c.depth <= f.depth {
                break;
            }
            if *column.get_or_insert(c.depth) == c.depth {
                children += c.count;
            }
        }
        let own = f.count.saturating_sub(children);
        if own == 0 {
            continue;
        }
        total += own;
        if f.thread {
            unknown += own;
        } else if f.image == program {
            let nz = f.symbol.starts_with("nz.m.") || f.symbol.starts_with("nz.g.");
            match (&f.place, nz) {
                (Some((file, line)), true) => {
                    let function = f.symbol.rsplit('.').next().unwrap_or("").to_owned();
                    *nazm
                        .entry((function, f.symbol.clone(), file.clone(), *line))
                        .or_default() += own;
                }
                _ if f.symbol.starts_with("nz.") || f.symbol.starts_with("nz_") => {
                    *runtime.entry(f.symbol.clone()).or_default() += own;
                }
                _ => *foreign.entry(f.symbol.clone()).or_default() += own,
            }
        } else {
            if WAITS.contains(&f.symbol.as_str()) {
                blocked += own;
            }
            *system
                .entry((f.symbol.clone(), f.image.clone()))
                .or_default() += own;
        }
    }
    let mut nazm: Vec<_> = nazm.into_iter().collect();
    nazm.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let by_count = |m: BTreeMap<String, u64>| {
        let mut v: Vec<_> = m.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.into_iter()
            .map(|(symbol, samples)| json!({ "symbol": symbol, "samples": samples }))
            .collect::<Vec<_>>()
    };
    let mut system: Vec<_> = system.into_iter().collect();
    system.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    json!({
        "sampler": "sample",
        "interval_ms": interval_ms,
        "total": total,
        "nazm": nazm.into_iter().map(|((function, symbol, file, line), samples)| json!({
            "function": function, "symbol": symbol, "file": file, "line": line, "samples": samples,
        })).collect::<Vec<_>>(),
        "runtime": by_count(runtime),
        "foreign": by_count(foreign),
        "system": system.into_iter().map(|((symbol, image), samples)| json!({
            "symbol": symbol, "image": image, "samples": samples,
            "blocked": WAITS.contains(&symbol.as_str()),
        })).collect::<Vec<_>>(),
        "blocked": blocked,
        "unattributed": unknown,
    })
}

pub(crate) fn run(file: &Path, backend: &str, sample: Option<u64>) -> ExitCode {
    // No sampler, no sampling: refused before anything is built, never a profile without the
    // samples asked for (N91).
    if sample.is_some() && !Path::new(SAMPLER).is_file() {
        eprintln!(
            "nazm: `--sample` runs the program under `{SAMPLER}`, macOS's sampler, which this \
             host does not have"
        );
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }
    // A directory of this process's own, removed on every path out.
    let work = std::env::temp_dir().join(format!("nazm-profile-{}", std::process::id()));
    if std::fs::create_dir_all(&work).is_err() {
        eprintln!("nazm: cannot make a working directory");
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }
    let code = profile(file, backend, &work, sample);
    let _ = std::fs::remove_dir_all(&work);
    code
}

fn profile(file: &Path, backend: &str, work: &Path, sample: Option<u64>) -> ExitCode {
    let exe = work.join("profiled");
    let Ok(me) = std::env::current_exe() else {
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    };
    let mut build = Command::new(&me);
    build.args(["build", "--backend", backend]);
    // A sample is attributed to a `.nz` line through the build's debug information.
    if sample.is_some() {
        build.args(["--debug", "--no-cache"]);
    }
    let built = build.arg(file).arg("-o").arg(&exe).output();
    match built {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            eprint!("{}", String::from_utf8_lossy(&o.stderr));
            return ExitCode::from(
                o.status
                    .code()
                    .and_then(|c| u8::try_from(c).ok())
                    .unwrap_or(1),
            );
        }
        Err(e) => {
            eprintln!("nazm: cannot build: {e}");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    }
    // The program's output goes to files, so nothing waits on a pipe while it is sampled.
    let (out_path, err_path) = (work.join("stdout"), work.join("stderr"));
    let (Ok(out_file), Ok(err_file)) = (
        std::fs::File::create(&out_path),
        std::fs::File::create(&err_path),
    ) else {
        eprintln!("nazm: cannot make the run's output files");
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    };
    let start = Instant::now();
    let child = Command::new(&exe)
        .env(nazm_lir::MEMORY_VARIABLE, "1")
        .env(nazm_lir::runtime::SCHED_VARIABLE, "1")
        .stdout(out_file)
        .stderr(err_file)
        .spawn();
    let Ok(mut child) = child else {
        eprintln!("nazm: cannot run the program");
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    };
    let report = work.join("samples.txt");
    let sampler = sample.and_then(|interval| {
        Command::new(SAMPLER)
            .arg(child.id().to_string())
            .args(["3600", &interval.to_string(), "-file"])
            .arg(&report)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()
    });
    let status = child.wait();
    let wall = start.elapsed();
    if let Some(mut s) = sampler {
        let _ = s.wait();
    }
    let Ok(status) = status else {
        eprintln!("nazm: cannot run the program");
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    };
    let o = std::process::Output {
        status,
        stdout: std::fs::read(&out_path).unwrap_or_default(),
        stderr: std::fs::read(&err_path).unwrap_or_default(),
    };
    let err = String::from_utf8_lossy(&o.stderr);
    // The memory line is `kind allocated=… reclaimed=… live=…` four times, in a fixed order.
    let mut memory = Map::new();
    let pairs = counts(&err, "nazm-memory:");
    for (kind, chunk) in ["sequences", "strings", "channels", "closures"]
        .iter()
        .zip(pairs.chunks(3))
    {
        let mut m = Map::new();
        for (k, v) in chunk {
            m.insert(k.clone(), json!(v));
        }
        memory.insert((*kind).to_owned(), Value::Object(m));
    }
    let scheduler: Map<String, Value> = counts(&err, "nazm-sched:")
        .into_iter()
        .map(|(k, v)| (k, json!(v)))
        .collect();
    // A sampler that never attached — the program was done first — reports no samples, which
    // is what happened.
    let samples = sample.map(|interval| {
        let text = std::fs::read_to_string(&report).unwrap_or_default();
        attribute(&text, "profiled", interval)
    });
    let doc = json!({
        "schema": "nazm.profile/2",
        "backend": backend,
        "exit": o.status.code(),
        "wall_ms": u64::try_from(wall.as_millis()).unwrap_or(u64::MAX),
        "stdout_bytes": o.stdout.len(),
        "memory": memory,
        "scheduler": scheduler,
        "samples": samples,
    });
    println!("{doc}");
    ExitCode::SUCCESS
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// A `sample` report's call tree, as the tool writes it: two threads, one blocked joining.
    const REPORT: &str =
        "Analysis of sampling profiled (pid 1) every 1 millisecond\n\nCall graph:\n    \
        30 Thread_1   DispatchQueue_1: com.apple.main-thread  (serial)\n    \
        + 20 _dyld_start  (in dyld) + 0  [0x1]\n    \
        + 10 nz.m.p_2Enz.main  (in profiled) + 48  [0x2]  p.nz:9\n    \
        +   7 nz.join_one  (in profiled) + 20  [0x3]\n    \
        +   ! 7 __ulock_wait  (in libsystem_kernel.dylib) + 8  [0x4]\n    \
        +   3 nz.m.p_2Enz.helper  (in profiled) + 4  [0x5]  p.nz:0\n    \
        12 Thread_2\n      \
        9 nz.m.p_2Enz.burn  (in profiled) + 236,64,...  [0x6,0x7,...]  p.nz:4\n      \
        2 c_hash  (in profiled) + 12  [0x8]\n      \
        1 nz.alloc  (in profiled) + 4  [0x9]\n\nTotal number in stack\n";

    #[test]
    fn each_sample_is_attributed_once_by_its_innermost_frame() {
        let v = attribute(REPORT, "profiled", 1);
        assert_eq!(v["total"], 42, "{v}");
        assert_eq!(v["blocked"], 7);
        assert_eq!(v["unattributed"], 0);
        let nazm = v["nazm"].as_array().unwrap();
        assert_eq!(
            nazm.iter()
                .map(|e| (
                    e["function"].as_str().unwrap(),
                    e["line"].as_u64().unwrap(),
                    e["samples"].as_u64().unwrap()
                ))
                .collect::<Vec<_>>(),
            [("burn", 4, 9), ("helper", 0, 3)],
            "main's samples are its callees': {v}"
        );
        assert_eq!(v["runtime"][0]["symbol"], "nz.alloc");
        assert_eq!(v["foreign"][0]["symbol"], "c_hash");
        assert_eq!(v["foreign"][0]["samples"], 2);
        let system = v["system"].as_array().unwrap();
        assert_eq!(system[0]["symbol"], "_dyld_start");
        assert_eq!(system[0]["blocked"], false);
        assert_eq!(system[1]["symbol"], "__ulock_wait");
        assert_eq!(system[1]["blocked"], true);
        // No report, no samples: a sampler that never attached.
        assert_eq!(attribute("", "profiled", 1)["total"], 0);
    }
}
