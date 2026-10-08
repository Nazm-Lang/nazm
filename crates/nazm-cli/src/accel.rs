//! `nazm accel FILE KERNEL --input FILE [--input FILE] [--reduce OP]` (N67, §7.69; N88, §7.89):
//! map an eligible Nazm function over one sequence of `Int`s, or zip it over two, on the GPU
//! through OpenCL, optionally fold the results in index order, and say exactly what moved and
//! when — then hold the result to the interpreter's.
//!
//! The device code is OpenCL C generated from the function's Core IR (`nazm_service::kernel`). The
//! host side is a small C program compiled against the system's OpenCL, run as a process: the
//! compiler links no GPU API itself, and holds no `unsafe` code to call one. Every transfer and the
//! one synchronisation are explicit in that program and timed in the report.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use nazm_diag::{codes, Diagnostic};
use nazm_span::Span;
use serde_json::{json, Value};

use crate::exit;

/// The host program, compiled once per run against the system's OpenCL.
const HOST: &str = include_str!("accel_host.c");

/// At most this many elements: a failure is recorded as `index · 256 + site` in one 32-bit word.
const MAX_ELEMENTS: usize = 8_000_000;

fn refuse(at: Span, message: String) -> Diagnostic {
    Diagnostic::error(codes::KERNEL_INELIGIBLE, at, message)
        .primary_label("not a kernel")
        .help("a kernel is a pure `(Int) -> Int` or `(Int, Int) -> Int` function of `Int`s and `Bool`s, calling only others like it, without recursion")
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
}

fn work_dir() -> std::io::Result<PathBuf> {
    crate::scratch::temporary("nazm-accel-")
}

pub(crate) fn run(
    file: &Path,
    kernel: &str,
    inputs: &[std::path::PathBuf],
    reduce: Option<Reduce>,
    check: bool,
    print: bool,
) -> ExitCode {
    let loaded = match crate::opened::opened(file, None, false) {
        Ok(l) => l,
        Err(code) => return ExitCode::from(code),
    };
    let mut columns: Vec<Vec<i64>> = Vec::with_capacity(inputs.len());
    for input in inputs {
        match std::fs::read_to_string(input) {
            Ok(text) => {
                let parsed: Result<Vec<i64>, _> = text.split_whitespace().map(str::parse).collect();
                match parsed {
                    Ok(v) => columns.push(v),
                    Err(e) => {
                        eprintln!(
                            "nazm: `{}` is not whitespace-separated `Int`s: {e}",
                            input.display()
                        );
                        return ExitCode::from(exit::CHECK_FAILED);
                    }
                }
            }
            Err(e) => {
                eprintln!("nazm: cannot read `{}`: {e}", input.display());
                return ExitCode::from(exit::NO_SUCH_FILE);
            }
        }
    }
    let elements: Vec<i64> = columns.first().cloned().unwrap_or_default();
    if elements.len() > MAX_ELEMENTS {
        eprintln!("nazm: `nazm accel` maps at most {MAX_ELEMENTS} elements in one launch");
        return ExitCode::from(exit::CHECK_FAILED);
    }
    let root = loaded.graph.module_of(nazm_span::FileId::ROOT);
    // The kernel, generated and checked eligible, and the interpreter's answer for every
    // element — the reference the device's results are held to.
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
            // One input per parameter, every one as long as the first (N88), before any device.
            let at = core
                .functions
                .iter()
                .find(|f| f.name == kernel && Some(f.module) == root)
                .map_or(Span::synthetic(), |f| f.name_span);
            if !print && columns.len() != k.arity as usize {
                return Err(vec![refuse(
                    at,
                    format!(
                        "`{kernel}` takes {} `Int`{}, so it runs over {} input{}, and {} {} given",
                        k.arity,
                        if k.arity == 1 { "" } else { "s" },
                        k.arity,
                        if k.arity == 1 { "" } else { "s" },
                        columns.len(),
                        if columns.len() == 1 { "was" } else { "were" }
                    ),
                )]);
            }
            if columns.iter().any(|c| c.len() != elements.len()) {
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
            let reference = if check && !print {
                let id = core
                    .functions
                    .iter()
                    .find(|f| f.name == kernel && Some(f.module) == root)
                    .map(|f| f.def);
                let mut interpreter = nazm_core::Interpreter::new(core.clone());
                let mut out = Vec::with_capacity(elements.len());
                let mut failed = None;
                if let Some(id) = id {
                    for i in 0..elements.len() {
                        interpreter.reset_limits(nazm_core::eval::Limits::default());
                        let args = columns
                            .iter()
                            .map(|c| nazm_core::Value::Int(c.get(i).copied().unwrap_or(0)))
                            .collect();
                        match interpreter.call(id, args) {
                            Ok(nazm_core::Value::Int(v)) => out.push(v),
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
            Ok((k, reference))
        },
    );
    let (k, reference) = match prepared {
        Ok(p) => p,
        Err(d) => {
            crate::emit(&d, &loaded.map, false);
            return ExitCode::from(exit::CHECK_FAILED);
        }
    };
    if print {
        print!("{}", k.source);
        return ExitCode::SUCCESS;
    }
    if !cfg!(target_os = "macos") {
        eprintln!(
            "nazm: `nazm accel` runs through the system OpenCL of macOS, and this host has none"
        );
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }
    // Exclusive and private (Q1-A1-02): `host` is compiled here and run. Only a directory this
    // process made is ever removed.
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
    let bytes = |c: &[i64]| -> Vec<u8> { c.iter().flat_map(|x| x.to_le_bytes()).collect() };
    let mut written = std::fs::write(dir.join("kernel.cl"), &k.source)
        .and_then(|()| std::fs::write(dir.join("host.c"), HOST));
    for (i, c) in columns.iter().enumerate() {
        written =
            written.and_then(|()| std::fs::write(dir.join(format!("input{i}.bin")), bytes(c)));
    }
    if let Err(e) = written {
        cleanup();
        eprintln!("nazm: cannot write {}: {e}", dir.display());
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }
    let built = Command::new(crate::backend::DRIVER)
        .args(["-O2", "-framework", "OpenCL", "-o"])
        .arg(dir.join("host"))
        .arg(dir.join("host.c"))
        .output();
    match built {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            cleanup();
            eprint!("{}", String::from_utf8_lossy(&o.stderr));
            eprintln!("nazm: the OpenCL host program did not build");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
        Err(e) => {
            cleanup();
            eprintln!("nazm: `{}` could not be run: {e}", crate::backend::DRIVER);
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    }
    let mut host = Command::new(dir.join("host"));
    host.arg(dir.join("kernel.cl")).arg(dir.join("output.bin"));
    for i in 0..columns.len() {
        host.arg(dir.join(format!("input{i}.bin")));
    }
    let ran = host.output();
    let (report, results) = match ran {
        Ok(o) if o.status.success() => {
            let r: Value = serde_json::from_slice(&o.stdout).unwrap_or(Value::Null);
            let out = std::fs::read(dir.join("output.bin")).unwrap_or_default();
            let results: Vec<i64> = out
                .chunks_exact(8)
                .map(|c| i64::from_le_bytes(c.try_into().unwrap_or([0; 8])))
                .collect();
            (r, results)
        }
        Ok(o) => {
            cleanup();
            eprint!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            eprintln!("nazm: the accelerator could not run the kernel");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
        Err(e) => {
            cleanup();
            eprintln!("nazm: the OpenCL host program could not be run: {e}");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    };
    cleanup();
    let s = |k: &str| {
        report
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let n = |k: &str| report.get(k).cloned().unwrap_or(Value::Null);
    let identity =
        nazm_cache::KernelKey::of(&k.source, &s("device"), &s("driver"), HOST).to_string();
    // The lowest failing index and its site, if any work item failed.
    let fail = report
        .get("fail")
        .and_then(Value::as_i64)
        .unwrap_or(i64::from(i32::MAX));
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
        (Some(r), None) => Some(r.fold(&results)),
        _ => None,
    };
    let reduce_ms = started.elapsed().as_secs_f64() * 1e3;
    let agrees = reference
        .as_ref()
        .map(|(want, failed)| match (failed, &failure) {
            (None, None) => *want == results && reduce.map(|r| r.fold(want)) == reduced,
            (Some((wi, wcode)), Some((gi, Some(site)))) => {
                *wi == *gi && wcode.as_str() == site.code
            }
            _ => false,
        });
    let checksum = results.iter().fold(0i64, |a, x| a.wrapping_add(*x));
    let doc = json!({
        "schema": "nazm.accel/2",
        "kernel": {
            "function": kernel,
            "functions": k.functions,
            "identity": identity,
            "shape": if k.arity == 2 { "zip" } else { "map" },
        },
        "device": { "api": "OpenCL", "name": s("device"), "driver": s("driver"), "version": s("version") },
        "elements": elements.len(),
        "inputs": columns.iter().map(|c| json!({ "elements": c.len(), "bytes": c.len() * 8 })).collect::<Vec<_>>(),
        "transfers": [
            { "direction": "host-to-device", "bytes": n("to_device_bytes"), "ms": n("to_device_ms") },
            { "direction": "device-to-host", "bytes": n("from_device_bytes"), "ms": n("from_device_ms") },
        ],
        "reduce": match (reduce, &reduced) {
            (None, _) => Value::Null,
            (Some(r), Some(Ok(v))) => json!({ "op": r.name(), "where": "host, in index order, after the transfer", "ms": reduce_ms, "value": v }),
            (Some(r), Some(Err(i))) => json!({ "op": r.name(), "where": "host, in index order, after the transfer", "ms": reduce_ms, "failure": "N0400", "index": i }),
            (Some(r), None) => json!({ "op": r.name(), "where": "host, in index order, after the transfer", "skipped": "the map failed" }),
        },
        "synchronisation": "one clFinish after the launch, before any read",
        "timings_ms": { "build": n("build_ms"), "run": n("run_ms") },
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
        "fallback": "none: a host without the device refuses, and nothing runs elsewhere silently",
    });
    println!("{doc}");
    if agrees == Some(false) {
        eprintln!("nazm: the accelerator's results differ from the interpreter's");
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
    use super::Reduce;

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
}
