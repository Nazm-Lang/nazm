//! Where generated code goes in a macOS object, which is how the preemption signal's handler knows
//! it interrupted generated code and not the runtime (R1.5 6.3). Preemption runs only on macOS, but
//! the objects are written on any host: both backends are asked for `aarch64-apple-darwin` and the
//! output is read back, so a function left in the default text section is found on Linux.

use std::path::Path;
use std::process::{Command, Output};

const PROGRAM: &str = "fn spin(n: Int) -> Int { let mut x = 0; while x < n { x = x + 1; } x }\n\
                       fn main() -> Int { spin(10) }\n";

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nazm"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn build(backend: &str, extra: &[&str]) -> (tempfile::TempDir, Output) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), PROGRAM).unwrap();
    let mut args = vec![
        "build",
        "--no-cache",
        "--backend",
        backend,
        "--target",
        "aarch64-apple-darwin",
        "--objects",
        "o",
    ];
    args.extend_from_slice(extra);
    args.push("p.nz");
    let out = nazm(d.path(), &args);
    (d, out)
}

#[test]
fn the_llvm_backend_puts_every_function_it_generates_in_the_preemption_section() {
    let (d, out) = build("llvm", &["--emit-ir"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The program's unit: `spin`, `main.body` and `main`. The runtime's functions are the
    // platform's and stay where they are.
    let text = std::fs::read_to_string(d.path().join("p.m0.ll")).unwrap();
    let generated: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("define ") && l.contains("@nz.m."))
        .collect();
    assert_eq!(generated.len(), 3, "{generated:#?}");
    for line in generated {
        assert!(
            line.contains("section \"__TEXT,__nz_text,regular,pure_instructions\""),
            "a generated function is outside the preemption section: {line}"
        );
    }
}

#[test]
fn the_cranelift_backend_puts_every_function_it_generates_in_the_preemption_section() {
    use object::read::{Object as _, ObjectSection as _, ObjectSymbol as _};
    let (d, out) = build("cranelift", &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = std::fs::read(d.path().join("o").join("00-m0.o")).unwrap();
    let file = object::read::File::parse(&*bytes).unwrap();
    let mut seen = 0;
    for sym in file.symbols() {
        if sym.kind() != object::SymbolKind::Text || sym.is_undefined() {
            continue;
        }
        let idx = sym.section_index().expect("a defined symbol has a section");
        let section = file.section_by_index(idx).unwrap();
        seen += 1;
        assert_eq!(
            (section.segment_name().ok().flatten(), section.name().ok()),
            (Some("__TEXT"), Some("__nz_text")),
            "{} is not in the preemption section",
            sym.name().unwrap_or("?")
        );
    }
    assert!(seen >= 2, "found {seen} code symbols to check");
}
