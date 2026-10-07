//! The stage drivers must assemble their output in one allocation.
//!
//! # What this guards
//!
//! `lexer.nz`, `parser.nz` and `check.nz` each used to build their output with
//! `out = str_concat(out, ..)` in a loop. Every concatenation allocates a new buffer and
//! copies the whole accumulated string, and the generated runtime never frees, so the
//! retained bytes are quadratic in the output length. Measured on `nzlex`: a 2,590-byte
//! input peaked at 41 MB, 10,690 bytes at 682 MB, and 21,490 bytes at 2,889 MB; 43,690
//! bytes could not finish inside 3 GB at all. `emit.nz` had already been moved to
//! `strs_push` + `str_join` for exactly this reason — see the note above `join` there.
//!
//! On 2026-09-21 ten of these ran at once (`cargo test` uses one thread per core) and the
//! machine stopped making progress.
//!
//! # Why two tests
//!
//! The source guard is cheap, needs nothing installed, and runs in every `cargo test`, so
//! the shape cannot come back unnoticed. The measured bound is the one that proves the
//! resource claim, but it needs a container, because macOS cannot impose a hard memory
//! ceiling on a child: `RLIMIT_AS` is an alias of `RLIMIT_RSS` there and `setrlimit(2)`
//! documents it as advisory — the system *prefers* to reclaim from an offender rather
//! than refusing its allocation.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// An image with `clang` in it. Built by `xtask` or by hand; see the failure message.
fn clang_image() -> String {
    std::env::var("NAZM_CLANG_IMAGE").unwrap_or_else(|_| "nazm-probe:deb".to_owned())
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the workspace root")
        .to_path_buf()
}

/// The three drivers whose output accumulator is the thing under guard.
const DRIVERS: &[&str] = &[
    "compiler/lexer.nz",
    "compiler/parser.nz",
    "compiler/check.nz",
];

#[test]
fn no_stage_driver_rebuilds_its_output_with_str_concat() {
    for driver in DRIVERS {
        let path = repo_root().join(driver);
        let src = std::fs::read_to_string(&path).expect("read driver");
        for (n, line) in src.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            assert!(
                !(code.contains("str_concat(out") || code.contains("out = str_concat")),
                "{}:{}: the output accumulator is being rebuilt with `str_concat`, which is \
                 quadratic in time and memory and has already stopped this machine once. \
                 Push the parts with `strs_push` and assemble them with `str_join`, the way \
                 `compiler/emit.nz` does.\n    {}",
                driver,
                n + 1,
                line.trim()
            );
        }
        assert!(
            src.contains("str_join("),
            "{driver}: expected the output to be assembled with `str_join`"
        );
    }
}

/// Returns false to skip. Fails loudly when Docker was *required* but is unusable:
/// a silent skip here would report green on no evidence.
fn docker_or_skip() -> bool {
    if std::env::var("NAZM_REQUIRE_DOCKER").as_deref() != Ok("1") {
        eprintln!(
            "skipping the measured bound: set NAZM_REQUIRE_DOCKER=1 to run it. \
             The source guard above still ran."
        );
        return false;
    }
    let reachable = Command::new("docker")
        .arg("info")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(
        reachable,
        "NAZM_REQUIRE_DOCKER=1 but no Docker daemon is reachable. This is the only test \
         that bounds the stage drivers' memory on real input; skipping it silently would \
         report green on no evidence."
    );
    true
}

/// 128 MiB. The migrated `nzlex` peaked at 7.4 MB on this input and 11.1 MB on one twice
/// its size, so this is roughly 11x headroom over measurement — loose enough to absorb
/// allocator and libc variation, and far too tight for the quadratic shape, which needed
/// more than 3 GiB for the same input.
const MEMORY_CAP_MB: u32 = 128;

/// Large enough that the pre-migration build could not finish it inside 3 GiB.
const INPUT_FUNCTIONS: usize = 1600;

#[test]
#[ignore = "integration-external: Docker (docs/integration-testing.md)"]
fn nzlex_stays_within_a_small_memory_budget_on_input_that_used_to_exhaust_it() {
    if !docker_or_skip() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("nazm-rr-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch");

    // The IR carries no target triple, so the host's `clang` and the container's agree on
    // what to produce; only the container can impose the ceiling.
    let ir = dir.join("nzlex.ll");
    let out = Command::new(NAZM)
        .args([
            "build",
            repo_root().join("compiler/lexer.nz").to_str().unwrap(),
        ])
        .arg("--emit-ir")
        .arg("-o")
        .arg(dir.join("nzlex"))
        .output()
        .expect("run nazm build");
    assert!(
        out.status.success(),
        "lexer.nz no longer builds: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(ir.exists(), "--emit-ir wrote no {}", ir.display());

    let input = dir.join("in.nz");
    let mut text = String::new();
    for i in 0..INPUT_FUNCTIONS {
        text.push_str(&format!("fn f{i}() -> Int {{ 1 + 2 }}\n"));
    }
    std::fs::write(&input, &text).expect("write input");

    // Compilation happens in its own container. `memory.peak` is read in the *second*
    // one, so the number below is the driver's execution plus page cache for the input
    // and output files — not clang, not scratch storage, not a supervisor. The
    // before/after figures in the module docs were taken the same way.
    let compile = Command::new("docker")
        .args(["run", "--rm", "--network", "none", "--log-driver", "none"])
        .arg("--mount")
        .arg(format!("type=bind,source={},target=/w", dir.display()))
        .arg(clang_image())
        .args(["sh", "-c", "clang -O0 -o /w/nzlex.bin /w/nzlex.ll"])
        .output()
        .expect("run docker");
    assert!(
        compile.status.success(),
        "could not compile the IR in `{}`. Build the image with:\n  \
         docker build -t {} - <<'EOF'\n  FROM debian:trixie-slim\n  RUN apt-get update && \
         apt-get install -y --no-install-recommends clang && rm -rf /var/lib/apt/lists/*\n  EOF\n\
         stderr: {}",
        clang_image(),
        clang_image(),
        String::from_utf8_lossy(&compile.stderr)
    );

    let script = "/w/nzlex.bin /w/in.nz > /w/out.txt 2>/dev/null; \
                  rc=$?; echo \"$(cat /sys/fs/cgroup/memory.peak) $(wc -c < /w/out.txt) $rc\"";
    let run = Command::new("docker")
        .args(["run", "--rm"])
        .args(["--memory", &format!("{MEMORY_CAP_MB}m")])
        .args(["--memory-swap", &format!("{MEMORY_CAP_MB}m")]) // equal: a cap that can be swapped past is not a cap
        .args(["--cpus", "1.0", "--pids-limit", "64", "--network", "none"])
        .args(["--log-driver", "none"])
        .arg("--mount")
        .arg(format!("type=bind,source={},target=/w", dir.display()))
        .arg(clang_image())
        .args(["sh", "-c", script])
        .output()
        .expect("run docker");

    let stdout = String::from_utf8_lossy(&run.stdout);
    let fields: Vec<&str> = stdout.split_whitespace().collect();
    assert!(
        run.status.success() && fields.len() == 3,
        "the container did not report a measurement.\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    let [peak, produced, code] = fields.as_slice() else {
        panic!("expected three fields from the container, got {fields:?}");
    };
    let peak: u64 = peak.parse().expect("memory.peak");
    let produced: u64 = produced.parse().expect("output size");
    let code: i32 = code.parse().expect("exit code");

    assert_eq!(
        code, 0,
        "nzlex did not finish: exit {code} (137 is the memory ceiling)"
    );
    assert!(produced > 0, "nzlex produced no output");
    let cap = u64::from(MEMORY_CAP_MB) * 1024 * 1024;
    assert!(
        peak < cap,
        "nzlex peaked at {peak} bytes against a {cap}-byte ceiling — the output is being \
         accumulated by copying again"
    );
}
