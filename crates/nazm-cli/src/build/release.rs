//! `nazm build --release` (R1.5, D15): a hardened macOS executable.
//!
//! Four things, each with the reason it is there and the test that holds it
//! (`crates/nazm-cli/tests/release.rs`):
//!
//! - **Stack protection.** Every function the generated LLVM text defines — the program's, the
//!   runtime's and the entry's — carries `sspstrong`, so one with a local array or a local whose
//!   address is taken checks a canary before it returns and calls `__stack_chk_fail` when an
//!   overrun has written over it. Written into the text, not asked of clang: `-fstack-protector`
//!   is a C front end's option and does nothing to LLVM IR handed to the driver (measured
//!   2026-10-15). The text is what the object cache keys, so a protected object is never an
//!   unprotected one's entry.
//! - **Position independence.** Linked with `-pie`: the executable loads at a random address.
//!   macOS links an executable this way already; `--release` asks for it rather than relying on it.
//! - **Stripped.** `strip` removes the symbol table but for what the dynamic loader needs, and the
//!   debug map with it: no function name of the program or the runtime is left in the file.
//! - **Signed ad hoc with the hardened runtime.** `codesign --sign - --options runtime`: the
//!   kernel refuses the process code injection, unsigned executable memory and
//!   `DYLD_*` variables. Ad hoc, so no identity is needed and none is claimed.

use std::path::Path;
use std::process::Command;

/// `text` with every function it defines protected by a stack canary (`sspstrong`).
///
/// The attribute goes after the parameter list's `)`, where a function's attributes begin; a
/// `define` that already says `ssp`, `sspstrong` or `sspreq` is left as it is.
pub(crate) fn stack_protected(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 4096);
    for line in text.split_inclusive('\n') {
        match protect_line(line) {
            Some(p) => out.push_str(&p),
            None => out.push_str(line),
        }
    }
    out
}

/// One `define` line with `sspstrong` after its parameter list, or `None` for any other line.
fn protect_line(line: &str) -> Option<String> {
    if !line.starts_with("define ") {
        return None;
    }
    let head = line.split('{').next().unwrap_or(line);
    if [" ssp ", " sspstrong ", " sspreq "]
        .iter()
        .any(|a| head.contains(a))
    {
        return None;
    }
    // The parameter list opens at the first `(` after the function's name.
    let at = line.find('@')?;
    let open = at + line.get(at..)?.find('(')?;
    let mut depth = 0usize;
    for (i, b) in line.bytes().enumerate().skip(open) {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let (before, after) = line.split_at(i + 1);
                    return Some(format!("{before} sspstrong{after}"));
                }
            }
            _ => {}
        }
    }
    None
}

/// The linker's arguments a release executable adds: position-independent.
pub(crate) fn link_args() -> Vec<String> {
    vec!["-Wl,-pie".to_owned()]
}

/// Strip the linked executable, then sign it ad hoc with the hardened runtime.
///
/// # Errors
///
/// The tool that failed and what it said.
pub(crate) fn finish(exe: &Path) -> Result<(), String> {
    tool("strip", &[exe.as_os_str()])?;
    tool(
        "codesign",
        &[
            "--force".as_ref(),
            "--sign".as_ref(),
            "-".as_ref(),
            "--options".as_ref(),
            "runtime".as_ref(),
            exe.as_os_str(),
        ],
    )
}

/// Run one of the toolchain's tools with the environment the driver is given, nothing else.
fn tool(name: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let mut c = Command::new(name);
    c.env_clear();
    for var in crate::backend::PASSED {
        if let Some(value) = std::env::var_os(var) {
            c.env(var, value);
        }
    }
    match c.args(args).output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(format!(
            "`{name}` failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => Err(format!("`{name}` could not be run: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_defined_function_and_only_those_is_protected() {
        let text = "declare void @use(ptr)\n\
                    define internal i64 @f(i64 %p0) \"probe-stack\"=\"inline-asm\" {   ; f\n\
                    entry:\n  ret i64 %p0\n}\n\
                    define hidden void @nz.fail(ptr %m, i64 %n) {\n  ret void\n}\n\
                    define { i64, i64 } @g(i64 dereferenceable(8) %n) {\n  ret void\n}\n\
                    define void @h() sspreq {\n  ret void\n}\n";
        let out = stack_protected(text);
        assert!(out.contains("define internal i64 @f(i64 %p0) sspstrong \"probe-stack\""));
        assert!(out.contains("define hidden void @nz.fail(ptr %m, i64 %n) sspstrong {"));
        assert!(out.contains("define { i64, i64 } @g(i64 dereferenceable(8) %n) sspstrong {"));
        assert!(out.contains("define void @h() sspreq {"), "{out}");
        assert!(out.contains("declare void @use(ptr)\n"));
        assert_eq!(out.matches("sspstrong").count(), 3);
    }
}
