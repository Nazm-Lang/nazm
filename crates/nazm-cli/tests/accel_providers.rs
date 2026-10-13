//! Gate 3: accelerator providers and float kernels (`docs/spec.md`, *Accelerator kernels*,
//! *Providers*; `docs/systems-domains.md` Part L).
//!
//! The CPU provider runs on any host with `clang` and threads, so these tests need no device: a
//! kernel's one source text, compiled as C and run on a thread pool, agrees with the interpreter for
//! `Int` kernels — the lowest failing index's failure included — and bit for bit for `Float32` and
//! `Float64` kernels. The ignored tests need macOS's OpenCL GPU: there the CPU provider is the
//! second oracle, and a `Float64` kernel, which Apple's GPU cannot run, falls back to the CPU and
//! says why.

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
fn inverse(x: Int) -> Int ! {} { 100 / (x - 3) }\n\
fn poly(x: Float64) -> Float64 ! {} { let y = x * 0.5 + 1.25; float_sqrt(float_abs(y * x - 3.0)) / (x + 0.1) }\n\
fn polyf(x: Float32) -> Float32 ! {} { let y = x * 0.5 + 1.25; float_sqrt(float_abs(y * x - 3.0)) / (x + 0.1) }\n\
fn rem(a: Float64, b: Float64) -> Float64 ! {} { if a < b { a % b } else { -b } }\n\
fn axpy(a: Float32, b: Float32) -> Float32 ! {} { 2.5 * a + b }\n\
fn mixed(x: Float64) -> Float64 ! {} { let n = 3; x }\n\
fn wide(x: Float32) -> Float64 ! {} { 0.0 }\n\
fn sine(x: Float64) -> Float64 ! {} { float_sin(x) }\n\
fn main() -> Int { 0 }\n";

fn setup(inputs: &[(&str, String)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("k.nz"), KERNELS).unwrap();
    for (name, text) in inputs {
        std::fs::write(d.path().join(name), text).unwrap();
    }
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

fn report(o: &Output) -> serde_json::Value {
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| {
        panic!(
            "{e}: {}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        )
    })
}

/// `n` numbers spread over -1000 to 1000, the same every run: a fixed linear congruential sequence.
fn floats(n: usize, seed: u64) -> String {
    let mut x = seed;
    (0..n)
        .map(|_| {
            x = x
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            #[allow(clippy::cast_precision_loss)]
            let v = ((x >> 11) as f64) / ((1u64 << 53) as f64) * 2000.0 - 1000.0;
            format!("{v:?} ")
        })
        .collect()
}

#[test]
fn the_cpu_provider_runs_the_same_kernel_and_agrees_with_the_interpreter() {
    let input: String = (1..=20_000).map(|i| format!("{i} ")).collect();
    let d = setup(&[("in.txt", input)]);
    let o = accel(
        d.path(),
        &[
            "--provider",
            "cpu",
            "k.nz",
            "collatz",
            "--input",
            "in.txt",
            "--reduce",
            "add",
        ],
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v = report(&o);
    assert_eq!(v["schema"], "nazm.accel/3");
    assert_eq!(v["provider"]["name"], "cpu");
    assert_eq!(v["provider"]["reason"], "asked for");
    assert_eq!(v["provider"]["vectorised"], true);
    assert!(v["provider"]["threads"].as_u64().unwrap() >= 1, "{v}");
    assert_eq!(v["device"]["api"], "C (the CPU provider)");
    assert_eq!(v["kernel"]["element"], "Int");
    assert_eq!(v["transfers"], serde_json::json!([]));
    assert_eq!(v["reference"], "agrees with the interpreter");
    assert_eq!(v["cross_check"], serde_json::Value::Null);
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
    assert_eq!(v["reduce"]["value"], want);
    // The same source text the device builds: the element function beside the kernel.
    let p = accel(
        d.path(),
        &["--print-kernel", "k.nz", "collatz", "--input", "in.txt"],
    );
    let src = String::from_utf8(p.stdout).unwrap();
    assert!(src.contains("static long nz_item(long a,"), "{src}");
    assert!(src.contains("__kernel void nz_kernel("), "{src}");
}

#[test]
fn a_failure_on_the_cpu_is_the_lowest_failing_index_at_its_site() {
    // Elements 9 and 5000 divide by zero; the pool's threads may meet either first.
    let input: String = (0..20_000)
        .map(|i| format!("{} ", if i == 9 || i == 5000 { 3 } else { i + 4 }))
        .collect();
    let d = setup(&[("in.txt", input)]);
    let o = accel(
        d.path(),
        &["--provider", "cpu", "k.nz", "inverse", "--input", "in.txt"],
    );
    assert_eq!(
        o.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let v = report(&o);
    assert_eq!(v["result"]["failure"], "N0401");
    assert_eq!(v["result"]["index"], 9);
    assert_eq!(v["reference"], "agrees with the interpreter");
    assert!(String::from_utf8_lossy(&o.stderr).contains("division by zero at element 9"));
}

#[test]
fn float_kernels_agree_with_the_interpreter_bit_for_bit() {
    let d = setup(&[
        ("f.txt", floats(50_000, 7)),
        ("g.txt", floats(50_000, 8)),
        (
            "edge.txt",
            "0.0 -0.0 inf -inf nan 1e-310 -3.0 0.1 1e300".to_owned(),
        ),
    ]);
    for (kernel, inputs, element) in [
        ("poly", vec!["f.txt"], "Float64"),
        ("polyf", vec!["f.txt"], "Float32"),
        ("rem", vec!["f.txt", "g.txt"], "Float64"),
        ("axpy", vec!["f.txt", "g.txt"], "Float32"),
        ("poly", vec!["edge.txt"], "Float64"),
        ("polyf", vec!["edge.txt"], "Float32"),
    ] {
        for scalar in [false, true] {
            let mut args = vec!["--provider", "cpu", "k.nz", kernel, "--reduce", "add"];
            for i in &inputs {
                args.extend(["--input", i]);
            }
            if scalar {
                args.push("--scalar");
            }
            let o = accel(d.path(), &args);
            assert!(
                o.status.success(),
                "{kernel}: {}",
                String::from_utf8_lossy(&o.stderr)
            );
            let v = report(&o);
            assert_eq!(v["kernel"]["element"], element, "{kernel}");
            assert_eq!(
                v["reference"], "agrees with the interpreter",
                "{kernel} {inputs:?}"
            );
            assert_eq!(v["provider"]["vectorised"], !scalar);
            assert!(v["reduce"]["value"].is_string(), "{v}");
        }
    }
}

#[test]
fn a_float_kernel_is_one_type_throughout_and_calls_only_what_is_exact() {
    let d = setup(&[("f.txt", "1.0 2.0".to_owned())]);
    for (kernel, why) in [
        (
            "mixed",
            "it has a value that is not an `Float64` or a `Bool`",
        ),
        ("wide", "`(T) -> T` or `(T, T) -> T`"),
        (
            "sine",
            "it calls the built-in `float_sin`, which has no device form",
        ),
    ] {
        let o = accel(
            d.path(),
            &["--provider", "cpu", "k.nz", kernel, "--input", "f.txt"],
        );
        assert_eq!(o.status.code(), Some(1), "{kernel}");
        let e = String::from_utf8_lossy(&o.stderr);
        assert!(
            e.contains("error[N0394]") && e.contains(why),
            "{kernel}: {e}"
        );
        assert!(o.stdout.is_empty(), "{kernel} reported a run");
    }
    // Input that is not the kernel's type, refused by its name.
    std::fs::write(d.path().join("bad.txt"), "1.0 two").unwrap();
    let o = accel(
        d.path(),
        &["--provider", "cpu", "k.nz", "poly", "--input", "bad.txt"],
    );
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("is not whitespace-separated `Float64`s"));
}

/// `auto`, the default, runs the kernel where it can and says where: on the GPU, or — on a host
/// without OpenCL, such as Linux — on the CPU provider, the reason in the report and on stderr.
#[test]
fn auto_runs_where_it_can_and_says_where() {
    let d = setup(&[("in.txt", "1 2 3 4".to_owned())]);
    let o = accel(d.path(), &["k.nz", "square", "--input", "in.txt"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v = report(&o);
    assert_eq!(v["reference"], "agrees with the interpreter");
    assert_eq!(v["result"]["checksum"], 30);
    let reason = v["provider"]["reason"].as_str().unwrap();
    assert!(reason.starts_with("auto: "), "{reason}");
    if v["provider"]["name"] == "cpu" {
        assert!(
            String::from_utf8_lossy(&o.stderr).contains("the CPU provider runs the kernel"),
            "a fallback is never silent"
        );
    } else {
        assert_eq!(v["provider"]["name"], "opencl");
        assert_eq!(reason, "auto: the GPU can run it");
    }
    if !cfg!(target_os = "macos") {
        assert_eq!(v["provider"]["name"], "cpu");
        assert!(reason.contains("this host has no OpenCL"), "{reason}");
    }
}

/// Needs macOS's OpenCL and a GPU: `cargo test -p nazm-cli --test cli_a_b -- --ignored accel_providers::`.
#[test]
#[ignore = "runs on the GPU"]
fn on_the_gpu_the_cpu_provider_is_the_second_oracle_and_float64_falls_back() {
    let input: String = (1..=20_000).map(|i| format!("{i} ")).collect();
    let d = setup(&[
        ("in.txt", input),
        ("f.txt", floats(50_000, 7)),
        ("g.txt", floats(50_000, 8)),
    ]);
    // `auto` on the GPU, the CPU provider holding it to account.
    let v = report(&accel(d.path(), &["k.nz", "collatz", "--input", "in.txt"]));
    assert_eq!(v["provider"]["name"], "opencl");
    assert_eq!(v["provider"]["reason"], "auto: the GPU can run it");
    assert_eq!(v["cross_check"], "agrees with the CPU provider");
    // A `Float32` kernel on the GPU, bit for bit: no contraction, division and square root
    // correctly rounded.
    for (kernel, inputs) in [("polyf", vec!["f.txt"]), ("axpy", vec!["f.txt", "g.txt"])] {
        let mut args = vec!["--provider", "opencl", "k.nz", kernel];
        for i in &inputs {
            args.extend(["--input", i]);
        }
        let o = accel(d.path(), &args);
        assert!(
            o.status.success(),
            "{kernel}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        let v = report(&o);
        assert_eq!(v["reference"], "agrees with the interpreter", "{kernel}");
        assert_eq!(v["cross_check"], "agrees with the CPU provider", "{kernel}");
    }
    // `Float64` needs `cl_khr_fp64`, which Apple's GPU has not: asked for, refused by name; under
    // `auto`, run on the CPU and said so.
    let o = accel(
        d.path(),
        &["--provider", "opencl", "k.nz", "poly", "--input", "f.txt"],
    );
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("cl_khr_fp64"));
    let o = accel(d.path(), &["k.nz", "poly", "--input", "f.txt"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v = report(&o);
    assert_eq!(v["provider"]["name"], "cpu");
    assert!(
        v["provider"]["reason"]
            .as_str()
            .unwrap()
            .contains("cl_khr_fp64"),
        "{v}"
    );
    assert!(String::from_utf8_lossy(&o.stderr).contains("the CPU provider runs the kernel"));
    assert_eq!(v["reference"], "agrees with the interpreter");
}
