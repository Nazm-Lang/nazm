//! `nazm accel FILE KERNEL --input FILE [--input FILE] [--reduce OP] [--provider P]` (N67, §7.69;
//! N88, §7.89; Gate 3): map an eligible Nazm function over one sequence, or zip it over two, on an
//! accelerator, optionally fold the results in index order, and say exactly what moved and when —
//! then hold the result to the interpreter's.
//!
//! The kernel is one source text generated from the function's Core IR (`nazm_service::kernel`):
//! OpenCL C, with an element function beside the device kernel. A **provider** runs it (Gate 3,
//! `docs/spec.md`, *Providers*): the OpenCL provider builds it for macOS's GPU through a small C
//! host program; the CPU provider compiles the same text as C behind a prelude and runs the
//! element function over a pool of threads. Each is a process: the compiler links no GPU API and
//! no thread pool of its own, and holds no `unsafe` code to call one. Every transfer and the one
//! synchronisation are explicit in the host program and timed in the report.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use nazm_diag::{codes, Diagnostic};
use nazm_service::kernel::{Elem, Kernel};
use nazm_span::Span;
use serde_json::{json, Value};

use crate::exit;

/// The OpenCL provider's host program, compiled once per run against the system's OpenCL.
const HOST: &str = include_str!("accel_host.c");

/// The CPU provider's host program (Gate 3): the kernel's text included after a prelude.
const CPU_HOST: &str = include_str!("accel_cpu.c");

/// At most this many elements: a failure is recorded as `index · 256 + site` in one 32-bit word.
const MAX_ELEMENTS: usize = 8_000_000;

fn refuse(at: Span, message: String) -> Diagnostic {
    Diagnostic::error(codes::KERNEL_INELIGIBLE, at, message)
        .primary_label("not a kernel")
        .help("a kernel is a pure `(T) -> T` or `(T, T) -> T` function — `T` an `Int`, `Float32` or `Float64` — of `T`s and `Bool`s, calling only others like it, without recursion")
}

/// A reduction (N88): its name, and the fold — in index order, checked as `ints_sum` is for
/// `add`, so a running sum that overflows fails at the element that made it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reduce {
    Add,
    Min,
    Max,
}

impl Reduce {
    pub(crate) fn by_name(name: &str) -> Option<Self> {
        match name {
            "add" => Some(Self::Add),
            "min" => Some(Self::Min),
            "max" => Some(Self::Max),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Min => "min",
            Self::Max => "max",
        }
    }

    /// The fold of `xs`: `Ok(None)` for an empty one under `min` or `max`, `Err(i)` where the
    /// running sum overflows at element `i`.
    fn fold(self, xs: &[i64]) -> Result<Option<i64>, usize> {
        match self {
            Self::Add => {
                let mut sum: i64 = 0;
                for (i, x) in xs.iter().enumerate() {
                    sum = sum.checked_add(*x).ok_or(i)?;
                }
                Ok(Some(sum))
            }
            Self::Min => Ok(xs.iter().copied().min()),
            Self::Max => Ok(xs.iter().copied().max()),
        }
    }

    /// The fold of a float kernel's results (Gate 3), in index order: the IEEE sum, or `min` and
    /// `max` as `float_min` and `float_max` — a NaN losing to a number. In `f64`, which holds every
    /// `Float32` exactly; a `Float32` sum is rounded to `Float32` at each step, as the language's.
    fn fold_float(self, xs: &[f64], single: bool) -> Option<f64> {
        let round = |x: f64| {
            if single {
                #[allow(clippy::cast_possible_truncation)]
                let r = f64::from(x as f32);
                r
            } else {
                x
            }
        };
        match self {
            Self::Add => Some(xs.iter().fold(0.0, |a, x| round(a + x))),
            Self::Min => xs.iter().copied().reduce(f64::min),
            Self::Max => xs.iter().copied().reduce(f64::max),
        }
    }
}

/// Which provider runs the kernel (Gate 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Choice {
    Auto,
    OpenCl,
    Cpu,
}

impl Choice {
    pub(crate) fn by_name(name: &str) -> Self {
        match name {
            "opencl" => Self::OpenCl,
            "cpu" => Self::Cpu,
            _ => Self::Auto,
        }
    }
}

/// What a provider gives back: the host program's report, the results as bits, and the build's
/// time.
struct Ran {
    report: Value,
    results: Vec<u64>,
    build_ms: f64,
}

/// Why a provider did not run the kernel: it is not there — `auto` moves on — or it failed.
enum NotRun {
    Unavailable(String),
    Failed(String),
}

/// The boundary (Gate 3): a provider runs one kernel's text over its inputs, as files in `dir`.
/// Nothing here is OpenCL's but the OpenCL provider's own host program.
trait Provider {
    fn name(&self) -> &'static str;
    /// The API it runs through, for the report's `device`.
    fn api(&self) -> &'static str;
    /// The host program's text, part of the kernel's identity.
    fn host(&self) -> String;
    fn run(&self, k: &Kernel, inputs: &[Vec<u8>], dir: &Path) -> Result<Ran, NotRun>;
}

struct OpenCl;

struct Cpu {
    scalar: bool,
}

/// `program` compiled from `source` in `dir` with `flags`, timed.
fn build(dir: &Path, program: &str, source: &str, flags: &[String]) -> Result<f64, NotRun> {
    let started = std::time::Instant::now();
    let built = Command::new(crate::backend::DRIVER)
        .current_dir(dir)
        .args(flags)
        .arg("-o")
        .arg(program)
        .arg(source)
        .output();
    match built {
        Ok(o) if o.status.success() => Ok(started.elapsed().as_secs_f64() * 1e3),
        Ok(o) => Err(NotRun::Failed(format!(
            "{}the host program did not build",
            String::from_utf8_lossy(&o.stderr)
        ))),
        Err(e) => Err(NotRun::Unavailable(format!(
            "`{}` could not be run: {e}",
            crate::backend::DRIVER
        ))),
    }
}

/// `program` run over the files in `dir`: its report and the results it wrote, as bits.
fn launch(
    dir: &Path,
    program: &str,
    elem: Elem,
    inputs: usize,
) -> Result<(Value, Vec<u64>), (Option<i32>, String)> {
    let mut host = Command::new(dir.join(program));
    host.arg(dir.join("kernel.cl")).arg(dir.join("output.bin"));
    for i in 0..inputs {
        host.arg(dir.join(format!("input{i}.bin")));
    }
    match host.output() {
        Ok(o) if o.status.success() => {
            let r: Value = serde_json::from_slice(&o.stdout).unwrap_or(Value::Null);
            let out = std::fs::read(dir.join("output.bin")).unwrap_or_default();
            Ok((r, from_bytes(&out, elem)))
        }
        Ok(o) => Err((
            o.status.code(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            ),
        )),
        Err(e) => Err((None, format!("the host program could not be run: {e}"))),
    }
}

impl Provider for OpenCl {
    fn name(&self) -> &'static str {
        "opencl"
    }

    fn api(&self) -> &'static str {
        "OpenCL"
    }

    fn host(&self) -> String {
        HOST.to_owned()
    }

    fn run(&self, k: &Kernel, inputs: &[Vec<u8>], dir: &Path) -> Result<Ran, NotRun> {
        if !cfg!(target_os = "macos") {
            return Err(NotRun::Unavailable(
                "this host has no OpenCL: the OpenCL provider is macOS's".to_owned(),
            ));
        }
        write(dir, "host.c", HOST)?;
        let mut flags = vec![
            "-O2".to_owned(),
            "-framework".to_owned(),
            "OpenCL".to_owned(),
            format!("-DNZ_SIZE={}", k.elem.bytes()),
        ];
        if k.elem != Elem::Int {
            flags.push("-DNZ_FLOAT".to_owned());
        }
        if k.elem == Elem::F64 {
            flags.push("-DNZ_FP64".to_owned());
        }
        let build_ms = build(dir, "host", "host.c", &flags)?;
        match launch(dir, "host", k.elem, inputs.len()) {
            Ok((report, results)) => Ok(Ran {
                report,
                results,
                build_ms,
            }),
            // No platform, no GPU, or no double precision on it: not this host's provider.
            Err((Some(3 | 5), why))
                if why.contains("\"platform\"")
                    || why.contains("\"device\"")
                    || why.contains("no fp64") =>
            {
                Err(NotRun::Unavailable(if why.contains("no fp64") {
                    "the GPU has no double precision (`cl_khr_fp64`), which a `Float64` kernel needs"
                        .to_owned()
                } else {
                    "this host's OpenCL has no GPU".to_owned()
                }))
            }
            Err((_, why)) => Err(NotRun::Failed(format!(
                "{why}the accelerator could not run the kernel"
            ))),
        }
    }
}

impl Provider for Cpu {
    fn name(&self) -> &'static str {
        "cpu"
    }

    fn api(&self) -> &'static str {
        "C (the CPU provider)"
    }

    fn host(&self) -> String {
        format!("{CPU_HOST}\n// scalar: {}\n", self.scalar)
    }

    fn run(&self, k: &Kernel, inputs: &[Vec<u8>], dir: &Path) -> Result<Ran, NotRun> {
        write(dir, "cpu.c", CPU_HOST)?;
        let mut flags = vec![
            "-O2".to_owned(),
            // The language's arithmetic, one operation at a time: no fused multiply-add.
            "-ffp-contract=off".to_owned(),
            "-pthread".to_owned(),
            "-Wno-unknown-pragmas".to_owned(),
            format!("-DNZ_T={}", k.elem.c()),
        ];
        if k.arity == 2 {
            flags.push("-DNZ_ZIP".to_owned());
        }
        if self.scalar {
            flags.extend(["-fno-vectorize".to_owned(), "-fno-slp-vectorize".to_owned()]);
            flags.push("-DNZ_VECTORISED=\"scalar\"".to_owned());
        } else {
            flags.push("-DNZ_VECTORISED=\"vectorised\"".to_owned());
        }
        flags.extend(crate::backend::system_libraries(None));
        let build_ms = build(dir, "cpu", "cpu.c", &flags)?;
        match launch(dir, "cpu", k.elem, inputs.len()) {
            Ok((report, results)) => Ok(Ran {
                report,
                results,
                build_ms,
            }),
            Err((_, why)) => Err(NotRun::Failed(format!(
                "{why}the CPU provider could not run the kernel"
            ))),
        }
    }
}

fn write(dir: &Path, name: &str, text: &str) -> Result<(), NotRun> {
    std::fs::write(dir.join(name), text)
        .map_err(|e| NotRun::Failed(format!("cannot write {}: {e}", dir.display())))
}

/// Elements as the bytes a provider reads: each little-endian, of the kernel's width.
fn to_bytes(xs: &[u64], elem: Elem) -> Vec<u8> {
    xs.iter()
        .flat_map(|x| x.to_le_bytes().into_iter().take(elem.bytes()))
        .collect()
}

/// What a provider wrote, as bits.
fn from_bytes(out: &[u8], elem: Elem) -> Vec<u64> {
    out.chunks_exact(elem.bytes())
        .map(|c| {
            let mut b = [0u8; 8];
            for (to, from) in b.iter_mut().zip(c) {
                *to = *from;
            }
            u64::from_le_bytes(b)
        })
        .collect()
}

/// One element of the kernel's type from its text, as bits.
fn parse(token: &str, elem: Elem) -> Option<u64> {
    match elem {
        #[allow(clippy::cast_sign_loss)]
        Elem::Int => token.parse::<i64>().ok().map(|v| v as u64),
        Elem::F32 => token.parse::<f32>().ok().map(|v| u64::from(v.to_bits())),
        Elem::F64 => token.parse::<f64>().ok().map(f64::to_bits),
    }
}

/// An element's bits as the interpreter's value.
fn value(bits: u64, elem: Elem) -> nazm_core::Value {
    match elem {
        #[allow(clippy::cast_possible_wrap)]
        Elem::Int => nazm_core::Value::Int(bits as i64),
        Elem::F32 => nazm_core::Value::Num(nazm_core::sema::numeric::NumVal {
            kind: nazm_core::sema::NumKind::F32,
            bits: u128::from(bits),
        }),
        Elem::F64 => nazm_core::Value::Num(nazm_core::sema::numeric::NumVal {
            kind: nazm_core::sema::NumKind::F64,
            bits: u128::from(bits),
        }),
    }
}

/// An element's bits as a number, for a float fold.
fn as_f64(bits: u64, elem: Elem) -> f64 {
    #[allow(clippy::cast_possible_truncation)]
    match elem {
        Elem::F32 => f64::from(f32::from_bits(bits as u32)),
        _ => f64::from_bits(bits),
    }
}

/// Whether two results are the same (Gate 3): bit for bit, every NaN alike — a NaN's payload is
/// the hardware's, and the language promises none.
fn same(a: &[u64], b: &[u64], elem: Elem) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x == y || (elem != Elem::Int && as_f64(*x, elem).is_nan() && as_f64(*y, elem).is_nan())
        })
}

fn work_dir() -> std::io::Result<PathBuf> {
    crate::scratch::temporary("nazm-accel-")
}

/// The fold of `results`, for the report and for comparison: an `Int` fold's value or failing
/// index, a float fold's value as the language writes it.
fn folded(r: Reduce, results: &[u64], elem: Elem) -> Result<Option<String>, usize> {
    if elem == Elem::Int {
        #[allow(clippy::cast_possible_wrap)]
        let xs: Vec<i64> = results.iter().map(|b| *b as i64).collect();
        r.fold(&xs).map(|v| v.map(|n| n.to_string()))
    } else {
        let xs: Vec<f64> = results.iter().map(|b| as_f64(*b, elem)).collect();
        Ok(r.fold_float(&xs, elem == Elem::F32).map(|v| {
            let n = if elem == Elem::F32 {
                #[allow(clippy::cast_possible_truncation)]
                nazm_core::sema::numeric::NumVal::f32(v as f32)
            } else {
                nazm_core::sema::numeric::NumVal::f64(v)
            };
            nazm_core::sema::numeric::to_str(n)
        }))
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(crate) fn run(
    file: &Path,
    kernel: &str,
    inputs: &[std::path::PathBuf],
    reduce: Option<Reduce>,
    check: bool,
    print: bool,
    choice: Choice,
    scalar: bool,
) -> ExitCode {
    let loaded = match crate::opened::opened(file, None, false) {
        Ok(l) => l,
        Err(code) => return ExitCode::from(code),
    };
    let mut texts: Vec<String> = Vec::with_capacity(inputs.len());
    for input in inputs {
        match std::fs::read_to_string(input) {
            Ok(text) => texts.push(text),
            Err(e) => {
                eprintln!("nazm: cannot read `{}`: {e}", input.display());
                return ExitCode::from(exit::NO_SUCH_FILE);
            }
        }
    }
    let root = loaded.graph.module_of(nazm_span::FileId::ROOT);
    // The kernel, generated and checked eligible; the inputs, read as its type; and the
    // interpreter's answer for every element — the reference the device's results are held to.
    let prepared = nazm_core::on_lowered(
        &loaded.map,
        &loaded.graph,
        nazm_core::Entry::Absent,
        |core, _| {
            let k = nazm_service::kernel::kernel(core, root, kernel).map_err(|e| {
                vec![refuse(
                    e.at,
                    format!("`{}` cannot run on an accelerator: {}", e.function, e.why),
                )]
            })?;
            if print {
                return Ok(Ok((k, Vec::new(), None)));
            }
            let mut columns: Vec<Vec<u64>> = Vec::with_capacity(texts.len());
            for (text, input) in texts.iter().zip(inputs) {
                let parsed: Option<Vec<u64>> =
                    text.split_whitespace().map(|t| parse(t, k.elem)).collect();
                match parsed {
                    Some(v) => columns.push(v),
                    None => {
                        return Ok(Err(format!(
                            "`{}` is not whitespace-separated `{}`s",
                            input.display(),
                            k.elem.name()
                        )))
                    }
                }
            }
            let elements = columns.first().map_or(0, Vec::len);
            if elements > MAX_ELEMENTS {
                return Ok(Err(format!(
                    "`nazm accel` maps at most {MAX_ELEMENTS} elements in one launch"
                )));
            }
            // One input per parameter, every one as long as the first (N88), before any device.
            let at = core
                .functions
                .iter()
                .find(|f| f.name == kernel && Some(f.module) == root)
                .map_or(Span::synthetic(), |f| f.name_span);
            if columns.len() != k.arity as usize {
                let t = k.elem.name();
                return Err(vec![refuse(
                    at,
                    format!(
                        "`{kernel}` takes {} `{t}`{}, so it runs over {} input{}, and {} {} given",
                        k.arity,
                        if k.arity == 1 { "" } else { "s" },
                        k.arity,
                        if k.arity == 1 { "" } else { "s" },
                        columns.len(),
                        if columns.len() == 1 { "was" } else { "were" }
                    ),
                )]);
            }
            if columns.iter().any(|c| c.len() != elements) {
                return Err(vec![refuse(
                    at,
                    format!(
                        "a zip runs over inputs of one length, and these have {}",
                        columns
                            .iter()
                            .map(|c| c.len().to_string())
                            .collect::<Vec<_>>()
                            .join(" and ")
                    ),
                )]);
            }
            let reference = if check {
                let id = core
                    .functions
                    .iter()
                    .find(|f| f.name == kernel && Some(f.module) == root)
                    .map(|f| f.def);
                let mut interpreter = nazm_core::Interpreter::new(core.clone());
                let mut out = Vec::with_capacity(elements);
                let mut failed = None;
                if let Some(id) = id {
                    for i in 0..elements {
                        interpreter.reset_limits(nazm_core::eval::Limits::default());
                        let args = columns
                            .iter()
                            .map(|c| value(c.get(i).copied().unwrap_or(0), k.elem))
                            .collect();
                        match interpreter.call(id, args) {
                            #[allow(clippy::cast_sign_loss)]
                            Ok(nazm_core::Value::Int(v)) => out.push(v as u64),
                            // An element is a float's bits, at most 64 of them.
                            #[allow(clippy::cast_possible_truncation)]
                            Ok(nazm_core::Value::Num(n)) => out.push(n.bits as u64),
                            Ok(_) => out.push(0),
                            Err(e) => {
                                failed = Some((i, e.diagnostic.code));
                                break;
                            }
                        }
                    }
                }
                Some((out, failed))
            } else {
                None
            };
            Ok(Ok((k, columns, reference)))
        },
    );
    let (k, columns, reference) = match prepared {
        Ok(Ok(p)) => p,
        Ok(Err(why)) => {
            eprintln!("nazm: {why}");
            return ExitCode::from(exit::CHECK_FAILED);
        }
        Err(d) => {
            crate::emit(&d, &loaded.map, false);
            return ExitCode::from(exit::CHECK_FAILED);
        }
    };
    if print {
        print!("{}", k.source);
        return ExitCode::SUCCESS;
    }
    let elements = columns.first().map_or(0, Vec::len);
    // Exclusive and private (Q1-A1-02): a host program is compiled here and run. Only a directory
    // this process made is ever removed.
    let dir = match work_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("nazm: cannot make a working directory: {e}");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    };
    let cleanup = || {
        let _ = std::fs::remove_dir_all(&dir);
    };
    let bytes: Vec<Vec<u8>> = columns.iter().map(|c| to_bytes(c, k.elem)).collect();
    let mut written = std::fs::write(dir.join("kernel.cl"), &k.source);
    for (i, b) in bytes.iter().enumerate() {
        written = written.and_then(|()| std::fs::write(dir.join(format!("input{i}.bin")), b));
    }
    if let Err(e) = written {
        cleanup();
        eprintln!("nazm: cannot write {}: {e}", dir.display());
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }
    // The provider: the one asked for, or under `auto` the GPU where it can run the kernel and
    // the CPU otherwise — and the report says which, and why.
    let cpu = Cpu { scalar };
    let (provider, ran, reason): (&dyn Provider, Result<Ran, NotRun>, String) = match choice {
        Choice::OpenCl => (
            &OpenCl,
            OpenCl.run(&k, &bytes, &dir),
            "asked for".to_owned(),
        ),
        Choice::Cpu => (&cpu, cpu.run(&k, &bytes, &dir), "asked for".to_owned()),
        Choice::Auto => match OpenCl.run(&k, &bytes, &dir) {
            Err(NotRun::Unavailable(why)) => {
                eprintln!("nazm: {why}; the CPU provider runs the kernel");
                (&cpu, cpu.run(&k, &bytes, &dir), format!("auto: {why}"))
            }
            other => (&OpenCl, other, "auto: the GPU can run it".to_owned()),
        },
    };
    let ran = match ran {
        Ok(r) => r,
        Err(NotRun::Unavailable(why) | NotRun::Failed(why)) => {
            cleanup();
            eprintln!("nazm: {why}");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    };
    // The second oracle (Gate 3): where the GPU ran it, the CPU provider runs it too.
    let second = (provider.name() == "opencl" && check).then(|| cpu.run(&k, &bytes, &dir));
    cleanup();
    let report = &ran.report;
    let results = &ran.results;
    let s = |key: &str| {
        report
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let n = |key: &str| report.get(key).cloned().unwrap_or(Value::Null);
    let identity =
        nazm_cache::KernelKey::of(&k.source, &s("device"), &s("driver"), &provider.host())
            .to_string();
    // The lowest failing index and its site, if any work item failed.
    let fail_word = |r: &Value| {
        r.get("fail")
            .and_then(Value::as_i64)
            .unwrap_or(i64::from(i32::MAX))
    };
    let fail = fail_word(report);
    let failure = (fail != i64::from(i32::MAX)).then(|| {
        let index = usize::try_from(fail >> 8).unwrap_or(0);
        let site = k
            .sites
            .get(usize::try_from(fail & 0xff).unwrap_or(0))
            .cloned();
        (index, site)
    });
    // The fold, on the host, in index order (N88): of the device's results, and of the
    // interpreter's, each held to the other.
    let started = std::time::Instant::now();
    let reduced = match (reduce, &failure) {
        (Some(r), None) => Some(folded(r, results, k.elem)),
        _ => None,
    };
    let reduce_ms = started.elapsed().as_secs_f64() * 1e3;
    let agrees = reference
        .as_ref()
        .map(|(want, failed)| match (failed, &failure) {
            (None, None) => {
                same(want, results, k.elem) && reduce.map(|r| folded(r, want, k.elem)) == reduced
            }
            (Some((wi, wcode)), Some((gi, Some(site)))) => {
                *wi == *gi && wcode.as_str() == site.code
            }
            _ => false,
        });
    let cross = second.map(|r| match r {
        Ok(c) => {
            if same(&c.results, results, k.elem) && fail_word(&c.report) == fail {
                "agrees with the CPU provider".to_owned()
            } else {
                "DIFFERS from the CPU provider".to_owned()
            }
        }
        Err(NotRun::Unavailable(why) | NotRun::Failed(why)) => {
            format!("not run: {}", why.lines().last().unwrap_or_default())
        }
    });
    // The wrapping sum of the results' bits: an `Int` kernel's signed, as it has been since N67.
    let sum = results.iter().fold(0u64, |a, x| a.wrapping_add(*x));
    #[allow(clippy::cast_possible_wrap)]
    let checksum = if k.elem == Elem::Int {
        json!(sum as i64)
    } else {
        json!(sum)
    };
    let opencl = provider.name() == "opencl";
    let reduce_where = if opencl {
        "host, in index order, after the transfer"
    } else {
        "host, in index order, after the pool joined"
    };
    let doc = json!({
        "schema": "nazm.accel/3",
        "kernel": {
            "function": kernel,
            "functions": k.functions,
            "identity": identity,
            "shape": if k.arity == 2 { "zip" } else { "map" },
            "element": k.elem.name(),
        },
        "provider": {
            "name": provider.name(),
            "reason": reason,
            "vectorised": if opencl { Value::Null } else { json!(!scalar) },
            "threads": if opencl { Value::Null } else { n("threads") },
        },
        "device": { "api": provider.api(), "name": s("device"), "driver": s("driver"), "version": s("version") },
        "elements": elements,
        "inputs": columns.iter().map(|c| json!({ "elements": c.len(), "bytes": c.len() * k.elem.bytes() })).collect::<Vec<_>>(),
        "transfers": if opencl {
            json!([
                { "direction": "host-to-device", "bytes": n("to_device_bytes"), "ms": n("to_device_ms") },
                { "direction": "device-to-host", "bytes": n("from_device_bytes"), "ms": n("from_device_ms") },
            ])
        } else {
            json!([])
        },
        "reduce": match (reduce, &reduced) {
            (None, _) => Value::Null,
            (Some(r), Some(Ok(v))) => json!({ "op": r.name(), "where": reduce_where, "ms": reduce_ms, "value": if k.elem == Elem::Int { v.as_ref().and_then(|t| t.parse::<i64>().ok()).map_or(Value::Null, |i| json!(i)) } else { v.as_ref().map_or(Value::Null, |t| json!(t)) } }),
            (Some(r), Some(Err(i))) => json!({ "op": r.name(), "where": reduce_where, "ms": reduce_ms, "failure": "N0400", "index": i }),
            (Some(r), None) => json!({ "op": r.name(), "where": reduce_where, "skipped": "the map failed" }),
        },
        "synchronisation": if opencl { "one clFinish after the launch, before any read" } else { "every thread of the pool joined before any read" },
        "timings_ms": { "build": if opencl { n("build_ms") } else { json!(ran.build_ms) }, "run": n("run_ms") },
        "result": match &failure {
            None => json!({ "checksum": checksum }),
            Some((index, site)) => json!({
                "failure": site.as_ref().map(|s| s.code),
                "index": index,
            }),
        },
        "reference": match agrees {
            None => json!("not checked"),
            Some(true) => json!("agrees with the interpreter"),
            Some(false) => json!("DIFFERS from the interpreter"),
        },
        "cross_check": cross.map_or(Value::Null, |c| json!(c)),
        "fallback": "stated, never silent: `auto` runs on the CPU provider where the GPU cannot run the kernel, and says why in `provider.reason`",
    });
    println!("{doc}");
    if agrees == Some(false) {
        eprintln!("nazm: the accelerator's results differ from the interpreter's");
        return ExitCode::from(exit::CHECK_FAILED);
    }
    if doc.get("cross_check").and_then(Value::as_str) == Some("DIFFERS from the CPU provider") {
        eprintln!("nazm: the GPU's results differ from the CPU provider's");
        return ExitCode::from(exit::CHECK_FAILED);
    }
    if let Some(Err(index)) = reduced {
        eprintln!("nazm: error[N0400]: this addition overflows `Int` — the running sum, at element {index}");
        return ExitCode::from(exit::RUNTIME_FAILED);
    }
    if let Some((index, Some(site))) = failure {
        let d = Diagnostic::error(
            nazm_diag::Code::new(site.code),
            site.at,
            format!("{} at element {index}", site.message),
        );
        crate::emit(&[d], &loaded.map, false);
        return ExitCode::from(exit::RUNTIME_FAILED);
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::{same, Reduce};
    use nazm_service::kernel::Elem;

    #[test]
    fn a_fold_is_in_index_order_and_add_checks_the_running_sum() {
        assert_eq!(Reduce::Add.fold(&[1, 2, 3]), Ok(Some(6)));
        // The total fits; the running sum after the second element does not (N88, as `ints_sum`).
        assert_eq!(Reduce::Add.fold(&[i64::MAX, 1, -1]), Err(1));
        assert_eq!(Reduce::Min.fold(&[3, -2, 9]), Ok(Some(-2)));
        assert_eq!(Reduce::Max.fold(&[3, -2, 9]), Ok(Some(9)));
        assert_eq!(Reduce::Max.fold(&[]), Ok(None));
        assert_eq!(Reduce::Add.fold(&[]), Ok(Some(0)));
        assert_eq!(Reduce::by_name("mul"), None);
    }

    #[test]
    fn a_float_fold_is_in_index_order_and_a_nan_loses() {
        // In index order: 1 + 1e16 + -1e16 is 0 in `Float64`; from the other end it is 1.
        assert_eq!(
            Reduce::Add.fold_float(&[1.0, 1e16, -1e16], false),
            Some(0.0)
        );
        assert_eq!(
            Reduce::Min.fold_float(&[f64::NAN, 2.0, -1.0], false),
            Some(-1.0)
        );
        assert_eq!(Reduce::Max.fold_float(&[2.0, f64::NAN], false), Some(2.0));
        assert_eq!(Reduce::Max.fold_float(&[], false), None);
        // A `Float32` sum rounds to `Float32` at each step: 2^24 + 1 is 2^24.
        assert_eq!(
            Reduce::Add.fold_float(&[16_777_216.0, 1.0], true),
            Some(16_777_216.0)
        );
    }

    #[test]
    fn results_are_the_same_bit_for_bit_and_every_nan_alike() {
        let nan_a = f64::NAN.to_bits();
        let nan_b = nan_a | 1;
        assert!(same(&[nan_a], &[nan_b], Elem::F64));
        assert!(!same(
            &[0.0f64.to_bits()],
            &[(-0.0f64).to_bits()],
            Elem::F64
        ));
        assert!(!same(&[1], &[2], Elem::Int));
    }
}
