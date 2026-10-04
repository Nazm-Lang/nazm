//! N64: WebAssembly — `wasm32-unknown-unknown`, its outside reached only by imports.
//!
//! `docs/architecture.md` §7.66. What a whole build shows: a program builds to WebAssembly objects,
//! `link.txt` with its `wasm-ld` line, the reference host `host.mjs` and `bounds.json`, the same bytes
//! twice; device registers, allocation, Cranelift and the board's watermark are refused for it by
//! name; `--debug` carries DWARF. The module's import section — what it can ask of its host — is
//! read from the linked module in the ignored tests, which link and run in `nazm-wasm:n64`
//! (`docker/wasm.Dockerfile`): absent authority is an absent import, and a corpus of the
//! repository's own programs runs as WebAssembly with the interpreter's output and status.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const WASM: &str = "wasm32-unknown-unknown";

const HELLO: &str = "fn fib(n: Int) -> Int { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }\n\
                     fn main(io: IoCap) -> Int ! { io } { print(\"hello from wasm\\n\"); fib(20) }\n";

const PURE: &str = "fn main() -> Int { let mut i = 0; let mut s = 0; \
                    while i < 100 { s = s + i * i; i = i + 1; } s }\n";

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn program(src: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    d
}

fn objects(d: &Path, extra: &[&str]) -> Output {
    let mut args = vec!["build", "--target", WASM, "--objects", "out"];
    args.extend_from_slice(extra);
    args.push("prog.nz");
    nazm(d, &args)
}

/// The `(module, name)` of every import a WebAssembly binary declares, in order.
fn imports(bytes: &[u8]) -> Vec<(String, String)> {
    assert_eq!(&bytes[..4], b"\0asm", "not WebAssembly");
    let leb = |b: &[u8], at: &mut usize| -> usize {
        let (mut n, mut shift) = (0usize, 0);
        loop {
            let byte = b[*at];
            *at += 1;
            n |= usize::from(byte & 0x7f) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                return n;
            }
        }
    };
    let name = |b: &[u8], at: &mut usize| -> String {
        let n = leb(b, at);
        let s = String::from_utf8(b[*at..*at + n].to_vec()).unwrap();
        *at += n;
        s
    };
    let mut at = 8;
    let mut out = Vec::new();
    while at < bytes.len() {
        let id = bytes[at];
        at += 1;
        let size = leb(bytes, &mut at);
        let end = at + size;
        if id == 2 {
            let mut p = at;
            for _ in 0..leb(bytes, &mut p) {
                let module = name(bytes, &mut p);
                let field = name(bytes, &mut p);
                let kind = bytes[p];
                p += 1;
                match kind {
                    0 => {
                        leb(bytes, &mut p);
                    }
                    1 => {
                        p += 1;
                        let flags = leb(bytes, &mut p);
                        leb(bytes, &mut p);
                        if flags & 1 == 1 {
                            leb(bytes, &mut p);
                        }
                    }
                    2 => {
                        let flags = leb(bytes, &mut p);
                        leb(bytes, &mut p);
                        if flags & 1 == 1 {
                            leb(bytes, &mut p);
                        }
                    }
                    _ => p += 2,
                }
                out.push((module, field));
            }
        }
        at = end;
    }
    out
}

#[test]
fn a_program_builds_to_webassembly_objects_a_link_line_and_a_host() {
    let d = program(HELLO);
    let o = objects(d.path(), &[]);
    assert!(o.status.success(), "{}", err(&o));
    let out = d.path().join("out");
    for name in ["00-m0.o", "01-runtime.o", "02-entry.o"] {
        let b = std::fs::read(out.join(name)).unwrap();
        assert_eq!(&b[..4], b"\0asm", "{name} is not WebAssembly");
    }
    let list = std::fs::read_to_string(out.join("link.txt")).unwrap();
    assert!(list.contains("target wasm32-unknown-unknown"), "{list}");
    assert!(
        list.contains("wasm-ld --no-entry --stack-first -z stack-size=65536 --export=nazm_main"),
        "{list}"
    );
    assert!(
        !out.join("link.ld").exists(),
        "a module has no linker script"
    );
    let host = std::fs::read_to_string(out.join("host.mjs")).unwrap();
    assert!(host.contains("new Set(['write', 'report'])"), "{host}");
    assert!(host.contains("does not grant"), "{host}");
    // The runtime's object asks for the host's `write` and nothing of a C library; the entry's
    // asks for `report`.
    let rt = imports(&std::fs::read(out.join("01-runtime.o")).unwrap());
    assert!(rt.contains(&("nazm_host".into(), "write".into())), "{rt:?}");
    assert!(
        rt.iter()
            .all(|(m, f)| m == "nazm_host" || (m == "env" && f.starts_with("__"))),
        "{rt:?}"
    );
    let entry = imports(&std::fs::read(out.join("02-entry.o")).unwrap());
    assert!(
        entry.contains(&("nazm_host".into(), "report".into())),
        "{entry:?}"
    );
    let bounds: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("bounds.json")).unwrap()).unwrap();
    // `fib` recurses: no stack bound, and why.
    assert_eq!(bounds["stack"]["kind"], "unknown", "{bounds}");
}

#[test]
fn a_module_s_bound_is_rooted_at_its_export() {
    let d = program(PURE);
    assert!(objects(d.path(), &[]).status.success());
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("out/bounds.json")).unwrap())
            .unwrap();
    assert_eq!(v["target"], WASM);
    assert_eq!(v["stack"]["kind"], "static bound", "{v}");
    assert_eq!(v["stack"]["path"][0], "nazm_main", "{v}");
    assert_eq!(v["stack"]["reserved"], 65536);
}

#[test]
fn the_same_program_gives_the_same_objects() {
    let d = program(HELLO);
    assert!(objects(d.path(), &[]).status.success());
    let first: Vec<Vec<u8>> = ["00-m0.o", "01-runtime.o", "02-entry.o"]
        .iter()
        .map(|n| std::fs::read(d.path().join("out").join(n)).unwrap())
        .collect();
    std::fs::remove_dir_all(d.path().join("out")).unwrap();
    assert!(objects(d.path(), &["--no-cache"]).status.success());
    for (i, n) in ["00-m0.o", "01-runtime.o", "02-entry.o"].iter().enumerate() {
        assert_eq!(
            std::fs::read(d.path().join("out").join(n)).unwrap(),
            first[i],
            "{n}"
        );
    }
}

#[test]
fn debug_information_reaches_the_objects() {
    let d = program(PURE);
    let o = objects(d.path(), &["--debug"]);
    assert!(o.status.success(), "{}", err(&o));
    let unit = std::fs::read(d.path().join("out/00-m0.o")).unwrap();
    let has = |s: &[u8]| unit.windows(s.len()).any(|w| w == s);
    assert!(has(b".debug_line"), "no line table");
    assert!(has(b".debug_info"), "no debug information");
    assert!(has(b"prog.nz"), "the source is not named");
}

#[test]
fn what_a_module_cannot_have_is_refused_by_name() {
    // No device registers: a module's only outside is its imports.
    let d = program("fn main(m: MmioCap) -> Int ! {} { mmio_read32(0) }\n");
    let o = objects(d.path(), &[]);
    assert!(!o.status.success());
    assert!(err(&o).contains("N0392"), "{}", err(&o));
    assert!(
        err(&o).contains("a build for `wasm32-unknown-unknown`"),
        "{}",
        err(&o)
    );
    // No heap.
    let d = program("fn main() -> Int { str_len(int_to_str(42)) }\n");
    let o = objects(d.path(), &[]);
    assert_eq!(o.status.code(), Some(4), "{}", err(&o));
    assert!(
        err(&o).contains("`wasm32-unknown-unknown` has no operating system"),
        "{}",
        err(&o)
    );
    assert!(err(&o).contains("strings ("), "{}", err(&o));
    // Not Cranelift, and not the board's watermark.
    let d = program(PURE);
    let o = nazm(
        d.path(),
        &[
            "build",
            "--backend",
            "cranelift",
            "--target",
            WASM,
            "--objects",
            "out",
            "prog.nz",
        ],
    );
    assert_eq!(o.status.code(), Some(4));
    let o = objects(d.path(), &["--stack-watermark"]);
    assert_eq!(o.status.code(), Some(4));
    assert!(
        err(&o).contains("`--stack-watermark` measures a board's stack"),
        "{}",
        err(&o)
    );
    // Nor linked here: the host has no `wasm-ld`.
    let o = nazm(d.path(), &["build", "--target", WASM, "prog.nz"]);
    assert_eq!(o.status.code(), Some(4));
    assert!(err(&o).contains("--objects DIR"), "{}", err(&o));
}

/// Link every directory of objects under `root` in `nazm-wasm:n64` and run each with its host:
/// `DIR/stdout`, `DIR/stderr`, `DIR/status` and `DIR/imports` (the linked module's import list).
fn link_and_run(root: &Path) {
    let script = "for d in /w/*/; do cd \"$d\" && \
                  wasm-ld --no-entry --stack-first -z stack-size=65536 --export=nazm_main \
                  -o module.wasm 00-m0.o 01-runtime.o 02-entry.o 2>link.err; \
                  node host.mjs module.wasm >stdout 2>stderr; echo $? >status; done";
    let o = Command::new("docker")
        .args(["run", "--rm", "--network", "none", "-v"])
        .arg(format!("{}:/w", root.display()))
        .args(["nazm-wasm:n64", "sh", "-c", script])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", err(&o));
}

/// Needs Docker and `nazm-wasm:n64`: `cargo test -p nazm-cli --test wasm -- --ignored`.
#[test]
#[ignore = "starts a container"]
fn absent_authority_is_an_absent_import() {
    let root = tempfile::tempdir().unwrap();
    for (name, src) in [("io", HELLO), ("pure", PURE)] {
        let d = root.path().join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("prog.nz"), src).unwrap();
        let o = objects(&d, &[]);
        assert!(o.status.success(), "{name}: {}", err(&o));
        for f in std::fs::read_dir(d.join("out")).unwrap().flatten() {
            std::fs::rename(f.path(), d.join(f.file_name())).unwrap();
        }
    }
    link_and_run(root.path());
    let module =
        |n: &str| imports(&std::fs::read(root.path().join(n).join("module.wasm")).unwrap());
    let host = |m: &str, f: &str| (m.to_owned(), f.to_owned());
    assert_eq!(
        module("io"),
        [host("nazm_host", "write"), host("nazm_host", "report")]
    );
    // No `IoCap`, no `write`: the module cannot ask for output at all.
    assert_eq!(module("pure"), [host("nazm_host", "report")]);
    let out = |n: &str, f: &str| std::fs::read_to_string(root.path().join(n).join(f)).unwrap();
    assert_eq!(out("io", "stdout"), "hello from wasm\n6765\n");
    assert_eq!(out("pure", "stdout"), "328350\n");
    assert_eq!(out("pure", "status").trim(), "0");
}

/// Every program in the repository's corpora, built for WebAssembly: each one either is refused by
/// part — counted, never stubbed — or runs under the reference host with the interpreter's
/// standard output and status. Needs Docker and `nazm-wasm:n64`.
#[test]
#[ignore = "starts a container"]
fn the_corpus_runs_as_webassembly_as_the_interpreter_runs_it() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut sources: Vec<PathBuf> = Vec::new();
    for dir in [
        "crates/nazm-cli/tests/transfers",
        "compiler/conformance",
        "bench/programs",
        "examples",
    ] {
        for e in std::fs::read_dir(repo.join(dir)).unwrap().flatten() {
            if e.path().extension().is_some_and(|x| x == "nz") {
                sources.push(e.path());
            }
        }
    }
    sources.sort();
    let root = tempfile::tempdir().unwrap();
    let mut built = Vec::new();
    let mut refused = 0;
    for (i, src) in sources.iter().enumerate() {
        let d = root.path().join(format!("p{i:03}"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::copy(src, d.join("prog.nz")).unwrap();
        let o = objects(&d, &[]);
        if o.status.code() == Some(4) && err(&o).contains("has no operating system") {
            refused += 1;
            std::fs::remove_dir_all(&d).unwrap();
            continue;
        }
        if !o.status.success() {
            // Not a program `nazm build` takes on any target (a refusal case, a library).
            std::fs::remove_dir_all(&d).unwrap();
            continue;
        }
        for f in std::fs::read_dir(d.join("out")).unwrap().flatten() {
            std::fs::rename(f.path(), d.join(f.file_name())).unwrap();
        }
        built.push((d, src.clone()));
    }
    link_and_run(root.path());
    let mut agreed = 0;
    for (d, src) in &built {
        let run = Command::new(NAZM)
            .arg("run")
            .arg(d.join("prog.nz"))
            .output()
            .unwrap();
        let stdout = std::fs::read_to_string(d.join("stdout")).unwrap();
        let status: i32 = std::fs::read_to_string(d.join("status"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(
            (
                String::from_utf8_lossy(&run.stdout).into_owned(),
                run.status.code()
            ),
            (stdout, Some(status)),
            "{}: link {}",
            src.display(),
            std::fs::read_to_string(d.join("link.err")).unwrap_or_default()
        );
        agreed += 1;
    }
    eprintln!(
        "{agreed} programs agree with the interpreter as WebAssembly; {refused} refused by part; {} sources",
        sources.len()
    );
    assert!(agreed >= 10, "only {agreed} programs ran");
}
