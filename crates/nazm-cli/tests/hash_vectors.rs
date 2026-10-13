//! R1e, S1: `@std/sha256` and `@std/blake3` (`docs/spec.md`, *The standard library*).
//!
//! SHA-256 is held to FIPS 180-4's examples — `"abc"`, the empty message, the 448- and 896-bit
//! messages and a million `a`s — and to messages of `i % 251` bytes on each side of the padding's
//! boundaries (55/56/57, 63/64/65, 119/120 bytes), whose digests were computed with Python's
//! `hashlib` when this file was written. BLAKE3 is held to the BLAKE3 team's `test_vectors.json`
//! (inputs of `i % 251` bytes; the first 32 bytes of each `hash`), embedded below and checked
//! against the `blake3` crate. The short vectors, and the same data given in uneven pieces, agree
//! under the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift with nothing live; the
//! long ones run natively, under LLVM `-O2` and Cranelift, where the interpreter would take
//! minutes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{run_bounded, Ran};

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// SHA-256 of FIPS 180-4's example messages.
const SHA256_FIPS: &[(&str, &str)] = &[
    (
        "abc",
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    ),
    (
        "",
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    ),
    (
        "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
    ),
    (
        "abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
        "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
    ),
];

/// FIPS 180-4's long example: one million `a`s.
const SHA256_MILLION_A: &str = "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";

/// SHA-256 of `i % 251` for `i` below each length, around the padding's boundaries.
const SHA256_RAMP: &[(usize, &str)] = &[
    (
        55,
        "463eb28e72f82e0a96c0a4cc53690c571281131f672aa229e0d45ae59b598b59",
    ),
    (
        56,
        "da2ae4d6b36748f2a318f23e7ab1dfdf45acdc9d049bd80e59de82a60895f562",
    ),
    (
        57,
        "2fe741af801cc238602ac0ec6a7b0c3a8a87c7fc7d7f02a3fe03d1c12eac4d8f",
    ),
    (
        63,
        "29af2686fd53374a36b0846694cc342177e428d1647515f078784d69cdb9e488",
    ),
    (
        64,
        "fdeab9acf3710362bd2658cdc9a29e8f9c757fcf9811603a8c447cd1d9151108",
    ),
    (
        65,
        "4bfd2c8b6f1eec7a2afeb48b934ee4b2694182027e6d0fc075074f2fabb31781",
    ),
    (
        119,
        "da18797ed7c3a777f0847f429724a2d8cd5138e6ed2895c3fa1a6d39d18f7ec6",
    ),
    (
        120,
        "f52b23db1fbb6ded89ef42a23ce0c8922c45f25c50b568a93bf1c075420bbb7c",
    ),
    (
        1000,
        "4e4c294b331f7a2099a379bec34b9f9fc03dc46ab465d998f4d683da53487e6d",
    ),
];

/// BLAKE3's official vectors (`test_vectors/test_vectors.json`, the default hash's first 32
/// bytes) for an input of `i % 251` for `i` below each length.
const BLAKE3: &[(usize, &str)] = &[
    (
        0,
        "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262",
    ),
    (
        1,
        "2d3adedff11b61f14c886e35afa036736dcd87a74d27b5c1510225d0f592e213",
    ),
    (
        63,
        "e9bc37a594daad83be9470df7f7b3798297c3d834ce80ba85d6e207627b7db7b",
    ),
    (
        64,
        "4eed7141ea4a5cd4b788606bd23f46e212af9cacebacdc7d1f4c6dc7f2511b98",
    ),
    (
        65,
        "de1e5fa0be70df6d2be8fffd0e99ceaa8eb6e8c93a63f2d8d1c30ecb6b263dee",
    ),
    (
        1023,
        "10108970eeda3eb932baac1428c7a2163b0e924c9a9e25b35bba72b28f70bd11",
    ),
    (
        1024,
        "42214739f095a406f3fc83deb889744ac00df831c10daa55189b5d121c855af7",
    ),
    (
        1025,
        "d00278ae47eb27b34faecf67b4fe263f82d5412916c1ffd97c8cb7fb814b8444",
    ),
    (
        2048,
        "e776b6028c7cd22a4d0ba182a8bf62205d2ef576467e838ed6f2529b85fba24a",
    ),
    (
        2049,
        "5f4d72f40d7a5f82b15ca2b2e44b1de3c2ef86c426c95c1af0b6879522563030",
    ),
    (
        3072,
        "b98cb0ff3623be03326b373de6b9095218513e64f1ee2edd2525c7ad1e5cffd2",
    ),
    (
        3073,
        "7124b49501012f81cc7f11ca069ec9226cecb8a2c850cfe644e327d22d3e1cd3",
    ),
    (
        4096,
        "015094013f57a5277b59d8475c0501042c0b642e531b0a1c8f58d2163229e969",
    ),
    (
        4097,
        "9b4052b38f1c5fc8b1f9ff7ac7b27cd242487b3d890d15c96a1c25b8aa0fb995",
    ),
    (
        5120,
        "9cadc15fed8b5d854562b26a9536d9707cadeda9b143978f319ab34230535833",
    ),
    (
        5121,
        "628bd2cb2004694adaab7bbd778a25df25c47b9d4155a55f8fbd79f2fe154cff",
    ),
    (
        6144,
        "3e2e5b74e048f3add6d21faab3f83aa44d3b2278afb83b80b3c35164ebeca205",
    ),
    (
        6145,
        "f1323a8631446cc50536a9f705ee5cb619424d46887f3c376c695b70e0f0507f",
    ),
    (
        7168,
        "61da957ec2499a95d6b8023e2b0e604ec7f6b50e80a9678b89d2628e99ada77a",
    ),
    (
        7169,
        "a003fc7a51754a9b3c7fae0367ab3d782dccf28855a03d435f8cfe74605e7817",
    ),
    (
        8192,
        "aae792484c8efe4f19e2ca7d371d8c467ffb10748d8a5a1ae579948f718a2a63",
    ),
    (
        8193,
        "bab6c09cb8ce8cf459261398d2e7aef35700bf488116ceb94a36d0f5f1b7bc3b",
    ),
    (
        16384,
        "f875d6646de28985646f34ee13be9a576fd515f76b5b0a26bb324735041ddde4",
    ),
    (
        31744,
        "62b6960e1a44bcc1eb1a611a8d6235b6b4b78f32e7abc4fb4c6cdcce94895c47",
    ),
    (
        102400,
        "bc3e3d41a1146b069abffad3c0d44860cf664390afce4d9661f7902e7943e085",
    ),
];

/// The longest BLAKE3 input every implementation is asked for: past three chunks, so the
/// interpreter merges subtrees too.
const SHORT_LIMIT: usize = 3073;

/// What every program shares: the inputs, and a hasher fed in uneven pieces.
const PRELUDE: &str = r#"use "@std/sha256";
use "@std/blake3";

fn ramp(n: Int) -> Bytes {
    let b = bytes_new(n);
    for i in 0..n { bytes_set(b, i, UInt8(i % 251)); }
    b
}

fn hex(d: Bytes) -> Str {
    let digits = "0123456789abcdef";
    let out = bytes_new(2 * bytes_len(d));
    for i in 0..bytes_len(d) {
        let b = Int(bytes_get(d, i));
        bytes_set(out, 2 * i, UInt8(str_byte(digits, b >> 4)));
        bytes_set(out, 2 * i + 1, UInt8(str_byte(digits, b & 15)));
    }
    bytes_to_str(out)
}

// Pieces of 1, 63, 64, 65, 1000 and 7 bytes, over and over.
fn piece(k: Int) -> Int {
    let sizes = [1, 63, 64, 65, 1000, 7];
    sizes[k % 6]
}

fn sha256_pieces(data: Bytes) -> Str {
    let h = sha256_new();
    let mut at = 0;
    let mut k = 0;
    while at < bytes_len(data) {
        let end = if at + piece(k) < bytes_len(data) { at + piece(k) } else { bytes_len(data) };
        sha256_update(h, bytes_slice(data, at, end));
        // Finishing leaves the hasher as it was.
        sha256_finish(h);
        at = end;
        k = k + 1;
    }
    hex(sha256_finish(h))
}

fn blake3_pieces(data: Bytes) -> Str {
    let h = blake3_new();
    let mut at = 0;
    let mut k = 0;
    while at < bytes_len(data) {
        let end = if at + piece(k) < bytes_len(data) { at + piece(k) } else { bytes_len(data) };
        blake3_update(h, bytes_slice(data, at, end));
        blake3_finish(h);
        at = end;
        k = k + 1;
    }
    hex(blake3_finish(h))
}
"#;

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

/// A program printing the short vectors, and what it should print.
fn short_program() -> (String, String) {
    let mut main = String::from("fn main(io: IoCap) -> Int ! {io} {\n");
    let mut want = String::new();
    for (text, digest) in SHA256_FIPS {
        writeln!(
            main,
            "    print(f\"sha256 {{sha256_hex(bytes_from_str(\"{text}\"))}}\\n\");"
        )
        .unwrap();
        writeln!(want, "sha256 {digest}").unwrap();
    }
    for (n, digest) in SHA256_RAMP {
        writeln!(
            main,
            "    print(f\"sha256 {n} {{hex(sha256(ramp({n})))}} {{sha256_pieces(ramp({n}))}}\\n\");"
        )
        .unwrap();
        writeln!(want, "sha256 {n} {digest} {digest}").unwrap();
    }
    for (n, digest) in BLAKE3.iter().filter(|(n, _)| *n <= SHORT_LIMIT) {
        writeln!(
            main,
            "    print(f\"blake3 {n} {{blake3_hex(ramp({n}))}} {{blake3_pieces(ramp({n}))}}\\n\");"
        )
        .unwrap();
        writeln!(want, "blake3 {n} {digest} {digest}").unwrap();
    }
    main.push_str("    0\n}\n");
    want.push_str("0\n");
    (format!("{PRELUDE}\n{main}"), want)
}

/// A program printing the long vectors, and what it should print.
fn long_program() -> (String, String) {
    let mut main = String::from(
        "fn main(io: IoCap) -> Int ! {io} {
    let a = bytes_new(1000);
    for i in 0..1000 { bytes_set(a, i, UInt8(97)); }
    let h = sha256_new();
    for i in 0..1000 { sha256_update(h, a); }
    let million = bytes_new(1000000);
    for i in 0..1000000 { bytes_set(million, i, UInt8(97)); }
    print(f\"sha256 million {hex(sha256_finish(h))} {hex(sha256(million))}\\n\");
",
    );
    let mut want = format!("sha256 million {SHA256_MILLION_A} {SHA256_MILLION_A}\n");
    for (n, digest) in BLAKE3.iter().filter(|(n, _)| *n > SHORT_LIMIT) {
        writeln!(
            main,
            "    print(f\"blake3 {n} {{blake3_hex(ramp({n}))}} {{blake3_pieces(ramp({n}))}}\\n\");"
        )
        .unwrap();
        writeln!(want, "blake3 {n} {digest} {digest}").unwrap();
    }
    main.push_str("    0\n}\n");
    want.push_str("0\n");
    (format!("{PRELUDE}\n{main}"), want)
}

/// Build `prog.nz` in `dir` with each native backend named, and run it.
fn native_runs(dir: &Path, which: &[(&str, &str, &str)]) -> Vec<(String, Ran)> {
    let mut runs = Vec::new();
    for (label, backend, opt) in which {
        let exe = dir.join(label.replace([' ', '-'], ""));
        let b = nazm(
            dir,
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
        runs.push((
            (*label).to_owned(),
            run_bounded(
                common::bounded_exe(&exe)
                    .current_dir(dir)
                    .env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
        ));
    }
    runs
}

fn assert_runs(runs: Vec<(String, Ran)>, want: &str) {
    for (label, r) in runs {
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
fn the_embedded_blake3_vectors_are_the_reference_implementations() {
    for (n, digest) in BLAKE3 {
        let input: Vec<u8> = (0..*n).map(|i| u8::try_from(i % 251).unwrap()).collect();
        assert_eq!(blake3::hash(&input).to_hex().as_str(), *digest, "{n}");
    }
}

#[test]
fn the_short_vectors_agree_everywhere() {
    let (program, want) = short_program();
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), program).unwrap();
    let mut runs = Vec::new();
    for (label, args) in [
        ("interpreter", &["run", "prog.nz"][..]),
        ("lir oracle", &["lir", "--run", "prog.nz"][..]),
    ] {
        runs.push((
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
    runs.extend(native_runs(
        d.path(),
        &[
            ("llvm -O0", "llvm", "0"),
            ("llvm -O2", "llvm", "2"),
            ("cranelift", "cranelift", "0"),
        ],
    ));
    assert_runs(runs, &want);
}

#[test]
fn the_long_vectors_hold_natively() {
    let (program, want) = long_program();
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), program).unwrap();
    let runs = native_runs(
        d.path(),
        &[("llvm -O2", "llvm", "2"), ("cranelift", "cranelift", "0")],
    );
    assert_runs(runs, &want);
}
