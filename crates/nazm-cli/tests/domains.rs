//! Gate 3: the domain reference workloads (`docs/internal/systems-domains.md` Parts E–J; `docs/user/spec.md`,
//! *Domain profiles*).
//!
//! Each workload in `tests/domains/` is held, as a locked package, to its domain's profile, and
//! built for the boards it runs on; the ignored tests boot each under QEMU in the `nazm-qemu:n86`
//! image at `-O0` and `-O2`. Every sensor is simulated, every board emulated: no claim here is
//! about hardware, and none is a certification.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::{run_bounded, Ran};

use std::path::{Path, PathBuf};
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/domains")
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(fixtures().join(name)).unwrap()
}

/// The workloads, their profiles, and whether a Cortex-M3 runs them: it has no floating point.
const WORKLOADS: [(&str, &str, bool); 6] = [
    ("robotics", "robotics", false),
    ("aerospace", "aerospace", false),
    ("automotive", "automotive", true),
    ("assurance", "assurance", true),
    ("industrial", "industrial", true),
    ("medical", "medical", true),
];

/// The CMSDK UART's data register on the MPS2, for the virt PL011's.
fn for_mps2(src: &str) -> String {
    src.replace(
        "static UART: Int = 150994944;",
        "static UART: Int = 1073758208;",
    )
}

/// `src` as the package `name`, locked, in a fresh directory with both boards' manifests.
fn package(name: &str, src: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let pkg = d.path().join(name);
    std::fs::create_dir_all(pkg.join("src")).unwrap();
    std::fs::write(
        pkg.join("nazm.toml"),
        format!(
            "schema = \"nazm.package/1\"\n[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nmain = \"main.nz\"\n"
        ),
    )
    .unwrap();
    std::fs::write(pkg.join("src/main.nz"), src).unwrap();
    for board in ["virt.toml", "mps2.toml", "virt-wd.toml", "mps2-wd.toml"] {
        std::fs::copy(fixtures().join(board), d.path().join(board)).unwrap();
    }
    let r = nazm(d.path(), &["lock", name]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    d
}

/// The package `name` built under `profile` for one board, into `out`.
fn build(d: &Path, name: &str, profile: &str, board: &str, opt: &str, out: &str) -> Ran {
    let target = if board == "mps2" {
        "thumbv7m-none-eabi"
    } else {
        "aarch64-unknown-none"
    };
    // The industrial controller's board has a watchdog.
    let manifest = if name == "industrial" {
        format!("{board}-wd.toml")
    } else {
        format!("{board}.toml")
    };
    nazm(
        d,
        &[
            "build",
            "--locked",
            "--profile",
            profile,
            "--opt-level",
            opt,
            "--target",
            target,
            "--board",
            &manifest,
            "--objects",
            out,
            name,
        ],
    )
}

#[test]
fn each_workload_meets_its_domain_profile_and_builds_for_its_boards() {
    for (name, profile, m3) in WORKLOADS {
        let d = package(name, &fixture(&format!("{name}.nz")));
        let r = nazm(d.path(), &["check", "--locked", "--profile", profile, name]);
        assert_eq!(r.code, Some(0), "{name}: {}", r.stderr);
        let r = build(d.path(), name, profile, "virt", "0", "o");
        assert_eq!(r.code, Some(0), "{name}: {}", r.stderr);
        if m3 {
            let d = package(name, &for_mps2(&fixture(&format!("{name}.nz"))));
            let r = build(d.path(), name, profile, "mps2", "0", "o");
            assert_eq!(r.code, Some(0), "{name} on the M3: {}", r.stderr);
        }
    }
}

/// A heapless board's runtime carries the C library's memory functions: code generation calls
/// `memcpy` to copy a `Mat3`, whatever allocates (Gate 3). Found by the robotics workload, whose
/// image did not link without them.
#[test]
fn a_heapless_boards_runtime_has_the_memory_functions() {
    let d = package("robotics", &fixture("robotics.nz"));
    let r = build(d.path(), "robotics", "robotics", "virt", "0", "o");
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let runtime = std::fs::read_dir(d.path().join("o"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.to_string_lossy().ends_with("runtime.o"))
        .unwrap();
    let bytes = std::fs::read(runtime).unwrap();
    for f in ["memcpy", "memmove", "memset", "memcmp"] {
        let name = format!("{f}\0");
        assert!(
            bytes.windows(name.len()).any(|w| w == name.as_bytes()),
            "{f}"
        );
    }
}

#[test]
fn the_profiles_tell_the_workloads_apart() {
    // The robotics controller's command is bounded with `float_sqrt`, which `aerospace` admits;
    // the transcendental functions it would refuse are not there.
    let d = package("robotics", &fixture("robotics.nz"));
    let r = nazm(
        d.path(),
        &["check", "--locked", "--profile", "aerospace", "robotics"],
    );
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    // An attitude from `float_atan2` is the host library's: `aerospace` refuses it.
    let atan = fixture("aerospace.nz").replace(
        "let incline = angle + noise();",
        "let incline = float_atan2(angle, 1.0) + noise();",
    );
    let d = package("aerospace", &atan);
    let r = nazm(
        d.path(),
        &["check", "--locked", "--profile", "aerospace", "aerospace"],
    );
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(
        r.stderr.contains("`no-float-nondeterminism`"),
        "{}",
        r.stderr
    );
    // `robotics` holds no package to a lockfile, so it admits the same program built loose.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.nz"), &atan).unwrap();
    let r = nazm(d.path(), &["check", "--profile", "robotics", "a.nz"]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    // A controller that recurses has no stack bound: `assurance` refuses it.
    let recursive = format!(
        "{}\nfn settle(t: Int, n: Int) -> Int ! {{}} {{ if n == 0 {{ t }} else {{ settle(plant(t, 0), n - 1) }} }}\n",
        fixture("assurance.nz")
    );
    let d = package("assurance", &recursive);
    let r = nazm(
        d.path(),
        &["check", "--locked", "--profile", "assurance", "assurance"],
    );
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(r.stderr.contains("`bounded-stack`"), "{}", r.stderr);
}

/// The watchdog is a board's, counted on its scheduler's ticks and fed from a task (Gate 3,
/// `docs/user/spec.md`, *Boards*); `industrial` asks that a task feed it.
#[test]
fn the_watchdog_is_a_boards_fed_from_a_task() {
    let src = fixture("industrial.nz");
    // A board without one.
    let d = package("industrial", &src);
    let r = nazm(
        d.path(),
        &[
            "build",
            "--target",
            "aarch64-unknown-none",
            "--board",
            "virt.toml",
            "--objects",
            "o",
            "industrial",
        ],
    );
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(r.stderr.contains("N0626"), "{}", r.stderr);
    assert!(r.stderr.contains("feeds a watchdog"), "{}", r.stderr);
    // A watchdog with no task to feed it.
    let still = "fn main(m: MmioCap) -> Int ! {} { 0 }\n";
    let d = package("still", still);
    let r = nazm(
        d.path(),
        &[
            "build",
            "--target",
            "aarch64-unknown-none",
            "--board",
            "virt-wd.toml",
            "--objects",
            "o",
            "still",
        ],
    );
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(r.stderr.contains("no `@task` to feed it"), "{}", r.stderr);
    // A watchdog of no ticks, or without the timer it counts.
    for (manifest, why) in [
        ("[watchdog]\nticks = 0\n", "watchdog.ticks"),
        ("[watchdog]\nticks = 5\nbark = 1\n", "watchdog.bark"),
    ] {
        let text = format!("{}\n{manifest}", fixture("virt.toml"));
        std::fs::write(d.path().join("bad.toml"), text).unwrap();
        let r = nazm(
            d.path(),
            &[
                "build",
                "--target",
                "aarch64-unknown-none",
                "--board",
                "bad.toml",
                "--objects",
                "o",
                "still",
            ],
        );
        assert!(
            r.stderr.contains("N0626") && r.stderr.contains(why),
            "{why}: {}",
            r.stderr
        );
    }
    let timerless: String = fixture("virt.toml")
        .lines()
        .filter(|l| !l.starts_with("timer_period_us"))
        .map(|l| format!("{l}\n"))
        .collect();
    std::fs::write(
        d.path().join("bad.toml"),
        format!("{timerless}\n[watchdog]\nticks = 5\n"),
    )
    .unwrap();
    let r = nazm(
        d.path(),
        &[
            "build",
            "--target",
            "aarch64-unknown-none",
            "--board",
            "bad.toml",
            "--objects",
            "o",
            "still",
        ],
    );
    assert!(r.stderr.contains("timer_period_us"), "{}", r.stderr);
    // Nothing but a board has one.
    let h = tempfile::tempdir().unwrap();
    std::fs::write(
        h.path().join("a.nz"),
        "fn main(m: MmioCap) -> Int ! {} { watchdog_feed() }\n",
    )
    .unwrap();
    let r = nazm(h.path(), &["run", "--interpret", "a.nz"]);
    assert!(r.stderr.contains("N0392"), "{}", r.stderr);
    // A feed is a device's authority.
    std::fs::write(
        h.path().join("b.nz"),
        "fn main() -> Int ! {} { watchdog_feed() }\n",
    )
    .unwrap();
    let r = nazm(h.path(), &["check", "b.nz"]);
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(r.stderr.contains("MmioCap"), "{}", r.stderr);
    // `industrial` asks that a task feed it: a controller that does not is refused at `main`.
    let unfed = src.replace("    watchdog_feed();\n", "");
    let d = package("industrial", &unfed);
    let r = nazm(
        d.path(),
        &["check", "--locked", "--profile", "industrial", "industrial"],
    );
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(r.stderr.contains("`watchdog`"), "{}", r.stderr);
    assert!(r.stderr.contains("watchdog_feed()"), "{}", r.stderr);
}

/// `medical` asks every function that changes a state to record it (Gate 3, `audit`): a change
/// made without `audit_transition` is refused by its site, and counting is not a change.
#[test]
fn medical_refuses_a_transition_it_has_no_record_of() {
    let src = fixture("medical.nz");
    let unrecorded = src.replace(
        "    let from = atomic_swap(STATE, to);\n    audit_transition(UART, m, 1, from, to)",
        // A call, but not a record's.
        "    pressure(atomic_swap(STATE, to))",
    );
    let d = package("medical", &unrecorded);
    let r = nazm(
        d.path(),
        &["check", "--locked", "--profile", "medical", "medical"],
    );
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(r.stderr.contains("`audit`"), "{}", r.stderr);
    assert!(
        r.stderr
            .contains("`transition` changes a state here and records no transition"),
        "{}",
        r.stderr
    );
    // Each of the three ways a state changes is a transition.
    for change in [
        "atomic_store(S, 1)",
        "atomic_swap(S, 1)",
        "if atomic_compare_swap(S, 0, 1) { 1 } else { 0 }",
    ] {
        let p = format!(
            "static S: Atomic = atomic_new(0);\nfn main(m: MmioCap) -> Int ! {{}} {{ {change} }}\n"
        );
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.nz"), &p).unwrap();
        let r = nazm(
            d.path(),
            &["check", "--profile", "medical", "--profile-report", "a.nz"],
        );
        let line = r
            .stdout
            .lines()
            .find(|l| l.contains("profile-report"))
            .unwrap()
            .to_owned();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        let audit = v["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == "audit")
            .unwrap();
        assert_eq!(audit["verdict"], "fail", "{change}");
    }
    // Counting is not a change of state.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("a.nz"),
        "static S: Atomic = atomic_new(0);\nfn main(m: MmioCap) -> Int ! {} { atomic_add(S, 1) }\n",
    )
    .unwrap();
    let r = nazm(
        d.path(),
        &["check", "--profile", "medical", "--profile-report", "a.nz"],
    );
    let line = r
        .stdout
        .lines()
        .find(|l| l.contains("profile-report"))
        .unwrap()
        .to_owned();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    let audit = v["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == "audit")
        .unwrap();
    assert_eq!(audit["verdict"], "pass");
}

#[test]
fn every_obligation_of_the_assurance_controller_is_proved_or_checked() {
    let d = package("assurance", &fixture("assurance.nz"));
    let r = nazm(
        d.path(),
        &["obligations", "--json", "assurance/src/main.nz"],
    );
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
    let statuses: Vec<&str> = v["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["status"].as_str().unwrap())
        .collect();
    assert!(
        statuses.iter().all(|s| *s == "proved" || *s == "checked"),
        "{statuses:?}"
    );
    assert!(statuses.contains(&"proved"), "{statuses:?}");
    // Each contract the controller states is among them.
    for kind in ["precondition", "postcondition", "call-precondition"] {
        assert!(
            v["obligations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|o| o["kind"] == kind),
            "{kind}"
        );
    }
}

/// `src` as the package `name`, built under `profile` at `opt` for `board`, linked and booted
/// under QEMU: its status and what it wrote.
fn boot(name: &str, profile: &str, src: &str, board: &str, opt: &str) -> (Option<i32>, String) {
    let src = if board == "mps2" {
        for_mps2(src)
    } else {
        src.to_owned()
    };
    let d = package(name, &src);
    let r = build(d.path(), name, profile, board, opt, "o");
    assert_eq!(r.code, Some(0), "{name} {board} -O{opt}: {}", r.stderr);
    let qemu = if board == "mps2" {
        "qemu-system-arm -M mps2-an385 -cpu cortex-m3 -nographic -net none -semihosting -kernel"
    } else {
        "qemu-system-aarch64 -M virt -cpu cortex-a53 -nographic -nic none -semihosting -kernel"
    };
    let out = Command::new("docker")
        .args(["run", "--rm", "--network", "none", "-v"])
        .arg(format!("{}:/w", d.path().display()))
        .args([
            "nazm-qemu:n86",
            "sh",
            "-c",
            &format!(
                "cd /w/o && ld.lld -T link.ld $(sed -n 's/^objects //p' link.txt) -o x.elf && \
                 timeout 60 {qemu} x.elf"
            ),
        ])
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

/// The robotics loop converges on its target, and a controller that outlasts its period stops
/// the machine safely.
#[test]
#[ignore = "starts a container"]
fn the_robotics_loop_converges_and_stops_safely_under_qemu() {
    let src = fixture("robotics.nz");
    let late = src.replace("let spin = 0;", "let spin = 5000000;");
    for opt in ["0", "2"] {
        let (code, got) = boot("robotics", "robotics", &src, "virt", opt);
        assert_eq!(code, Some(0), "-O{opt}: {got}");
        // x = 1.000, y = 2.000, z = 0.499 or 0.500, in thousandths.
        assert!(got.starts_with("1002000"), "-O{opt}: {got}");
        let (code, got) = boot("robotics", "robotics", &late, "virt", opt);
        assert_eq!(code, Some(2), "-O{opt}: {got}");
        assert!(got.contains("N0414"), "-O{opt}: {got}");
        assert!(got.ends_with("SAFE\n"), "-O{opt}: {got}");
    }
}

/// The aerospace loop's telemetry, its fault transition and its shutdown, the same at both levels.
#[test]
#[ignore = "starts a container"]
fn the_aerospace_loop_reports_faults_and_shuts_down_under_qemu() {
    let src = fixture("aerospace.nz");
    let (code, at_o0) = boot("aerospace", "aerospace", &src, "virt", "0");
    assert_eq!(code, Some(0), "{at_o0}");
    let lines: Vec<&str> = at_o0.lines().collect();
    assert_eq!(lines.first(), Some(&"T 50 246"), "{at_o0}");
    assert!(lines.contains(&"FAULT 300"), "{at_o0}");
    assert_eq!(lines.last(), Some(&"101"), "{at_o0}");
    let (code, at_o2) = boot("aerospace", "aerospace", &src, "virt", "2");
    assert_eq!(code, Some(0), "{at_o2}");
    assert_eq!(at_o0, at_o2, "the arithmetic is the same at either level");
}

/// The checksum a frame line `#IIII:SS:HH:LL*CC` should carry, and the one it does.
fn checksum(frame: &str) -> (u32, u32) {
    let (body, sum) = frame.trim_start_matches('#').split_once('*').unwrap();
    let bytes: Vec<u32> = body
        .split(':')
        .flat_map(|h| {
            let n = u32::from_str_radix(h, 16).unwrap();
            if h.len() == 4 {
                vec![n >> 8, n & 255]
            } else {
                vec![n]
            }
        })
        .collect();
    (
        bytes.iter().sum::<u32>() & 255,
        u32::from_str_radix(sum, 16).unwrap(),
    )
}

/// The ECU's state machine across its frames, the same on both boards at both levels.
#[test]
#[ignore = "starts a container"]
fn the_ecu_runs_its_state_machine_on_both_boards_under_qemu() {
    let src = fixture("automotive.nz");
    let mut first: Option<String> = None;
    for board in ["virt", "mps2"] {
        for opt in ["0", "2"] {
            let (code, got) = boot("automotive", "automotive", &src, board, opt);
            assert_eq!(code, Some(0), "{board} -O{opt}: {got}");
            let frames: Vec<&str> = got.lines().filter(|l| l.starts_with('#')).collect();
            assert_eq!(frames.len(), 31, "{board} -O{opt}: {got}");
            for f in &frames {
                let (want, have) = checksum(f);
                assert_eq!(want, have, "{board} -O{opt}: {f}");
            }
            let mut states: Vec<&str> = frames.iter().map(|f| &f[6..8]).collect();
            states.dedup();
            assert_eq!(states, ["01", "02", "03"], "{board} -O{opt}: {got}");
            assert_eq!(got.lines().last(), Some("30700"), "{board} -O{opt}: {got}");
            match &first {
                None => first = Some(got),
                Some(f) => assert_eq!(f, &got, "{board} -O{opt}"),
            }
        }
    }
}

/// The heater controller holds its temperature on both boards, and trips, the heater off for good,
/// when its limit is below what it holds.
#[test]
#[ignore = "starts a container"]
fn the_assurance_controller_holds_and_trips_on_both_boards_under_qemu() {
    let src = fixture("assurance.nz");
    let hot = src.replace("t > 800", "t > 450");
    for board in ["virt", "mps2"] {
        for opt in ["0", "2"] {
            let (code, got) = boot("assurance", "assurance", &src, board, opt);
            assert_eq!(code, Some(0), "{board} -O{opt}: {got}");
            assert_eq!(got, "481\n", "{board} -O{opt}");
            let (code, got) = boot("assurance", "assurance", &hot, board, opt);
            assert_eq!(code, Some(0), "{board} -O{opt}: {got}");
            // Tripped, and cooled towards the room's 20.0 degrees with the heater off.
            let n: i64 = got.trim().parse().unwrap();
            assert!(
                (10_000..10_450).contains(&n),
                "tripped: {board} -O{opt}: {got}"
            );
        }
    }
}

/// The batch controller's three batches on both boards; its fail-safe state when the tank
/// overfills; and the watchdog, when the loop hangs, stopping the machine with its outputs safe.
/// The hang is one `industrial` admits: a loop whose bound is stated, and too long to finish.
#[test]
#[ignore = "starts a container"]
fn the_industrial_controller_batches_fails_safe_and_is_watched_under_qemu() {
    let src = fixture("industrial.nz");
    let overfill = src.replace("state == 1 && level >= 100", "state == 1 && level >= 200");
    // A loop `bounded-loops` bounds — 10^12 turns — that no deadline survives: a bound is not an
    // execution time, which is what the watchdog watches.
    let hang = src.replace(
        "    plant();\n",
        "    if tick == 25 {\n        let mut j = 0;\n        while j < 1000000000000 {\n            \
         atomic_add(SPIN, 1);\n            j = j + 1;\n        }\n    };\n    plant();\n",
    );
    for board in ["virt", "mps2"] {
        for opt in ["0", "2"] {
            let (code, got) = boot("industrial", "industrial", &src, board, opt);
            assert_eq!(code, Some(0), "{board} -O{opt}: {got}");
            assert_eq!(got, "IFHDIFHDIFHDI\n3\n", "{board} -O{opt}");
            let (code, got) = boot("industrial", "industrial", &overfill, board, opt);
            assert_eq!(code, Some(0), "{board} -O{opt}: {got}");
            assert!(got.starts_with("IFS\n9"), "{board} -O{opt}: {got}");
            let (code, got) = boot("industrial", "industrial", &hang, board, opt);
            assert_eq!(code, Some(2), "{board} -O{opt}: {got}");
            assert!(got.contains("N0415"), "{board} -O{opt}: {got}");
            assert!(got.ends_with("SAFE\n"), "{board} -O{opt}: {got}");
        }
    }
}

/// The pump's run on both boards: each transition's audit record a JSON object on its own line,
/// in order, and the fault state on an occlusion.
#[test]
#[ignore = "starts a container"]
fn the_infusion_pump_records_every_transition_under_qemu() {
    let src = fixture("medical.nz");
    let occluded = src.replace(
        "    40 + tick - tick",
        "    if tick >= 20 { 400 } else { 40 }",
    );
    let records = |out: &str| -> Vec<(i64, i64)> {
        out.lines()
            .filter(|l| l.starts_with('{'))
            .map(|l| {
                let v: serde_json::Value = serde_json::from_str(l).unwrap();
                assert_eq!(v["audit"], 1, "{l}");
                (v["from"].as_i64().unwrap(), v["to"].as_i64().unwrap())
            })
            .collect()
    };
    for board in ["virt", "mps2"] {
        for opt in ["0", "2"] {
            let (code, got) = boot("medical", "medical", &src, board, opt);
            assert_eq!(code, Some(0), "{board} -O{opt}: {got}");
            assert_eq!(
                records(&got),
                [(0, 0), (0, 1), (1, 2), (2, 3), (3, 2), (2, 0)],
                "{board} -O{opt}: {got}"
            );
            assert_eq!(got.lines().last(), Some("500"), "{board} -O{opt}");
            let (code, got) = boot("medical", "medical", &occluded, board, opt);
            assert_eq!(code, Some(0), "{board} -O{opt}: {got}");
            assert_eq!(
                records(&got).last(),
                Some(&(2, 4)),
                "{board} -O{opt}: {got}"
            );
            assert_eq!(got.lines().last(), Some("4055"), "{board} -O{opt}");
        }
    }
}
