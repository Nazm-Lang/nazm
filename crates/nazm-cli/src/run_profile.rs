//! `nazm profile FILE` (N58, `docs/architecture.md` §7.60): build the program, run it once with
//! its memory and scheduler counters on, and print `nazm.profile/1` — counts of events, never
//! samples, and nothing the compiler measured about itself (that is `nazm.timings/1`).

use std::path::Path;
use std::process::{Command, ExitCode};
use std::time::Instant;

use serde_json::{json, Map, Value};

use crate::exit;

/// `key=value` pairs after `prefix` on the first line of `text` that starts with it.
fn counts(text: &str, prefix: &str) -> Vec<(String, u64)> {
    let Some(line) = text.lines().find(|l| l.starts_with(prefix)) else {
        return Vec::new();
    };
    line.split_whitespace()
        .filter_map(|w| {
            let (k, v) = w.split_once('=')?;
            Some((k.to_owned(), v.parse().ok()?))
        })
        .collect()
}

pub(crate) fn run(file: &Path, backend: &str) -> ExitCode {
    // A directory of this process's own, removed on every path out.
    let work = std::env::temp_dir().join(format!("nazm-profile-{}", std::process::id()));
    if std::fs::create_dir_all(&work).is_err() {
        eprintln!("nazm: cannot make a working directory");
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    }
    let code = profile(file, backend, &work);
    let _ = std::fs::remove_dir_all(&work);
    code
}

fn profile(file: &Path, backend: &str, work: &Path) -> ExitCode {
    let exe = work.join("profiled");
    let Ok(me) = std::env::current_exe() else {
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    };
    let built = Command::new(&me)
        .args(["build", "--backend", backend])
        .arg(file)
        .arg("-o")
        .arg(&exe)
        .output();
    match built {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            eprint!("{}", String::from_utf8_lossy(&o.stderr));
            return ExitCode::from(
                o.status
                    .code()
                    .and_then(|c| u8::try_from(c).ok())
                    .unwrap_or(1),
            );
        }
        Err(e) => {
            eprintln!("nazm: cannot build: {e}");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    }
    let start = Instant::now();
    let ran = Command::new(&exe)
        .env(nazm_lir::MEMORY_VARIABLE, "1")
        .env(nazm_lir::runtime::SCHED_VARIABLE, "1")
        .output();
    let wall = start.elapsed();
    let Ok(o) = ran else {
        eprintln!("nazm: cannot run the program");
        return ExitCode::from(exit::TOOLCHAIN_FAILED);
    };
    let err = String::from_utf8_lossy(&o.stderr);
    // The memory line is `kind allocated=… reclaimed=… live=…` four times, in a fixed order.
    let mut memory = Map::new();
    let pairs = counts(&err, "nazm-memory:");
    for (kind, chunk) in ["sequences", "strings", "channels", "closures"]
        .iter()
        .zip(pairs.chunks(3))
    {
        let mut m = Map::new();
        for (k, v) in chunk {
            m.insert(k.clone(), json!(v));
        }
        memory.insert((*kind).to_owned(), Value::Object(m));
    }
    let scheduler: Map<String, Value> = counts(&err, "nazm-sched:")
        .into_iter()
        .map(|(k, v)| (k, json!(v)))
        .collect();
    let doc = json!({
        "schema": "nazm.profile/1",
        "backend": backend,
        "exit": o.status.code(),
        "wall_ms": u64::try_from(wall.as_millis()).unwrap_or(u64::MAX),
        "stdout_bytes": o.stdout.len(),
        "memory": memory,
        "scheduler": scheduler,
    });
    println!("{doc}");
    ExitCode::SUCCESS
}
