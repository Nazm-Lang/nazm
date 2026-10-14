//! R1b, L13b: `Int128`, `UInt128` and `widening_mul` (`docs/spec.md`, *Numbers*).
//!
//! Every operation at 128 bits — arithmetic at each bound, shifts, rotations, bit counts, the
//! conversions by every mode, comparisons past `i128`, a `Vec` and an `Option` of one, `Bytes` at
//! sixteen — the same under the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift, with
//! nothing live; every failure with its code everywhere; derived hashes and a map keyed by one; a
//! constant across modules; and what is refused — a literal past `UInt128`, a C export of one, and
//! a WebAssembly build that reaches the runtime's wide part.

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

/// `body` inside a `main` that can print, under every implementation, each with the memory
/// report asked for.
fn everywhere(header: &str, body: &str) -> Vec<(String, Ran)> {
    let src = format!(
        "{header}fn show(out: OutCap, s: Str) -> Int ! {{io}} {{ print(str_concat(s, \"\\n\")) }}\n\
         fn main(out: OutCap) -> Int ! {{io}} {{\n{body}\n0\n}}\n"
    );
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let mut out = Vec::new();
    for (label, args) in [
        ("interpreter", &["run", "prog.nz"][..]),
        ("lir oracle", &["lir", "--run", "prog.nz"][..]),
    ] {
        out.push((
            label.to_owned(),
            run_bounded(
                Command::new(NAZM)
                    .current_dir(d.path())
                    .args(args)
                    .env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
        ));
    }
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = d.path().join(label.replace([' ', '-'], ""));
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
        out.push((
            label.to_owned(),
            run_bounded(
                common::bounded_exe(&exe).env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
        ));
    }
    out
}

/// `body` prints `expected`, one line each, everywhere, and leaves nothing live where a memory
/// report is written.
fn agrees(header: &str, body: &str, expected: &[&str]) {
    let want = format!("{}\n0\n", expected.join("\n"));
    for (label, r) in everywhere(header, body) {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert_eq!(r.stdout, want, "{label}");
        if label != "lir oracle" {
            let line = r
                .stderr
                .lines()
                .find(|l| l.starts_with("nazm-memory:"))
                .unwrap_or_else(|| panic!("{label}: no memory report: {r:?}"));
            assert!(
                line.split_whitespace()
                    .filter(|w| w.starts_with("live="))
                    .all(|w| w == "live=0"),
                "{label}: {line}"
            );
        }
    }
}

#[test]
fn every_operation_at_128_bits_agrees_everywhere() {
    agrees(
        "",
        "let mx = ~UInt128(0);
let imax = Int128(mx >> 1);
let imin = -imax - Int128(1);
show(out, num_to_str(mx));
show(out, num_to_str(imax));
show(out, num_to_str(imin));
show(out, num_to_str(Int128(9223372036854775807) * Int128(1000) / Int128(7)));
show(out, num_to_str(UInt128(18446744073709551615) * UInt128(18446744073709551615)));
show(out, num_to_str(Int128(-3) * Int128(5)));
show(out, num_to_str(Int128(-85070591730234615865843651857942052864) * Int128(2)));
show(out, num_to_str(imin % Int128(-1)));
show(out, num_to_str(Int128(-7) % Int128(3)));
show(out, num_to_str(imin >> 127));
show(out, num_to_str(UInt128(1) << 127));
show(out, num_to_str(rotate_left(UInt128(3), 127)));
show(out, num_to_str(swap_bytes(UInt128(1))));
show(out, num_to_str(leading_zeros(UInt128(1))));
show(out, num_to_str(trailing_zeros(UInt128(0))));
show(out, num_to_str(count_ones(imin)));
show(out, num_to_str(saturating_add(mx, UInt128(5))));
show(out, num_to_str(saturating_mul(imin, Int128(2))));
show(out, num_to_str(saturating_sub(Int128(0), imin)));
show(out, num_to_str(wrapping_mul(mx, mx)));
show(out, num_to_str(wrapping_add(imax, Int128(1))));
show(out, num_to_str(saturate_convert[Int](mx)));
show(out, num_to_str(saturate_convert[Int128](mx)));
show(out, num_to_str(saturate_convert[UInt128](imin)));
show(out, num_to_str(wrap_convert[Int](mx)));
show(out, num_to_str(wrap_convert[Int128](mx)));
show(out, num_to_str(saturate_convert[Int128](1.0e40)));
show(out, num_to_str(saturate_convert[UInt128](-5.0)));
show(out, num_to_str(saturate_convert[UInt128](0.0 / 0.0)));
show(out, num_to_str(Int128(1.0e30)));
show(out, num_to_str(Float32(mx)));
show(out, num_to_str(Float64(imin)));
show(out, num_to_str(UInt128(UInt64(18446744073709551615))));
if mx - UInt128(1) > UInt128(imax) { show(out, \"unsigned\"); } else { show(out, \"wrong\"); }
if imin < Int128(0) { show(out, \"signed\"); } else { show(out, \"wrong\"); }
match try_convert[Int](imax) { Option.Some(value: v) => show(out, \"wrong\"), Option.None() => show(out, \"none\"), };
match checked_mul(imax, Int128(2)) { Option.Some(value: v) => show(out, \"wrong\"), Option.None() => show(out, \"none\"), };
let xs = vec_new[UInt128]();
vec_push(xs, mx);
vec_push(xs, UInt128(7));
show(out, num_to_str(vec_get(xs, 0) - vec_get(xs, 1)));
show(out, num_to_str(widening_mul(UInt64(18446744073709551615), UInt64(18446744073709551615))));
show(out, num_to_str(widening_mul(UInt64(4294967296), UInt64(4294967296)) >> 64));",
        &[
            "340282366920938463463374607431768211455",
            "170141183460469231731687303715884105727",
            "-170141183460469231731687303715884105728",
            "1317624576693539401000",
            "340282366920938463426481119284349108225",
            "-15",
            "-170141183460469231731687303715884105728",
            "0",
            "-1",
            "-1",
            "170141183460469231731687303715884105728",
            "170141183460469231731687303715884105729",
            "1329227995784915872903807060280344576",
            "127",
            "128",
            "1",
            "340282366920938463463374607431768211455",
            "-170141183460469231731687303715884105728",
            "170141183460469231731687303715884105727",
            "1",
            "-170141183460469231731687303715884105728",
            "9223372036854775807",
            "170141183460469231731687303715884105727",
            "0",
            "-1",
            "-1",
            "170141183460469231731687303715884105727",
            "0",
            "0",
            "1000000000000000019884624838656",
            "inf",
            "-1.7014118346046923e+38",
            "18446744073709551615",
            "unsigned",
            "signed",
            "none",
            "none",
            "340282366920938463463374607431768211448",
            "340282366920938463426481119284349108225",
            "1",
        ],
    );
}

#[test]
fn literals_and_bytes_reach_the_whole_width() {
    agrees(
        "",
        "show(out, num_to_str(UInt128(340282366920938463463374607431768211455)));
show(out, num_to_str(Int128(-170141183460469231731687303715884105728)));
let b = bytes_new(32);
bytes_write_le(b, 0, UInt128(1) << 100 | UInt128(258));
bytes_write_be(b, 16, Int128(-2));
show(out, num_to_str(bytes_read_le[UInt128](b, 0)));
show(out, num_to_str(bytes_read_be[Int128](b, 16)));
show(out, num_to_str(bytes_read_le[UInt64](b, 0)));
show(out, num_to_str(bytes_get(b, 31)));",
        &[
            "340282366920938463463374607431768211455",
            "-170141183460469231731687303715884105728",
            "1267650600228229401496703205634",
            "-2",
            "258",
            "254",
        ],
    );
}

#[test]
fn every_128_bit_failure_traps_with_its_code_everywhere() {
    for (body, code, what) in [
        (
            "show(out, num_to_str(~UInt128(0) + UInt128(1)));",
            "N0400",
            "addition overflows `UInt128`",
        ),
        (
            "show(out, num_to_str(Int128(~UInt128(0) >> 1) * Int128(2)));",
            "N0400",
            "multiplication overflows `Int128`",
        ),
        (
            "show(out, num_to_str(~UInt128(0) * UInt128(2)));",
            "N0400",
            "multiplication overflows `UInt128`",
        ),
        (
            "let m = -Int128(~UInt128(0) >> 1) - Int128(1); show(out, num_to_str(m / Int128(-1)));",
            "N0400",
            "division overflows `Int128`",
        ),
        (
            "show(out, num_to_str(Int128(5) % Int128(0)));",
            "N0401",
            "division by zero",
        ),
        (
            "show(out, num_to_str(-UInt128(1)));",
            "N0400",
            "negation overflows `UInt128`",
        ),
        (
            "show(out, num_to_str(Int(~UInt128(0))));",
            "N0400",
            "conversion overflows `Int`",
        ),
        (
            "show(out, num_to_str(Int128(1.0e39)));",
            "N0400",
            "conversion overflows `Int128`",
        ),
        (
            "show(out, num_to_str(UInt128(0.0 / 0.0)));",
            "N0400",
            "conversion overflows `UInt128`",
        ),
    ] {
        for (label, r) in everywhere("", body) {
            assert_eq!(r.code, Some(2), "{label}: {body}: {r:?}");
            assert!(r.stdout.is_empty(), "{label}: {body}: {r:?}");
            assert!(
                r.stderr.contains(&format!("error[{code}]")) && r.stderr.contains(what),
                "{label}: {body}: {r:?}"
            );
        }
    }
}

#[test]
fn a_128_bit_key_hashes_as_its_two_words_combined() {
    agrees(
        "use \"@std/hash\";\nuse \"@std/hashmap\";\nstruct P { a: UInt128, b: Int128, }\n",
        "let big = ~UInt128(0);
let m = hashmap_new[UInt128, Int]();
hashmap_set(m, big, 1);
hashmap_set(m, UInt128(5), 2);
hashmap_set(m, big, 3);
hashmap_set(m, big - UInt128(1), 4);
show(out, num_to_str(hashmap_len(m)));
let w = hash_combine(hash_u64(UInt64(18446744073709551615)), hash_u64(UInt64(18446744073709551615)));
if Hash.hash(big) == w { show(out, \"words\"); } else { show(out, \"wrong\"); }
if Hash.hash(Int128(-1)) == w { show(out, \"signed words\"); } else { show(out, \"wrong\"); }
let p = P(a: UInt128(1) << 100, b: Int128(-3));
let derived = hash_combine(hash_combine(hash_u64(UInt64(0)), Hash.hash(p.a)), Hash.hash(p.b));
if Hash.hash(p) == derived { show(out, \"derived\"); } else { show(out, \"wrong\"); }
if p == P(a: UInt128(1) << 100, b: Int128(-3)) { show(out, \"equal\"); } else { show(out, \"wrong\"); }
if p < P(a: UInt128(1) << 100, b: Int128(-2)) { show(out, \"ordered\"); } else { show(out, \"wrong\"); }",
        &["3", "words", "signed words", "derived", "equal", "ordered"],
    );
}

#[test]
fn a_128_bit_constant_crosses_modules_and_the_cache() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("lib.nz"),
        "pub const BIG: UInt128 = 340282366920938463463374607431768211455;\n\
         pub const LOW: Int128 = -170141183460469231731687303715884105728;\n\
         pub const SMALL: Int8 = -5;\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join("main.nz"),
        "use \"lib.nz\";\n\
         fn show(out: OutCap, s: Str) -> Int ! {io} { print(str_concat(s, \"\\n\")) }\n\
         fn main(out: OutCap) -> Int ! {io} {\n\
         show(out, num_to_str(BIG));\nshow(out, num_to_str(LOW));\nshow(out, num_to_str(SMALL));\n0\n}\n",
    )
    .unwrap();
    let want = "340282366920938463463374607431768211455\n\
                -170141183460469231731687303715884105728\n-5\n0\n";
    // Twice, then built. `nazm run` keeps no check cache; the interface's copy of each value is
    // read back by `durable_identity::a_128_bit_constant_reads_back_from_its_interface_whole`.
    for _ in 0..2 {
        let r = nazm(d.path(), &["run", "main.nz"]);
        assert_eq!(r.stdout, want, "{r:?}");
    }
    let b = nazm(d.path(), &["build", "main.nz", "-o", "m"]);
    assert_eq!(b.code, Some(0), "{b:?}");
    let r = run_bounded(&mut common::bounded_exe(&d.path().join("m")), "m");
    assert_eq!(r.stdout, want, "{r:?}");
}

#[test]
fn what_128_bits_cannot_do_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let check = |src: &str| {
        std::fs::write(d.path().join("r.nz"), src).unwrap();
        nazm(d.path(), &["check", "r.nz"])
    };
    // One past `UInt128`.
    let r =
        check("fn main() -> Int { let x = UInt128(340282366920938463463374607431768211456); 0 }\n");
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(r.stderr.contains("N0003"), "{r:?}");
    // A negated literal past `Int`'s magnitude, where an `Int` is expected.
    let r = check("fn main() -> Int { -9223372036854775809 }\n");
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(r.stderr.contains("N0003"), "{r:?}");
    // C has no standard 128-bit type.
    let r = check(
        "pub extern \"C\" fn twice(x: Int128) -> Int128 ! {} = \"twice\" { x * Int128(2) }\n\
         fn main() -> Int { 0 }\n",
    );
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(r.stderr.contains("N0391"), "{r:?}");
    // WebAssembly has no wide part.
    std::fs::write(
        d.path().join("w.nz"),
        "fn main() -> Int { Int(Int128(42) / Int128(2)) }\n",
    )
    .unwrap();
    let r = nazm(
        d.path(),
        &[
            "build",
            "--target",
            "wasm32-wasip1",
            "w.nz",
            "--objects",
            "objs",
        ],
    );
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(r.stderr.contains("128-bit integers"), "{r:?}");
}

#[test]
fn a_128_bit_value_crosses_a_call_in_a_cranelift_build_for_x86_64() {
    // x86_64's System V passes an `Int128` as a register pair, which Cranelift does only with
    // LLVM's ABI extensions on; without them it refuses the signature. A cross build writes
    // the objects without a linker, so this host can show it.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn wide(a: Int128, b: Int128) -> Int128 { a + b }\n\
         fn main() { println(num_to_str(wide(Int128(1), Int128(2)))) }\n",
    )
    .unwrap();
    let b = nazm(
        d.path(),
        &[
            "build",
            "--backend",
            "cranelift",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--objects",
            "out",
            "prog.nz",
        ],
    );
    assert_eq!(b.code, Some(0), "{b:?}");
    assert!(d.path().join("out/link.txt").exists(), "{b:?}");
}
