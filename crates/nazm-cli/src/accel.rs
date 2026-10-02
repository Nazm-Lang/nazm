//! `nazm accel FILE KERNEL --input FILE` (N67, `docs/architecture.md` §7.69): map an eligible Nazm
//! function over a sequence of `Int`s on the GPU, through OpenCL, and say exactly what moved and
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
        .help("a kernel is a pure `(Int) -> Int` function of `Int`s and `Bool`s, calling only others like it, without recursion")
}

fn work_dir() -> PathBuf {
    std::env::temp_dir().join(format!("nazm-accel-{}", std::process::id()))
}

pub(crate) fn run(file: &Path, kernel: &str, input: &Path, check: bool, print: bool) -> ExitCode {
    let source_root = match crate::root::decide(file, None) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nazm: {e}");
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };
    let loaded = match crate::load::load(file, source_root.path()) {
        Ok(f) => f,
        Err((partial, diagnostics)) => {
            crate::emit(&diagnostics, &partial.map, false);
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };
    let elements: Vec<i64> = match std::fs::read_to_string(input) {
        Ok(text) => {
            let parsed: Result<Vec<i64>, _> = text.split_whitespace().map(str::parse).collect();
            match parsed {
                Ok(v) => v,
                Err(e) => {
                    eprintln!(
                        "nazm: `{}` is not whitespace-separated `Int`s: {e}",
                        input.display()
                    );
                    return ExitCode::from(exit::NO_SUCH_FILE);
                }
            }
        }
        Err(e) => {
            eprintln!("nazm: cannot read `{}`: {e}", input.display());
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };
    if elements.len() > MAX_ELEMENTS {
        eprintln!("nazm: `nazm accel` maps at most {MAX_ELEMENTS} elements in one launch");
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
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
                    for (i, x) in elements.iter().enumerate() {
                        interpreter.reset_limits(nazm_core::eval::Limits::default());
                        match interpreter.call(id, vec![nazm_core::Value::Int(*x)]) {
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
    let dir = work_dir();
    let cleanup = || {
        let _ = std::fs::remove_dir_all(&dir);
    };
    let bytes: Vec<u8> = elements.iter().flat_map(|x| x.to_le_bytes()).collect();
    let written = std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(dir.join("kernel.cl"), &k.source))
        .and_then(|()| std::fs::write(dir.join("input.bin"), &bytes))
        .and_then(|()| std::fs::write(dir.join("host.c"), HOST));
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
    let ran = Command::new(dir.join("host"))
        .arg(dir.join("kernel.cl"))
        .arg(dir.join("input.bin"))
        .arg(dir.join("output.bin"))
        .output();
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
    let agrees = reference
        .as_ref()
        .map(|(want, failed)| match (failed, &failure) {
            (None, None) => *want == results,
            (Some((wi, wcode)), Some((gi, Some(site)))) => {
                *wi == *gi && wcode.as_str() == site.code
            }
            _ => false,
        });
    let checksum = results.iter().fold(0i64, |a, x| a.wrapping_add(*x));
    let doc = json!({
        "schema": "nazm.accel/1",
        "kernel": { "function": kernel, "functions": k.functions, "identity": identity },
        "device": { "api": "OpenCL", "name": s("device"), "driver": s("driver"), "version": s("version") },
        "elements": elements.len(),
        "transfers": [
            { "direction": "host-to-device", "bytes": n("to_device_bytes"), "ms": n("to_device_ms") },
            { "direction": "device-to-host", "bytes": n("from_device_bytes"), "ms": n("from_device_ms") },
        ],
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
    if let Some((index, Some(site))) = failure {
        let d = Diagnostic::error(
            nazm_diag::Code::new(site.code),
            site.at,
            format!("{} at element {index}", site.message),
        );
        crate::emit(&[d], &loaded.map, false);
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}
