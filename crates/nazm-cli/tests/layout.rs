//! N68: layout specialisation — a `Vec` of records, one array per field.
//!
//! `docs/architecture.md` §7.70. What whole builds show: `--layout soa` changes no answer and no
//! failure — growth through many capacities, set, pop, get, out-of-bounds and empty-pop failures —
//! under both optimisation levels against the interpreter and the element-by-element layout; a
//! record whose fields are reordered is the same program and the same code; the layout is in every
//! object's identity, so neither layout's object is ever taken for the other's; an ineligible
//! element stays element by element; and `nazm explain-cost` says which and why.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

const PARTICLES: &str = "struct Particle { x: Int, y: Int, z: Int, vx: Int, vy: Int, vz: Int, mass: Int, alive: Bool }\n\
fn main() -> Int {\n\
    let ps = vec_new[Particle]();\n\
    let mut i = 0;\n\
    while i < 1000 { vec_push(ps, Particle(x: i, y: i * 2, z: 3, vx: 1, vy: -1, vz: 0, mass: i % 7, alive: i % 3 == 0)); i = i + 1; }\n\
    vec_set(ps, 5, Particle(x: 100, y: 0, z: 0, vx: 0, vy: 0, vz: 0, mass: 9, alive: true));\n\
    let last = vec_pop(ps);\n\
    let mut s = 0;\n\
    let mut k = 0;\n\
    while k < vec_len(ps) { let p = vec_get(ps, k); if p.alive { s = s + p.x + p.mass; } else { s = s - p.vy; } k = k + 1; }\n\
    s + last.y + vec_len(ps)\n\
}\n";

/// What `src` prints and how it ends, under the interpreter and under each native build.
fn everywhere(src: &str) -> Vec<(String, String, Option<i32>, String)> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let run = nazm(d.path(), &["run", "prog.nz"]);
    let mut out = vec![(
        "run".to_owned(),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        run.status.code(),
        err(&run)
            .lines()
            .find(|l| l.contains("error["))
            .unwrap_or_default()
            .split(" at ")
            .next()
            .unwrap_or_default()
            .to_owned(),
    )];
    for layout in ["aos", "soa"] {
        for opt in ["0", "2"] {
            let exe = d.path().join(format!("p-{layout}-{opt}"));
            let b = nazm(
                d.path(),
                &[
                    "build",
                    "--layout",
                    layout,
                    "--opt-level",
                    opt,
                    "prog.nz",
                    "-o",
                    exe.to_str().unwrap(),
                ],
            );
            assert!(b.status.success(), "{layout} -O{opt}: {}", err(&b));
            let o = Command::new(&exe).output().unwrap();
            out.push((
                format!("{layout} -O{opt}"),
                String::from_utf8_lossy(&o.stdout).into_owned(),
                o.status.code(),
                err(&o)
                    .lines()
                    .find(|l| l.contains("error["))
                    .unwrap_or_default()
                    .trim_start_matches("nazm: ")
                    .split(" at ")
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
            ));
        }
    }
    out
}

#[test]
fn the_field_by_field_layout_changes_no_answer() {
    let all = everywhere(PARTICLES);
    let (_, want, status, _) = &all[0];
    assert_eq!(status, &Some(0));
    for (tier, got, s, _) in &all {
        assert_eq!((got, s), (want, &Some(0)), "{tier}");
    }
}

#[test]
fn the_field_by_field_layout_changes_no_failure() {
    for body in [
        // Past the end, after growth through several capacities.
        "let ps = vec_new[P](); let mut i = 0; while i < 37 { vec_push(ps, P(a: i, b: true)); i = i + 1; } vec_get(ps, 37).a",
        // Before the start.
        "let ps = vec_new[P](); vec_push(ps, P(a: 1, b: false)); vec_set(ps, -1, P(a: 2, b: true))",
        // Popped empty.
        "let ps = vec_new[P](); vec_push(ps, P(a: 1, b: false)); let x = vec_pop(ps); let y = vec_pop(ps); x.a + y.a",
    ] {
        let src = format!("struct P {{ a: Int, b: Bool }}\nfn main() -> Int {{ {body} }}\n");
        let all = everywhere(&src);
        let want = all[0].3.clone();
        assert!(want.starts_with("error[N0405]"), "{want}");
        for (tier, _, status, failure) in &all {
            assert_eq!(status, &Some(2), "{tier}: {body}");
            assert_eq!(failure, &want, "{tier}: {body}");
        }
    }
}

#[test]
fn reordering_a_records_fields_changes_neither_meaning_nor_code() {
    let d = tempfile::tempdir().unwrap();
    let a = "struct P { a: Int, b: Bool, c: Int }\nfn main() -> Int { let ps = vec_new[P](); vec_push(ps, P(a: 1, b: true, c: 3)); vec_push(ps, P(c: 30, a: 10, b: false)); let p = vec_get(ps, 1); p.a + p.c + if vec_get(ps, 0).b { 100 } else { 0 } }\n";
    let b = a.replace(
        "struct P { a: Int, b: Bool, c: Int }",
        "struct P { c: Int, a: Int, b: Bool }",
    );
    let mut ir = Vec::new();
    for (name, src) in [("a", a.to_owned()), ("b", b)] {
        std::fs::create_dir_all(d.path().join(name)).unwrap();
        std::fs::write(d.path().join(name).join("prog.nz"), src).unwrap();
        let o = nazm(
            &d.path().join(name),
            &[
                "build",
                "--layout",
                "soa",
                "prog.nz",
                "-o",
                "p",
                "--emit-ir",
            ],
        );
        assert!(o.status.success(), "{}", err(&o));
        let out = Command::new(d.path().join(name).join("p"))
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "140\n");
        ir.push(std::fs::read_to_string(d.path().join(name).join("p.m0.ll")).unwrap());
    }
    assert_eq!(ir[0], ir[1], "a declaration order changed the emitted code");
}

#[test]
fn the_layout_is_in_every_objects_identity() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), PARTICLES).unwrap();
    let report = |layout: &str| -> serde_json::Value {
        let o = nazm(
            d.path(),
            &[
                "build",
                "--layout",
                layout,
                "--build-report",
                "prog.nz",
                "-o",
                "p",
            ],
        );
        assert!(o.status.success(), "{}", err(&o));
        let line = String::from_utf8_lossy(&o.stdout)
            .lines()
            .find(|l| l.contains("nazm.build-report/1"))
            .unwrap_or_default()
            .to_owned();
        serde_json::from_str(&line).unwrap_or(serde_json::Value::Null)
    };
    let first = report("aos");
    let soa = report("soa");
    let again = report("aos");
    let reused = |v: &serde_json::Value| {
        v.pointer("/native/reused")
            .and_then(serde_json::Value::as_u64)
    };
    // The unit whose code the layout changes is compiled afresh for the other layout — only the
    // runtime and entry, whose bytes are the same in both, are reused — and every unit is reused
    // when the first layout is asked for again.
    let compiled = |v: &serde_json::Value| {
        v.pointer("/native/compiled")
            .and_then(serde_json::Value::as_u64)
    };
    assert!(first.is_object() && soa.is_object(), "{first} {soa}");
    assert_eq!(compiled(&soa), Some(1), "{soa}");
    assert_eq!(reused(&again), Some(3), "{again}");
    // The emitted code differs, so so does every key derived from it.
    let ir = |layout: &str| {
        let o = nazm(
            d.path(),
            &[
                "build",
                "--layout",
                layout,
                "prog.nz",
                "-o",
                "q",
                "--emit-ir",
            ],
        );
        assert!(o.status.success());
        std::fs::read_to_string(d.path().join("q.m0.ll")).unwrap()
    };
    let (a, s) = (ir("aos"), ir("soa"));
    assert!(
        !a.contains("call ptr @nz.soa_grow("),
        "aos used the field-by-field layout"
    );
    assert!(s.contains("call ptr @nz.soa_grow("), "soa did not");
}

#[test]
fn an_ineligible_element_stays_whole_and_the_reason_is_reported() {
    let d = tempfile::tempdir().unwrap();
    let src = "struct P { x: Int, ok: Bool }\nstruct N { s: Str, n: Int }\nfn main() -> Int { let a = vec_new[P](); vec_push(a, P(x: 2, ok: true)); let b = vec_new[N](); vec_push(b, N(s: \"hi\", n: 5)); vec_push(b, N(s: \"abc\", n: 6)); vec_push(b, N(s: \"defg\", n: 7)); vec_push(b, N(s: \"hijkl\", n: 8)); let mut k = 0; let mut t = 0; while k < vec_len(b) { t = t + str_len(vec_get(b, k).s) * 10 + vec_get(b, k).n; k = k + 1; } let c = vec_new[Int](); vec_push(c, 7); vec_get(a, 0).x + t + vec_get(c, 0) }\n";
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let o = nazm(
        d.path(),
        &["build", "--layout", "soa", "prog.nz", "-o", "p"],
    );
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(
        String::from_utf8_lossy(&Command::new(d.path().join("p")).output().unwrap().stdout),
        "175\n"
    );
    let o = nazm(d.path(), &["explain-cost", "--json", "prog.nz"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let l = v["layouts"].as_array().unwrap();
    let of = |t: &str| {
        l.iter()
            .find(|x| x["type"] == t)
            .map(|x| x["soa"].clone())
            .unwrap()
    };
    assert_eq!(of("Vec[P]"), "eligible");
    assert_eq!(of("Vec[Int]"), "its element is not a record");
    assert!(of("Vec[N]")
        .as_str()
        .unwrap()
        .starts_with("a field is not an `Int` or a `Bool`"));
}

#[test]
fn the_layout_is_asked_for_by_name_and_is_the_llvm_backends() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), PARTICLES).unwrap();
    let o = nazm(
        d.path(),
        &["build", "--layout", "columns", "prog.nz", "-o", "p"],
    );
    assert_eq!(o.status.code(), Some(4));
    assert!(
        err(&o).contains("`--layout columns` is not a layout"),
        "{}",
        err(&o)
    );
    let o = nazm(
        d.path(),
        &[
            "build",
            "--layout",
            "soa",
            "--backend",
            "cranelift",
            "prog.nz",
            "-o",
            "p",
        ],
    );
    assert_eq!(o.status.code(), Some(4));
    assert!(
        err(&o).contains("`--layout soa` is the LLVM backend's"),
        "{}",
        err(&o)
    );
}
