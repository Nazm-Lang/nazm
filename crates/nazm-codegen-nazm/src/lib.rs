//! The Nazm backend: each unit's LIR, printed, compiled by Nazm's own code generator.
//!
//! `docs/architecture.md` §7.43 is its constitution, and `openspec/changes/r9-code-generator/`
//! (decision D15) its design. The generator, `nzcg`, is a program written in Nazm
//! (`toolchain/codegen/`); this crate has no code generation of its own. Per unit it lowers and
//! validates the LIR exactly as the Cranelift backend does, prints it as `nazm lir --ops` does,
//! runs `nzcg --target TRIPLE [--one-thread] UNIT.lir OUTDIR` and reads back the one object the
//! generator wrote. A construct the generator refuses (its exit status 3, one `nzcg: refused:`
//! line each) is `N0101`, named, with nothing produced and no fallback to another backend; any
//! other failure is `N0900`, a compiler defect.
//!
//! The generator is found by [`VARIABLE`] (`NAZM_NZCG`), or else installed beside `nazm` as
//! `../libexec/nazm/nzcg`. Its BLAKE3 digest is part of [`Backend::version`], so a changed
//! generator is a new cache key, as Cranelift's revision and settings are. In process, with no
//! subprocess, comes at R8, when the compiler driving it is itself written in Nazm.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

use nazm_diag::{codes, Diagnostic};
use nazm_lir::backend::{units, Arch, Backend, BackendId, Code, Generated, Os, Planned, Target};
use nazm_lir::runtime::RUNTIME_ABI_REVISION;
use nazm_lir::{Native, Needs, SourceNames, Unit};
use nazm_span::Span;

/// The environment variable that names the generator, in place of the installed one.
pub const VARIABLE: &str = "NAZM_NZCG";

/// Where the generator is installed, relative to the directory holding `nazm`.
pub const INSTALLED: &str = "../libexec/nazm/nzcg";

/// The exit status with which the generator refuses what it does not carry (its design, D9).
const REFUSED: i32 = 3;

/// Whether the generator carries `target` before 1.0: the two macOS targets (its design, D13),
/// and `x86_64-pc-windows-gnu` (section 8: PE/COFF, Windows x64's convention, the failure bit as
/// LLVM keeps it there), which R8 needs where Cranelift builds it today. Its ELF writer exists
/// but is not run anywhere, so Linux is refused until after 1.0.
#[must_use]
pub fn carries(target: &Target) -> bool {
    (target.os == Os::Macos && matches!(target.arch, Arch::Aarch64 | Arch::X86_64))
        || (target.os == Os::Windows && target.arch == Arch::X86_64)
}

/// The generator this process would run: [`VARIABLE`] when it is set, else [`INSTALLED`]
/// beside the running executable.
///
/// # Errors
///
/// Where it is not, said in a sentence that names the path looked at.
pub fn locate() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os(VARIABLE) {
        let p = PathBuf::from(p);
        return if p.is_file() {
            Ok(p)
        } else {
            Err(format!(
                "`{VARIABLE}` names `{}`, which is not a file",
                p.display()
            ))
        };
    }
    let exe = std::env::current_exe().map_err(|e| format!("this executable's path: {e}"))?;
    let installed = exe
        .parent()
        .map(|d| d.join(INSTALLED))
        .ok_or_else(|| "this executable has no directory".to_owned())?;
    if installed.is_file() {
        Ok(installed)
    } else {
        Err(format!(
            "Nazm's code generator is not installed at `{}`; set `{VARIABLE}` to an `nzcg` \
             built from `toolchain/codegen/`",
            installed.display()
        ))
    }
}

/// A unit's LIR as `nazm lir --ops` prints it: the module, and with `debug` its positions and
/// one `; file INDEX NAME` line for each file they name (the module holds files only as
/// indices, and a line table needs their names).
#[must_use]
pub fn text(m: &nazm_lir::op::Module, names: &SourceNames<'_>, debug: bool) -> String {
    let mut out = nazm_lir::op::print::module(m);
    if debug {
        let mut files = std::collections::BTreeSet::new();
        for f in &m.functions {
            if let Some((file, _)) = f.at {
                files.insert(file);
            }
            for b in &f.blocks {
                for i in &b.insts {
                    if let nazm_lir::op::ir::Inst::Loc(p) = i {
                        files.insert(p.file);
                    }
                }
            }
        }
        for i in files {
            out.push_str(&format!("; file {i} {}\n", names.debug_file(i)));
        }
    }
    out
}

/// The Nazm backend.
#[derive(Debug, Clone)]
pub struct NazmCodegen {
    target: Target,
    /// Whether each object carries DWARF line tables (the generator's D10).
    debug: bool,
    /// Whether an eligible `Vec[R]` is laid out field by field (N68), as LIR decides it.
    soa: bool,
    /// Whether every object allocates through the counted heap (R1d, D5: `--heap-limit`).
    counted_heap: bool,
    /// Whether a program that calls no C allocates through the small-block heap (R1.5 3.1).
    small_heap: bool,
    /// The generator, when one was named rather than looked for.
    named: Option<PathBuf>,
    /// The generator and its digest, found and read once, when first needed: a build with
    /// another backend never reads it.
    program: OnceLock<Result<(PathBuf, String), String>>,
}

impl NazmCodegen {
    /// The Nazm backend for `target`, running the generator [`locate`] finds.
    #[must_use]
    pub const fn new(target: Target) -> Self {
        Self {
            target,
            debug: false,
            soa: false,
            counted_heap: false,
            small_heap: false,
            named: None,
            program: OnceLock::new(),
        }
    }

    /// The same backend, running the generator at `path`.
    #[must_use]
    pub fn with_program(mut self, path: PathBuf) -> Self {
        self.named = Some(path);
        self.program = OnceLock::new();
        self
    }

    /// The same backend, writing line tables into every object.
    #[must_use]
    pub const fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    /// The same backend, laying out every eligible `Vec[R]` field by field (N68).
    #[must_use]
    pub const fn with_soa(mut self, soa: bool) -> Self {
        self.soa = soa;
        self
    }

    /// The same backend, every object allocating through the counted heap (R1d, D5).
    #[must_use]
    pub const fn with_counted_heap(mut self, counted: bool) -> Self {
        self.counted_heap = counted;
        self
    }

    /// The same backend, a program that calls no C allocating through the small-block heap
    /// (R1.5 3.1, `nazm_lir::backend::small_heap`): its LIR's imports of C's allocator renamed,
    /// which the object's key then covers.
    #[must_use]
    pub const fn with_small_heap(mut self, small: bool) -> Self {
        self.small_heap = small;
        self
    }

    /// The generator and its BLAKE3 digest.
    ///
    /// # Errors
    ///
    /// Why there is none to run.
    pub fn program(&self) -> Result<&(PathBuf, String), String> {
        self.program
            .get_or_init(|| {
                let path = match &self.named {
                    Some(p) => p.clone(),
                    None => locate()?,
                };
                let bytes = std::fs::read(&path)
                    .map_err(|e| format!("Nazm's code generator `{}`: {e}", path.display()))?;
                Ok((path, blake3::hash(&bytes).to_hex().to_string()))
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// One unit's LIR, validated, on this target (N105) — as the Cranelift backend lowers it.
    fn lir(
        &self,
        native: &Native,
        unit: &Unit,
        names: &SourceNames<'_>,
    ) -> Result<nazm_lir::op::Module, Vec<Diagnostic>> {
        let data = nazm_lir::abi::DataLayout::of(&self.target);
        let mut m = nazm_lir::op::lower_unit(native, unit, names, data, self.debug, self.soa)?;
        nazm_lir::op::validate::module(&m)
            .map_err(|e| vec![defect(format!("LIR does not validate: {}", e.join("; ")))])?;
        if self.counted_heap {
            nazm_lir::op::count_heap(&mut m);
        } else if nazm_lir::backend::small_heap(self.small_heap, native) {
            nazm_lir::op::small_heap(&mut m);
        }
        Ok(m)
    }

    /// What an object is a function of: the unit's LIR, the generator (its digest, in the
    /// version), the target, the runtime, and whether the program has one thread — with one,
    /// the generator reads the stack limit as a plain global.
    fn key(&self, m: &nazm_lir::op::Module, one_thread: bool) -> String {
        let mut h = blake3::Hasher::new();
        h.update(self.version().as_bytes());
        h.update(self.target.triple.as_bytes());
        h.update(&RUNTIME_ABI_REVISION.to_le_bytes());
        h.update(nazm_lir::runtime::digest().as_bytes());
        h.update(&[u8::from(one_thread)]);
        h.update(nazm_lir::op::digest(m).as_bytes());
        h.finalize().to_hex().to_string()
    }

    /// Run the generator over `lir` in a scratch directory of its own, and read back its object.
    fn run(&self, lir: &str, one_thread: bool) -> Result<Vec<u8>, Vec<Diagnostic>> {
        let (program, _) = self.program().map_err(|e| vec![defect(e)])?;
        let dir = scratch().map_err(|e| vec![defect(format!("a scratch directory: {e}"))])?;
        let result = generate_in(program, &dir, &self.target.triple, one_thread, lir);
        let _ = std::fs::remove_dir_all(&dir);
        result
    }
}

/// `nzcg` over `lir` in `dir`.
fn generate_in(
    program: &Path,
    dir: &Path,
    triple: &str,
    one_thread: bool,
    lir: &str,
) -> Result<Vec<u8>, Vec<Diagnostic>> {
    let input = dir.join("unit.lir");
    let out = dir.join("objects");
    std::fs::write(&input, lir).map_err(|e| vec![defect(format!("writing the LIR: {e}"))])?;
    std::fs::create_dir(&out).map_err(|e| vec![defect(format!("{}: {e}", out.display()))])?;
    let mut cmd = Command::new(program);
    cmd.arg("--target").arg(triple);
    if one_thread {
        cmd.arg("--one-thread");
    }
    cmd.arg(&input).arg(&out);
    let o = cmd.output().map_err(|e| {
        vec![defect(format!(
            "Nazm's code generator `{}` did not run: {e}",
            program.display()
        ))]
    })?;
    let stderr = String::from_utf8_lossy(&o.stderr);
    match o.status.code() {
        Some(0) => {
            let object = out.join("unit-0.o");
            let extra = std::fs::read_dir(&out).map_or(0, Iterator::count);
            if extra != 1 {
                return Err(vec![defect(format!(
                    "Nazm's code generator wrote {extra} objects for one unit"
                ))]);
            }
            std::fs::read(&object).map_err(|e| {
                vec![defect(format!(
                    "Nazm's code generator wrote no object: {e}"
                ))]
            })
        }
        Some(REFUSED) => Err(refusals(&stderr)),
        status => Err(vec![defect(format!(
            "Nazm's code generator failed ({}): {}",
            status.map_or_else(|| "a signal".to_owned(), |s| format!("status {s}")),
            stderr.trim_end()
        ))]),
    }
}

/// One `N0101` per construct the generator refused, each named as it named it.
fn refusals(stderr: &str) -> Vec<Diagnostic> {
    let named: Vec<Diagnostic> = stderr
        .lines()
        .filter_map(|l| l.strip_prefix("nzcg: refused: "))
        .map(|what| refusal(what.to_owned()))
        .collect();
    if named.is_empty() {
        vec![defect(format!(
            "Nazm's code generator refused without naming what: {}",
            stderr.trim_end()
        ))]
    } else {
        named
    }
}

fn refusal(what: String) -> Diagnostic {
    Diagnostic::error(
        codes::UNSUPPORTED_CONSTRUCT,
        Span::synthetic(),
        format!("the Nazm backend cannot build this: {what}"),
    )
    .help("`nazm build --backend llvm` is the default backend")
}

fn defect(what: String) -> Diagnostic {
    Diagnostic::error(codes::COMPILER_DEFECT, Span::synthetic(), what)
}

/// A fresh directory for one run of the generator, created exclusively and, on Unix, with mode
/// 0700: never one someone else prepared on a shared temporary directory (Q1-A1-02).
fn scratch() -> std::io::Result<PathBuf> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let base = std::env::temp_dir();
    let pid = std::process::id();
    let mut last = None;
    for _ in 0..64 {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = base.join(format!("nazm-nzcg-{pid}-{n}"));
        match make(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no free scratch directory name")))
}

#[cfg(unix)]
fn make(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(dir)
}

#[cfg(not(unix))]
fn make(dir: &Path) -> std::io::Result<()> {
    std::fs::DirBuilder::new().create(dir)
}

impl Backend for NazmCodegen {
    fn id(&self) -> BackendId {
        BackendId::Nazm
    }

    fn version(&self) -> String {
        let digest = self
            .program()
            .map_or_else(|_| "none".to_owned(), |(_, d)| d.clone());
        format!(
            "nzcg-{digest} runtime-abi={RUNTIME_ABI_REVISION}{}",
            if self.debug { " debug" } else { "" }
        )
    }

    fn target(&self) -> &Target {
        &self.target
    }

    fn plan(&self, native: &Native, names: &SourceNames<'_>) -> Vec<Planned> {
        units(native)
            .into_iter()
            .map(|(name, unit, instance)| Planned {
                // A debug object is generated every time, never reused (N91). A unit whose LIR
                // cannot be lowered, or with no generator to run, has no key: generating it
                // reports why.
                key: if self.debug || self.program().is_err() {
                    None
                } else {
                    self.lir(native, unit, names)
                        .ok()
                        .map(|m| self.key(&m, native.single_thread))
                },
                name,
                instance,
            })
            .collect()
    }

    fn generate(
        &self,
        native: &Native,
        names: &SourceNames<'_>,
        unit: usize,
    ) -> Result<Generated, Vec<Diagnostic>> {
        let Some((_, u, _)) = nazm_lir::backend::unit(native, unit) else {
            return Err(vec![defect(format!("there is no unit {unit}"))]);
        };
        if !carries(&self.target) {
            return Err(vec![refusal(format!(
                "the target `{}` (before 1.0 Nazm's code generator builds for \
                 aarch64-apple-darwin, x86_64-apple-darwin and x86_64-pc-windows-gnu)",
                self.target.triple
            ))]);
        }
        let m = self.lir(native, u, names)?;
        let bytes = self.run(&text(&m, names, self.debug), native.single_thread)?;
        Ok(Generated {
            code: Code::Object(bytes),
            needs: m.needs,
        })
    }

    /// Every part of the runtime, as Cranelift's: a reused object must not need generating to
    /// say what it reaches.
    fn baseline(&self) -> Needs {
        Needs {
            tasks: true,
            channels: true,
            sequences: true,
            io: true,
            join: true,
            strings: true,
            shims: true,
            stack: true,
            closures: true,
            foreign: true,
            numbers: true,
            files: true,
            net: true,
            wide: true,
            // TLS links the platform's library: only where the program reaches it.
            tls: false,
            single: false,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_is_one_n0101_per_construct_named() {
        let d = refusals(
            "nzcg: refused: `f` in section `.x` (section placement, a board's, is not carried)\n\
             nzcg: refused: terminator kind 99, in `g` of a.nz\n",
        );
        assert_eq!(d.len(), 2);
        assert!(d.iter().all(|x| x.code == codes::UNSUPPORTED_CONSTRUCT));
        assert!(d[0].message.contains("in section `.x`"), "{}", d[0].message);
        assert!(
            d[1].message.contains("terminator kind 99"),
            "{}",
            d[1].message
        );
    }

    #[test]
    fn a_refusal_that_names_nothing_is_a_defect_not_a_silent_success() {
        let d = refusals("something else\n");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].code, codes::COMPILER_DEFECT);
    }

    #[test]
    fn only_the_macos_targets_and_windows_x64_are_carried_before_1_0() {
        for t in [
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-pc-windows-gnu",
        ] {
            assert!(carries(&Target::parse(t).unwrap()), "{t}");
        }
        for t in [
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu",
            "wasm32-wasip1",
            "aarch64-apple-ios",
            "aarch64-unknown-none",
        ] {
            assert!(!carries(&Target::parse(t).unwrap()), "{t}");
        }
    }

    #[test]
    fn the_generators_digest_is_in_the_version_and_moves_with_it() {
        let d = std::env::temp_dir().join(format!("nazm-nzcg-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let (a, b) = (d.join("a"), d.join("b"));
        std::fs::write(&a, b"one generator").unwrap();
        std::fs::write(&b, b"another generator").unwrap();
        let host = Target::parse("aarch64-apple-darwin").unwrap();
        let va = NazmCodegen::new(host.clone()).with_program(a).version();
        let vb = NazmCodegen::new(host).with_program(b).version();
        let _ = std::fs::remove_dir_all(&d);
        assert_ne!(va, vb);
        assert!(
            va.starts_with("nzcg-") && !va.starts_with("nzcg-none"),
            "{va}"
        );
    }

    #[test]
    fn a_missing_generator_is_named_and_gives_no_key() {
        let host = Target::parse("aarch64-apple-darwin").unwrap();
        let b = NazmCodegen::new(host).with_program(PathBuf::from("/nonexistent/nzcg"));
        let e = b.program().unwrap_err();
        assert!(e.contains("/nonexistent/nzcg"), "{e}");
        assert!(b.version().starts_with("nzcg-none"));
    }
}
