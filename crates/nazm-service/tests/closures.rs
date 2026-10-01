//! N50: the service over closures.
//!
//! A closure's body reads a captured binding through a copy of its own — a slot of the
//! closure's definition — and a reader means the binding it copies. The service answers in
//! the reader's terms: references and rename see one binding, with every use, inside closures
//! and out.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use nazm_core::__syntax::PARSE_STACK_BYTES;
use nazm_service::LanguageService;

fn on_the_service_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(PARSE_STACK_BYTES)
            .spawn_scoped(scope, f)
            .expect("spawn")
            .join()
            .expect("join")
    })
}

const SRC: &str = "fn main() -> Int {\n    let step = 3;\n    \
                   let f = fn(x: Int) -> Int { x + step };\n    \
                   let g = fn(y: Int) -> Int { let h = fn(z: Int) -> Int { z * step }; h(y) };\n    \
                   f(1) + g(2) + step\n}\n";

fn every(needle: &str) -> Vec<u32> {
    SRC.match_indices(needle)
        .map(|(i, _)| u32::try_from(i).unwrap())
        .collect()
}

#[test]
fn a_captured_binding_is_one_binding_to_references() {
    let dir = tempfile::tempdir().unwrap();
    let m = dir.path().join("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&m, 1, SRC.into());
        assert!(out[0].diagnostics.is_empty(), "{out:#?}");
        let at = every("step");
        // From the use inside the innermost closure: the declaration, and every use.
        let r = s.references(&m, 1, at[2] + 1).expect("references");
        let mut found: Vec<u32> = r.uses.iter().map(|l| l.start).collect();
        found.extend(r.declaration.iter().map(|l| l.start));
        found.sort_unstable();
        found.dedup();
        assert_eq!(found, at, "{r:?}");
    });
}

#[test]
fn renaming_a_captured_binding_renames_it_inside_every_closure() {
    let dir = tempfile::tempdir().unwrap();
    let m = dir.path().join("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&m, 1, SRC.into());
        assert!(out[0].diagnostics.is_empty(), "{out:#?}");
        let at = every("step");
        // From the use inside the first closure.
        let plan = s.rename(&m, 1, at[1] + 1, "stride").expect("a plan");
        assert_eq!(plan.files.len(), 1, "{plan:#?}");
        let f = &plan.files[0];
        let mut text = f.text.to_string();
        for e in f.edits.iter().rev() {
            assert_eq!(&f.text[e.start as usize..e.end as usize], e.expected);
            text.replace_range(e.start as usize..e.end as usize, &e.replacement);
        }
        assert_eq!(text, SRC.replace("step", "stride"));
        let out = s.change(&m, 2, text);
        assert!(out[0].diagnostics.is_empty(), "{out:#?}");
    });
}
