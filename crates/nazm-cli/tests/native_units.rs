//! Native compilation across module boundaries.
//!
//! # Why a file of its own
//!
//! `tests/build.rs` compiles one file at a time, which is the right shape for testing
//! arithmetic and refusals and the wrong one for testing what happens *between* modules.
//! Everything here has at least two, and the questions are the ones a second module
//! creates: whose symbol is this, whose file is that position in, and what does the
//! linker see.
//!
//! # Two oracles, as everywhere else
//!
//! Where a program produces a value or a failure, the interpreter is asked the same
//! question and the two must agree. A backend that disagreed with the interpreter about
//! which file a division by zero is in would be reporting about a program nobody wrote.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(&path, text).expect("write");
}

/// What running something produced, with the paths it printed left exactly as they were.
#[derive(Debug, PartialEq, Eq)]
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn ran(out: &std::process::Output) -> Ran {
    Ran {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).trim().to_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
    }
}

/// Build the first file of `files` and run the executable, from inside the project
/// directory so that the paths a program prints are the relative ones the user typed.
fn build_and_run(dir: &Path, files: &[(&str, &str)]) -> Ran {
    for (name, text) in files {
        write(dir, name, text);
    }
    let built = Command::new(NAZM)
        .args(["build", files[0].0, "-o", "prog"])
        .current_dir(dir)
        .output()
        .expect("build");
    assert!(
        built.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    ran(&Command::new(dir.join("prog"))
        .current_dir(dir)
        .output()
        .expect("execute"))
}

fn interpret(dir: &Path, files: &[(&str, &str)]) -> Ran {
    for (name, text) in files {
        write(dir, name, text);
    }
    ran(&Command::new(NAZM)
        .args(["run", files[0].0])
        .current_dir(dir)
        .output()
        .expect("run"))
}

#[test]
fn a_runtime_failure_names_the_module_it_happened_in() {
    // Until 2026-09-22 the emitted message carried the file `nazm build` was invoked on,
    // for every failure in every module. The line and column were already right — they
    // come from the `SourceMap` — so this printed `main.nz` with `lib.nz`'s position: a
    // location that points somewhere real and wrong, which is worse than one that points
    // nowhere.
    let dir = tempfile::tempdir().expect("tempdir");
    let files = &[
        ("main.nz", "use \"lib.nz\";\nfn main() -> Int { boom(1) }\n"),
        (
            "lib.nz",
            "fn zero() -> Int { 0 }\npub fn boom(n: Int) -> Int { n / zero() }\n",
        ),
    ];

    let native = build_and_run(dir.path(), files);
    assert_eq!(native.code, Some(2));
    assert_eq!(
        native.stderr,
        "nazm: error[N0401]: division by zero at lib.nz:2:32"
    );

    // And the interpreter agrees about which file and where in it. The rendering differs
    // — a compiled program has no diagnostic framework linked into it — but the location
    // is the same claim, and the two must not make different ones.
    let interpreted = interpret(dir.path(), files);
    assert_eq!(interpreted.code, Some(2));
    assert!(
        interpreted.stderr.contains("--> lib.nz:2:32"),
        "{}",
        interpreted.stderr
    );
}

#[test]
fn the_root_module_still_names_itself() {
    // The other half, and the reason the fix is a change of *source* rather than of
    // behaviour: a single-file program reports exactly what it always did.
    let dir = tempfile::tempdir().expect("tempdir");
    let files = &[("one.nz", "fn main() -> Int { 9 / 0 }\n")];

    let native = build_and_run(dir.path(), files);
    assert_eq!(native.code, Some(2));
    assert_eq!(
        native.stderr,
        "nazm: error[N0401]: division by zero at one.nz:1:22"
    );
}

// ---------------------------------------------------------------------------
// Separate code generation
// ---------------------------------------------------------------------------

/// Build with `--emit-ir` and return every artefact, by its name.
///
/// One per Nazm module, plus the runtime and the entry wrapper. Reading them is how a
/// test can ask what was generated *before* the linker combined it — which is the only
/// place the separation is visible.
fn artefacts(dir: &Path, files: &[(&str, &str)]) -> Vec<(String, String)> {
    for (name, text) in files {
        write(dir, name, text);
    }
    let built = Command::new(NAZM)
        .args(["build", files[0].0, "-o", "prog", "--emit-ir"])
        .current_dir(dir)
        .output()
        .expect("build");
    assert!(
        built.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );

    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read dir").flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|x| x == "ll") {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_owned();
            out.push((name, std::fs::read_to_string(&path).expect("read ir")));
        }
    }
    out.sort();
    assert!(!out.is_empty(), "no artefacts were written");
    out
}

/// Every symbol any artefact defines, as `linkage symbol`.
fn defined(artefacts: &[(String, String)]) -> Vec<String> {
    let mut out: Vec<String> = artefacts
        .iter()
        .flat_map(|(_, ir)| ir.lines())
        .filter(|l| l.starts_with("define "))
        .map(|l| {
            let head = l.split('(').next().unwrap_or_default();
            let mut parts: Vec<&str> = head.split_whitespace().collect();
            let symbol = parts.pop().unwrap_or_default().to_owned();
            let linkage = if head.contains(" internal ") {
                "internal"
            } else if head.contains(" hidden ") {
                "hidden"
            } else {
                "external"
            };
            format!("{linkage} {symbol}")
        })
        .collect();
    out.sort();
    out
}

/// Every Nazm definition symbol, with no linkage and no runtime.
fn nazm_symbols(artefacts: &[(String, String)]) -> Vec<String> {
    let mut out: Vec<String> = defined(artefacts)
        .into_iter()
        .filter_map(|s| s.split_whitespace().nth(1).map(str::to_owned))
        .filter(|s| s.starts_with("@nz.m.") || s.starts_with("@nz.u."))
        .collect();
    out.sort();
    out
}

#[test]
fn a_call_into_an_imported_module_is_a_reference_the_linker_resolves() {
    // §23 of the directive. The caller's artefact *declares* the callee and defines it
    // nowhere; the callee's artefact defines it; the linker joins them. No name is looked
    // up, and no whole-program number decides anything.
    let dir = tempfile::tempdir().expect("tempdir");
    let files = &[
        ("a.nz", "use \"b.nz\";\nfn main() -> Int { twice(4) }\n"),
        ("b.nz", "pub fn twice(n: Int) -> Int { n + n }\n"),
    ];
    let art = artefacts(dir.path(), files);

    let caller = &art
        .iter()
        .find(|(_, ir)| ir.contains("a.nz\""))
        .expect("a unit for a.nz")
        .1;
    assert!(
        caller.contains("declare i64 @nz.m.b_2Enz.twice(i64)"),
        "the caller should declare the callee, not define it:\n{caller}"
    );
    assert!(!caller.contains("define hidden i64 @nz.m.b_2Enz.twice"));

    let callee = &art
        .iter()
        .find(|(_, ir)| ir.contains("b.nz\""))
        .expect("a unit for b.nz")
        .1;
    assert!(callee.contains("define hidden i64 @nz.m.b_2Enz.twice(i64 %p0)"));

    assert_eq!(build_and_run(dir.path(), files).stdout, "8");
}

#[test]
fn one_imported_module_may_call_another() {
    let dir = tempfile::tempdir().expect("tempdir");
    let files = &[
        ("a.nz", "use \"b.nz\";\nfn main() -> Int { mid(3) }\n"),
        (
            "b.nz",
            "use \"c.nz\";\npub fn mid(n: Int) -> Int { deep(n) + 1 }\n",
        ),
        ("c.nz", "pub fn deep(n: Int) -> Int { n * 10 }\n"),
    ];
    assert_eq!(build_and_run(dir.path(), files).stdout, "31");
    assert_eq!(interpret(dir.path(), files).stdout, "31");
}

#[test]
fn three_modules_may_each_define_a_private_helper_of_the_same_name() {
    // The miscompile class this whole line of work began with: before N2 two modules'
    // private `helper` both became `@nz.f0` and clang rejected the module. Now each is a
    // different symbol in a different object, and all three are `internal` — nothing
    // outside a module can name its private definitions, so nothing needs to.
    let dir = tempfile::tempdir().expect("tempdir");
    let files = &[
        (
            "a.nz",
            "use \"b.nz\";\nuse \"c.nz\";\nfn helper() -> Int { 1 }\n\
             fn main() -> Int { helper() + from_b() + from_c() }\n",
        ),
        (
            "b.nz",
            "fn helper() -> Int { 20 }\npub fn from_b() -> Int { helper() }\n",
        ),
        (
            "c.nz",
            "fn helper() -> Int { 300 }\npub fn from_c() -> Int { helper() }\n",
        ),
    ];
    let art = artefacts(dir.path(), files);
    let symbols = defined(&art);
    for m in ["a_2Enz", "b_2Enz", "c_2Enz"] {
        assert!(
            symbols.contains(&format!("internal @nz.m.{m}.helper")),
            "{m}'s helper should be its own internal definition: {symbols:?}"
        );
    }
    assert_eq!(build_and_run(dir.path(), files).stdout, "321");
}

#[test]
fn a_diamond_generates_the_shared_module_once() {
    // §25. `D` is reached through both `B` and `C`, and is one module with one identity,
    // so it is code-generated once. Compiling it per incoming edge would give the linker
    // two definitions of one symbol and fail — which is what makes this testable at all.
    let dir = tempfile::tempdir().expect("tempdir");
    let files = &[
        (
            "a.nz",
            "use \"b.nz\";\nuse \"c.nz\";\nfn main() -> Int { from_b() + from_c() }\n",
        ),
        (
            "b.nz",
            "use \"d.nz\";\npub fn from_b() -> Int { shared() }\n",
        ),
        (
            "c.nz",
            "use \"d.nz\";\npub fn from_c() -> Int { shared() * 2 }\n",
        ),
        ("d.nz", "pub fn shared() -> Int { 7 }\n"),
    ];
    let art = artefacts(dir.path(), files);
    assert_eq!(
        defined(&art)
            .iter()
            .filter(|s| s.ends_with("@nz.m.d_2Enz.shared"))
            .count(),
        1,
        "the shared module was generated more than once"
    );
    // Four modules, plus the runtime and the entry.
    assert_eq!(
        art.len(),
        6,
        "{:?}",
        art.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );
    assert_eq!(build_and_run(dir.path(), files).stdout, "21");
}

#[test]
fn a_legal_module_cycle_compiles_and_links() {
    // §13 and §26. An import cycle is legal and says nothing about recursion: `A::f`
    // calls `B::g`, `B` imports `A` and calls nothing in it. A linker has no opinion
    // about the order objects were produced in, which is exactly why separate generation
    // needs no topological order.
    let dir = tempfile::tempdir().expect("tempdir");
    let files = &[
        (
            "a.nz",
            "use \"b.nz\";\npub fn shared() -> Int { 5 }\nfn main() -> Int { g() }\n",
        ),
        (
            "b.nz",
            "use \"a.nz\";\npub fn g() -> Int { 37 }\npub fn unused() -> Int { shared() }\n",
        ),
    ];
    assert_eq!(build_and_run(dir.path(), files).stdout, "37");
    assert_eq!(interpret(dir.path(), files).stdout, "37");
}

#[test]
fn a_call_cycle_across_modules_compiles_and_its_runaway_is_n0408() {
    // §11 and §26's second half. The cycle analysis is whole-program and stays one:
    // `A::f → B::g → A::f` is a cycle no single module can see. Until N49 it was refused;
    // since, both functions check their stack first, so a terminating cycle runs and a
    // runaway one reports `N0408` instead of dying of a signal.
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\npub fn f(n: Int) -> Int { if n == 0 { 0 } else { g(n - 1) + 1 } }\n\
         pub fn h(n: Int) -> Int { k(n + 1) }\nfn main() -> Int { f(5) }\n",
    );
    write(
        dir.path(),
        "b.nz",
        "use \"a.nz\";\npub fn g(n: Int) -> Int { f(n) }\npub fn k(n: Int) -> Int { h(n + 1) }\n",
    );
    let out = Command::new(NAZM)
        .args(["build", "a.nz", "-o", "prog"])
        .current_dir(dir.path())
        .output()
        .expect("build");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ran = Command::new(dir.path().join("prog")).output().expect("run");
    assert_eq!(String::from_utf8_lossy(&ran.stdout).trim(), "5");

    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\npub fn f(n: Int) -> Int { if n == 0 { 0 } else { g(n - 1) + 1 } }\n\
         pub fn h(n: Int) -> Int { k(n + 1) }\nfn main() -> Int { h(0) }\n",
    );
    let out = Command::new(NAZM)
        .args(["build", "a.nz", "-o", "runaway"])
        .current_dir(dir.path())
        .output()
        .expect("build");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ran = common::bounded_exe(&dir.path().join("runaway"))
        .output()
        .expect("run");
    let err = String::from_utf8_lossy(&ran.stderr);
    assert_eq!(ran.status.code(), Some(2), "{err}");
    assert!(
        err.contains("error[N0408]") && (err.contains(" at a.nz:") || err.contains(" at b.nz:")),
        "{err}"
    );
}

#[test]
fn an_imported_modules_main_is_not_the_process_entry() {
    // N2's rule, now also a linker fact. The imported `main` is an ordinary definition
    // with an ordinary symbol; the platform entry calls the **root** module's.
    let dir = tempfile::tempdir().expect("tempdir");
    let files = &[
        ("app.nz", "use \"lib.nz\";\nfn main() -> Int { helper() }\n"),
        (
            "lib.nz",
            "pub fn helper() -> Int { 11 }\nfn main() -> Int { 99 }\n",
        ),
    ];
    let art = artefacts(dir.path(), files);
    let entry = &art
        .iter()
        .find(|(_, ir)| ir.contains("<nazm entry>"))
        .expect("an entry unit")
        .1;
    assert!(entry.contains("call i64 @nz.m.app_2Enz.main()"), "{entry}");
    assert!(!entry.contains("@nz.m.lib_2Enz.main"), "{entry}");
    // And the imported one is still generated, because it is one of that module's
    // definitions and nothing more.
    assert!(defined(&art).contains(&"internal @nz.m.lib_2Enz.main".to_owned()));
    assert_eq!(build_and_run(dir.path(), files).stdout, "11");
}

#[test]
fn string_constants_from_two_modules_do_not_collide() {
    // §19. Each unit numbers its own constants from zero, so two artefacts both contain
    // `@nz.l.s.0` (N105's name for a unit's first literal). That is safe because the linkage is `private`, which makes them local
    // labels rather than symbol-table entries — established here by linking and running
    // rather than by reasoning about it.
    let dir = tempfile::tempdir().expect("tempdir");
    let files = &[
        (
            "a.nz",
            "use \"b.nz\";\nfn main() -> Int { str_len(\"aaaa\") + from_b() }\n",
        ),
        ("b.nz", "pub fn from_b() -> Int { str_len(\"bbbbbbb\") }\n"),
    ];
    let art = artefacts(dir.path(), files);
    assert!(
        art.iter()
            .filter(|(_, ir)| ir.contains("@nz.l.s.0 = private"))
            .count()
            >= 2,
        "both units should have numbered a constant from zero"
    );
    assert_eq!(build_and_run(dir.path(), files).stdout, "11");
}

#[test]
fn the_runtime_is_defined_exactly_once() {
    // §16. Every definition that must exist once per program lives in one artefact. A
    // second copy would be a duplicate symbol and the link would fail — so the strongest
    // evidence is that a program using every helper set links at all.
    let dir = tempfile::tempdir().expect("tempdir");
    let files = &[
        (
            "a.nz",
            "use \"b.nz\";\nfn main(tasks: SpawnCap) -> Int { work(tasks) + str_len(int_to_str(7)) }\n",
        ),
        (
            "b.nz",
            "pub fn work(tasks: SpawnCap) -> Int {\n    let got = ints_new();\n    \
             let c = chan_new(1);\n    scope { spawn feed(c); }\n    \
             let more = chan_recv(c, got);\n    ints_get(got, 0) + 4\n}\n\
             pub fn feed(c: Chan) -> Int { chan_send(c, 30); chan_close(c); 0 }\n",
        ),
    ];
    let art = artefacts(dir.path(), files);
    let symbols = defined(&art);
    for helper in [
        "@nz.fail",
        "@nz.report",
        "@nz.arr_new",
        "@nz.arr_grow",
        "@nz.spawn",
        "@nz.scope_join",
        "@nz.chan_new",
        "@nz.chan_send",
        "@nz.chan_recv",
        "@nz.chan_close",
    ] {
        assert_eq!(
            symbols.iter().filter(|s| s.ends_with(helper)).count(),
            1,
            "{helper} should be defined once: {symbols:?}"
        );
    }
    assert_eq!(
        symbols.iter().filter(|s| s.ends_with("@main")).count(),
        1,
        "exactly one process entry"
    );
    assert_eq!(build_and_run(dir.path(), files).stdout, "35");
}

#[test]
fn two_checkouts_generate_the_same_symbols() {
    // §6 and §19 of the directive. Nothing about where a checkout sits reaches a symbol,
    // because nothing about it reaches a `DefKey`.
    let files = &[
        ("a.nz", "use \"lib/b.nz\";\nfn main() -> Int { twice(4) }\n"),
        ("lib/b.nz", "pub fn twice(n: Int) -> Int { n + n }\n"),
    ];
    let one = tempfile::tempdir().expect("tempdir");
    let two = tempfile::tempdir().expect("tempdir");
    assert_ne!(one.path(), two.path());
    assert_eq!(
        nazm_symbols(&artefacts(one.path(), files)),
        nazm_symbols(&artefacts(two.path(), files))
    );
}

#[test]
fn a_shared_module_gets_the_same_symbols_from_either_entry_point() {
    // §6's entry-point independence, which is N4's declared source root doing its job:
    // `lib/common.nz` is the same module whichever application named it, so its
    // definitions are the same definitions and get the same symbols.
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "nazm.root", "");
    let common = (
        "lib/common.nz",
        "fn hidden_helper() -> Int { 2 }\npub fn shared(n: Int) -> Int { n * hidden_helper() }\n",
    );
    let from_app = artefacts(
        dir.path(),
        &[
            (
                "app.nz",
                "use \"lib/common.nz\";\nfn main() -> Int { shared(3) }\n",
            ),
            common,
        ],
    );
    let from_tool = artefacts(
        dir.path(),
        &[
            (
                "tool.nz",
                "use \"lib/common.nz\";\nfn main() -> Int { shared(5) }\n",
            ),
            common,
        ],
    );

    let of_common = |art: &[(String, String)]| {
        let mut s: Vec<String> = nazm_symbols(art)
            .into_iter()
            .filter(|s| s.contains("lib_2Fcommon"))
            .collect();
        s.sort();
        s
    };
    assert_eq!(of_common(&from_app), of_common(&from_tool));
    assert_eq!(
        of_common(&from_app),
        vec![
            "@nz.m.lib_2Fcommon_2Enz.hidden_5Fhelper".to_owned(),
            "@nz.m.lib_2Fcommon_2Enz.shared".to_owned(),
        ]
    );
}

#[test]
fn pub_chooses_linkage_and_never_spelling() {
    // §5. `pub` says which Nazm modules may name a definition. It decides whether the
    // linker must be able to bind the symbol — and it does not decide what the symbol is
    // called, because the symbol comes from the definition's identity, which `pub` is not
    // part of.
    let dir = tempfile::tempdir().expect("tempdir");
    let private = artefacts(
        dir.path(),
        &[(
            "a.nz",
            "fn spare() -> Int { 4 }\nfn main() -> Int { spare() }\n",
        )],
    );
    let exported = artefacts(
        dir.path(),
        &[(
            "a.nz",
            "pub fn spare() -> Int { 4 }\nfn main() -> Int { spare() }\n",
        )],
    );
    assert_eq!(nazm_symbols(&private), nazm_symbols(&exported));
    assert!(defined(&private).contains(&"internal @nz.m.a_2Enz.spare".to_owned()));
    assert!(defined(&exported).contains(&"hidden @nz.m.a_2Enz.spare".to_owned()));
}

#[test]
fn a_native_build_is_unaffected_by_the_semantic_cache() {
    // The two caches are keyed on different things and answer different questions, so
    // neither may reach the other. `nazm build` does not read a check result at all — it
    // needs the resolution only checking a body produces, and nothing persists one — and a
    // warm semantic store must therefore change nothing about what is emitted.
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "nazm.root", "");
    let files = &[
        ("a.nz", "use \"b.nz\";\nfn main() -> Int { twice(6) }\n"),
        ("b.nz", "pub fn twice(n: Int) -> Int { n + n }\n"),
    ];
    let cold = artefacts(dir.path(), files);
    assert!(
        !dir.path().join(".nazm").join("check").exists(),
        "a build wrote a semantic entry"
    );

    // Warm the semantic cache, then build again.
    let checked = Command::new(NAZM)
        .args(["check", "a.nz"])
        .current_dir(dir.path())
        .output()
        .expect("check");
    assert!(checked.status.success());
    assert!(
        dir.path().join(".nazm").join("check").exists(),
        "check should have cached"
    );

    let warm = artefacts(dir.path(), files);
    assert_eq!(nazm_symbols(&cold), nazm_symbols(&warm));
    for ((name, a), (_, b)) in cold.iter().zip(&warm) {
        assert_eq!(a, b, "{name} changed when the semantic store was warm");
    }
    assert_eq!(build_and_run(dir.path(), files).stdout, "12");
}

#[test]
fn a_backend_refusal_inside_an_imported_module_names_that_module() {
    // §29. Emission moved per module; a diagnostic must still point where the source is,
    // which is the property N1 established and N5 must not spend. The refusal here is
    // polymorphic recursion (`N0357`): native recursion, this case until N49, compiles now.
    let dir = tempfile::tempdir().expect("tempdir");
    let app = "use \"lib.nz\";\nfn main() -> Int { grow(1, 2) }\n";
    write(dir.path(), "app.nz", app);
    write(
        dir.path(),
        "lib.nz",
        "pub fn grow[T](x: T, n: Int) -> Int { if n == 0 { 0 } else { let v = vec_new[T](); vec_push(v, x); grow(v, n - 1) + 1 } }\n",
    );

    // The rendered form names the file outright.
    let rendered = Command::new(NAZM)
        .args(["build", "app.nz", "-o", "prog"])
        .current_dir(dir.path())
        .output()
        .expect("build");
    assert!(!rendered.status.success());
    let text = String::from_utf8_lossy(&rendered.stderr);
    assert!(text.contains("--> lib.nz:1:100"), "{text}");
    assert!(text.contains("N0357"), "{text}");
    assert!(
        !dir.path().join("prog").exists(),
        "nothing should be written"
    );

    // The machine form identifies it by an offset into the concatenation of every file in
    // load order — `docs/diagnostics.md` fixes that shape, and a `file` field would be a
    // schema version rather than a side effect of moving emission. What N5 must not break
    // is that the offset lands in the right file, so that is what this asserts.
    let json = Command::new(NAZM)
        .args(["build", "app.nz", "-o", "prog", "--json"])
        .current_dir(dir.path())
        .output()
        .expect("build");
    let reported: Vec<serde_json::Value> = String::from_utf8_lossy(&json.stderr)
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let refusal = reported
        .iter()
        .find(|d| d["code"] == "N0357")
        .expect("a refusal");
    let at = refusal["primary"]["span"]["start"]
        .as_u64()
        .expect("an offset");
    assert!(
        at >= app.len() as u64,
        "the offset should be inside `lib.nz`, past the {} bytes of `app.nz`: {at}",
        app.len()
    );
}

// ---------------------------------------------------------------------------
// The name a generated artefact carries
// ---------------------------------------------------------------------------

/// Build the same project from an absolute path, and return the IR of every artefact.
fn artefacts_from_elsewhere(dir: &Path, files: &[(&str, &str)]) -> Vec<(String, String)> {
    for (name, text) in files {
        write(dir, name, text);
    }
    // The root marker, so that the compilation begun from outside still measures
    // identities from the project rather than from wherever the shell happens to be.
    write(dir, "nazm.root", "");
    let out = dir.join("out");
    let built = Command::new(NAZM)
        .args(["build"])
        .arg(dir.join(files[0].0))
        .args(["-o"])
        .arg(&out)
        .arg("--emit-ir")
        .current_dir(std::env::temp_dir())
        .output()
        .expect("build");
    assert!(
        built.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );

    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read dir").flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|x| x == "ll") {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .replace("out.", "prog.");
            found.push((name, std::fs::read_to_string(&path).expect("read ir")));
        }
    }
    found.sort();
    found
}

const TWO: &[(&str, &str)] = &[
    (
        "app.nz",
        "use \"lib/util.nz\";\nfn main() -> Int { parse(7) }\n",
    ),
    ("lib/util.nz", "pub fn parse(n: Int) -> Int { n / 0 }\n"),
];

#[test]
fn a_generated_artefact_names_a_module_by_its_durable_identity() {
    // Until 2026-09-22 the `source_filename` and every failure message carried the path
    // the command line spelled, so the same project built from two directories produced
    // two different programs that no reader of the source could tell apart. The position
    // was right in both; what moved was the shell's opinion of where the project is.
    let here = tempfile::tempdir().expect("tempdir");
    let there = tempfile::tempdir().expect("tempdir");

    let near = artefacts(here.path(), TWO);
    let far = artefacts_from_elsewhere(there.path(), TWO);

    assert_eq!(
        near.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
        far.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>()
    );
    for ((name, a), (_, b)) in near.iter().zip(&far) {
        assert_eq!(a, b, "{name} differs between checkouts");
    }
    // And the name it carries is the logical one, not either directory's.
    let all = near.iter().map(|(_, ir)| ir.as_str()).collect::<String>();
    assert!(all.contains("source_filename = \"lib/util.nz\""), "{all}");
    assert!(!all.contains(&here.path().display().to_string()), "{all}");
}

#[test]
fn a_runtime_failure_from_an_absolute_build_names_the_logical_module() {
    // The other half: the message a person reads is still a location they can open,
    // stated relative to the project rather than to whoever invoked the compiler.
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, text) in TWO {
        write(dir.path(), name, text);
    }
    write(dir.path(), "nazm.root", "");
    let out = dir.path().join("prog");
    let built = Command::new(NAZM)
        .args(["build"])
        .arg(dir.path().join("app.nz"))
        .args(["-o"])
        .arg(&out)
        .current_dir(std::env::temp_dir())
        .output()
        .expect("build");
    assert!(built.status.success(), "{:?}", built);

    let r = ran(&Command::new(&out).output().expect("run"));
    assert_eq!(r.code, Some(2));
    assert_eq!(
        r.stderr,
        "nazm: error[N0401]: division by zero at lib/util.nz:1:33"
    );
}
