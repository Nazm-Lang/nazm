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
/// * `TMP`, `TEMP` — `TMPDIR`'s names on Windows, where clang cannot link without one: with
///   neither, its temporary files would go to the Windows directory, which a standard account
///   may not write (W2). They do not reach the object.
/// * `SystemRoot` — where Windows itself is, which its own libraries read as they start.
/// * `DEVELOPER_DIR`, `SDKROOT` — which toolchain and which SDK. Both reach the object,
///   and both arrive in the key through the resolved configuration.
/// * the deployment targets — same mechanism, same arrival.
///
/// Everything else is removed, including `CCC_OVERRIDE_OPTIONS`, whose entire purpose is
/// to rewrite the arguments a driver was given.
pub(crate) const PASSED: &[&str] = &[
    "PATH",
    "LD_LIBRARY_PATH",
    "TMPDIR",
    "TMP",
    "TEMP",
    "SystemRoot",
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
    /// Whether every object is LLVM bitcode, optimised together at the link (R1.5 3.4, design
    /// D13): the program's units, the standard library's and the runtime's as one program.
    whole: bool,
    /// Where the linker keeps what it optimised, by content, for the next link of a
    /// whole-program build (ThinLTO's cache): the project's `.nazm/lto`, or none.
    lto_cache: Option<PathBuf>,
    /// The triple to generate for when it is not the host's (N57): handed to clang as
    /// `-target`, and so part of the resolved configuration every key carries. `None` for a
    /// host build, whose command lines are unchanged.
    cross: Option<String>,
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
    pub(crate) fn discover(opt_level: u8, work: &Path, cross: Option<&str>) -> Self {
        Self::discover_whole(opt_level, work, cross, false)
    }

    /// [`Self::discover`], every object bitcode optimised together at the link when `whole`
    /// (R1.5 3.4): the probe resolves the same arguments, so every key says which it is.
    pub(crate) fn discover_whole(
        opt_level: u8,
        work: &Path,
        cross: Option<&str>,
        whole: bool,
    ) -> Self {
        Self {
            opt_level,
            whole,
            lto_cache: None,
            cross: cross.map(str::to_owned),
            settled: probe(opt_level, work, cross, whole),
        }
    }

    /// A backend that will compile and never be identified — for `--no-cache`.
    ///
    /// The probe is skipped rather than ignored, so a cache-disabled build runs exactly
    /// the processes the build ran before there was a cache. That is what makes it a
    /// usable baseline for the equivalence tests.
    /// Whole-program when `whole` (R1.5 3.4), as [`Self::discover_whole`].
    pub(crate) fn unidentified(opt_level: u8, cross: Option<&str>, whole: bool) -> Self {
        Self {
            opt_level,
            whole,
            lto_cache: None,
            cross: cross.map(str::to_owned),
            settled: None,
        }
    }

    /// The same backend, a whole-program link keeping what it optimised in `dir` (R1.5 3.4):
    /// a link whose inputs are unchanged reuses it rather than optimising again. The cache is
    /// keyed by content, so it changes no executable.
    pub(crate) fn with_lto_cache(mut self, dir: Option<PathBuf>) -> Self {
        self.lto_cache = dir;
        self
    }

    pub(crate) fn settled(&self) -> Option<Settled> {
        self.settled
    }

    /// Compile one LLVM unit into one object.
    ///
    /// **Independently generated.** No other unit's IR is in scope, which is what makes
    /// this separate code generation rather than concatenation with extra steps.
    pub(crate) fn compile(&self, ir: &Path, object: &Path) -> Outcome {
        run(self
            .command()
            .args(compile_args(
                self.opt_level,
                self.cross.as_deref(),
                self.whole,
            ))
            .args(["-o".as_ref(), object.as_os_str(), ir.as_os_str()]))
    }

    /// [`Self::compile`], with the code generator's frame size for every function on standard
    /// error (N63): clang's prologue/epilogue remarks, which the board's stack bound reads. The
    /// remark's pass is `prologepilog` up to LLVM 22 and `prolog-epilog` from LLVM 23, which
    /// reports nothing for the old name; the option is a regular expression, so it names both.
    pub(crate) fn compile_reporting_frames(&self, ir: &Path, object: &Path) -> Outcome {
        run(self
            .command()
            .args(compile_args(
                self.opt_level,
                self.cross.as_deref(),
                self.whole,
            ))
            .arg("-Rpass-analysis=^prolog-?epilog$")
            .args(["-o".as_ref(), object.as_os_str(), ir.as_os_str()]))
    }

    /// Link the objects into an executable.
    ///
    /// Not cached, and not keyed: a linker does not produce an object, and its version and
    /// options belong to a `LinkKey` that does not exist. It runs on every build.
    ///
    /// Dead-stripped (R1.5, D14): what nothing reachable from the entry calls is left out.
    pub(crate) fn link(&self, output: &Path, objects: &[PathBuf]) -> Outcome {
        let mut c = self.link_command(output, objects);
        c.args(dead_strip_args(self.cross.as_deref()));
        run(&mut c)
    }

    /// [`Self::link`] for `nazm build --release` (R1.5, D15): position-independent as well.
    pub(crate) fn link_release(&self, output: &Path, objects: &[PathBuf]) -> Outcome {
        let mut c = self.link_command(output, objects);
        c.args(dead_strip_args(self.cross.as_deref()));
        c.args(crate::build::release_link_args());
        run(&mut c)
    }

    /// Link the objects into a shared library (N85, §7.86): `-shared`, everything else as an
    /// executable's link. Only the exports are visible outside it — every other symbol the
    /// compiler and the runtime define is hidden — so nothing else is its interface.
    ///
    /// The library names itself by its file name, found through the loader's search path
    /// (`@rpath/NAME` on macOS, the soname `NAME` elsewhere), never by the temporary path it
    /// was linked at: a C program links it with `-L` or its path and runs it with an rpath.
    pub(crate) fn link_shared(&self, output: &Path, objects: &[PathBuf]) -> Outcome {
        let mut c = self.link_command(output, objects);
        let name = output
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let apple = self
            .cross
            .as_deref()
            .map_or(std::env::consts::OS == "macos", |t| t.contains("apple"));
        c.arg("-shared");
        if apple {
            c.arg(format!("-Wl,-install_name,@rpath/{name}"));
        } else {
            c.arg(format!("-Wl,-soname,{name}"));
        }
        run(&mut c)
    }

    /// The link, as a command: what `link` runs, built apart so its environment can be read.
    fn link_command(&self, output: &Path, objects: &[PathBuf]) -> Command {
        let mut c = self.command();
        c
            // The Apple linker otherwise mixes a time into the executable's UUID, so two links
            // of the same objects differ (N46, reproducible builds). Ignored elsewhere.
            .env("ZERO_AR_DATE", "1")
            .args(link_args(self.opt_level, self.cross.as_deref(), self.whole))
            .args(
                self.lto_cache
                    .iter()
                    .filter(|_| self.whole)
                    .map(|d| format!("-Wl,-cache_path_lto,{}", d.display())),
            )
            .arg("-o")
            .arg(output)
            .args(objects)
            // After the objects, which is where a linker that drops unneeded libraries
            // (`--as-needed`, Ubuntu's default) still sees what they need.
            .args(system_libraries(self.cross.as_deref()));
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
        driver_command()
    }
}

/// The driver as every build runs it: an environment built from nothing but [`PASSED`]. `nazm
/// doctor` asks it its questions the same way, so its answer is the build's.
pub(crate) fn driver_command() -> Command {
    let mut c = Command::new(DRIVER);
    c.env_clear();
    for name in PASSED {
        if let Some(value) = std::env::var_os(name) {
            c.env(name, value);
        }
    }
    c
}

/// The arguments that turn one LLVM file into one object.
///
/// One function, used by the compilation and by the probe that describes it. Two lists
/// would be two lists that can differ, and the difference would be a key describing an
/// invocation nobody ran.
fn compile_args(opt_level: u8, cross: Option<&str>, whole: bool) -> Vec<String> {
    let mut a = vec![
        format!("-O{opt_level}"),
        // The units carry no target triple, so clang supplies the host's and would
        // otherwise warn about overriding nothing.
        "-Wno-override-module".to_owned(),
        "-c".to_owned(),
    ];
    a.extend(target_args(cross));
    // A section per function and per datum on ELF (R1.5, D14), so `--gc-sections` can leave out
    // each one nothing reaches. Mach-O needs none: its linker strips by symbol.
    if family(cross) == Some(Family::Elf) {
        a.extend(["-ffunction-sections", "-fdata-sections"].map(str::to_owned));
    }
    // Whole-program optimisation (R1.5 3.4): bitcode, which the link optimises as one module.
    if whole {
        a.push("-flto=thin".to_owned());
    }
    a
}

/// Whether a build of an executable at `opt_level` for `cross` (the host's when `None`) is
/// optimised as one program at the link (R1.5 3.4, design D13): `-O2` and above, on macOS, whose
/// linker (`ld64`) reads the bitcode through the toolchain's own `libLTO`. Elsewhere — Linux,
/// Windows through MinGW's linker, iOS, WebAssembly and the boards — each object is optimised
/// alone, as before.
pub(crate) fn whole_program(opt_level: u8, cross: Option<&str>) -> bool {
    let macos = match cross {
        None => std::env::consts::OS == "macos",
        Some(t) => t.contains("-apple-darwin") || t.contains("-apple-macos"),
    };
    opt_level >= 2 && macos
}

/// The object format of a hosted target whose linker strips what is unreachable (R1.5, D14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    /// macOS and iOS: `ld64`, `-dead_strip`.
    MachO,
    /// Linux and Android: `--gc-sections`, over the sections `-ffunction-sections` makes.
    Elf,
}

/// The family of the target being built for, `None` where nothing is stripped: Windows,
/// WebAssembly and the boards link as they did before D14.
fn family(cross: Option<&str>) -> Option<Family> {
    let (os, triple) = match cross {
        None => (std::env::consts::OS, ""),
        Some(t) => ("", t),
    };
    if os == "macos" || os == "ios" || triple.contains("-apple-") {
        Some(Family::MachO)
    } else if os == "linux" || os == "android" || triple.contains("-linux-") {
        Some(Family::Elf)
    } else {
        None
    }
}

/// The linker's arguments for dead-stripping an executable (R1.5, D14). Never a library's: a
/// static or shared library keeps every function for its C callers.
fn dead_strip_args(cross: Option<&str>) -> Vec<String> {
    match family(cross) {
        Some(Family::MachO) => vec!["-Wl,-dead_strip".to_owned()],
        Some(Family::Elf) => vec!["-Wl,--gc-sections".to_owned()],
        None => Vec::new(),
    }
}

fn link_args(opt_level: u8, cross: Option<&str>, whole: bool) -> Vec<String> {
    let mut a = vec![format!("-O{opt_level}"), "-Wno-override-module".to_owned()];
    a.extend(target_args(cross));
    if whole {
        a.push("-flto=thin".to_owned());
    }
    a
}

/// The system libraries a link needs beyond the C library, for the target being linked: on
/// Linux, the C library's mathematics (`-lm`), which Gate 2's float built-ins call (`round`,
/// `sin`, `pow`, …) and which glibc keeps apart; on macOS they are in `libSystem`. M1: Gate 2C
/// linked without it, so every Linux build calling one failed to link.
/// On Windows, MinGW-w64's (W2): [`windows_libraries`], as `link.txt` names them.
pub(crate) fn system_libraries(cross: Option<&str>) -> Vec<String> {
    let linux = cross.map_or(std::env::consts::OS == "linux", |t| t.contains("linux"));
    let windows = cross.map_or(std::env::consts::OS == "windows", |t| t.contains("windows"));
    if linux {
        vec!["-lm".to_owned()]
    } else if windows {
        windows_libraries()
    } else {
        Vec::new()
    }
}

/// The bytes a Windows executable reserves for its main thread: 8 MiB, the main thread's stack
/// on macOS and Linux. `--stack-size` still runs `main` on a thread of its own size.
pub(crate) const WINDOWS_MAIN_STACK: u64 = 8 * 1024 * 1024;

/// MinGW-w64 (Gate 3): Winsock and BCrypt, and the C runtime's own libraries linked in
/// statically, so the executable needs no MinGW DLL beside it. The main thread reserves 8 MiB,
/// as macOS and Linux give it, where Windows' default is 1 MiB (`docs/spec.md`, *The limit*):
/// the same recursion meets `N0408` at the same depth everywhere.
pub(crate) fn windows_libraries() -> Vec<String> {
    vec![
        "-static".to_owned(),
        "-pthread".to_owned(),
        "-lws2_32".to_owned(),
        "-lbcrypt".to_owned(),
        format!("-Wl,--stack,{WINDOWS_MAIN_STACK}"),
    ]
}

/// `-target TRIPLE` for a cross build (N57), nothing for the host's; for the AArch64 board,
/// strict alignment (Gate 3). RISC-V's triple is LLVM's
/// spelling with the ISA, the ABI and the code model beside it (N86): RV64GC, `lp64d`, and
/// `medany`, since the board's image is at `0x8000_0000`, past `medlow`'s reach.
fn target_args(cross: Option<&str>) -> Vec<String> {
    match cross {
        None => Vec::new(),
        Some("riscv64gc-unknown-none-elf") => [
            "-target",
            "riscv64-unknown-elf",
            "-march=rv64gc",
            "-mabi=lp64d",
            "-mcmodel=medany",
        ]
        .map(str::to_owned)
        .to_vec(),
        // A board runs with its MMU off, where every access is to Device memory and one not
        // aligned to its size faults (Gate 3): no unaligned access, a SIMD register's included.
        // The mobile targets (Gate 3): Android at API level 30, whose Bionic has every call the
        // runtime makes (`getentropy` since 28); iOS 17, on the device.
        Some("aarch64-linux-android") => ["-target", "aarch64-linux-android30"]
            .map(str::to_owned)
            .to_vec(),
        Some("aarch64-apple-ios") => ["-target", "arm64-apple-ios17.0"]
            .map(str::to_owned)
            .to_vec(),
        // WASI (Gate 3): bulk memory, so LLVM's copies are instructions and never calls to the
        // runtime's `memcpy`, whose length is the runtime's 64 bits.
        Some("wasm32-wasip1") => ["-target", "wasm32-wasip1", "-mbulk-memory"]
            .map(str::to_owned)
            .to_vec(),
        Some("aarch64-unknown-none") => ["-target", "aarch64-unknown-none", "-mstrict-align"]
            .map(str::to_owned)
            .to_vec(),
        // The Cortex-M33 (M1): its single-precision FPU, FPv5, and the hard-float convention.
        Some("thumbv8m.main-none-eabihf") => [
            "-target",
            "thumbv8m.main-none-eabihf",
            "-mfpu=fpv5-sp-d16",
            "-mfloat-abi=hard",
        ]
        .map(str::to_owned)
        .to_vec(),
        // RV32IMC (M1), in LLVM's spelling as RV64's is: Zicsr for the runtime's interrupt mask
        // and trap vector, Zifencei, the `ilp32` ABI, and no atomic or floating-point extension.
        Some("riscv32imc-unknown-none-elf") => [
            "-target",
            "riscv32-unknown-elf",
            "-march=rv32imc_zicsr_zifencei",
            "-mabi=ilp32",
        ]
        .map(str::to_owned)
        .to_vec(),
        Some(t) => vec!["-target".to_owned(), t.to_owned()],
    }
}

/// Whether the driver has a code generator for `arch`, by its own list (`-print-targets`, N86).
pub(crate) fn generates(arch: &str) -> bool {
    Command::new(DRIVER)
        .arg("-print-targets")
        .output()
        .is_ok_and(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l.split_whitespace().next() == Some(arch))
        })
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
fn probe(opt_level: u8, work: &Path, cross: Option<&str>, whole: bool) -> Option<Settled> {
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
        .args(compile_args(opt_level, cross, whole))
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

    #[test]
    fn a_hosted_executable_is_dead_stripped_and_nothing_else_is() {
        // R1.5, D14: ld64's `-dead_strip`; on ELF, `--gc-sections` over a section per function.
        let apple = [
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "aarch64-apple-ios",
        ];
        for t in apple {
            assert_eq!(dead_strip_args(Some(t)), ["-Wl,-dead_strip"], "{t}");
            assert!(!compile_args(0, Some(t), false).contains(&"-ffunction-sections".to_owned()));
        }
        let elf = [
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu",
            "aarch64-linux-android",
        ];
        for t in elf {
            assert_eq!(dead_strip_args(Some(t)), ["-Wl,--gc-sections"], "{t}");
            let args = compile_args(2, Some(t), false);
            assert!(args.contains(&"-ffunction-sections".to_owned()), "{t}");
            assert!(args.contains(&"-fdata-sections".to_owned()), "{t}");
        }
        for t in [
            "x86_64-pc-windows-gnu",
            "aarch64-unknown-none",
            "riscv64gc-unknown-none-elf",
            "thumbv7m-none-eabi",
            "thumbv8m.main-none-eabihf",
            "riscv32imc-unknown-none-elf",
            "wasm32-unknown-unknown",
            "wasm32-wasip1",
        ] {
            assert!(dead_strip_args(Some(t)).is_empty(), "{t}");
            assert!(!compile_args(0, Some(t), false).contains(&"-ffunction-sections".to_owned()));
        }
    }

    #[test]
    fn a_linux_link_names_the_c_librarys_mathematics_and_no_other_does() {
        // M1: glibc keeps `round`, `sin`, `pow` and the rest of Gate 2's float built-ins in
        // libm, which a Linux link must name; macOS has them in libSystem; a board or a
        // WebAssembly module links neither.
        for t in ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"] {
            assert_eq!(system_libraries(Some(t)), ["-lm"], "{t}");
        }
        for t in [
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "aarch64-unknown-none",
            "riscv64gc-unknown-none-elf",
            "wasm32-unknown-unknown",
        ] {
            assert!(system_libraries(Some(t)).is_empty(), "{t}");
        }
        // MinGW-w64's, as `link.txt` names them (W2: a Windows host's own build links them).
        assert_eq!(
            system_libraries(Some("x86_64-pc-windows-gnu")).join(" "),
            "-static -pthread -lws2_32 -lbcrypt -Wl,--stack,8388608"
        );
        assert_eq!(
            system_libraries(None).is_empty(),
            !matches!(std::env::consts::OS, "linux" | "windows"),
            "the host's own link"
        );
    }

    #[test]
    fn no_compile_line_asks_for_the_host_s_own_cpu() {
        // N66 (§7.68): vectorised code is each target's baseline — NEON, SSE2 — so no object
        // depends on the machine that built it, and the object key's triple and level suffice.
        for opt in [0, 2, 3] {
            for cross in [
                None,
                Some("x86_64-apple-darwin"),
                Some("aarch64-unknown-none"),
                Some("riscv64gc-unknown-none-elf"),
                Some("thumbv8m.main-none-eabihf"),
                Some("riscv32imc-unknown-none-elf"),
            ] {
                for a in compile_args(opt, cross, false)
                    .iter()
                    .chain(link_args(opt, cross, false).iter())
                {
                    // RISC-V's `-march` is the target's ISA baseline (N86, M1), not a CPU.
                    assert!(
                        (!a.starts_with("-march")
                            || a == "-march=rv64gc"
                            || a == "-march=rv32imc_zicsr_zifencei")
                            && !a.starts_with("-mcpu")
                            && !a.starts_with("-mtune")
                            && !a.contains("native"),
                        "{a}"
                    );
                }
            }
        }
    }

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
        let Some(zero) = Backend::discover(0, dir.path(), None).settled() else {
            // No `clang`. `nazm build` says so for itself; this has nothing to measure.
            return;
        };
        let two = Backend::discover(2, dir.path(), None)
            .settled()
            .expect("settled");

        // One machine, one compiler: the identity does not move between two runs or two
        // optimisation levels.
        assert_eq!(zero.identity, two.identity);
        assert_eq!(
            zero.identity,
            Backend::discover(0, dir.path(), None)
                .settled()
                .expect("again")
                .identity
        );
        // And the configuration does, because the optimisation level is part of it.
        assert_ne!(zero.config, two.config);

        // The probe leaves nothing behind but the directory it was given.
        assert!(!dir.path().join("probe.ll").exists());

        // A whole-program build's objects are bitcode (R1.5 3.4), so its configuration, and
        // every key it carries, is not a separately optimised build's.
        let whole = Backend::discover_whole(2, dir.path(), None, true)
            .settled()
            .expect("settled");
        assert_ne!(whole.config, two.config);
    }

    #[test]
    fn only_an_optimised_macos_build_is_optimised_whole() {
        // R1.5 3.4: ld64 reads the bitcode through the toolchain's libLTO; MinGW's linker, the
        // boards' and WebAssembly's do not, and Linux is after 1.0.
        for t in ["aarch64-apple-darwin", "x86_64-apple-darwin"] {
            assert!(whole_program(2, Some(t)), "{t}");
            assert!(whole_program(3, Some(t)), "{t}");
            assert!(!whole_program(0, Some(t)), "{t}");
            assert!(!whole_program(1, Some(t)), "{t}");
        }
        for t in [
            "x86_64-pc-windows-gnu",
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "aarch64-apple-ios",
            "aarch64-linux-android",
            "wasm32-wasip1",
            "aarch64-unknown-none",
        ] {
            assert!(!whole_program(2, Some(t)), "{t}");
        }
        assert_eq!(whole_program(2, None), std::env::consts::OS == "macos");
        let flto = "-flto=thin".to_owned();
        assert!(compile_args(2, None, true).contains(&flto));
        assert!(link_args(2, None, true).contains(&flto));
        assert!(!compile_args(2, None, false).contains(&flto));
        assert!(!link_args(2, None, false).contains(&flto));
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
        let c = Backend::unidentified(0, None, false).link_command(Path::new("out"), &[]);
        let zero = c
            .get_envs()
            .find(|(k, _)| *k == "ZERO_AR_DATE")
            .and_then(|(_, v)| v);
        assert_eq!(zero, Some(std::ffi::OsStr::new("1")));
    }
}
