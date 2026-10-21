//! `cargo xtask contained`: choosing a workload's profile and bounds, running it in the
//! container, collecting its journal, and the cache and probe subcommands.

use std::process::{Command, ExitCode};

use crate::{campaign, contained};

/// `cargo xtask contained <workload>` — the only sanctioned way to run a generated
/// compiler stage, the selfhost suite, the bootstrap, fuzzing, the mutation catalogue or
/// a heavy benchmark. See `contained.rs` for why an environment variable is not enough.
pub(crate) fn run_contained(args: &[String]) -> ExitCode {
    use contained::{Bounds, Outcome};
    use std::time::Duration;

    let repo = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("contained: cannot resolve the repository root: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut bounds = Bounds::default();

    // `--platform amd64` (Gate 3): the workload runs in the `linux/amd64` image, emulated on an
    // Apple-silicon host. Before `--profile`, so either order of the two is not needed.
    let args = match args.first().map(String::as_str) {
        Some("--platform") => {
            if args.get(1).map(String::as_str) != Some("amd64") {
                eprintln!(
                    "contained: --platform takes `amd64`; without it the host's own image runs"
                );
                return ExitCode::FAILURE;
            }
            contained::use_amd64(true);
            args.get(2..).unwrap_or_default()
        }
        _ => {
            contained::use_amd64(false);
            args
        }
    };

    // `--profile NAME` before the workload chooses its internal parallelism (N14.1).
    // Without one, each workload takes the profile its calibration adopted.
    let (chosen, args) = match args.first().map(String::as_str) {
        Some("--profile") => {
            let Some(p) = args
                .get(1)
                .and_then(|n| contained::ExecutionProfile::by_name(n))
            else {
                let names: Vec<&str> = contained::ExecutionProfile::ALL
                    .iter()
                    .map(|p| p.name)
                    .collect();
                eprintln!("contained: --profile is one of {}", names.join(", "));
                return ExitCode::FAILURE;
            };
            (Some(p), args.get(2..).unwrap_or_default())
        }
        _ => (None, args),
    };
    // `--cache bench` (N32-H3): a benchmark workload's release builds kept in their own
    // harness-owned namespace. What a benchmark measures is the binary it runs, which is
    // the same bytes wherever it was compiled; only the compile is saved.
    let (explicit_cache, args) = match args.first().map(String::as_str) {
        Some("--cache") => {
            match args.get(1).map(String::as_str) {
                Some("bench") => (Some("bench"), args.get(2..).unwrap_or_default()),
                _ => {
                    eprintln!("contained: --cache takes `bench`; the other namespaces are the workloads' own");
                    return ExitCode::FAILURE;
                }
            }
        }
        _ => (None, args),
    };
    let workload = args.first().map(String::as_str).unwrap_or_default();
    let profile = chosen.unwrap_or_else(|| default_profile(workload));
    bounds.profile = profile;
    bounds.milli_cpus = profile.milli_cpus;
    let j = profile.cargo_jobs;

    let (name, argv): (&str, Vec<String>) = match args.first().map(String::as_str) {
        Some("probe") => return contained_probe(&repo),
        Some("cache") => return contained_cache(args.get(1).map(String::as_str)),
        Some("selfhost") => (
            "selfhost",
            shell(&cargo_test("-p nazm-cli --test selfhost", profile)),
        ),
        // The reference front end is **built here**, not taken from the image.
        //
        // `compiler/bootstrap.sh` requires `$nazm` to exist and does not build it, and
        // inside the container `CARGO_TARGET_DIR` is `/work/target` — which the image
        // warms with a binary from the day it was built. Until 2026-09-22 that binary was
        // what stage 1 used and what the reference check ran, so a bootstrap could report
        // on a front end months older than the source mounted at `/src` and say nothing
        // about it. The build is incremental against the image's warm cache and costs
        // seconds; the alternative is evidence that silently names the wrong compiler.
        Some("bootstrap") => {
            bounds.deadline = Duration::from_secs(1800);
            (
                "bootstrap",
                shell(&format!(
                    "cargo build --locked -p nazm-cli --jobs {j} \
                     && sh compiler/bootstrap.sh /work/bootstrap"
                )),
            )
        }
        // R6 (`rewrite-in-nazm` 7.1): the Nazm toolchain builds itself — the seed builds the
        // builder (front end, pipeline, code generator, each written in Nazm), and each stage's
        // builder builds the next, under the bounded heap, to a fixpoint. The seed is built
        // here for the reason the bootstrap's is.
        // `selfbuild stages=N` narrows a run for a slice's diagnosis, and `backend=llvm` builds every
        // stage on the LLVM backend (R8 slice 5); nothing else is passed.
        Some("selfbuild") => {
            bounds.deadline = Duration::from_secs(3600);
            let mut settings = String::new();
            for a in args.get(1..).unwrap_or_default() {
                let ok = a
                    .strip_prefix("stages=")
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                    || matches!(a.as_str(), "backend=nazm" | "backend=llvm");
                if !ok {
                    eprintln!(
                        "contained: selfbuild takes only `stages=N` and `backend=nazm|llvm`, not {a:?}"
                    );
                    return ExitCode::FAILURE;
                }
                settings.push(' ');
                settings.push_str(a);
            }
            (
                "selfbuild",
                shell(&format!(
                    "cargo build --locked -p nazm-cli --jobs {j} \
                     && bash toolchain/selfbuild/selfbuild.sh /work/selfbuild{settings}"
                )),
            )
        }
        // A scheduled profile runs the same binaries through `xtask workspace-tests`, built
        // here in the same target; any other runs `cargo test --workspace` as it always did.
        // Either way the container's memory peak is reported after the suite (N32-H3).
        Some("tests") => {
            let suite = if profile.test_workers > 1 || profile.name == "p4w1t4" {
                format!(
                    "cargo build --locked -q -p xtask --jobs {j} && /work/target/debug/xtask workspace-tests \
                     --jobs {j} --workers {} --test-threads {} --shape {}",
                    profile.test_workers, profile.test_threads, profile.name
                )
            } else {
                cargo_test("--workspace", profile)
            };
            (
                "tests",
                shell(&format!(
                    "{suite}; s=$?; echo \"contained: memory peak $(cat /sys/fs/cgroup/memory.peak 2>/dev/null) bytes\" >&2; exit $s"
                )),
            )
        }
        // The release candidate: every gate, then the candidate archived and handed out.
        //
        // It has to be exported, because `/work` is the container's writable layer and
        // goes with the container — a candidate assembled there and merely reported on is
        // a candidate nobody has. The archive is deterministic in the ways that are free
        // to make so (sorted entries, fixed owner and mtime), and since R1 every date inside
        // it is the commit's (SOURCE_DATE_EPOCH, below) — so two runs of the same revision
        // are meant to produce the same bytes, which the release record checks.
        Some("release") => {
            bounds.deadline = Duration::from_secs(2400);
            let dest = repo.join("target").join("release-candidate");
            bounds.export = Some(contained::ExportPolicy {
                manifest: "/work/rc/EXPORTS".to_owned(),
                dest: dest.clone(),
                allowed: vec![
                    "nazm-release-candidate.tar.gz".to_owned(),
                    "MANIFEST.txt".to_owned(),
                    "PROVENANCE.txt".to_owned(),
                ],
                // The archive carries a release binary and the whole documentation set.
                // 64 MiB is generous for that and still far below the writable layer's
                // own cap, so a runaway cannot reach the host through this path.
                max_bytes: 64 * 1024 * 1024,
            });
            if let Err(e) = std::fs::create_dir_all(&dest) {
                eprintln!("contained: cannot prepare {}: {e}", dest.display());
                return ExitCode::FAILURE;
            }
            // The revision, established here rather than inside. `git` is not in the
            // image and has no business being there, so the container cannot answer
            // "which commit is this?" — it would record `unknown` and a clean tree it had
            // no way to check. The supervisor mounted the tree and knows both.
            let commit = Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&repo)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
                .unwrap_or_default();
            let porcelain = Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(&repo)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
            let Some(dirty) = porcelain else {
                eprintln!(
                    "contained: cannot read the tree's state with git, so the candidate \
                     could not be attributed to a revision"
                );
                return ExitCode::FAILURE;
            };
            if commit.is_empty() {
                eprintln!("contained: cannot establish the source commit");
                return ExitCode::FAILURE;
            }
            let clean = if dirty.is_empty() { "yes" } else { "no" };
            // The commit's own time, as SOURCE_DATE_EPOCH: every date written into the
            // candidate is this instant rather than the run's, so two runs of one commit can
            // produce the same archive (R1). Digits only, or nothing is passed.
            let epoch = Command::new("git")
                .args(["log", "-1", "--format=%ct", "HEAD"])
                .current_dir(&repo)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
            let Some(epoch) = epoch else {
                eprintln!("contained: cannot read the commit's time, so the candidate's dates would be the run's");
                return ExitCode::FAILURE;
            };
            // Only the verdict crosses, not the porcelain text: a changed path may contain
            // a quote, and interpolating it into a shell command is how a filename becomes
            // an instruction.
            let identity = format!(
                "NAZM_SOURCE_COMMIT={commit} NAZM_SOURCE_TREE_CLEAN={clean} SOURCE_DATE_EPOCH={epoch} "
            );
            (
                "release",
                shell(
                    &(identity
                        + "sh scripts/release-candidate.sh \
                     && mkdir -p /work/rc \
                     && cp /work/target/release-candidate/MANIFEST.txt /work/rc/ \
                     && cp /work/target/release-candidate/PROVENANCE.txt /work/rc/ \
                     && tar --sort=name --owner=0 --group=0 --numeric-owner --mtime=@0 \
                            -czf /work/rc/nazm-release-candidate.tar.gz \
                            -C /work/target/release-candidate . \
                     && cd /work/rc \
                     && for f in nazm-release-candidate.tar.gz MANIFEST.txt PROVENANCE.txt; do \
                            printf '%s %s\\n' \"$f\" \"$(wc -c < $f | tr -d ' ')\"; \
                        done > EXPORTS \
                     && cat EXPORTS"),
                ),
            )
        }
        Some("mutate") => {
            // The catalogue edits sources, so it needs a tree of its own. `/src` is
            // read-only and stays that way; the copy is disposable and lives on the
            // capped writable layer. `.git` is excluded because nothing here reads it.
            //
            // `.nazm` is excluded too, and the runner is told the copy is disposable so
            // it clears derived caches before every test step (`campaign::reset_derived`
            // says why: a check entry is keyed by the checker's declared identity, which
            // a mutant shares with the pristine checker).
            //
            // One container is one campaign session: one warm worker. One copy, one
            // pristine build, one baseline, then every mutant in turn against the same
            // target directory — at most one mutant injected at a time, each restored and
            // digest-checked before the next. A session that runs out of budget ends
            // *incomplete* and `--resume` continues it from the journal in a new one.
            //
            // The target is `/work/target`, which the image warms with the workspace's
            // registry dependencies: the container's own writable layer, discarded with
            // it, so nothing is shared with the host or with another session. Workspace
            // crates live at `/work/mut`, a path the image's artefacts were never built
            // for, so every one of them is compiled from the copy; only dependencies whose
            // package, version and features are unchanged are reused, which is what
            // Cargo's fingerprints key them on. N32-H: this halved the cold build.
            bounds.deadline = Duration::from_secs(3000);
            let passed = match mutate_passthrough(args.get(1..).unwrap_or_default()) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("contained: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let commit = Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&repo)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
                .filter(|c| c.chars().all(|ch| ch.is_ascii_hexdigit()))
                .unwrap_or_default();
            let commit = if commit.is_empty() {
                String::new()
            } else {
                format!(" --source-commit {commit}")
            };
            // Budget: stop starting mutants with enough of the deadline left for the one
            // in flight to finish in the ordinary case. Past it, the session is
            // incomplete and resumable, not failed.
            let budget = if passed.contains("--budget-secs")
                || passed.contains("--plan")
                || passed.contains("--verify-killer")
            {
                String::new()
            } else {
                " --budget-secs 2400".to_owned()
            };
            // `timeout` inside the container, so a supervisor that dies without killing
            // it still leaves a container that stops itself. TERM, not KILL: `timeout`
            // then exits 124, which says "the session ran out" in a way an exit of 137
            // cannot — the contained runner reads 137 as a memory kill, and on
            // 2026-09-24 it did, for a session whose only problem was a mutant still in
            // flight at the deadline. `exec` all the way down, so
            // a SIGINT the docker client proxies reaches the runner itself, and when it
            // dies the pid namespace takes every descendant with it.
            let deadline = bounds.deadline.as_secs().saturating_sub(20);
            let par = mutate_parallelism(profile);
            (
                "mutate",
                shell(&format!(
                    "rm -rf /work/mut && mkdir -p /work/mut /work/campaigns \
                     && tar -C /src -cf - --exclude=./target --exclude=./.git --exclude=.nazm . | tar -C /work/mut -xmf - \
                     && cd /work/mut && export CARGO_TARGET_DIR=/work/target \
                     && cargo build -q -p xtask --jobs {j} \
                     && exec timeout -s TERM -k 15 {deadline} /work/target/debug/xtask mutate \
                        --reset-derived-caches --disposable-target --export-journal --campaign-dir /work/campaigns \
                        --resume-from /src/target/mutation-campaigns {par}{commit}{budget}{passed}"
                )),
            )
        }
        Some("bench") => {
            // Results are produced in bounded container storage and exported by name.
            // The checkout stays read-only: a writable bind mount would put every write
            // on the host filesystem, which `--storage-opt` does not bound.
            bounds.deadline = Duration::from_secs(2400);
            let saving = args.iter().any(|a| a == "--save");
            let mode = if saving { "--save" } else { "--check" };
            // The supervisor decides the permitted names, from the mode it was asked for
            // and the platform it measures itself — never from the manifest, which is the
            // workload's own output. A check may export the record and nothing else, so
            // an ordinary check cannot write a baseline at all.
            let slug = match contained::image_platform_slug() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("contained: cannot establish the image's platform: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let mut allowed = vec!["record.json".to_owned()];
            if saving {
                allowed.push(format!("baseline-{slug}.json"));
            }
            bounds.export = Some(contained::ExportPolicy {
                manifest: "/work/benchout/EXPORTS".to_owned(),
                dest: repo.join("bench"),
                allowed,
                // A record is one JSON object per measurement — a few kilobytes. A
                // megabyte is already far more than it can legitimately be, and far below
                // anything that could fill a disk.
                max_bytes: 1024 * 1024,
            });
            // The slug is computed from the machine that ran the benchmarks, not from
            // this one. A Linux run must name `baseline-linux-aarch64.json` even though
            // the host calling it is Darwin.
            //
            // On a check run the manifest lists only the record, so an ordinary check
            // cannot write a baseline at all. The seeded `baseline.json` is never listed:
            // it is another platform's file, copied in only so the comparison can read it.
            let manifest = if saving {
                r#"slug=$(uname -sm | tr 'A-Z ' 'a-z-' | tr -c 'a-z0-9-' '-' | sed 's/-*$//');                    : > /work/benchout/EXPORTS;                    for f in record.json baseline-$slug.json; do                      [ -f /work/benchout/$f ] && echo "$f $(wc -c < /work/benchout/$f)" >> /work/benchout/EXPORTS;                    done"#
            } else {
                r#": > /work/benchout/EXPORTS;                    [ -f /work/benchout/record.json ] && echo "record.json $(wc -c < /work/benchout/record.json)" >> /work/benchout/EXPORTS"#
            };
            (
                "bench",
                shell(&format!(
                    "mkdir -p /work/benchout \
                     && cp /src/bench/*.json /work/benchout/ 2>/dev/null; \
                     export NAZM_BENCH_DIR=/work/benchout; \
                     cargo build --release --locked -p nazm-cli --jobs 1 >/dev/null \
                     && cargo run -q -p xtask -- bench --runs 5 {mode} \
                     && if find /work/benchout -name '*.json' -size +1M | grep -q .; then \
                          echo 'bench: an artefact exceeded 1 MiB; refusing to export' >&2; exit 3; \
                        fi \
                     && {manifest}"
                )),
            )
        }
        // The coverage-guided fuzz campaign (N93): `fuzz [SECONDS] [TARGET…]`, each target for
        // SECONDS (300 by default). Its minimised corpora and its report are exported by name
        // into `target/fuzz/`; nothing else leaves the container.
        Some("fuzz") => {
            let seconds: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(300);
            let targets: Vec<&str> = args
                .iter()
                .skip(2)
                .map(String::as_str)
                .filter(|t| ["front", "lower", "run", "lir", "manifest"].contains(t))
                .collect();
            let count = if targets.is_empty() { 5 } else { targets.len() } as u64;
            // The cold instrumented build, then each target twice its budget (the run and the
            // minimisation).
            bounds.deadline = Duration::from_secs(1800 + seconds * 2 * count);
            let dest = repo.join("target/fuzz");
            if let Err(e) = std::fs::create_dir_all(&dest) {
                eprintln!("contained: cannot make {}: {e}", dest.display());
                return ExitCode::FAILURE;
            }
            bounds.export = Some(contained::ExportPolicy {
                manifest: "/work/fuzzout/EXPORTS".to_owned(),
                dest,
                allowed: vec!["fuzz-min.tar".to_owned(), "report.txt".to_owned()],
                max_bytes: 8 * 1024 * 1024,
            });
            (
                "fuzz",
                shell(&format!(
                    "CARGO_BUILD_JOBS={j} sh /src/fuzz/run.sh {seconds} {}; s=$?; : > /work/fuzzout/EXPORTS; \
                     for f in fuzz-min.tar report.txt; do \
                       [ -f /work/fuzzout/$f ] && echo \"$f $(wc -c < /work/fuzzout/$f)\" >> /work/fuzzout/EXPORTS; \
                     done; exit $s",
                    targets.join(" ")
                )),
            )
        }
        // A prebuilt toolchain for the image's own host (Gate 3, `docs/internal/systems-domains.md` Part
        // A): the release binary packaged by `scripts/host-archive.sh` and exported by name into
        // `target/archives/`. The name is the supervisor's, from the platform it measures.
        Some("archive") => {
            bounds.deadline = Duration::from_secs(3600);
            let slug = match contained::image_platform_slug() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("contained: cannot establish the image's platform: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let triple = match slug.as_str() {
                "linux-aarch64" => "aarch64-unknown-linux-gnu",
                "linux-x86-64" => "x86_64-unknown-linux-gnu",
                other => {
                    eprintln!(
                        "contained: no host archive is defined for the image's platform `{other}`"
                    );
                    return ExitCode::FAILURE;
                }
            };
            let dest = repo.join("target/archives");
            if let Err(e) = std::fs::create_dir_all(&dest) {
                eprintln!("contained: cannot make {}: {e}", dest.display());
                return ExitCode::FAILURE;
            }
            let name = format!("nazm-{triple}.tar.gz");
            bounds.export = Some(contained::ExportPolicy {
                manifest: "/work/archiveout/EXPORTS".to_owned(),
                dest,
                allowed: vec![name.clone()],
                // A release binary with everything inside it, compressed: tens of megabytes.
                max_bytes: 128 * 1024 * 1024,
            });
            (
                "archive",
                shell(&format!(
                    "mkdir -p /work/archiveout \
                     && cargo build --release --locked -p nazm-cli --jobs {j} \
                     && sh /src/scripts/host-archive.sh {triple} /work/target/release/nazm /work/archiveout \
                     && echo \"{name} $(wc -c < /work/archiveout/{name})\" > /work/archiveout/EXPORTS"
                )),
            )
        }
        Some("run") => {
            let cmd = args.get(1..).unwrap_or_default().join(" ");
            if cmd.trim().is_empty() {
                eprintln!("contained: `run` needs a command");
                return ExitCode::FAILURE;
            }
            ("run", shell(&cmd))
        }
        other => {
            eprintln!(
                "contained: unknown workload {:?}\n  probe | tests | selfhost | bootstrap | selfbuild | bench | mutate | fuzz [SECONDS] [TARGET...] | archive | run <command>",
                other.unwrap_or("<none>")
            );
            return ExitCode::FAILURE;
        }
    };

    // The persistent verification target, for the workloads that build the workspace and
    // are judged on it (N32-H2). A benchmark, a release and an arbitrary `run` never share
    // it: their numbers and artefacts must not depend on what an earlier session left.
    if let Some(namespace) = contained::cache_namespace(name).or(explicit_cache) {
        match contained::cache_ensure(&repo, namespace) {
            Ok(v) => bounds.cache = Some(v),
            Err(e) => {
                eprintln!("contained: cannot prepare the verification cache: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    let borrowed: Vec<&str> = argv.iter().map(String::as_str).collect();
    // An emulated image runs several times slower than the host's own; its deadline is four
    // times the native one, so a slow run is not mistaken for a hung one (Gate 3).
    if contained::image() == contained::IMAGE_AMD64 {
        bounds.deadline *= 4;
        eprintln!(
            "contained: linux/amd64 under emulation; the deadline is four times the native one"
        );
    }
    eprintln!(
        "contained: {name} — {} MiB memory (swap equal), {:.1} CPU, {} pids, {} GiB disk, {} s deadline",
        bounds.memory_mb,
        f64::from(bounds.milli_cpus) / 1000.0,
        bounds.pids,
        bounds.storage_gb,
        bounds.deadline.as_secs()
    );
    let r = contained::run(&repo, name, &borrowed, &bounds);
    if name == "mutate" {
        collect_journal(&repo, &r.stdout);
    }
    print!("{}", r.stdout);
    eprint!("{}", r.stderr);
    eprintln!(
        "\ncontained: {name} {} in {:.1}s — stdout {} B, stderr {} B, oom_killed={}, survivors={}",
        r.outcome.word(),
        r.elapsed.as_secs_f64(),
        r.stdout_bytes,
        r.stderr_bytes,
        r.oom_killed,
        contained::survivors()
    );
    match r.outcome {
        Outcome::Passed => ExitCode::SUCCESS,
        Outcome::Unavailable(why) => {
            // Explicit refusal. A silent skip would report green on no evidence.
            eprintln!("contained: REFUSED — containment is unavailable: {why}");
            ExitCode::FAILURE
        }
        Outcome::Failed(124) if name == "mutate" => {
            eprintln!(
                "contained: mutate ended INCOMPLETE — the session deadline passed with a \
                 mutant still in flight. That mutant was restored with the container and never \
                 journaled, so it has no verdict and runs again. Continue with \
                 `cargo xtask contained mutate --resume` and the same selection."
            );
            ExitCode::from(3)
        }
        Outcome::Failed(3) if name == "mutate" => {
            eprintln!(
                "contained: mutate ended INCOMPLETE — the session budget ran out before every \
                 selected mutant was finalized. Nothing about the rest is established. \
                 Continue with `cargo xtask contained mutate --resume` and the same selection."
            );
            ExitCode::from(3)
        }
        other => {
            eprintln!("contained: {name} did not pass ({})", other.word());
            ExitCode::FAILURE
        }
    }
}

/// The profile a workload runs under when `--profile` is not given. N14.1.
///
/// The workspace suite and the selfhost suite take P4T4, which their calibration qualified
/// (`docs/internal/performance-log.md`). Everything else is P1. `mutate` is P1 because every parallel
/// profile tried crossed the frozen total-memory gate on it, not because its verdicts
/// differed. `bench` is P1 for good: it measures the programs and the compiler, and a
/// measurement shares the machine with nothing. `--profile p1` reproduces any older
/// result.
fn default_profile(workload: &str) -> contained::ExecutionProfile {
    match workload {
        // The fuzz campaign builds the whole compiler instrumented, cold (N93): four jobs.
        "tests" | "selfhost" | "fuzz" => contained::ExecutionProfile::P4T4,
        _ => contained::ExecutionProfile::P1,
    }
}

/// A contained `cargo test` over `scope`, with the profile's job and thread counts.
fn cargo_test(scope: &str, p: contained::ExecutionProfile) -> String {
    format!(
        "cargo test --locked {scope} --jobs {} -- --test-threads {}",
        p.cargo_jobs, p.test_threads
    )
}

/// The parallelism a contained campaign passes to its runner. It sizes each step; the
/// runner still takes one mutant at a time.
fn mutate_parallelism(p: contained::ExecutionProfile) -> String {
    format!(
        "--cargo-jobs {} --test-threads {}",
        p.cargo_jobs, p.test_threads
    )
}

/// The options `contained mutate` passes through, validated and quoted.
///
/// Rebuilt from a closed list rather than forwarded. Every value is interpolated into a
/// shell command, so each is restricted to the characters a mutation name, a test path
/// or a number uses, and single-quoted as well.
fn mutate_passthrough(args: &[String]) -> Result<String, String> {
    const WITH_VALUE: &[&str] = &[
        "--only",
        "--strategy",
        "--budget-secs",
        "--verify-killer",
        "--killer",
    ];
    const SWITCHES: &[&str] = &["--plan", "--resume", "--verify-killers"];
    let safe = |v: &str| {
        !v.is_empty()
            && v.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '=' | '.'))
    };
    let mut out = String::new();
    let mut i = 0;
    while let Some(a) = args.get(i) {
        if SWITCHES.contains(&a.as_str()) {
            out.push_str(&format!(" {a}"));
            i += 1;
        } else if WITH_VALUE.contains(&a.as_str()) {
            let Some(v) = args.get(i + 1) else {
                return Err(format!("{a} needs a value"));
            };
            if !safe(v) {
                return Err(format!(
                    "{a} {v:?}: only letters, digits and - _ : = . are accepted"
                ));
            }
            out.push_str(&format!(" {a} '{v}'"));
            i += 2;
        } else {
            return Err(format!("mutate: unknown option {a}"));
        }
    }
    Ok(out)
}

/// Append the journal lines a contained campaign printed to the host's copy, under
/// `target/mutation-campaigns/<key>/journal.jsonl`.
///
/// The container's own journal dies with it. What it printed is the only record that
/// crosses, so it is collected whatever the outcome, a timeout included: every line is
/// a mutant that was restored and verified before it was printed, and a later session
/// resumes from exactly these.
fn collect_journal(repo: &std::path::Path, stdout: &str) {
    use std::io::Write as _;
    let mut by_key: std::collections::BTreeMap<&str, Vec<&str>> = std::collections::BTreeMap::new();
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix(campaign::EXPORT_PREFIX) {
            if let Some((key, json)) = rest.split_once(' ') {
                if key.len() == 16 && key.chars().all(|c| c.is_ascii_hexdigit()) {
                    by_key.entry(key).or_default().push(json);
                }
            }
        }
    }
    for (key, lines) in by_key {
        let dir = repo.join("target").join("mutation-campaigns").join(key);
        if std::fs::create_dir_all(&dir).is_err() {
            eprintln!("contained: cannot create {}", dir.display());
            continue;
        }
        let path = dir.join("journal.jsonl");
        let fresh = !path.exists();
        let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        else {
            eprintln!("contained: cannot open {}", path.display());
            continue;
        };
        let mut n = 0;
        for (i, l) in lines.iter().enumerate() {
            // The header opens a fresh file and is otherwise already there.
            if i == 0 && !fresh && l.contains("\"kind\":\"campaign\"") {
                continue;
            }
            if writeln!(f, "{l}").is_ok() {
                n += 1;
            }
        }
        let _ = f.sync_all();
        eprintln!(
            "contained: {n} journal line(s) for campaign {key} kept at {}",
            path.display()
        );
    }
}

/// `sh -c`, not `sh -lc`: a login shell re-reads `/etc/profile` and drops the image's
/// PATH, which is where the pinned toolchain lives.
fn shell(cmd: &str) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!("cd /src && {cmd}"),
    ]
}

/// `contained cache` lists the harness-owned verification caches; `contained cache clear`
/// removes them. Nothing that is not labelled as the harness's is touched. N32-H2.
fn contained_cache(what: Option<&str>) -> ExitCode {
    match what {
        None | Some("list") => {
            let owned = contained::cache_list();
            if owned.is_empty() {
                println!("no verification cache");
            }
            for v in owned {
                println!(
                    "{}  namespace {}  key {}  created {}",
                    v.name, v.namespace, v.key, v.created
                );
            }
            ExitCode::SUCCESS
        }
        Some("clear") => {
            for n in contained::cache_clear() {
                println!("removed {n}");
            }
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("contained cache: `list` or `clear`, not `{other}`");
            ExitCode::FAILURE
        }
    }
}

/// Verifies that each limit is real on this daemon before anything depends on it. Every
/// probe's own maximum is small, so it stays safe even if enforcement silently fails.
fn contained_probe(repo: &std::path::Path) -> ExitCode {
    use contained::{Bounds, Outcome};
    use std::time::Duration;

    let tight = Bounds {
        memory_mb: 256,
        milli_cpus: 1000,
        profile: contained::ExecutionProfile::P1,
        pids: 64,
        storage_gb: 1,
        scratch_mb: 16,
        fsize_bytes: 4 * 1024 * 1024,
        deadline: Duration::from_secs(20),
        output_cap: 256 * 1024,
        export: None,
        cache: None,
    };

    // (name, argv, expected outcome, what it proves)
    let cases: Vec<(&str, Vec<String>, Outcome, &str)> = vec![
        (
            "limits-visible",
            shell("cat /sys/fs/cgroup/memory.max /sys/fs/cgroup/memory.swap.max /sys/fs/cgroup/pids.max"),
            Outcome::Passed,
            "the cgroup reports the caps that were asked for",
        ),
        (
            "memory",
            // Allocates in chunks and memsets every one. A shell pipeline would only
            // stream through small buffers and prove nothing; reserving address space
            // without touching it would prove nothing either. 400 MiB against a 256 MiB
            // cap is small enough to stay safe on the host even if enforcement fails.
            shell(
                "printf '%s' '#include <stdlib.h>\n#include <string.h>\n#include <stdio.h>\nint main(void){for(long i=0;i<50;i++){char*p=malloc(8L<<20);if(!p){printf(\"MALLOC_FAILED %ld\\n\",i);return 3;}memset(p,(int)(i&0xff),8L<<20);}printf(\"COMPLETED\\n\");return 0;}' > /scratch/m.c && clang -O0 -o /scratch/m /scratch/m.c && /scratch/m",
            ),
            Outcome::MemoryLimit,
            "allocation past the cap is refused, not merely reclaimed",
        ),
        (
            "deadline",
            shell("sleep 120"),
            Outcome::Timeout,
            "a workload outliving its deadline is killed at the container",
        ),
        (
            "output",
            shell("i=0; while [ $i -lt 400 ]; do head -c 65536 /dev/zero | tr '\\0' 'x'; i=$((i+1)); done"),
            Outcome::OutputOverflow,
            "a flood is drained and capped, not buffered",
        ),
        (
            "scratch",
            shell("dd if=/dev/zero of=/scratch/f bs=1M count=64 2>&1 | tail -1; test ! -s /scratch/f || test $(stat -c%s /scratch/f) -lt 67108864"),
            Outcome::Passed,
            "the tmpfs refuses to grow past its size",
        ),
        (
            "descendant",
            shell("(sleep 120 &) ; echo detached; sleep 120"),
            Outcome::Timeout,
            "a detached grandchild dies with the container",
        ),
    ];

    let mut failures = 0;
    println!("contained probe — verifying each bound is real before anything relies on it\n");
    for (name, argv, want, proves) in cases {
        let borrowed: Vec<&str> = argv.iter().map(String::as_str).collect();
        let r = contained::run(repo, &format!("probe-{name}"), &borrowed, &tight);
        if let Outcome::Unavailable(why) = &r.outcome {
            eprintln!("REFUSED — containment is unavailable: {why}");
            return ExitCode::FAILURE;
        }
        let ok = r.outcome == want;
        if !ok {
            failures += 1;
        }
        println!(
            "  {:<11} {:<16} expected {:<16} {:.1}s  {}",
            if ok { "ok" } else { "MISMATCH" },
            name,
            want.word(),
            r.elapsed.as_secs_f64(),
            proves
        );
        if !ok {
            println!(
                "      got {} (stdout {} B, stderr {} B)",
                r.outcome.word(),
                r.stdout_bytes,
                r.stderr_bytes
            );
        }
    }
    let left = contained::survivors();
    println!("\n  survivors after all probes: {left}");
    if failures == 0 && left == 0 {
        println!("  containment verified on this daemon");
        ExitCode::SUCCESS
    } else {
        eprintln!("  containment NOT verified: {failures} mismatched, {left} survivors");
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contained::ExecutionProfile;

    #[test]
    fn only_the_calibrated_workloads_leave_p1() {
        assert_eq!(default_profile("tests"), ExecutionProfile::P4T4);
        assert_eq!(default_profile("selfhost"), ExecutionProfile::P4T4);
        for w in [
            "mutate",
            "bootstrap",
            "selfbuild",
            "bench",
            "release",
            "probe",
            "run",
            "",
        ] {
            assert_eq!(default_profile(w), ExecutionProfile::P1, "{w}");
        }
    }

    #[test]
    fn p1_is_still_one_cpu_one_job_one_thread() {
        let p1 = ExecutionProfile::P1;
        assert_eq!(
            (p1.milli_cpus, p1.cargo_jobs, p1.test_threads),
            (1000, 1, 1)
        );
        assert_eq!(ExecutionProfile::by_name("p1"), Some(p1));
        assert_eq!(ExecutionProfile::by_name("p6"), None);
        assert_eq!(
            cargo_test("--workspace", p1),
            "cargo test --locked --workspace --jobs 1 -- --test-threads 1"
        );
        assert_eq!(mutate_parallelism(p1), "--cargo-jobs 1 --test-threads 1");
    }

    #[test]
    fn a_profile_reaches_every_step_it_sizes() {
        let p = ExecutionProfile::P4T4;
        assert_eq!(
            cargo_test("-p nazm-cli --test selfhost", p),
            "cargo test --locked -p nazm-cli --test selfhost --jobs 4 -- --test-threads 4"
        );
        assert_eq!(mutate_parallelism(p), "--cargo-jobs 4 --test-threads 4");
    }

    #[test]
    fn p8t8_is_chosen_by_name_and_is_no_workload_s_default() {
        let p = ExecutionProfile::P8T8;
        assert_eq!(ExecutionProfile::by_name("p8t8"), Some(p));
        assert_eq!((p.milli_cpus, p.cargo_jobs, p.test_threads), (8000, 8, 8));
        for w in [
            "tests",
            "selfhost",
            "selfbuild",
            "bootstrap",
            "mutate",
            "bench",
            "run",
        ] {
            assert_ne!(default_profile(w), p, "{w}");
        }
    }
}
