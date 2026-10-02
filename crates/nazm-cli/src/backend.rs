//! The object compiler, as one controlled and identified transformation.
//!
//! ```text
//! LLVM unit bytes + codegen configuration + backend identity  →  object
//! ```
//!
//! Everything that runs `clang` lives here, and it is one place on purpose: the arguments
//! that are **hashed** and the arguments that are **executed** are built by the same
//! function, so a cache key cannot describe an invocation that did not happen.
//!
//! # Asking the compiler what it is going to do
//!
//! `nazm build` passes three arguments. The driver turns them into about two hundred, and
//! almost none of the ones that decide the object are among the three: the target triple
//! with its deployment version, the CPU and its thirty feature flags, the relocation
//! model, the ABI, the resource directory. A key built from what was *asked for* would
//! leave every one of them out, and each of them changes the object — so two machines with
//! one clang version and different processors would share entries they must not.
//!
//! So the configuration is read from the compiler instead. `clang -###` prints the
//! version it identifies as and the exact command it would run, without running it. One
//! extra process per build buys a configuration that is complete by construction and that
//! keeps being complete when the driver learns a new flag.
//!
//! # A controlled environment
//!
//! Measured on 2026-09-22, on this machine:
//!
//! | Variable | Effect on the object of a pure LLVM input |
//! | --- | --- |
//! | `CCC_OVERRIDE_OPTIONS=+-O2` | an `-O0` invocation produced the `-O2` object, byte for byte |
//! | `SDKROOT` | changed `LC_BUILD_VERSION`'s `minos` from 27.0 to 27.2 |
//! | `MACOSX_DEPLOYMENT_TARGET` | changed it to whatever was asked for |
//! | the working directory | none: identical bytes from two directories |
//! | the input and output paths | none: identical bytes under three spellings |
//!
//! Hashing the environment would be the wrong answer to the first three — a set of
//! variables nobody can finish enumerating, and a key that changes when an unrelated one
//! does. The invocation is made hermetic instead: the child's environment is built from
//! nothing, and only the variables below are put into it. Each is there for a reason that
//! is not "it was already set".
//!
//! Anything passed through is still covered, because the probe runs in exactly the same
//! environment as the compilation it describes — a deployment target arrives in the key as
//! `-triple arm64-apple-macosx12.0.0`, which is where it actually matters.

use std::path::{Path, PathBuf};
use std::process::Command;

use nazm_cache::{BackendIdentity, CodegenConfig};

/// The driver this build shells out to.
pub(crate) const DRIVER: &str = "clang";

/// The variables the object compiler is allowed to see.
///
/// * `PATH` — how the driver is found at all. Its only effect on the object is *which*
///   compiler runs, and that is exactly what [`BackendIdentity`] measures, using this same
///   lookup — so it is passed through and is not itself an input.
/// * `LD_LIBRARY_PATH` — a compiler installed outside the system prefix may not start
///   without it. Removing it would break a legitimate installation, which is a failure and
///   not a wrong answer, but a failure nobody asked for.
/// * `TMPDIR` — where a crash report goes. It does not reach the object.
/// * `DEVELOPER_DIR`, `SDKROOT` — which toolchain and which SDK. Both reach the object,
///   and both arrive in the key through the resolved configuration.
/// * the deployment targets — same mechanism, same arrival.
///
/// Everything else is removed, including `CCC_OVERRIDE_OPTIONS`, whose entire purpose is
/// to rewrite the arguments a driver was given.
const PASSED: &[&str] = &[
    "PATH",
    "LD_LIBRARY_PATH",
    "TMPDIR",
    "DEVELOPER_DIR",
    "SDKROOT",
    "MACOSX_DEPLOYMENT_TARGET",
    "IPHONEOS_DEPLOYMENT_TARGET",
    "TVOS_DEPLOYMENT_TARGET",
    "WATCHOS_DEPLOYMENT_TARGET",
    "XROS_DEPLOYMENT_TARGET",
];

/// The file the probe compiles, in the build's own working directory.
const PROBE: &str = "probe";

/// What running the driver produced.
pub(crate) enum Outcome {
    Ran(std::process::Output),
    Missing,
    Failed(std::io::Error),
}

/// The object compiler for one build.
#[derive(Debug, Clone)]
pub(crate) struct Backend {
    opt_level: u8,
    /// What this backend is and how it is configured, where that could be established.
    ///
    /// `None` is not an error: the build compiles everything and reuses nothing. A
    /// configuration that could not be read is a configuration that must not be guessed,
    /// because a guess that omitted the target would be a key two targets share.
    settled: Option<Settled>,
}

/// A backend that answered when it was asked what it would do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Settled {
    pub identity: BackendIdentity,
    pub config: CodegenConfig,
}

impl Backend {
    /// Ask the driver what it is, and what it would do, at this optimisation level.
    ///
    /// `work` is the build's own temporary directory; a small file is written into it for
    /// the driver to resolve a command line against, because `-###` refuses an input that
    /// does not exist. Nothing else is written and nothing outside it is touched.
    pub(crate) fn discover(opt_level: u8, work: &Path) -> Self {
        Self {
            opt_level,
            settled: probe(opt_level, work),
        }
    }

    /// A backend that will compile and never be identified — for `--no-cache`.
    ///
    /// The probe is skipped rather than ignored, so a cache-disabled build runs exactly
    /// the processes the build ran before there was a cache. That is what makes it a
    /// usable baseline for the equivalence tests.
    pub(crate) fn unidentified(opt_level: u8) -> Self {
        Self {
            opt_level,
            settled: None,
        }
    }

    pub(crate) fn settled(&self) -> Option<Settled> {
        self.settled
    }

    /// Compile one LLVM unit into one object.
    ///
    /// **Independently generated.** No other unit's IR is in scope, which is what makes
    /// this separate code generation rather than concatenation with extra steps.
    pub(crate) fn compile(&self, ir: &Path, object: &Path) -> Outcome {
        run(self.command().args(compile_args(self.opt_level)).args([
            "-o".as_ref(),
            object.as_os_str(),
            ir.as_os_str(),
        ]))
    }

    /// Link the objects into an executable.
    ///
    /// Not cached, and not keyed: a linker does not produce an object, and its version and
    /// options belong to a `LinkKey` that does not exist. It runs on every build.
    pub(crate) fn link(&self, output: &Path, objects: &[PathBuf]) -> Outcome {
        run(&mut self.link_command(output, objects))
    }

    /// The link, as a command: what `link` runs, built apart so its environment can be read.
    fn link_command(&self, output: &Path, objects: &[PathBuf]) -> Command {
        let mut c = self.command();
        c
            // The Apple linker otherwise mixes a time into the executable's UUID, so two links
            // of the same objects differ (N46, reproducible builds). Ignored elsewhere.
            .env("ZERO_AR_DATE", "1")
            .args(link_args(self.opt_level))
            .arg("-o")
            .arg(output)
            .args(objects);
        c
    }

    /// Gather a macOS executable's debug information into a `.dSYM` bundle (N48), with the
    /// `dsymutil` beside the toolchain; the bundle is what a debugger reads once the objects
    /// the executable was linked from are gone.
    pub(crate) fn gather_debug_info(&self, exe: &Path, dsym: &Path) -> Result<(), String> {
        let mut c = Command::new("dsymutil");
        c.env_clear();
        for name in PASSED {
            if let Some(value) = std::env::var_os(name) {
                c.env(name, value);
            }
        }
        match c.arg(exe).arg("-o").arg(dsym).output() {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => Err(String::from_utf8_lossy(&o.stderr).into_owned()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// The driver, with an environment built from nothing.
    fn command(&self) -> Command {
        let mut c = Command::new(DRIVER);
        c.env_clear();
        for name in PASSED {
            if let Some(value) = std::env::var_os(name) {
                c.env(name, value);
            }
        }
        c
    }
}

/// The arguments that turn one LLVM file into one object.
///
/// One function, used by the compilation and by the probe that describes it. Two lists
/// would be two lists that can differ, and the difference would be a key describing an
/// invocation nobody ran.
fn compile_args(opt_level: u8) -> Vec<String> {
    vec![
        format!("-O{opt_level}"),
        // The units carry no target triple, so clang supplies the host's and would
        // otherwise warn about overriding nothing.
        "-Wno-override-module".to_owned(),
        "-c".to_owned(),
    ]
}

fn link_args(opt_level: u8) -> Vec<String> {
    vec![format!("-O{opt_level}"), "-Wno-override-module".to_owned()]
}

pub(crate) fn run_command(command: &mut Command) -> Outcome {
    run(command)
}

fn run(command: &mut Command) -> Outcome {
    match command.output() {
        Ok(o) => Outcome::Ran(o),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Outcome::Missing,
        Err(e) => Outcome::Failed(e),
    }
}

/// Ask the driver to resolve a compilation without performing it.
fn probe(opt_level: u8, work: &Path) -> Option<Settled> {
    let input = work.join(format!("{PROBE}.ll"));
    let output = work.join(format!("{PROBE}.o"));
    // Empty is enough: `-###` resolves a command line and never reads the file.
    std::fs::write(&input, b"").ok()?;

    let mut c = Command::new(DRIVER);
    c.env_clear();
    for name in PASSED {
        if let Some(value) = std::env::var_os(name) {
            c.env(name, value);
        }
    }
    let out = c
        .arg("-###")
        .args(compile_args(opt_level))
        .args(["-o".as_ref(), output.as_os_str(), input.as_os_str()])
        .output()
        .ok()?;
    let _ = std::fs::remove_file(&input);

    // `-###` writes to stderr: the banner first, then one line per job it would run.
    let report = String::from_utf8_lossy(&out.stderr);
    let banner = banner(&report);
    let arguments = resolved(&report, &input, &output);
    if banner.is_empty() || arguments.is_empty() {
        return None;
    }
    Some(Settled {
        identity: BackendIdentity::of(DRIVER, &banner),
        config: configuration(arguments),
    })
}

/// The code generation configuration: what the driver resolved, and the runtime ABI
/// revision. The runtime's contract decides what a unit's calls into it mean, so an object
/// built against another revision is another object (N43, §7.45).
fn configuration(mut arguments: Vec<String>) -> CodegenConfig {
    arguments.push(runtime_abi(nazm_lir::runtime::RUNTIME_ABI_REVISION));
    CodegenConfig::of(&arguments)
}

fn runtime_abi(revision: u32) -> String {
    format!(
        "nazm-runtime-abi={revision}+{}",
        nazm_lir::runtime::digest()
    )
}

/// What the driver says it is, minus where it says it is installed.
///
/// A path is not an identity: the same compiler reached through two paths is one
/// compiler, and `docs/architecture.md` §7.6 keeps every path out of every key that is not
/// the toolchain's own resolved configuration.
fn banner(report: &str) -> String {
    report
        .lines()
        .take_while(|l| !l.starts_with(' '))
        .filter(|l| !l.starts_with("InstalledDir:"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The arguments the driver resolved, with this invocation's own file names removed.
///
/// # What is removed, and why each one may be
///
/// | Removed | Measured |
/// | --- | --- |
/// | the job's own path | kept as its file name, so `clang` and an external assembler stay apart while the directory does not reach the key |
/// | `-main-file-name` | the input's name; two names for one text produced identical objects |
/// | `-o`, and the input and output paths | identical objects under three spellings |
/// | `-fdebug-compilation-dir`, `-fcoverage-compilation-dir` | the working directory; identical objects from two directories |
///
/// Everything else is kept, including arguments this compiler has never heard of. The
/// safe direction is to keep: an argument wrongly kept costs a cache miss, and an argument
/// wrongly removed is a wrong hit.
fn resolved(report: &str, input: &Path, output: &Path) -> Vec<String> {
    let names: Vec<String> = [input, output]
        .iter()
        .flat_map(|p| {
            [
                p.to_string_lossy().into_owned(),
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            ]
        })
        .collect();

    let mut out = Vec::new();
    for line in report.lines().filter(|l| l.starts_with(' ')) {
        let job = split(line);
        let Some((program, rest)) = job.split_first() else {
            continue;
        };
        out.push(format!(
            "@{}",
            Path::new(program)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        ));
        let mut it = rest.iter();
        while let Some(a) = it.next() {
            if a == "-main-file-name" || a == "-o" {
                let _ = it.next();
                continue;
            }
            if a.starts_with("-fdebug-compilation-dir")
                || a.starts_with("-fcoverage-compilation-dir")
            {
                // The `=` form carries its value; the separated form does not.
                if !a.contains('=') {
                    let _ = it.next();
                }
                continue;
            }
            if names.iter().any(|n| n == a) {
                continue;
            }
            out.push(a.clone());
        }
    }
    out
}

/// The quoted arguments of one `-###` job line.
///
/// Clang writes each argument in double quotes, escaping a quote and a backslash with a
/// backslash. Anything outside quotes is separator.
fn split(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut inside = false;
    let mut escaped = false;
    for c in line.chars() {
        if escaped {
            current.push(c);
            escaped = false;
        } else if c == '\\' && inside {
            escaped = true;
        } else if c == '"' {
            if inside {
                out.push(std::mem::take(&mut current));
            }
            inside = !inside;
        } else if inside {
            current.push(c);
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const REPORT: &str = concat!(
        "Apple clang version 21.0.0 (clang-2100.3.34.2)\n",
        "Target: arm64-apple-darwin27.2.0\n",
        "Thread model: posix\n",
        "InstalledDir: /Library/Developer/CommandLineTools/usr/bin\n",
        " \"/Library/Developer/CommandLineTools/usr/bin/clang\" \"-cc1\" \"-triple\" ",
        "\"arm64-apple-macosx27.0.0\" \"-O0\" \"-emit-obj\" \"-main-file-name\" \"probe.ll\" ",
        "\"-target-cpu\" \"apple-m1\" \"-target-feature\" \"+neon\" ",
        "\"-fdebug-compilation-dir=/tmp/build/.nazm-1\" ",
        "\"-fcoverage-compilation-dir\" \"/tmp/build/.nazm-1\" ",
        "\"-o\" \"/tmp/build/.nazm-1/probe.o\" \"-x\" \"ir\" \"/tmp/build/.nazm-1/probe.ll\"\n",
    );

    fn args() -> Vec<String> {
        resolved(
            REPORT,
            Path::new("/tmp/build/.nazm-1/probe.ll"),
            Path::new("/tmp/build/.nazm-1/probe.o"),
        )
    }

    #[test]
    fn the_banner_is_what_the_compiler_says_it_is_without_where_it_lives() {
        assert_eq!(
            banner(REPORT),
            "Apple clang version 21.0.0 (clang-2100.3.34.2)\n\
             Target: arm64-apple-darwin27.2.0\n\
             Thread model: posix"
        );
    }

    #[test]
    fn the_configuration_keeps_everything_that_decides_the_object() {
        let a = args();
        // The three that a key built from `nazm build`'s own arguments would have missed.
        assert!(a.contains(&"arm64-apple-macosx27.0.0".to_owned()), "{a:?}");
        assert!(a.contains(&"apple-m1".to_owned()), "{a:?}");
        assert!(a.contains(&"+neon".to_owned()), "{a:?}");
        assert!(a.contains(&"-O0".to_owned()), "{a:?}");
        // And it says which program would run, without saying where it is.
        assert_eq!(a.first().map(String::as_str), Some("@clang"));
    }

    #[test]
    fn it_removes_this_invocations_own_files_and_nothing_else() {
        let a = args();
        for gone in [
            "-main-file-name",
            "probe.ll",
            "probe.o",
            "-o",
            "/tmp/build/.nazm-1/probe.ll",
            "/tmp/build/.nazm-1/probe.o",
        ] {
            assert!(!a.iter().any(|x| x == gone), "{gone} survived: {a:?}");
        }
        assert!(
            !a.iter().any(|x| x.contains("compilation-dir")),
            "a compilation directory survived: {a:?}"
        );
        // The separated form's value went with it rather than being left behind as a
        // bare directory that would put the checkout into the key.
        assert!(
            !a.iter().any(|x| x == "/tmp/build/.nazm-1"),
            "a directory survived: {a:?}"
        );
        // `-x ir` is a real input and stays.
        assert!(
            a.contains(&"-x".to_owned()) && a.contains(&"ir".to_owned()),
            "{a:?}"
        );
    }

    #[test]
    fn two_builds_of_one_project_in_two_directories_resolve_to_one_configuration() {
        // The property that makes an object reusable across checkouts. Same toolchain,
        // same flags, different working directory and different temporary names.
        let here = args();
        let there = resolved(
            &REPORT.replace("/tmp/build/.nazm-1", "/other/place/.nazm-99"),
            Path::new("/other/place/.nazm-99/probe.ll"),
            Path::new("/other/place/.nazm-99/probe.o"),
        );
        assert_eq!(CodegenConfig::of(&here), CodegenConfig::of(&there));
    }

    #[test]
    fn a_report_with_no_job_settles_nothing() {
        // A driver that did not say what it would do is a driver whose configuration must
        // not be guessed: the build compiles everything rather than keying on a fiction.
        assert!(resolved(
            "Apple clang version 21.0.0\n",
            Path::new("a"),
            Path::new("b")
        )
        .is_empty());
    }

    /// The probe against the toolchain this machine actually has.
    ///
    /// Not a mock: the point of asking the compiler what it will do is that nobody has to
    /// predict the answer, and a test that predicted it would be testing the prediction.
    #[test]
    fn the_runtime_s_digest_is_part_of_every_configuration() {
        // The revision is what a person bumps; the digest is what was emitted (N53).
        let digest = nazm_lir::runtime::digest();
        assert_eq!(digest.len(), 64);
        assert!(runtime_abi(nazm_lir::runtime::RUNTIME_ABI_REVISION).ends_with(digest));
    }

    #[test]
    fn the_runtime_abi_revision_is_part_of_every_configuration() {
        let resolved = vec!["-cc1".to_owned(), "-O0".to_owned()];
        let with = configuration(resolved.clone());
        assert_ne!(with, CodegenConfig::of(&resolved));
        let mut this = resolved.clone();
        this.push(runtime_abi(nazm_lir::runtime::RUNTIME_ABI_REVISION));
        assert_eq!(with, CodegenConfig::of(&this));
        let mut older = resolved;
        older.push(runtime_abi(nazm_lir::runtime::RUNTIME_ABI_REVISION - 1));
        assert_ne!(with, CodegenConfig::of(&older));
    }

    #[test]
    fn this_machine_s_backend_identifies_itself_and_its_configuration() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(zero) = Backend::discover(0, dir.path()).settled() else {
            // No `clang`. `nazm build` says so for itself; this has nothing to measure.
            return;
        };
        let two = Backend::discover(2, dir.path()).settled().expect("settled");

        // One machine, one compiler: the identity does not move between two runs or two
        // optimisation levels.
        assert_eq!(zero.identity, two.identity);
        assert_eq!(
            zero.identity,
            Backend::discover(0, dir.path())
                .settled()
                .expect("again")
                .identity
        );
        // And the configuration does, because the optimisation level is part of it.
        assert_ne!(zero.config, two.config);

        // The probe leaves nothing behind but the directory it was given.
        assert!(!dir.path().join("probe.ll").exists());
    }

    #[test]
    fn quoting_survives_a_path_with_a_space_or_a_quote() {
        let line = " \"/a b/clang\" \"-cc1\" \"say \\\"hi\\\"\" \"end\"";
        assert_eq!(
            split(line),
            vec![
                "/a b/clang".to_owned(),
                "-cc1".to_owned(),
                "say \"hi\"".to_owned(),
                "end".to_owned()
            ]
        );
    }

    #[test]
    fn the_link_leaves_time_out_of_the_executable_on_every_host() {
        // Checked on the command rather than on two executables, so a host whose linker
        // ignores the variable still sees it go missing.
        let c = Backend::unidentified(0).link_command(Path::new("out"), &[]);
        let zero = c
            .get_envs()
            .find(|(k, _)| *k == "ZERO_AR_DATE")
            .and_then(|(_, v)| v);
        assert_eq!(zero, Some(std::ffi::OsStr::new("1")));
    }
}
