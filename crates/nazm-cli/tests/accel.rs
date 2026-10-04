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
    clippy::indexing_slicing,
    // The numeric oracle (N88) computes the spec's division and draws generated values.
    clippy::integer_division,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
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
fn three(x: Int, y: Int, z: Int) -> Int ! {} { x + y + z }\n\
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
        // A zip of two is a kernel since N88; three are not.
        (
            "three",
            "its signature must be `(Int) -> Int` or `(Int, Int) -> Int`",
        ),
        (
            "two",
            "`two` takes 2 `Int`s, so it runs over 2 inputs, and 1 was given",
        ),
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
    assert_eq!(v["schema"], "nazm.accel/2");
    assert_eq!(v["kernel"]["shape"], "map");
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

#[test]
fn a_zip_needs_two_inputs_of_one_length_before_any_device() {
    let d = setup("1 2 3\n");
    std::fs::write(d.path().join("four.txt"), "1 2 3 4\n").unwrap();
    let o = accel(
        d.path(),
        &["k.nz", "two", "--input", "in.txt", "--input", "four.txt"],
    );
    assert_eq!(o.status.code(), Some(1));
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(
        e.contains("error[N0394]") && e.contains("these have 3 and 4"),
        "{e}"
    );
    assert!(o.stdout.is_empty());
    // A zip's kernel reads two buffers, element by element.
    let source = kernel_source("two");
    assert!(source.contains("__global const long *nz_in2"), "{source}");
    assert!(
        source.contains("nz_in[nz_index], nz_in2[nz_index]"),
        "{source}"
    );
}

/// Needs macOS's OpenCL and a GPU (N88): a zip and the three reductions, each held to the
/// interpreter and to an independent computation; `add` fails where the running sum overflows.
#[test]
#[ignore = "runs on the GPU"]
fn a_zip_and_a_reduction_run_on_the_gpu_and_agree() {
    let n: i64 = 50_000;
    let a: String = (1..=n).map(|i| format!("{i} ")).collect();
    let b: String = (1..=n).map(|i| format!("{} ", i % 7)).collect();
    let d = setup(&a);
    std::fs::write(d.path().join("b.txt"), &b).unwrap();
    let want_sum: i64 = (1..=n).map(|i| i * (i % 7) + i).sum();
    let want_max: i64 = (1..=n).map(|i| i + i % 7).max().unwrap();
    let want_min: i64 = (1..=n).map(|i| i + i % 7).min().unwrap();
    for (op, want) in [("add", want_sum), ("max", want_max), ("min", want_min)] {
        let kernel = if op == "add" { "zipped" } else { "two" };
        let src = format!("{KERNELS}fn zipped(x: Int, y: Int) -> Int ! {{}} {{ x * y + x }}\n");
        std::fs::write(d.path().join("k.nz"), src).unwrap();
        let v = report(&accel(
            d.path(),
            &[
                "k.nz", kernel, "--input", "in.txt", "--input", "b.txt", "--reduce", op,
            ],
        ));
        assert_eq!(v["kernel"]["shape"], "zip");
        assert_eq!(v["reference"], "agrees with the interpreter", "{v}");
        assert_eq!(v["reduce"]["value"].as_i64(), Some(want), "{op}: {v}");
        assert_eq!(v["inputs"].as_array().unwrap().len(), 2);
    }
    // `add` checks the running sum, as `ints_sum` does, though the total fits.
    std::fs::write(d.path().join("ov.txt"), "9223372036854775807 1 -1\n").unwrap();
    let o = accel(
        d.path(),
        &["k.nz", "square", "--input", "ov.txt", "--reduce", "add"],
    );
    assert_eq!(o.status.code(), Some(2));
}

/// One generated kernel's body over `x` and `y`, and its meaning in 128-bit arithmetic: the
/// value, or the code of the first failure in evaluation order.
#[derive(Clone, Debug)]
enum Gen {
    X,
    Y,
    Lit(i64),
    Neg(Box<Gen>),
    Bin(char, Box<Gen>, Box<Gen>),
    If(Box<Gen>, Box<Gen>, Box<Gen>, Box<Gen>),
}

const EDGES: [i64; 9] = [0, 1, -1, 2, 7, -3, i64::MAX, i64::MIN + 1, 3037000500];

fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

fn generate(seed: &mut u64, depth: u32) -> Gen {
    let pick = next(seed) % if depth == 0 { 3 } else { 7 };
    match pick {
        0 => Gen::X,
        1 => Gen::Y,
        2 => Gen::Lit(EDGES[(next(seed) % EDGES.len() as u64) as usize]),
        3 => Gen::Neg(Box::new(generate(seed, depth - 1))),
        4 | 5 => {
            let op = ['+', '-', '*', '/', '%'][(next(seed) % 5) as usize];
            Gen::Bin(
                op,
                Box::new(generate(seed, depth - 1)),
                Box::new(generate(seed, depth - 1)),
            )
        }
        _ => Gen::If(
            Box::new(generate(seed, depth - 1)),
            Box::new(generate(seed, depth - 1)),
            Box::new(generate(seed, depth - 1)),
            Box::new(generate(seed, depth - 1)),
        ),
    }
}

fn source(g: &Gen) -> String {
    match g {
        Gen::X => "x".to_owned(),
        Gen::Y => "y".to_owned(),
        Gen::Lit(v) if *v < 0 => format!("(0 - {})", -(i128::from(*v))),
        Gen::Lit(v) => v.to_string(),
        Gen::Neg(a) => format!("(0 - {})", source(a)),
        Gen::Bin(op, a, b) => format!("({} {op} {})", source(a), source(b)),
        Gen::If(a, b, t, e) => format!(
            "(if {} < {} {{ {} }} else {{ {} }})",
            source(a),
            source(b),
            source(t),
            source(e)
        ),
    }
}

/// `Ok(value)` or `Err(code)`, by the spec's checked arithmetic.
fn meaning(g: &Gen, x: i64, y: i64) -> Result<i64, &'static str> {
    let fit = |v: i128| i64::try_from(v).map_err(|_| "N0400");
    match g {
        Gen::X => Ok(x),
        Gen::Y => Ok(y),
        Gen::Lit(v) if *v < 0 => fit(0 - (-i128::from(*v))),
        Gen::Lit(v) => Ok(*v),
        Gen::Neg(a) => fit(0 - i128::from(meaning(a, x, y)?)),
        Gen::Bin(op, a, b) => {
            let l = i128::from(meaning(a, x, y)?);
            let r = i128::from(meaning(b, x, y)?);
            match op {
                '+' => fit(l + r),
                '-' => fit(l - r),
                '*' => fit(l * r),
                '/' if r == 0 => Err("N0401"),
                '/' => fit(l / r),
                '%' if r == 0 => Err("N0401"),
                _ => fit(l % r),
            }
        }
        Gen::If(a, b, t, e) => {
            if meaning(a, x, y)? < meaning(b, x, y)? {
                meaning(t, x, y)
            } else {
                meaning(e, x, y)
            }
        }
    }
}

/// The numeric oracle on generated inputs (N88, §7.89): kernels generated over `+ - * / %`,
/// comparisons, `if` and literals at `Int`'s edges, over elements across its range, each run on
/// the GPU and held to the interpreter (inside `nazm accel`) and to the spec's arithmetic in 128
/// bits (here): the results, or the lowest failing index and its code. Needs macOS's OpenCL.
#[test]
#[ignore = "runs on the GPU"]
fn generated_kernels_agree_with_the_interpreter_and_with_128_bit_arithmetic() {
    let mut seed: u64 = 0x88;
    let d = setup("0\n");
    let mut compared = 0;
    for k in 0..40 {
        let g = generate(&mut seed, 4);
        let src = format!(
            "fn gen(x: Int, y: Int) -> Int ! {{}} {{ {} }}\nfn main() -> Int {{ 0 }}\n",
            source(&g)
        );
        std::fs::write(d.path().join("g.nz"), &src).unwrap();
        let xs: Vec<i64> = (0..200)
            .map(|i| {
                if i % 9 == 0 {
                    EDGES[(i / 9) % EDGES.len()]
                } else {
                    (next(&mut seed) as i64) >> (next(&mut seed) % 63)
                }
            })
            .collect();
        let ys: Vec<i64> = (0..200)
            .map(|i| {
                if i % 11 == 0 {
                    EDGES[(i / 11) % EDGES.len()]
                } else {
                    (next(&mut seed) as i64) >> (next(&mut seed) % 63)
                }
            })
            .collect();
        let line = |v: &[i64]| v.iter().map(i64::to_string).collect::<Vec<_>>().join(" ");
        std::fs::write(d.path().join("x.txt"), line(&xs)).unwrap();
        std::fs::write(d.path().join("y.txt"), line(&ys)).unwrap();
        let o = accel(
            d.path(),
            &["g.nz", "gen", "--input", "x.txt", "--input", "y.txt"],
        );
        let v = report(&o);
        assert_eq!(
            v["reference"], "agrees with the interpreter",
            "kernel {k}: {src} {v}"
        );
        let first = xs
            .iter()
            .zip(&ys)
            .enumerate()
            .find_map(|(i, (x, y))| meaning(&g, *x, *y).err().map(|c| (i, c)));
        match first {
            Some((i, code)) => {
                assert_eq!(
                    v["result"]["index"].as_u64(),
                    Some(i as u64),
                    "kernel {k}: {src}"
                );
                assert_eq!(
                    v["result"]["failure"].as_str(),
                    Some(code),
                    "kernel {k}: {src}"
                );
            }
            None => {
                let sum = xs.iter().zip(&ys).fold(0i64, |a, (x, y)| {
                    a.wrapping_add(meaning(&g, *x, *y).unwrap())
                });
                assert_eq!(
                    v["result"]["checksum"].as_i64(),
                    Some(sum),
                    "kernel {k}: {src}"
                );
            }
        }
        compared += 1;
    }
    assert_eq!(compared, 40);
}
