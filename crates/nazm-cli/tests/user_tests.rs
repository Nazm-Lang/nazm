//! Gate 2: user tests, fuzz targets and benchmarks.
//!
//! `docs/general-purpose.md` §18–§19, `docs/spec.md` *Tests and benchmarks*. What this holds: `nazm
//! test` runs every `@test` function of a file under the interpreter and as a native build, by
//! name, each in its own process; `Ok` passes, `Err` fails with its message, a trap fails, and
//! `@test(fails)` inverts; `--filter` narrows; a `@fuzz` target is driven with deterministic inputs;
//! the file's own `main` is not run and every diagnostic still lands in the file; `@std/test`'s
//! assertions and property helper; `nazm bench` measures a `@bench` function natively.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::differential;

use differential::{codes, nazm};

const TESTS: &str = "use \"@std/test\";\nuse \"@std/random\";\n\n\
fn double(n: Int) -> Int { n * 2 }\n\n\
fn main() -> Int { double(21) }\n\n\
@test\nfn doubles() -> Result[Int, Str] {\n    assert_eq_int(\"double(2)\", double(2), 4)?;\n    assert_true(\"positive\", double(1) > 0)\n}\n\n\
@test\nfn reports_its_failure() -> Result[Int, Str] {\n    assert_eq_int(\"double(3)\", double(3), 7)\n}\n\n\
@test(fails)\nfn expects_to_trap() -> Result[Int, Str] {\n    let zero = 0;\n    Result[Int, Str].Ok(value: 5 / zero)\n}\n\n\
@test(fails)\nfn wrongly_expects_to_fail() -> Result[Int, Str] {\n    Result[Int, Str].Ok(value: 0)\n}\n\n\
@test\nfn holds_authority(out: OutCap) -> Result[Int, Str] {\n    print(\"from a test\\n\");\n    assert_some(\"a value\", Option[Int].Some(value: 1))?;\n    assert_ok(\"ok\", Result[Int, Str].Ok(value: 2))\n}\n\n\
@test\nfn a_property() -> Result[Int, Str] {\n    property(UInt64(7), 50, fn(r: Rng) -> Result[Int, Str] {\n        let n = random_int(r, 0, 1000);\n        assert_eq_int(\"halved\", double(n) / 2, n)\n    })\n}\n\n\
@cfg(os = \"nowhere\")\n@test\nfn never_built() -> Result[Int, Str] {\n    Result[Int, Str].Err(error: \"ran\")\n}\n";

fn json_rows(stdout: &str) -> Vec<serde_json::Value> {
    stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

#[test]
fn every_test_function_runs_by_name_in_both_implementations() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("calc.nz"), TESTS).unwrap();
    let r = nazm(d.path(), &["test", "--json", "calc.nz"]);
    assert_eq!(r.code, Some(1), "{}\n{}", r.stdout, r.stderr);
    let rows = json_rows(&r.stdout);
    let verdict = |name: &str| {
        let row = rows
            .iter()
            .find(|v| v["name"] == name)
            .unwrap_or_else(|| panic!("no row for {name}: {}", r.stdout));
        assert_eq!(row["kind"], "test", "{row}");
        assert!(row["native"].is_object(), "{row}");
        row["passed"].as_bool().unwrap()
    };
    assert!(verdict("calc::doubles"));
    assert!(!verdict("calc::reports_its_failure"));
    assert!(verdict("calc::expects_to_trap"));
    assert!(!verdict("calc::wrongly_expects_to_fail"));
    assert!(verdict("calc::holds_authority"));
    assert!(verdict("calc::a_property"));
    assert!(rows.iter().all(|v| v["name"] != "calc::never_built"));
    assert_eq!(rows.len(), 6, "{}", r.stdout);
    let failure = rows
        .iter()
        .find(|v| v["name"] == "calc::reports_its_failure")
        .unwrap();
    for leg in ["interpreted", "native"] {
        let out = failure[leg]["output"].as_str().unwrap();
        assert!(
            out.contains("double(3): expected 7, found 6"),
            "{leg}: {out}"
        );
    }
}

#[test]
fn a_filter_narrows_and_the_human_report_names_each_test() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("calc.nz"), TESTS).unwrap();
    let r = nazm(
        d.path(),
        &["test", "--interpret-only", "--filter", "doubles", "calc.nz"],
    );
    assert_eq!(r.code, Some(0), "{}\n{}", r.stdout, r.stderr);
    assert!(r.stdout.contains("ok    calc::doubles"), "{}", r.stdout);
    assert!(!r.stdout.contains("reports_its_failure"), "{}", r.stdout);
    assert!(r.stderr.contains("1 of 1 passed"), "{}", r.stderr);
    let none = nazm(
        d.path(),
        &["test", "--filter", "nothing-matches", "calc.nz"],
    );
    assert_ne!(none.code, Some(0), "{}", none.stderr);
}

#[test]
fn a_fuzz_target_gets_the_same_inputs_every_run() {
    let src = "@fuzz\nfn short_inputs(input: Bytes) -> Result[Int, Str] {\n    \
               if bytes_len(input) < 40 { Result[Int, Str].Ok(value: 0) } else { Result[Int, Str].Err(error: int_to_str(bytes_len(input))) }\n}\n\n\
               @fuzz\nfn any_input(input: Bytes) -> Result[Int, Str] {\n    Result[Int, Str].Ok(value: bytes_len(input))\n}\n";
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("f.nz"), src).unwrap();
    let r = nazm(d.path(), &["test", "--json", "--fuzz", "200", "f.nz"]);
    let rows = json_rows(&r.stdout);
    assert_eq!(rows.len(), 2, "{}\n{}", r.stdout, r.stderr);
    let short = rows
        .iter()
        .find(|v| v["name"] == "f::short_inputs")
        .unwrap();
    assert_eq!(short["kind"], "fuzz");
    assert_eq!(short["passed"], false);
    let first = short["interpreted"]["output"].as_str().unwrap().to_owned();
    assert!(first.starts_with("case "), "{first}");
    assert_eq!(short["native"]["output"].as_str().unwrap(), first);
    let any = rows.iter().find(|v| v["name"] == "f::any_input").unwrap();
    assert_eq!(any["passed"], true, "{any}");
    let again = json_rows(&nazm(d.path(), &["test", "--json", "--fuzz", "200", "f.nz"]).stdout);
    let again = again
        .iter()
        .find(|v| v["name"] == "f::short_inputs")
        .unwrap();
    assert_eq!(again["interpreted"]["output"].as_str().unwrap(), first);
}

#[test]
fn a_test_or_benchmark_of_the_wrong_shape_is_refused_by_name() {
    for (src, why) in [
        ("@test\nfn t() -> Int { 0 }\nfn main() -> Int { 0 }\n", "a test answers Int"),
        ("@test\nfn t(n: Int) -> Result[Int, Str] { Result[Int, Str].Ok(value: n) }\nfn main() -> Int { 0 }\n", "a test takes an Int"),
        ("@test\nfn t[T]() -> Result[Int, Str] { Result[Int, Str].Ok(value: 0) }\nfn main() -> Int { 0 }\n", "a generic test"),
        ("@fuzz\nfn f(n: Int) -> Result[Int, Str] { Result[Int, Str].Ok(value: n) }\nfn main() -> Int { 0 }\n", "a fuzz target without Bytes"),
        ("@bench\nfn b() -> Str { \"\" }\nfn main() -> Int { 0 }\n", "a benchmark answers Str"),
        ("@test\nfn main() -> Int { 0 }\n", "main as a test"),
    ] {
        assert_eq!(codes(src), ["N0619"], "{why}");
    }
}

#[test]
fn a_test_files_diagnostic_lands_where_it_is_written_and_is_no_expected_failure() {
    // `@test(fails)` too: a program that does not check is not a test that failed.
    let src = "fn main() -> Int { 0 }\n\n@test(fails)\nfn t() -> Result[Int, Str] {\n    Result[Int, Str].Ok(value: \"no\")\n}\n";
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("t.nz"), src).unwrap();
    // The interpreter's leg alone: a native build that fails would fail the case by itself.
    let r = nazm(d.path(), &["test", "--json", "--interpret-only", "t.nz"]);
    let rows = json_rows(&r.stdout);
    let out = rows[0]["interpreted"]["output"].as_str().unwrap();
    assert!(out.contains("t.nz:5:"), "{out}");
    assert_eq!(rows[0]["passed"], false, "{}", rows[0]);
}

#[test]
fn a_benchmark_is_measured_natively_and_reported_per_iteration() {
    let src = "fn fib(n: Int) -> Int { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }\n\n\
               @bench\nfn fib_fifteen() -> Int { fib(15) }\n\nfn main() -> Int { 0 }\n";
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("b.nz"), src).unwrap();
    let r = nazm(d.path(), &["bench", "--json", "--iterations", "50", "b.nz"]);
    assert_eq!(r.code, Some(0), "{}\n{}", r.stdout, r.stderr);
    let rows = json_rows(&r.stdout);
    assert_eq!(rows.len(), 1, "{}", r.stdout);
    let row = &rows[0];
    assert_eq!(row["schema"], "nazm.bench/1");
    assert_eq!(row["name"], "b::fib_fifteen");
    assert_eq!(row["iterations"], 50);
    assert!(row["ns_per_iteration"].as_u64().unwrap() > 0, "{row}");
    let human = nazm(d.path(), &["bench", "--iterations", "5", "b.nz"]);
    assert!(human.stdout.contains("b::fib_fifteen"), "{}", human.stdout);
}
