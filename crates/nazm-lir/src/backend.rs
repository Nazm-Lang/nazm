//! The backend boundary (N41): what a code generator is handed, what it hands back, and the
//! target it was asked for.
//!
//! `docs/architecture.md` §7.43. A backend reads a [`Native`] — validated MIR, and the units,
//! symbols, linkage and layout tables `crate::lower` settled from it for every backend alike —
//! and returns one artefact per unit: LLVM text for clang to compile, or object bytes. Nothing
//! of any code generator's own types appears here, so no crate above a backend's can depend on
//! one by accident.

use nazm_diag::Diagnostic;

use crate::lower::Native;
use crate::names::SourceNames;
use crate::Needs;

/// Which backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BackendId {
    /// LLVM IR as text, compiled by clang. The default, and the differential reference.
    Llvm,
    /// Cranelift, in process, to object bytes.
    Cranelift,
    /// Nazm's own code generator, `nzcg` (R9), run per unit on the printed LIR.
    Nazm,
}

impl BackendId {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Llvm => "llvm",
            Self::Cranelift => "cranelift",
            Self::Nazm => "nazm",
        }
    }

    #[must_use]
    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            "llvm" => Some(Self::Llvm),
            "cranelift" => Some(Self::Cranelift),
            "nazm" => Some(Self::Nazm),
            _ => None,
        }
    }
}

/// A processor family this compiler generates code for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arch {
    Aarch64,
    X86_64,
    /// WebAssembly, 32-bit linear memory (N64, §7.66).
    Wasm32,
    /// RISC-V, RV64GC (N86, §7.87): a board only.
    Riscv64,
    /// Arm Cortex-M, ARMv7-M Thumb-2 (Gate 3, `docs/spec.md`, *Boards*): a microcontroller board
    /// only, with 32-bit pointers and a 64-bit `Int`, as wasm32 has.
    Thumbv7m,
}

/// An operating system this compiler links for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Os {
    Macos,
    Linux,
    /// Windows, through MinGW-w64's C runtime (Gate 3, `docs/spec.md` *Windows x86_64*): its
    /// programs are run under Wine here, never claimed as run on a Windows host.
    Windows,
    /// No operating system and no C library (N62, §7.64): a freestanding image.
    None,
    /// WASI preview 1 (Gate 3, `docs/spec.md` *WASI*): WebAssembly whose outside is the
    /// `wasi_snapshot_preview1` imports, with a heap and no threads.
    Wasi,
    /// Android (Gate 3, `docs/spec.md` *Mobile*): Linux's kernel with Bionic, built with the NDK.
    Android,
    /// iOS (Gate 3, `docs/spec.md` *Mobile*): Darwin, built against Xcode's iOS SDK.
    Ios,
}

/// A target, by its explicit triple.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Target {
    pub triple: String,
    pub arch: Arch,
    pub os: Os,
}

impl Target {
    /// The v1 matrix: 64-bit ARM and x86 on macOS and Linux, x86 on Windows with MinGW-w64's
    /// toolchain (Gate 3), and the freestanding targets. A triple outside it is refused by name
    /// rather than guessed at.
    ///
    /// # Errors
    ///
    /// A triple this compiler has no code generator or runtime for.
    pub fn parse(triple: &str) -> Result<Self, String> {
        let (arch, os) = match triple {
            "aarch64-apple-darwin" => (Arch::Aarch64, Os::Macos),
            "aarch64-unknown-linux-gnu" => (Arch::Aarch64, Os::Linux),
            "x86_64-apple-darwin" => (Arch::X86_64, Os::Macos),
            "x86_64-unknown-linux-gnu" => (Arch::X86_64, Os::Linux),
            "x86_64-pc-windows-gnu" => (Arch::X86_64, Os::Windows),
            "aarch64-unknown-none" => (Arch::Aarch64, Os::None),
            "wasm32-unknown-unknown" => (Arch::Wasm32, Os::None),
            "wasm32-wasip1" => (Arch::Wasm32, Os::Wasi),
            "aarch64-linux-android" => (Arch::Aarch64, Os::Android),
            "aarch64-apple-ios" => (Arch::Aarch64, Os::Ios),
            "riscv64gc-unknown-none-elf" => (Arch::Riscv64, Os::None),
            "thumbv7m-none-eabi" => (Arch::Thumbv7m, Os::None),
            other => {
                return Err(format!(
                    "`{other}` is not a target this compiler generates code for; the \
                     targets are aarch64-apple-darwin, aarch64-unknown-linux-gnu, \
                     x86_64-apple-darwin, x86_64-unknown-linux-gnu, x86_64-pc-windows-gnu and, \
                     freestanding, \
                     aarch64-unknown-none, riscv64gc-unknown-none-elf, thumbv7m-none-eabi and \
                     wasm32-unknown-unknown, WebAssembly with WASI, wasm32-wasip1, and the mobile \
                     targets aarch64-linux-android and aarch64-apple-ios"
                ))
            }
        };
        Ok(Self {
            triple: triple.to_owned(),
            arch,
            os,
        })
    }

    /// Whether this is a board (N62, N86): a target with device registers, a linker script and a
    /// UART — every freestanding target that is not WebAssembly.
    #[must_use]
    pub fn is_board(&self) -> bool {
        self.os == Os::None && self.arch != Arch::Wasm32
    }

    /// How a Nazm function's failure crosses a call in the LLVM backend's code for this target
    /// (4.2, `docs/architecture.md` §7.115): a forced unwind on the hosted targets, whose system
    /// has an unwinder and the C personality; the failure bit on WebAssembly, WASI and the boards,
    /// which have none, and on Windows until its unwinding is verified under Wine.
    #[must_use]
    pub fn failures(&self) -> crate::op::llvm::Failures {
        match self.os {
            Os::Macos | Os::Linux | Os::Android | Os::Ios => crate::op::llvm::Failures::Unwind,
            Os::Windows | Os::None | Os::Wasi => crate::op::llvm::Failures::Bit,
        }
    }

    /// Whether this is WebAssembly (N64): freestanding, its outside reached only by imports.
    #[must_use]
    pub fn is_wasm(&self) -> bool {
        self.arch == Arch::Wasm32
    }

    /// Whether this is WebAssembly with WASI (Gate 3): its imports WASI's, with a heap.
    #[must_use]
    pub fn is_wasi(&self) -> bool {
        self.os == Os::Wasi
    }

    /// The machine this compiler is running on.
    ///
    /// # Errors
    ///
    /// A host outside the v1 matrix.
    pub fn host() -> Result<Self, String> {
        let triple = match (std::env::consts::ARCH, std::env::consts::OS) {
            ("aarch64", "macos") => "aarch64-apple-darwin",
            ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
            ("x86_64", "macos") => "x86_64-apple-darwin",
            ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
            (a, o) => {
                return Err(format!(
                    "this host ({a}-{o}) is outside the v1 target matrix"
                ))
            }
        };
        Self::parse(triple)
    }

    /// The target a build asked for: the host, or any triple of the matrix (N57, §7.59).
    ///
    /// # Errors
    ///
    /// A triple outside the matrix, or a host outside it.
    pub fn requested(triple: Option<&str>) -> Result<Self, String> {
        let host = Self::host()?;
        let Some(t) = triple else {
            return Ok(host);
        };
        Self::parse(t)
    }

    /// Whether the host's own toolchain links an executable for this target (N57): the host
    /// itself, or the other macOS architecture, whose SDK the Apple toolchain carries. Any other
    /// target is built to objects for its own toolchain to link.
    #[must_use]
    pub fn links_on(&self, host: &Self) -> bool {
        self == host || (self.os == Os::Macos && host.os == Os::Macos)
    }
}

/// What a unit became.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Code {
    /// LLVM IR text, for clang to compile to an object.
    Llvm(String),
    /// An object, ready to link.
    Object(Vec<u8>),
}

/// One unit, before anything is generated for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    /// `m{i}` for the i-th module's unit, `g{i}` for the i-th instance's.
    pub name: String,
    /// Whether it is a generic instance's unit.
    pub instance: bool,
    /// What the unit's object is a function of — its MIR, its layouts, the target, the
    /// backend's version and settings, the runtime ABI revision — when a backend can say so
    /// before generating it, so a cached object is reused without generating anything. `None`
    /// for LLVM text, whose key is its own bytes once they exist.
    pub key: Option<String>,
}

/// One unit, generated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    pub code: Code,
    /// What the unit reaches of the runtime.
    pub needs: Needs,
}

/// Everything a backend produced for one program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emitted {
    pub units: Vec<(Planned, Generated)>,
    /// What the program's units reach of the runtime.
    pub needs: Needs,
}

/// A code generator.
pub trait Backend {
    fn id(&self) -> BackendId;
    /// Its version and every codegen-affecting setting, as one string that enters every cache
    /// key: a new code generator or a changed setting is a new key.
    fn version(&self) -> String;
    fn target(&self) -> &Target;
    /// Every unit of a program, modules first and then instances, in the order they link.
    fn plan(&self, native: &Native, names: &SourceNames<'_>) -> Vec<Planned>;
    /// One unit, by its position in [`Self::plan`].
    ///
    /// # Errors
    ///
    /// An operation or type outside this backend's support set (`N0101`), named, with nothing
    /// produced; or a compiler defect (`N0900`).
    fn generate(
        &self,
        native: &Native,
        names: &SourceNames<'_>,
        unit: usize,
    ) -> Result<Generated, Vec<Diagnostic>>;
    /// What of the runtime any unit of this backend's may reach, whatever it contains — so a
    /// unit reused from the cache needs no generation to say what it links against.
    fn baseline(&self) -> Needs;

    /// Every unit, generated.
    ///
    /// # Errors
    ///
    /// As [`Self::generate`], collected over every unit.
    fn emit(&self, native: &Native, names: &SourceNames<'_>) -> Result<Emitted, Vec<Diagnostic>> {
        let mut needs = self.baseline();
        let mut units = Vec::new();
        let mut errors = Vec::new();
        for (i, p) in self.plan(native, names).into_iter().enumerate() {
            match self.generate(native, names, i) {
                Ok(g) => {
                    needs = needs.union(g.needs);
                    units.push((p, g));
                }
                Err(mut e) => errors.append(&mut e),
            }
        }
        if errors.is_empty() {
            Ok(Emitted { units, needs })
        } else {
            Err(errors)
        }
    }
}

/// A native program's units, modules first and then instances, with their artefact names.
#[must_use]
pub fn units(native: &Native) -> Vec<(String, &crate::ir::Unit, bool)> {
    native
        .units
        .iter()
        .enumerate()
        .map(|(i, u)| (format!("m{i}"), u, false))
        .chain(
            native
                .instances
                .iter()
                .enumerate()
                .map(|(i, u)| (format!("g{i}"), u, true)),
        )
        .collect()
}

/// The LLVM text backend.
#[derive(Debug, Clone)]
pub struct LlvmText {
    target: Target,
    /// Whether each unit carries debug information (N48).
    debug: bool,
    /// Whether an eligible `Vec[R]` is laid out field by field (N68, §7.70).
    soa: bool,
    /// Whether every unit allocates through the counted heap (R1d, D5: `--heap-limit`).
    counted_heap: bool,
}

impl LlvmText {
    #[must_use]
    pub const fn new(target: Target) -> Self {
        Self {
            target,
            debug: false,
            soa: false,
            counted_heap: false,
        }
    }

    /// The same backend, every unit allocating through the counted heap (R1d, D5).
    #[must_use]
    pub const fn with_counted_heap(mut self, counted: bool) -> Self {
        self.counted_heap = counted;
        self
    }

    /// The same backend, laying out every eligible `Vec[R]` as one array per field (N68).
    #[must_use]
    pub const fn with_soa(mut self, soa: bool) -> Self {
        self.soa = soa;
        self
    }

    /// The same backend, writing debug information into every unit it generates (N48).
    #[must_use]
    pub const fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }
}

impl Backend for LlvmText {
    fn id(&self) -> BackendId {
        BackendId::Llvm
    }

    fn version(&self) -> String {
        // The object cache already keys LLVM text on its own bytes and on the clang that
        // compiles it (`crates/nazm-cli/src/backend.rs`); this names the emitter.
        match (self.debug, self.soa) {
            (true, true) => "llvm-text debug soa".to_owned(),
            (true, false) => "llvm-text debug".to_owned(),
            (false, true) => "llvm-text soa".to_owned(),
            (false, false) => "llvm-text".to_owned(),
        }
    }

    fn target(&self) -> &Target {
        &self.target
    }

    fn plan(&self, native: &Native, _names: &SourceNames<'_>) -> Vec<Planned> {
        units(native)
            .into_iter()
            .map(|(name, _, instance)| Planned {
                name,
                instance,
                key: None,
            })
            .collect()
    }

    fn generate(
        &self,
        native: &Native,
        names: &SourceNames<'_>,
        unit: usize,
    ) -> Result<Generated, Vec<Diagnostic>> {
        let Some((_, u, _)) = units(native).into_iter().nth(unit) else {
            return Ok(Generated {
                code: Code::Llvm(String::new()),
                needs: Needs::default(),
            });
        };
        // The unit as LIR on this target, validated (N105, §7.106): every decision a program can
        // observe is made there, and this backend prints it.
        let m = crate::op::lower_unit(
            native,
            u,
            names,
            crate::abi::DataLayout::of(&self.target),
            self.debug,
            self.soa,
        )?;
        if let Err(e) = crate::op::validate::module(&m) {
            return Err(vec![Diagnostic::error(
                nazm_diag::codes::COMPILER_DEFECT,
                nazm_span::Span::synthetic(),
                format!("LIR does not validate: {}", e.join("; ")),
            )]);
        }
        let text = crate::op::llvm::module(&m, self.target.failures());
        let ir = if self.debug {
            crate::debug::attach(&text, &|i| names.debug_file(i))
        } else {
            text
        };
        let needs = m.needs;
        // A freestanding image has one thread and no thread-local storage (N62, §7.64), and a
        // hosted program whose `main` holds no authority to start a thread has one thread too
        // (the speed milestone): the failure state and stack limit the unit declares are ordinary
        // globals there, as the runtime defines them (`Needs::single`).
        let one_thread = |l: &str| {
            nazm_runtime::SINGLE_THREAD_STATE
                .iter()
                .any(|g| l.starts_with(&format!("@{g} = ")))
        };
        let ir = if self.target.os == Os::None || native.single_thread {
            ir.lines()
                .map(|l| {
                    if l.starts_with('@')
                        && l.contains(" thread_local global ")
                        && (self.target.os == Os::None || one_thread(l))
                    {
                        l.replacen(" thread_local ", " ", 1)
                    } else {
                        l.to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        } else {
            ir
        };
        // A hosted unit inlines the runtime's reference counting where it optimises (R1, 5b.9); a
        // board's runtime defines its own.
        let ir = if self.target.os == Os::None {
            ir
        } else {
            nazm_runtime::inline_reference_counting(&ir)
        };
        // A build with a heap limit (R1d, D5): the unit's allocations, the copies above included,
        // are the counted heap's.
        let ir = if self.counted_heap {
            nazm_runtime::heap::count_heap(&ir)
        } else {
            ir
        };
        Ok(Generated {
            code: Code::Llvm(ir),
            needs,
        })
    }

    fn baseline(&self) -> Needs {
        Needs::default()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_triple_is_refused_by_name() {
        let e = Target::parse("riscv64gc-unknown-linux-gnu").unwrap_err();
        assert!(e.contains("riscv64gc"), "{e}");
    }

    #[test]
    fn a_foreign_target_is_itself_and_linked_only_where_the_host_can() {
        // N57: a requested triple is that target, never the host in disguise.
        let host = Target::host().unwrap();
        for t in [
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu",
        ] {
            assert_eq!(
                Target::requested(Some(t)).unwrap(),
                Target::parse(t).unwrap()
            );
        }
        assert_eq!(Target::requested(None).unwrap(), host);
        // The Apple toolchain links both macOS architectures; nothing else is linked from
        // another operating system or another Linux architecture.
        let mac_arm = Target::parse("aarch64-apple-darwin").unwrap();
        let mac_x86 = Target::parse("x86_64-apple-darwin").unwrap();
        let linux_arm = Target::parse("aarch64-unknown-linux-gnu").unwrap();
        let linux_x86 = Target::parse("x86_64-unknown-linux-gnu").unwrap();
        assert!(mac_x86.links_on(&mac_arm) && mac_arm.links_on(&mac_x86));
        assert!(!linux_arm.links_on(&mac_arm) && !mac_arm.links_on(&linux_arm));
        assert!(!linux_x86.links_on(&linux_arm) && linux_arm.links_on(&linux_arm));
    }
}

/// What this compiler has shown of a target under one backend (N95, `docs/architecture.md` §7.96).
/// A cell is run, run under Wine, run on a Linux kernel as Bionic, compiled only, or refused;
/// there is no sixth answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Programs built for it ran — on the host, under Rosetta, in a container or an emulator —
    /// and agreed with the interpreter. The evidence: a test, or a contained command.
    Run(&'static str),
    /// Programs built for Windows ran under Wine in a Linux container and agreed with the
    /// interpreter (Gate 3, `docs/systems-domains.md` Part A-W). Never evidence from a Windows
    /// host, and never reported as plain run-verified.
    Wine(&'static str),
    /// Programs built for Android ran as static Bionic executables on an aarch64 Linux kernel in a
    /// container and agreed with what they should print (Gate 3, `docs/spec.md` *Mobile*): Android's
    /// C library on Linux, never an Android device or emulator, and never reported as plain
    /// run-verified.
    Bionic(&'static str),
    /// Its objects are built and read from their headers; nothing built for it has run here.
    /// The reason, and the test that reads the objects.
    Compile(&'static str),
    /// Refused by name, with why.
    Unsupported(&'static str),
}

impl Support {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Run(_) => "run-verified",
            Self::Wine(_) => "run-verified under Wine",
            Self::Bionic(_) => "run-verified on a Linux kernel",
            Self::Compile(_) => "compile-only",
            Self::Unsupported(_) => "unsupported",
        }
    }

    #[must_use]
    pub const fn evidence(self) -> &'static str {
        match self {
            Self::Run(e)
            | Self::Wine(e)
            | Self::Bionic(e)
            | Self::Compile(e)
            | Self::Unsupported(e) => e,
        }
    }
}

/// One row of the support matrix.
#[derive(Debug, Clone, Copy)]
pub struct Cell {
    pub triple: &'static str,
    /// The CPU the LLVM backend's objects are compiled for: what the host's C compiler driver,
    /// Apple clang, resolves for the triple, which `platform_matrix.rs` checks against it. Another
    /// driver may choose another — clang 19 resolves `core2` for `x86_64-apple-darwin`.
    pub llvm_cpu: &'static str,
    /// The CPU Cranelift compiles for: the triple's baseline ISA, no feature of the building
    /// host detected.
    pub cranelift_cpu: &'static str,
    pub llvm: Support,
    pub cranelift: Support,
    /// What a debugger has been shown to do with a `--debug` build for it (N48, N58, N91).
    pub debugger: &'static str,
}

const NO_CRANELIFT_BOARD: Support = Support::Unsupported(
    "Cranelift's object writer has no format for a target without an operating system",
);

/// The support matrix (N95): every target the compiler accepts, and what is known of each.
pub const SUPPORT: &[Cell] = &[
    Cell {
        triple: "aarch64-apple-darwin",
        llvm_cpu: "apple-m1",
        cranelift_cpu: "aarch64 baseline",
        llvm: Support::Run(
            "a_debug_build_runs_as_an_ordinary_one_and_an_ordinary_one_carries_no_debug_information",
        ),
        cranelift: Support::Run(
            "a_debug_build_runs_as_an_ordinary_one_and_an_ordinary_one_carries_no_debug_information",
        ),
        debugger: "lldb, breakpoints resolved statically, both backends",
    },
    Cell {
        triple: "x86_64-apple-darwin",
        llvm_cpu: "penryn",
        cranelift_cpu: "x86-64 baseline (SSE2)",
        llvm: Support::Run("a_program_for_the_other_macos_architecture_runs_where_rosetta_does"),
        cranelift: Support::Run(
            "a_program_for_the_other_macos_architecture_runs_where_rosetta_does",
        ),
        debugger: "not exercised",
    },
    Cell {
        triple: "aarch64-unknown-linux-gnu",
        llvm_cpu: "generic",
        cranelift_cpu: "aarch64 baseline",
        llvm: Support::Run("cargo xtask contained tests"),
        cranelift: Support::Run("cargo xtask contained tests"),
        debugger: "gdb, live, both backends (nazm-debug image)",
    },
    Cell {
        triple: "x86_64-unknown-linux-gnu",
        llvm_cpu: "x86-64",
        cranelift_cpu: "x86-64 baseline (SSE2)",
        llvm: Support::Compile("every_target_is_built_by_both_backends_in_its_own_format"),
        cranelift: Support::Compile("every_target_is_built_by_both_backends_in_its_own_format"),
        debugger: "not exercised",
    },
    Cell {
        triple: "x86_64-pc-windows-gnu",
        llvm_cpu: "x86-64",
        cranelift_cpu: "x86-64 baseline (SSE2)",
        llvm: Support::Wine("the_programs_run_under_wine_as_they_do_natively"),
        cranelift: Support::Wine("the_programs_run_under_wine_as_they_do_natively"),
        debugger: "not exercised",
    },
    Cell {
        triple: "aarch64-unknown-none",
        llvm_cpu: "generic",
        cranelift_cpu: "none",
        llvm: Support::Run("the_image_boots_on_qemu_and_reports_on_the_uart"),
        cranelift: NO_CRANELIFT_BOARD,
        debugger: "not exercised",
    },
    Cell {
        triple: "riscv64gc-unknown-none-elf",
        llvm_cpu: "generic-rv64",
        cranelift_cpu: "none",
        llvm: Support::Run("both_boards_boot_and_agree_under_qemu"),
        cranelift: NO_CRANELIFT_BOARD,
        debugger: "not exercised",
    },
    Cell {
        triple: "thumbv7m-none-eabi",
        llvm_cpu: "cortex-m3",
        cranelift_cpu: "none",
        llvm: Support::Run("an_image_for_the_cortex_m_board_boots_under_qemu"),
        cranelift: NO_CRANELIFT_BOARD,
        debugger: "not exercised",
    },
    Cell {
        triple: "wasm32-unknown-unknown",
        llvm_cpu: "generic",
        cranelift_cpu: "none",
        llvm: Support::Run("the_corpus_runs_as_webassembly_as_the_interpreter_runs_it"),
        cranelift: Support::Unsupported("this Cranelift is built without a WebAssembly backend"),
        debugger: "DWARF sections present; a debugger not exercised",
    },
    Cell {
        triple: "wasm32-wasip1",
        llvm_cpu: "generic",
        cranelift_cpu: "none",
        llvm: Support::Run("a_wasi_module_has_a_heap_streams_arguments_files_and_clocks"),
        cranelift: Support::Unsupported("this Cranelift is built without a WebAssembly backend"),
        debugger: "DWARF sections present; a debugger not exercised",
    },
    Cell {
        triple: "aarch64-linux-android",
        llvm_cpu: "generic",
        cranelift_cpu: "none",
        llvm: Support::Bionic(
            "a_static_bionic_executable_runs_a_nazm_library_and_a_whole_program_on_linux",
        ),
        cranelift: NO_CRANELIFT_MOBILE,
        debugger: "not exercised",
    },
    Cell {
        triple: "aarch64-apple-ios",
        llvm_cpu: "apple-a7",
        cranelift_cpu: "none",
        llvm: Support::Compile("an_ios_archive_and_program_link_against_the_ios_sdk"),
        cranelift: NO_CRANELIFT_MOBILE,
        debugger: "not exercised",
    },
];

const NO_CRANELIFT_MOBILE: Support = Support::Unsupported(
    "the mobile targets are the LLVM backend's: Cranelift is not exercised for them",
);

/// What is refused by decision rather than by what this machine lacks, and why.
pub const NON_GOALS: &[(&str, &str)] = &[
    (
        "Windows with Microsoft's toolchain, and Windows ARM64",
        "unsupported: `x86_64-pc-windows-msvc` needs Microsoft's SDK and C runtime, which are not \
         downloaded or accepted here, and no Windows ARM64 machine or emulator is here; Windows \
         x86_64 is `x86_64-pc-windows-gnu`, with MinGW-w64's",
    ),
    (
        "32-bit hosted targets",
        "a non-goal: `Int` is 64-bit and every hosted layout assumes 64-bit pointers; wasm32 — \
         freestanding, or with WASI's imports and its own heap — and the Cortex-M board \
         (`thumbv7m-none-eabi`) are the 32-bit targets, where a C handle, a C string result and a \
         callback are refused by name (`N0381`, Q1-A1-05) because a C pointer crosses as 64 bits",
    ),
    (
        "big-endian targets",
        "a non-goal: no big-endian machine or image is here to run one, and the DWARF writer and \
         layouts assume little-endian",
    ),
];
