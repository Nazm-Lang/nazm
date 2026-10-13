//! Gate 3: the domain profiles (`docs/spec.md`, *Restriction profiles*, *Domain profiles*;
//! `docs/systems-domains.md` Parts E–J).
//!
//! Each domain profile is a composition of the rules that already exist, with at most a domain rule
//! of its own; none changes what an accepted program does, and none is a certification. These tests
//! hold each composition to its definition, show the domain rules refusing by site, and hold a
//! board's periodic program, as a locked package, to every domain profile it is written for.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::{run_bounded, Ran};

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use nazm_service::profile::{Profile, Rule};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

fn rules(p: Profile) -> BTreeSet<Rule> {
    p.rules().iter().copied().collect()
}

fn union(ps: &[Profile], extra: &[Rule]) -> BTreeSet<Rule> {
    ps.iter()
        .flat_map(|p| p.rules().iter().copied())
        .chain(extra.iter().copied())
        .collect()
}

#[test]
fn each_domain_profile_is_its_composition() {
    use Profile::{
        Aerospace, Assurance, Automotive, Critical, Embedded, Industrial, Medical, Realtime,
        Robotics,
    };
    assert_eq!(
        rules(Robotics),
        union(&[Realtime], &[Rule::ExplicitAuthority])
    );
    assert_eq!(rules(Automotive), union(&[Realtime, Critical], &[]));
    assert_eq!(
        rules(Aerospace),
        union(
            &[Embedded, Realtime, Critical],
            &[Rule::NoFloatNondeterminism]
        )
    );
    assert_eq!(
        rules(Assurance),
        union(
            &[Critical],
            &[
                Rule::NoRecursion,
                Rule::BoundedLoops,
                Rule::BoundedStack,
                Rule::NoHeap
            ]
        )
    );
    assert_eq!(
        rules(Industrial),
        union(&[Realtime, Critical], &[Rule::Watchdog])
    );
    assert_eq!(rules(Medical), union(&[Realtime, Critical], &[Rule::Audit]));
    // Each is a profile by its name, and no rule appears twice in one.
    for p in [
        Robotics, Automotive, Aerospace, Assurance, Industrial, Medical,
    ] {
        assert_eq!(Profile::by_name(p.name()), Some(p));
        assert_eq!(p.rules().len(), rules(p).len(), "{}", p.name());
    }
}

/// The verdict `--profile-report` gives each rule of `profile` for `src`, a loose file.
fn verdicts(src: &str, profile: &str) -> Vec<(String, String)> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let r = nazm(
        d.path(),
        &["check", "--profile", profile, "--profile-report", "prog.nz"],
    );
    let line = r
        .stdout
        .lines()
        .find(|l| l.contains("nazm.profile-report/1"))
        .unwrap_or_else(|| panic!("no report: {r:?}"))
        .to_owned();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    v["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["id"].as_str().unwrap().to_owned(),
                r["verdict"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn verdict_of<'a>(v: &'a [(String, String)], id: &str) -> &'a str {
    v.iter()
        .find(|(r, _)| r == id)
        .map_or("absent", |(_, x)| x.as_str())
}

#[test]
fn aerospace_refuses_a_float_the_host_library_answers_by_its_site() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn heading(x: Float64, y: Float64) -> Float64 ! {} { float_atan2(y, x) }\n\
         fn main() -> Int ! {} { if heading(1.0, 1.0) > 0.0 { 0 } else { 1 } }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "--profile", "aerospace", "prog.nz"]);
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(
        r.stderr.contains("`no-float-nondeterminism`"),
        "{}",
        r.stderr
    );
    assert!(r.stderr.contains("prog.nz:1:"), "{}", r.stderr);
    // Every transcendental built-in is refused, and the correctly rounded ones are not.
    for f in [
        "float_sin(x)",
        "float_cos(x)",
        "float_tan(x)",
        "float_exp(x)",
        "float_ln(x)",
        "float_log2(x)",
        "float_log10(x)",
        "float_pow(x, x)",
        "float_atan2(x, x)",
    ] {
        let src = format!(
            "fn f(x: Float64) -> Float64 ! {{}} {{ {f} }}\nfn main() -> Int ! {{}} {{ if f(2.0) > 0.0 {{ 0 }} else {{ 1 }} }}\n"
        );
        assert_eq!(
            verdict_of(&verdicts(&src, "aerospace"), "no-float-nondeterminism"),
            "fail",
            "{f}"
        );
    }
    for f in [
        "float_sqrt(x)",
        "float_abs(x)",
        "float_floor(x)",
        "x * x + 1.0",
    ] {
        let src = format!(
            "fn f(x: Float64) -> Float64 ! {{}} {{ {f} }}\nfn main() -> Int ! {{}} {{ if f(2.0) > 0.0 {{ 0 }} else {{ 1 }} }}\n"
        );
        assert_eq!(
            verdict_of(&verdicts(&src, "aerospace"), "no-float-nondeterminism"),
            "pass",
            "{f}"
        );
    }
    // Without the profile, the program is the language's.
    assert_eq!(nazm(d.path(), &["check", "prog.nz"]).code, Some(0));
}

#[test]
fn the_domain_profiles_refuse_by_the_rules_they_compose() {
    // A call cycle: `assurance` and `automotive` have no stack bound for it.
    let cycle = "fn down(n: Int) -> Int ! {} { if n == 0 { 0 } else { down(n - 1) } }\n\
                 fn main() -> Int ! {} { down(3) }\n";
    for p in ["assurance", "automotive", "robotics", "aerospace"] {
        assert_eq!(
            verdict_of(&verdicts(cycle, p), "bounded-stack"),
            "fail",
            "{p}"
        );
    }
    // A loose file is no locked package.
    let plain = "fn main() -> Int ! {} { 0 }\n";
    for p in ["assurance", "automotive", "aerospace"] {
        assert_eq!(
            verdict_of(&verdicts(plain, p), "locked-build"),
            "fail",
            "{p}"
        );
    }
    assert_eq!(
        verdict_of(&verdicts(plain, "robotics"), "locked-build"),
        "absent"
    );
    // An allocation: `assurance` holds memory to the stack and the statics.
    let alloc = "fn main() -> Int ! {} { let v = vec_new[Int](); vec_push(v, 1); vec_len(v) }\n";
    assert_eq!(verdict_of(&verdicts(alloc, "assurance"), "no-heap"), "fail");
}

/// A board's periodic program as a locked package: accepted by every domain profile it is written
/// for, and built for the board under each.
#[test]
fn a_locked_board_package_meets_each_domain_profile() {
    let d = tempfile::tempdir().unwrap();
    let pkg = d.path().join("ecu");
    std::fs::create_dir_all(pkg.join("src")).unwrap();
    std::fs::write(
        pkg.join("nazm.toml"),
        "schema = \"nazm.package/1\"\n[package]\nname = \"ecu\"\nversion = \"0.1.0\"\nmain = \"main.nz\"\n",
    )
    .unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/realtime_tasks/periodic.nz"),
        pkg.join("src/main.nz"),
    )
    .unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/realtime_tasks/virt.toml"),
        d.path().join("virt.toml"),
    )
    .unwrap();
    let r = nazm(d.path(), &["lock", "ecu"]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    for p in ["robotics", "automotive", "aerospace", "assurance"] {
        let r = nazm(d.path(), &["check", "--locked", "--profile", p, "ecu"]);
        assert_eq!(r.code, Some(0), "{p}: {}", r.stderr);
        let r = nazm(
            d.path(),
            &[
                "build",
                "--locked",
                "--profile",
                p,
                "--target",
                "aarch64-unknown-none",
                "--board",
                "virt.toml",
                "--objects",
                p,
                "ecu",
            ],
        );
        assert_eq!(r.code, Some(0), "{p}: {}", r.stderr);
    }
}
