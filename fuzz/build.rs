//! The coverage callbacks, compiled by the host's C compiler with no instrumentation (N93).

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_else(|_| ".".to_owned()));
    let object = out.join("cov.o");
    let library = out.join("libnzcov.a");
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_owned());
    let ok = Command::new(&cc)
        .args(["-O2", "-fPIC", "-c", "cov.c", "-o"])
        .arg(&object)
        .status()
        .is_ok_and(|s| s.success())
        && Command::new("ar")
            .arg("crs")
            .arg(&library)
            .arg(&object)
            .status()
            .is_ok_and(|s| s.success());
    if !ok {
        panic!("could not compile cov.c with `{cc}`");
    }
    println!("cargo:rerun-if-changed=cov.c");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=nzcov");
}
