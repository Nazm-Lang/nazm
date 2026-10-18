//! Gate 2: floating point, fixed-width integers, bit operations and conversions.
//!
//! `docs/general-purpose.md` §1–§3 and the spec's *Numbers* sections. What only a whole run
//! shows: that every numeric operation means the same in the interpreter and in both native
//! backends at both optimisation levels — the same text, digit for digit, the same traps with
//! the same codes — and that every misuse is refused before anything runs. The interpreter's
//! answers come from `nazm_sema::numeric`; every expected line below was worked out from the
//! specification, not copied from a run.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::{run_bounded, Ran};

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

/// The program under the interpreter, and built and run by each backend at each level.
fn everywhere(src: &str) -> Vec<(String, Ran)> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let mut out = vec![(
        "interpreter".to_owned(),
        run_bounded(
            Command::new(NAZM)
                .current_dir(d.path())
                .args(["run", "--interpret", "prog.nz"]),
            "nazm run",
        ),
    )];
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = d
            .path()
            .join(crate::common::exe_name(label.replace([' ', '-'], "")));
        let b = nazm(
            d.path(),
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        let mut c = common::bounded_exe(&exe);
        out.push((label.to_owned(), run_bounded(&mut c, label)));
    }
    out
}

/// Every implementation prints exactly `expected` — then `main`'s result, `0` — and exits 0.
fn agrees(body: &str, expected: &str) {
    let expected = format!("{expected}0\n");
    let src = format!(
        "fn show(out: OutCap, s: Str) -> Int ! {{io}} {{ print(str_concat(s, \"\\n\")) }}\n\
         fn main(out: OutCap) -> Int ! {{io}} {{\n{body}\n0\n}}\n"
    );
    for (label, r) in everywhere(&src) {
        assert_eq!(r.code, Some(0), "{label}: {}\n{src}", r.stderr);
        assert_eq!(r.stdout, expected, "{label}\n{src}");
    }
}

/// Every implementation stops with exit status 2 and `code`.
fn traps(body: &str, code: &str) {
    let src = format!("fn main(out: OutCap) -> Int ! {{io}} {{\n{body}\n}}\n");
    for (label, r) in everywhere(&src) {
        assert_eq!(r.code, Some(2), "{label}: {r:?}\n{src}");
        assert!(r.stderr.contains(code), "{label}: {}\n{src}", r.stderr);
    }
}

/// The codes `nazm check` refuses a program with.
fn refused(src: &str) -> Vec<String> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let r = nazm(d.path(), &["check", "--json", "prog.nz"]);
    assert_eq!(r.code, Some(1), "expected a refusal: {r:?}\n{src}");
    r.stderr
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|d| d["severity"] == "error")
        .filter_map(|d| d["code"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn floats_are_ieee_754_and_written_by_their_shortest_digits() {
    agrees(
        r#"
show(out, num_to_str(0.1 + 0.2));
show(out, num_to_str(1.5 * 4.0));
show(out, num_to_str(100.0));
show(out, num_to_str(1e21));
show(out, num_to_str(1e20));
show(out, num_to_str(0.000001));
show(out, num_to_str(1.5e-7));
show(out, num_to_str(-2.5));
show(out, num_to_str(1.0 / 0.0));
show(out, num_to_str(-1.0 / 0.0));
show(out, num_to_str(0.0 / 0.0));
show(out, num_to_str(-0.0));
show(out, num_to_str(1.7976931348623157e308));
show(out, num_to_str(5e-324));
show(out, num_to_str(7.0 % 2.5));
show(out, num_to_str(float_sqrt(2.0)));
show(out, num_to_str(Float32(0.1)));
show(out, num_to_str(Float32(16777217.0)));
show(out, num_to_str(Float32(0.1) + Float32(0.2)));
"#,
        "0.30000000000000004\n6\n100\n1e+21\n100000000000000000000\n0.000001\n1.5e-7\n-2.5\ninf\n-inf\nnan\n-0\n1.7976931348623157e+308\n5e-324\n2\n1.4142135623730951\n0.1\n16777216\n0.3\n",
    );
}

#[test]
fn float_comparison_is_ieee_754_a_nan_equals_nothing() {
    agrees(
        r#"
let nan = 0.0 / 0.0;
if nan == nan { show(out, "eq"); } else { show(out, "ne"); };
if nan != nan { show(out, "ne"); } else { show(out, "eq"); };
if nan < 1.0 || nan >= 1.0 { show(out, "ordered"); } else { show(out, "unordered"); };
if -0.0 == 0.0 { show(out, "zeros equal"); } else { show(out, "zeros differ"); };
if float_is_nan(nan) && float_is_infinite(1.0 / 0.0) && float_is_finite(2.0) { show(out, "classified"); } else { show(out, "no"); };
show(out, num_to_str(float_min(nan, 3.0)));
show(out, num_to_str(float_max(2.0, nan)));
"#,
        "ne\nne\nunordered\nzeros equal\nclassified\n3\n2\n",
    );
}

#[test]
fn float_functions_round_and_convert_as_specified() {
    agrees(
        r#"
show(out, num_to_str(float_floor(-2.5)));
show(out, num_to_str(float_ceil(-2.5)));
show(out, num_to_str(float_trunc(-2.5)));
show(out, num_to_str(float_round(-2.5)));
show(out, num_to_str(float_round(2.5)));
show(out, num_to_str(float_abs(-3.25)));
show(out, num_to_str(Float64(9007199254740993)));
show(out, num_to_str(Int(-7.9)));
show(out, num_to_str(saturate_convert[Int8](1000.0)));
show(out, num_to_str(saturate_convert[UInt8](-5.0)));
show(out, num_to_str(saturate_convert[Int32](0.0 / 0.0)));
show(out, num_to_str(float64_to_bits(1.0)));
show(out, num_to_str(float64_from_bits(float64_to_bits(0.5))));
show(out, num_to_str(float32_to_bits(Float32(1.0))));
show(out, num_to_str(float_parse_raw("2.5e3")));
if float_parses("1.") && float_parses(".5") && !float_parses("1e") && !float_parses(" 1") { show(out, "parsing"); } else { show(out, "no"); };
"#,
        "-3\n-2\n-2\n-3\n3\n3.25\n9007199254740992\n-7\n127\n0\n0\n4607182418800017408\n0.5\n1065353216\n2500\nparsing\n",
    );
}

#[test]
fn every_integer_width_has_its_own_range_and_signedness() {
    agrees(
        r#"
show(out, num_to_str(UInt8(200) + UInt8(55)));
show(out, num_to_str(Int8(-128) / Int8(2)));
show(out, num_to_str(Int8(-128) % Int8(-1)));
show(out, num_to_str(UInt16(65535) / UInt16(256)));
show(out, num_to_str(Int16(-7) % Int16(3)));
show(out, num_to_str(UInt32(4000000000) / UInt32(3)));
show(out, num_to_str(Int32(-2147483648) + Int32(1)));
show(out, num_to_str(UInt64(18446744073709551615)));
show(out, num_to_str(UInt64(18446744073709551615) / UInt64(2)));
if UInt64(18446744073709551615) > UInt64(1) { show(out, "unsigned order"); } else { show(out, "signed order"); };
if UInt8(200) > UInt8(100) { show(out, "u8 order"); } else { show(out, "wrong"); };
show(out, num_to_str(-Int32(5)));
"#,
        "255\n-64\n0\n255\n-1\n1333333333\n-2147483647\n18446744073709551615\n9223372036854775807\nunsigned order\nu8 order\n-5\n",
    );
}

#[test]
fn wrapping_and_saturating_arithmetic_never_trap() {
    agrees(
        r#"
show(out, num_to_str(wrapping_add(UInt8(250), UInt8(10))));
show(out, num_to_str(wrapping_sub(UInt8(3), UInt8(10))));
show(out, num_to_str(wrapping_mul(Int8(64), Int8(2))));
show(out, num_to_str(wrapping_add(9223372036854775807, 1)));
show(out, num_to_str(saturating_add(UInt8(250), UInt8(10))));
show(out, num_to_str(saturating_sub(UInt8(3), UInt8(10))));
show(out, num_to_str(saturating_add(Int8(100), Int8(100))));
show(out, num_to_str(saturating_sub(Int8(-100), Int8(100))));
show(out, num_to_str(saturating_mul(Int16(300), Int16(-300))));
show(out, num_to_str(saturating_mul(UInt32(70000), UInt32(70000))));
"#,
        "4\n249\n-128\n-9223372036854775808\n255\n0\n127\n-128\n-32768\n4294967295\n",
    );
}

#[test]
fn bit_operations_shifts_rotations_and_counts() {
    agrees(
        r#"
let m = UInt32(0xFF00_FF00);
show(out, num_to_str((m >> 8) & 0xFF));
show(out, num_to_str(~m));
show(out, num_to_str(m | 0xF));
show(out, num_to_str(m ^ m));
show(out, num_to_str(Int8(-128) >> 7));
show(out, num_to_str(UInt8(128) >> 7));
show(out, num_to_str(UInt8(0b1000_0001) << 1));
show(out, num_to_str(1 << 62));
show(out, num_to_str(~0));
show(out, num_to_str(6 & 3 | 8 ^ 1));
if 12 & 4 == 4 { show(out, "precedence"); } else { show(out, "c precedence"); };
show(out, num_to_str(rotate_left(UInt8(0b1000_0001), 1)));
show(out, num_to_str(rotate_right(UInt16(1), 1)));
show(out, num_to_str(rotate_left(UInt8(1), 9)));
show(out, num_to_str(count_ones(m)));
show(out, num_to_str(leading_zeros(UInt16(1))));
show(out, num_to_str(trailing_zeros(UInt32(0))));
show(out, num_to_str(trailing_zeros(Int8(-128))));
show(out, num_to_str(swap_bytes(UInt16(0x1234))));
show(out, num_to_str(to_big_endian(UInt32(1))));
show(out, num_to_str(from_big_endian(to_big_endian(UInt32(305419896)))));
show(out, num_to_str(to_little_endian(UInt32(7))));
show(out, num_to_str(from_little_endian(UInt64(9))));
show(out, num_to_str(0x7fff_ffff_ffff_fff0 + 0o7 - 0b111));
"#,
        "255\n16711935\n4278255375\n0\n-1\n1\n2\n4611686018427387904\n-1\n11\nprecedence\n3\n32768\n2\n16\n15\n32\n7\n13330\n16777216\n305419896\n7\n9\n9223372036854775792\n",
    );
}

#[test]
fn conversions_are_exact_wrapping_saturating_or_tried() {
    agrees(
        r#"
show(out, num_to_str(wrap_convert[UInt8](300)));
show(out, num_to_str(wrap_convert[Int8](UInt8(255))));
show(out, num_to_str(saturate_convert[UInt8](300)));
show(out, num_to_str(saturate_convert[Int16](-70000)));
show(out, num_to_str(saturate_convert[UInt64](-1)));
show(out, num_to_str(saturate_convert[Int](UInt64(18446744073709551615))));
show(out, num_to_str(UInt64(Int8(5))));
show(out, num_to_str(Int(UInt32(4294967295))));
show(out, num_to_str(Float32(16777217)));
match try_convert[UInt8](300) { Option.Some(value: v) => show(out, "some"), Option.None() => show(out, "none"), };
match try_convert[UInt8](200) { Option.Some(value: v) => show(out, num_to_str(v)), Option.None() => show(out, "none"), };
match try_convert[Int32](3.9) { Option.Some(value: v) => show(out, num_to_str(v)), Option.None() => show(out, "none"), };
match try_convert[Int32](0.0 / 0.0) { Option.Some(value: v) => show(out, "some"), Option.None() => show(out, "nan none"), };
match try_convert[Int8](-128.9) { Option.Some(value: v) => show(out, num_to_str(v)), Option.None() => show(out, "none"), };
match try_convert[UInt8](-0.5) { Option.Some(value: v) => show(out, num_to_str(v)), Option.None() => show(out, "none"), };
match try_convert[Int](9223372036854775807.0) { Option.Some(value: v) => show(out, "some"), Option.None() => show(out, "2^63 none"), };
"#,
        "44\n-1\n255\n-32768\n0\n9223372036854775807\n5\n4294967295\n16777216\nnone\n200\n3\nnan none\n-128\n0\n2^63 none\n",
    );
}

#[test]
fn numbers_live_in_records_vectors_and_tasks() {
    let src = r#"
struct Point { x: Float64, y: Float64, tag: UInt8, }

fn norm(p: Point) -> Float64 { float_sqrt(p.x * p.x + p.y * p.y) }

fn worker(c: Chan[Float64], x: Float64) -> Int ! {spawn} { chan_send_of(c, x * 2.0); 0 }

fn main(out: OutCap, s: SpawnCap) -> Int ! {io, spawn} {
    let p = Point(x: 3.0, y: 4.0, tag: UInt8(7));
    let q = Point(x: 3.0, y: 4.0, tag: UInt8(7));
    print(str_concat(num_to_str(norm(p)), "\n"));
    if p == q { print("equal\n"); } else { print("differ\n"); };
    let xs = vec_new[Int16]();
    vec_push(xs, Int16(-3));
    vec_push(xs, 1000);
    print(str_concat(num_to_str(vec_get(xs, 0) * vec_get(xs, 1)), "\n"));
    let c = chan_new_of[Float64](1);
    scope { spawn worker(c, 1.25); }
    let got = vec_new[Float64]();
    chan_recv_of(c, got);
    print(str_concat(num_to_str(vec_get(got, 0)), "\n"));
    0
}
"#;
    for (label, r) in everywhere(src) {
        assert_eq!(r.code, Some(0), "{label}: {}", r.stderr);
        assert_eq!(r.stdout, "5\nequal\n-3000\n2.5\n0\n", "{label}");
    }
}

#[test]
fn every_integer_failure_traps_with_its_code_everywhere() {
    traps("let x = UInt8(255) + UInt8(1); 0", "N0400");
    traps("let x = UInt8(0) - UInt8(1); 0", "N0400");
    traps("let x = Int8(-128) * Int8(-1); 0", "N0400");
    traps("let x = Int16(-32768) / Int16(-1); 0", "N0400");
    traps("let x = -UInt32(1); 0", "N0400");
    traps(
        "let x = UInt64(18446744073709551615) + UInt64(1); 0",
        "N0400",
    );
    traps("let z = UInt32(0); let x = UInt32(5) / z; 0", "N0401");
    traps("let z = Int8(0); let x = Int8(5) % z; 0", "N0401");
    traps("let n = 8; let x = UInt8(1) << n; 0", "N0412");
    traps("let n = -1; let x = 1 >> n; 0", "N0412");
    traps("let n = -1; let x = rotate_left(UInt8(1), n); 0", "N0412");
    traps("let v = 300; let x = UInt8(v); 0", "N0400");
    traps("let v = -1; let x = UInt64(v); 0", "N0400");
    traps("let f = 1e20; let x = Int(f); 0", "N0400");
    traps("let f = 0.0 / 0.0; let x = Int32(f); 0", "N0400");
}

#[test]
fn numeric_misuse_is_refused_before_anything_runs() {
    let main = |body: &str| format!("fn main() -> Int {{ {body} }}\n");
    // No implicit conversion between numeric types.
    assert_eq!(refused(&main("let x = UInt8(1) + 1.5; 0")), ["N0304"]);
    assert_eq!(
        refused(&main("let x = Int32(1) + 1; let y = x + UInt32(1); 0")),
        ["N0304"]
    );
    assert_eq!(refused(&main("let x = 1.5 & 2.5; 0")), ["N0304"]);
    // A literal out of the range it is written at.
    assert_eq!(refused(&main("let x = UInt8(256); 0")), ["N0003"]);
    assert_eq!(refused(&main("let x = Int8(-129); 0")), ["N0003"]);
    assert_eq!(refused(&main("let x = 18446744073709551615; 0")), ["N0003"]);
    // A constant shift out of range.
    assert_eq!(refused(&main("let x = UInt8(1) << 8; 0")), ["N0617"]);
    assert_eq!(refused(&main("let x = 1 >> -1; 0")), ["N0617"]);
    // A built-in outside its class.
    assert_eq!(refused(&main("let x = count_ones(1.5); 0")), ["N0300"]);
    assert_eq!(refused(&main("let x = float_sqrt(4); 0")), ["N0300"]);
    // Conversions say what they convert to, and wrapping is between integers.
    assert_eq!(refused(&main("let x = try_convert(5); 0")), ["N0354"]);
    assert_eq!(
        refused(&main("let x = wrap_convert[UInt8](1.5); 0")),
        ["N0300"]
    );
    assert_eq!(refused(&main("let x = UInt8(\"a\"); 0")), ["N0300"]);
    // The conversion names are reserved, as built-ins' are.
    assert_eq!(
        refused("fn try_convert(x: Int) -> Int { x }\nfn main() -> Int { 0 }\n"),
        ["N0203"]
    );
}

/// The C library's functions may differ in the last place between hosts (`docs/general-purpose.md`
/// §1), so each is compared rounded to six decimal places — which every conforming library
/// agrees on — rather than digit for digit.
#[test]
fn the_c_librarys_functions_agree_to_six_places() {
    agrees(
        r#"
show(out, num_to_str(float_round(float_sin(1.0) * 1e6)));
show(out, num_to_str(float_round(float_cos(1.0) * 1e6)));
show(out, num_to_str(float_round(float_tan(1.0) * 1e6)));
show(out, num_to_str(float_round(float_exp(1.0) * 1e6)));
show(out, num_to_str(float_round(float_ln(10.0) * 1e6)));
show(out, num_to_str(float_round(float_log2(10.0) * 1e6)));
show(out, num_to_str(float_round(float_log10(2.0) * 1e6)));
show(out, num_to_str(float_round(float_pow(2.0, 0.5) * 1e6)));
show(out, num_to_str(float_round(float_atan2(1.0, 1.0) * 1e6)));
show(out, num_to_str(float_round(float_sin(Float32(1.0)) * Float32(1e4))));
show(out, num_to_str(float32_from_bits(float32_to_bits(Float32(2.5)))));
"#,
        "841471\n540302\n1557408\n2718282\n2302585\n3321928\n301030\n1414214\n785398\n8415\n2.5\n",
    );
}
