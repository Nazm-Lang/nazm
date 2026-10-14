//! N39: `nazm core-ir`, and the two backends reading one Core IR.
//!
//! `crates/nazm-core/tests/core_ir.rs` holds the representation's laws; this holds what is
//! only visible from outside a process — that the printed form is the same bytes from one run
//! to the next, that two modules' same-named definitions stay two identities, that a program
//! which does not check prints nothing, and that the interpreter and a compiled executable,
//! both starting from the same Core IR, agree about the shipped examples.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn core_ir(file: &Path, extra: &[&str]) -> Output {
    Command::new(NAZM)
        .arg("core-ir")
        .args(extra)
        .arg(file)
        .output()
        .expect("run nazm core-ir")
}

#[test]
fn the_printed_core_ir_is_the_same_bytes_in_every_process() {
    for example in [
        "examples/pipeline.nz",
        "examples/capabilities/ledger.nz",
        "compiler/emit.nz",
    ] {
        let file = repo().join(example);
        let first = core_ir(&file, &["--json"]);
        assert!(first.status.success(), "{example}: {first:?}");
        let text = String::from_utf8(first.stdout.clone()).expect("UTF-8");
        assert!(
            text.starts_with("{\"schema\":\"nazm.core-ir/3\""),
            "{example}"
        );
        assert_eq!(text.lines().count(), 1, "--json is one line");
        for _ in 0..2 {
            assert_eq!(
                core_ir(&file, &["--json"]).stdout,
                first.stdout,
                "{example}"
            );
        }
        // Indented, the same object.
        let pretty = core_ir(&file, &[]);
        assert!(pretty.status.success());
        let a: serde_json::Value = serde_json::from_slice(&first.stdout).expect("JSON");
        let b: serde_json::Value = serde_json::from_slice(&pretty.stdout).expect("JSON");
        assert_eq!(a, b, "{example}");
    }
}

#[test]
fn two_modules_same_named_helpers_are_two_identities() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("nazm.root"), "").unwrap();
    std::fs::write(
        dir.path().join("main.nz"),
        "use \"lib.nz\";\nfn helper() -> Int { 1 }\nfn main() -> Int { helper() + twice() }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lib.nz"),
        "fn helper() -> Int { 20 }\npub fn twice() -> Int { helper() + helper() }\n",
    )
    .unwrap();
    let out = core_ir(&dir.path().join("main.nz"), &["--json", "--no-spans"]);
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let callees = |def: &str| -> Vec<String> {
        let f = v["functions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["def"] == def)
            .unwrap_or_else(|| panic!("no {def}"));
        let text = f["body"].to_string();
        let mut found = Vec::new();
        let mut rest = text.as_str();
        while let Some(at) = rest.find("\"callee\":\"") {
            rest = &rest[at + 10..];
            let end = rest.find('"').unwrap();
            found.push(rest[..end].to_owned());
        }
        found
    };
    assert_eq!(
        callees("main.nz::fn main"),
        vec!["main.nz::fn helper", "lib.nz::fn twice"]
    );
    assert_eq!(
        callees("lib.nz::fn twice"),
        vec!["lib.nz::fn helper", "lib.nz::fn helper"]
    );
    // No span was asked for, and none is printed.
    assert!(!String::from_utf8_lossy(&out.stdout).contains("\"at\""));
}

/// `nazm.core-ir/3` (R3): what a lowering outside the workspace needs, printed as the checker
/// settled it — function and array types spelled, each function's visibility, captures and
/// attributes, and the modules, type table, statics and impl table, a default method included.
#[test]
fn core_ir_3_prints_the_tables_a_lowering_needs() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("nazm.root"), "").unwrap();
    std::fs::write(
        dir.path().join("prog.nz"),
        "pub extern \"C\" fn nz_triple(x: Int) -> Int ! {} = \"nz_triple\" { x * 3 }\n\
         static COUNT: Atomic = atomic_new(7);\n\
         trait Size {\n    fn size(self: Self) -> Int;\n    \
         fn twice(self: Self) -> Int { self.size() * 2 }\n}\n\
         struct Holder[T] { item: T, n: Int, }\n\
         impl[T] Size for Holder[T] { fn size(self: Holder[T]) -> Int { self.n } }\n\
         pub fn sum(a: [Int; 3]) -> Int { a[0] + a[1] + a[2] }\n\
         fn main() -> Int {\n    let k = 10;\n    let add = fn(x: Int) -> Int { x + k };\n    \
         let h = Holder[Str](item: \"s\", n: 4);\n    \
         add(sum([1, 2, 3])) + h.twice() + nz_triple(1) + atomic_load(COUNT)\n}\n",
    )
    .unwrap();
    let out = core_ir(&dir.path().join("prog.nz"), &["--json", "--no-spans"]);
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema"], "nazm.core-ir/3");
    let function = |def: &str| -> &serde_json::Value {
        v["functions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["def"] == def)
            .unwrap_or_else(|| panic!("no {def}"))
    };
    // Visibility, and an array type spelled where `/2` wrote `?`.
    let sum = function("prog.nz::fn sum");
    assert_eq!(sum["pub"], true);
    assert_eq!(sum["locals"][0]["type"], "[Int; 3]");
    let main = function("prog.nz::fn main");
    assert_eq!(main["pub"], false);
    assert!(
        main["locals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["name"] == "add" && l["type"] == "fn(Int) -> Int"),
        "{main}"
    );
    // The closure's body, and the local holding its one capture.
    let closure = function("prog.nz::fn main#0");
    assert_eq!(closure["closure"], true);
    assert_eq!(closure["captures"], serde_json::json!([2]));
    assert_eq!(closure["locals"][2]["name"], "k");
    assert_eq!(main["closure"], false);
    // The export symbol.
    let export = function("prog.nz::fn nz_triple");
    assert_eq!(export["attributes"]["export"], "nz_triple");
    assert_eq!(export["attributes"]["on_failure"], false);
    assert_eq!(sum["attributes"]["export"], serde_json::Value::Null);
    // Modules in load order.
    assert_eq!(v["modules"][0]["name"], "prog.nz");
    assert_eq!(v["modules"][0]["root"], true);
    assert!(v["modules"][0]["imports"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("@core/prelude")));
    // The type table, in the checker's id order: the definition, then its instances.
    let records = v["types"]["records"].as_array().unwrap();
    let holder = records
        .iter()
        .find(|r| r["display"] == "Holder[Str]")
        .expect("Holder[Str] in the type table");
    assert_eq!(holder["type"], "prog.nz::record Holder[Str]");
    assert_eq!(holder["definition"], "prog.nz::record Holder");
    assert_eq!(holder["type_args"], serde_json::json!(["Str"]));
    assert_eq!(
        holder["fields"],
        serde_json::json!([{"name": "item", "type": "Str"}, {"name": "n", "type": "Int"}])
    );
    assert_eq!(holder["canonical"], serde_json::json!([0, 1]));
    for (i, r) in records.iter().enumerate() {
        assert_eq!(r["id"], i, "records in id order");
    }
    // The static.
    assert_eq!(
        v["statics"],
        serde_json::json!([{"def": "prog.nz::static COUNT", "module": "prog.nz", "name": "COUNT",
            "type": "Atomic", "cell": true, "value": 7, "section": null}])
    );
    // The impl table: the impl's own method, and the trait's default body at the impl's type.
    let size = v["impls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["trait"] == "prog.nz::trait Size")
        .expect("the impl of Size");
    assert_eq!(size["self"], "prog.nz::record Holder");
    assert_eq!(
        size["methods"][0]["function"],
        "prog.nz::fn Size/Holder/size"
    );
    assert_eq!(size["methods"][0]["default"], false);
    assert_eq!(size["methods"][1]["method"], "prog.nz::trait Size.twice");
    assert_eq!(
        size["methods"][1]["function"],
        "prog.nz::fn Size/default/twice"
    );
    assert_eq!(size["methods"][1]["default"], true);
    assert_eq!(
        size["methods"][1]["type_args"],
        serde_json::json!(["prog.nz::record Holder"])
    );
}

#[test]
fn a_program_that_does_not_check_prints_no_core_ir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("bad.nz");
    std::fs::write(&file, "fn main() -> Int { \"not an Int\" }\n").unwrap();
    let out = core_ir(&file, &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        out.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("N0300"), "{stderr}");
    assert!(
        !stderr.contains("N0900"),
        "a user's error is not a compiler defect"
    );
}

/// What `nazm run` and a compiled executable of the same source did.
fn both(example: &str) -> ((Option<i32>, String), (Option<i32>, String)) {
    let src = repo().join(example);
    let dir = tempfile::tempdir().expect("tempdir");
    let ran = Command::new(NAZM)
        .arg("run")
        .arg("--interpret")
        .arg(&src)
        .current_dir(dir.path())
        .output()
        .expect("nazm run");
    let exe = dir.path().join("prog");
    let built = Command::new(NAZM)
        .args(["build", "--no-cache"])
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("nazm build");
    assert!(built.status.success(), "{example}: {built:?}");
    let native = common::bounded_exe(&exe)
        .current_dir(dir.path())
        .output()
        .expect("run the executable");
    (
        (
            ran.status.code(),
            String::from_utf8_lossy(&ran.stdout).into_owned(),
        ),
        (
            native.status.code(),
            String::from_utf8_lossy(&native.stdout).into_owned(),
        ),
    )
}

#[test]
fn the_interpreter_and_the_native_backend_agree_about_the_shipped_examples() {
    // Every example that builds: both backends start from the one Core IR, so they must say
    // the same thing — the value `main` produced and what was printed on the way.
    for example in [
        "examples/sum.nz",
        "examples/max.nz",
        "examples/primes.nz",
        "examples/search.nz",
        "examples/pipeline.nz",
        "examples/worker-pool.nz",
        "examples/capabilities/workers.nz",
        "examples/capabilities/ledger.nz",
        "examples/effects/pure.nz",
        "examples/effects/tasks.nz",
        "examples/provenance/tally.nz",
    ] {
        let (interpreted, native) = both(example);
        // Some need a file or an argument and stop with a runtime error without one; the
        // two backends must agree about that too.
        assert!(
            matches!(interpreted.0, Some(0 | 2)),
            "{example}: {interpreted:?}"
        );
        assert_eq!(interpreted, native, "{example}: the two backends disagree");
    }
}

/// The median of `n` timings of `f`, in milliseconds.
fn median_ms(n: usize, f: &dyn Fn()) -> f64 {
    let mut t: Vec<f64> = (0..n)
        .map(|_| {
            let start = std::time::Instant::now();
            f();
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    t.sort_by(f64::total_cmp);
    t[n.div_euclid(2)]
}

/// What each phase costs, cumulatively, on compiler-scale sources: parse; parse and check; to
/// Core IR (lowering and verifying it); to LIR (the native backend's lowering, with instance
/// planning); to LLVM text. Release only, by hand and in the release-benchmark stage:
/// `cargo test --release -p nazm-cli --test cli_c_d -- --ignored --nocapture core_ir::`.
#[test]
#[ignore = "a measurement: run in release, by hand or in the release-benchmark stage"]
fn what_each_phase_costs_on_the_compiler() {
    let synthetic = tempfile::tempdir().unwrap();
    let chain = synthetic.path().join("chain.nz");
    let mut src = String::new();
    for i in 0..3_000 {
        src.push_str(&format!(
            "fn f{i}(x: Int) -> Int {{ if x > {i} {{ x }} else {{ f{}(x + 1) }} }}\n",
            i + 1
        ));
    }
    src.push_str("fn f3000(x: Int) -> Int { x }\nfn main() -> Int { f0(0) }\n");
    std::fs::write(&chain, src).unwrap();

    for (label, path) in [
        ("compiler/emit.nz", repo().join("compiler/emit.nz")),
        ("compiler/check.nz", repo().join("compiler/check.nz")),
        ("a 3,001-function chain", chain),
    ] {
        let dir = path.parent().unwrap().to_path_buf();
        let loaded = nazm_service::load::load(&path, &dir).expect("load");
        let (map, graph) = (&loaded.map, &loaded.graph);
        let names = nazm_lir::SourceNames::of(map, graph);
        let on_stack = |f: &(dyn Fn() + Sync)| {
            nazm_core::eval::on_interpreter_stack(0, f).unwrap();
        };
        let runs = 15;
        let parse = median_ms(runs, &|| {
            on_stack(&|| {
                let (p, d) = nazm_core::__syntax::parse_map(map);
                assert!(d.is_empty());
                drop(p);
            });
        });
        let check = median_ms(runs, &|| {
            assert!(nazm_core::check_map(map, graph).is_empty())
        });
        let core = median_ms(runs, &|| {
            nazm_core::on_lowered(map, graph, nazm_core::Entry::Required, |c, _| {
                Ok(c.functions.len())
            })
            .unwrap();
        });
        // Of the Core IR column, what verifying it costs: the same check the lowering runs.
        let verify = nazm_core::on_lowered(map, graph, nazm_core::Entry::Required, |c, r| {
            Ok(median_ms(runs, &|| {
                assert!(nazm_core::cir::verify(c, r).is_empty())
            }))
        })
        .unwrap();
        // MIR (N40): the lowering from Core IR, and — of that column — its validation.
        let mir = median_ms(runs, &|| {
            nazm_core::on_lowered(map, graph, nazm_core::Entry::Absent, |c, r| {
                nazm_mir::lower(c, r, graph).map(|p| p.functions.len())
            })
            .unwrap();
        });
        let mir_verify = nazm_core::on_lowered(map, graph, nazm_core::Entry::Absent, |c, r| {
            let p = nazm_mir::lower(c, r, graph)?;
            Ok(median_ms(
                runs,
                &|| assert!(nazm_mir::verify(&p).is_empty()),
            ))
        })
        .unwrap();
        let (layout, emitted) = if label == "compiler/check.nz" {
            // A library-shaped module the native subset refuses as a program: its native
            // columns are not measured.
            (f64::NAN, f64::NAN)
        } else {
            let layout = median_ms(runs, &|| {
                nazm_core::on_lowered(map, graph, nazm_core::Entry::Required, |c, r| {
                    let p = nazm_mir::lower(c, r, graph)?;
                    assert!(nazm_mir::verify(&p).is_empty());
                    nazm_lir::lower(p, &names)
                        .map(|n| n.units.len())
                        .map_err(|e| e.diagnostics)
                })
                .unwrap();
            });
            let emitted = median_ms(runs, &|| {
                nazm_core::on_lowered(map, graph, nazm_core::Entry::Required, |c, r| {
                    let p = nazm_mir::lower(c, r, graph)?;
                    assert!(nazm_mir::verify(&p).is_empty());
                    let native = nazm_lir::lower(p, &names).map_err(|e| e.diagnostics)?;
                    Ok(native
                        .units
                        .iter()
                        .chain(&native.instances)
                        .map(|u| nazm_lir::emit_unit(&native, u, &names).0.len())
                        .sum::<usize>())
                })
                .unwrap();
            });
            (layout, emitted)
        };
        println!(
            "phases {label}: parse {parse:.2} ms · +check {:.2} · +Core IR {:.2} (verify {verify:.2}) \
             · +MIR {:.2} (verify {mir_verify:.2}) · +layout {:.2} · +LLVM text {:.2} · total {:.2} ms \
             (median of {runs})",
            check - parse,
            core - check,
            mir - core,
            layout - mir - mir_verify,
            emitted - layout,
            if emitted.is_nan() { mir } else { emitted }
        );
    }
}
