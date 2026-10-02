//! N67: accelerator kernels — eligibility, the device's meaning, transfers, identity.
//!
//! `docs/architecture.md` §7.69. Without a device, on any host: the generated OpenCL C carries
//! every checked operation and the lowest-index failure rule, and an ineligible function is refused
//! by name before a device is touched. With macOS's OpenCL on Apple's GPU (the ignored tests, run
//! by hand on such a host): a kernel's results equal the interpreter's element by element, a
//! failure is the lowest failing index's, at its site, and every transfer is in the report.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const KERNELS: &str = "fn square(x: Int) -> Int ! {} { x * x }\n\
fn collatz(n: Int) -> Int ! {} {\n\
    let mut x = n;\n\
    let mut steps = 0;\n\
    while x > 1 {\n\
        if x % 2 == 0 { x = x / 2; } else { x = 3 * x + 1; }\n\
        steps = steps + 1;\n\
    }\n\
    steps + square(n % 7)\n\
}\n\
fn grow(x: Int) -> Int ! {} { x * 3037000500 }\n\
fn inverse(x: Int) -> Int ! {} { 100 / (x - 3) }\n\
fn edges(x: Int) -> Int ! {} { if x == 0 { (0 - 9223372036854775807 - 1) % -1 } else { if x == 1 { (0 - 9223372036854775807 - 1) / 2 } else { -x } } }\n\
fn record(x: Int) -> Int ! {} { let p = Option[Int].Some(value: x); 0 }\n\
fn rec(x: Int) -> Int ! {} { if x == 0 { 0 } else { rec(x - 1) } }\n\
fn loud(x: Int) -> Int ! { io } { 0 }\n\
fn strs(x: Int) -> Int ! {} { str_len(\"ab\") + x }\n\
fn two(x: Int, y: Int) -> Int ! {} { x + y }\n\
fn main() -> Int { 0 }\n";

fn setup(input: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("k.nz"), KERNELS).unwrap();
    std::fs::write(d.path().join("in.txt"), input).unwrap();
    d
}

fn accel(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .arg("accel")
        .args(args)
        .output()
        .unwrap()
}

fn kernel_source(name: &str) -> String {
    let d = setup("1\n");
    let o = accel(
        d.path(),
        &["--print-kernel", "k.nz", name, "--input", "in.txt"],
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}

#[test]
fn the_kernel_carries_every_checked_operation_and_the_lowest_index_rule() {
    let c = kernel_source("collatz");
    assert!(c.contains("__kernel void nz_kernel(__global const long *nz_in, __global long *nz_out, __global int *nz_fail)"), "{c}");
    assert!(
        c.contains("atomic_min(nz_fail, (int)(nz_index * 256 + (site)))"),
        "{c}"
    );
    // A helper is compiled before its caller, and a failure in it stops the caller.
    let square = c.find("static long nz_f0(").unwrap();
    let collatz = c.find("static long nz_f1(").unwrap();
    assert!(square < collatz, "{c}");
    assert!(c.contains("if (*nz_failed) return 0;"), "{c}");
    // Multiplication by the high word; addition by the sign rule; division by zero first.
    assert!(c.contains("mul_hi("), "{c}");
    assert!(c.contains(") & ((1L) ^ "), "{c}");
    assert!(c.contains("if (2L == 0L) NZ_FAIL("), "{c}");
    let e = kernel_source("edges");
    // `i64::MIN % -1` is 0, where C's is undefined; `i64::MIN / -1` fails.
    assert!(e.contains("== -1L) ? 0L : "), "{e}");
    assert!(e.contains("== (-9223372036854775807L - 1L) && "), "{e}");
    // Negation fails on the one `Int` it cannot negate.
    assert!(
        e.contains("if (l0 == (-9223372036854775807L - 1L)) NZ_FAIL("),
        "{e}"
    );
}

#[test]
fn an_ineligible_function_is_refused_by_name_before_any_device() {
    for (name, why) in [
        ("record", "it has a value that is not an `Int` or a `Bool`"),
        ("rec", "it is in a call cycle"),
        ("loud", "it is not pure"),
        (
            "strs",
            "it calls the built-in `str_len`, which has no device form",
        ),
        ("two", "its signature must be `(Int) -> Int`"),
        ("nothere", "there is no function `nothere`"),
    ] {
        let d = setup("1\n");
        let o = accel(d.path(), &["k.nz", name, "--input", "in.txt"]);
        assert_eq!(o.status.code(), Some(1), "{name}");
        let e = String::from_utf8_lossy(&o.stderr);
        assert!(e.contains("error[N0394]"), "{name}: {e}");
        assert!(e.contains(why), "{name}: {e}");
        assert!(o.stdout.is_empty(), "{name} reported a run");
    }
}

#[test]
fn input_that_is_not_ints_is_refused() {
    let d = setup("1 2 three\n");
    let o = accel(d.path(), &["k.nz", "collatz", "--input", "in.txt"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("is not whitespace-separated `Int`s"));
}

fn report(o: &Output) -> serde_json::Value {
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| {
        panic!(
            "{e}: {}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        )
    })
}

/// Needs macOS's OpenCL and a GPU: `cargo test -p nazm-cli --test accel -- --ignored`.
#[test]
#[ignore = "runs on the GPU"]
fn a_kernel_runs_on_the_gpu_and_agrees_with_the_interpreter() {
    let input: String = (1..=20_000).map(|i| format!("{i} ")).collect();
    let d = setup(&input);
    let o = accel(d.path(), &["k.nz", "collatz", "--input", "in.txt"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v = report(&o);
    assert_eq!(v["schema"], "nazm.accel/1");
    assert_eq!(v["reference"], "agrees with the interpreter");
    assert_eq!(v["device"]["api"], "OpenCL");
    assert_eq!(v["elements"], 20_000);
    // Each transfer: the elements and the one failure word.
    assert_eq!(v["transfers"][0]["direction"], "host-to-device");
    assert_eq!(v["transfers"][0]["bytes"], 20_000 * 8 + 4);
    assert_eq!(v["transfers"][1]["direction"], "device-to-host");
    assert_eq!(v["transfers"][1]["bytes"], 20_000 * 8 + 4);
    assert_eq!(
        v["kernel"]["functions"],
        serde_json::json!(["collatz", "square"])
    );
    let id = v["kernel"]["identity"].as_str().unwrap().to_owned();
    assert_eq!(id.len(), 64);
    // The same kernel on the same device is the same identity; another kernel is another.
    let again = report(&accel(
        d.path(),
        &["--no-check", "k.nz", "collatz", "--input", "in.txt"],
    ));
    assert_eq!(again["kernel"]["identity"], id.as_str());
    assert_eq!(again["reference"], "not checked");
    let other = report(&accel(d.path(), &["k.nz", "square", "--input", "in.txt"]));
    assert_ne!(other["kernel"]["identity"], id.as_str());
    // Python's answer, computed independently: sum of Collatz steps plus (n % 7)^2.
    let want: i64 = (1..=20_000i64)
        .map(|n| {
            let (mut x, mut s) = (n, 0);
            while x > 1 {
                x = if x % 2 == 0 { x >> 1 } else { 3 * x + 1 };
                s += 1;
            }
            s + (n % 7) * (n % 7)
        })
        .sum();
    assert_eq!(v["result"]["checksum"], want);
}

#[test]
#[ignore = "runs on the GPU"]
fn a_failure_on_the_gpu_is_the_lowest_failing_index_at_its_site() {
    let d = setup("1 2 3 4 5 3037000500 7 8 9 -3037000501 1\n");
    let o = accel(d.path(), &["k.nz", "grow", "--input", "in.txt"]);
    assert_eq!(o.status.code(), Some(2));
    let v = report(&o);
    assert_eq!(v["result"]["failure"], "N0400");
    assert_eq!(v["result"]["index"], 5);
    assert_eq!(v["reference"], "agrees with the interpreter");
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(
        e.contains("this multiplication overflows `Int` at element 5"),
        "{e}"
    );

    let d = setup("0 1 2 3 4\n");
    let o = accel(d.path(), &["k.nz", "inverse", "--input", "in.txt"]);
    assert_eq!(o.status.code(), Some(2));
    let v = report(&o);
    assert_eq!(
        (
            v["result"]["failure"].as_str(),
            v["result"]["index"].as_u64()
        ),
        (Some("N0401"), Some(3))
    );
    assert!(String::from_utf8_lossy(&o.stderr).contains("division by zero at element 3"));

    let d = setup("0 1 5 -9223372036854775807\n");
    let v = report(&accel(d.path(), &["k.nz", "edges", "--input", "in.txt"]));
    assert_eq!(v["reference"], "agrees with the interpreter");
}
