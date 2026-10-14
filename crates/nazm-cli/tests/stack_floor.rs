//! R1, 4.9: the interpreter's call depth is bounded by the stack its thread has, not only by an
//! estimate of a call's cost. A call nested in an expression in the recursive position costs
//! several evaluator frames on a debug build; however deep the recursion and whatever the
//! `--max-depth`, `nazm run` ends with `N0403`, never a signal.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::differential;

#[test]
fn heavy_recursion_reports_n0403_and_never_crashes() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn g(n: Int) -> Int { n }\nfn f(n: Int) -> Int { n }\n\
         fn down(n: Int) -> Int { if n == 0 { 0 } else { 1 + f(g(down(n - 1))) } }\n\
         fn main() -> Int { down(1000000) }\n",
    )
    .unwrap();
    for depth in [None, Some("15000")] {
        let mut args = vec!["run", "prog.nz"];
        if let Some(n) = depth {
            args.extend(["--max-depth", n]);
        }
        let r = differential::nazm(d.path(), &args);
        assert!(r.code.is_some(), "{depth:?}: killed by a signal: {r:?}");
        assert_eq!(r.code, Some(2), "{depth:?}: {r:?}");
        assert!(r.stderr.contains("error[N0403]"), "{depth:?}: {r:?}");
    }
}
