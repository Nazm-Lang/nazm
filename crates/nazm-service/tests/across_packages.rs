//! N74: the language service across a package boundary.
//!
//! `docs/architecture.md` §7.76. A program that depends on another package is one compilation to
//! the service, as it is to `nazm check`: diagnostics, definition, references, hover and workspace
//! symbols reach the dependency's sources, and renaming the dependency's public function is refused
//! — its other importers are programs no open document loads, so no edit could be shown complete.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use nazm_service::rename::Refusal;
use nazm_service::{LanguageService, Target};

mod common;
use common::on_the_service_stack;

const APP_MANIFEST: &str = "schema = \"nazm.package/1\"\n\n[package]\nname = \"app\"\nversion = \"0.1.0\"\nmain = \"main.nz\"\n\n[dependencies]\ngeometry = { path = \"../geometry\", version = \"1.0.0\" }\n";
const GEO_MANIFEST: &str =
    "schema = \"nazm.package/1\"\n\n[package]\nname = \"geometry\"\nversion = \"1.0.0\"\n";
const APP: &str =
    "use \"geometry:area.nz\";\n\nfn main() -> Int ! {} { rectangle(6, 7) + rectangle(1, 2) }\n";
const AREA: &str =
    "/// The area of a w by h rectangle.\npub fn rectangle(w: Int, h: Int) -> Int ! {} { w * h }\n";

fn workspace() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    for (p, t) in [
        ("app/nazm.toml", APP_MANIFEST),
        ("app/src/main.nz", APP),
        ("geometry/nazm.toml", GEO_MANIFEST),
        ("geometry/src/area.nz", AREA),
    ] {
        let f = d.path().join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, t).unwrap();
    }
    let (app, area) = (
        d.path().join("app/src/main.nz"),
        d.path().join("geometry/src/area.nz"),
    );
    (d, app, area)
}

fn same(a: &Path, b: &Path) -> bool {
    a.canonicalize().ok() == b.canonicalize().ok()
}

fn offset(text: &str, needle: &str, n: usize) -> u32 {
    u32::try_from(text.match_indices(needle).nth(n).unwrap().0 + 1).unwrap()
}

#[test]
fn definition_references_hover_and_symbols_reach_the_dependency() {
    let (_d, app, area) = workspace();
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&app, 1, APP.into());
        assert!(out.iter().all(|p| p.diagnostics.is_empty()), "{out:#?}");
        let d = s
            .definition(&app, 1, offset(APP, "rectangle", 1))
            .expect("a definition");
        let Target::File(l) = &d.target else {
            panic!("{d:?}")
        };
        assert!(same(&l.path, &area), "{}", l.path.display());
        assert_eq!(&l.text[l.start as usize..l.end as usize], "rectangle");
        let r = s
            .references(&app, 1, offset(APP, "rectangle", 1))
            .expect("references");
        assert!(same(&r.declaration.as_ref().unwrap().path, &area));
        assert_eq!(r.uses.iter().filter(|u| same(&u.path, &app)).count(), 2);
        let h = s
            .hover(&app, 1, offset(APP, "rectangle", 1))
            .expect("hover");
        assert!(
            h.text.contains("fn rectangle(w: Int, h: Int) -> Int"),
            "{}",
            h.text
        );
        let found = s.workspace_symbols("rectangle");
        assert!(
            found
                .iter()
                .any(|w| w.name == "rectangle" && same(&w.declaration.path, &area)),
            "{found:#?}"
        );
    });
}

#[test]
fn a_dependencys_error_is_published_against_the_dependency() {
    let (_d, app, area) = workspace();
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&app, 1, APP.into());
        let out = s.open(
            &area,
            1,
            "pub fn rectangle(w: Int, h: Int) -> Int ! {} { w * true }\n".into(),
        );
        let mine = out
            .iter()
            .find(|p| same(&p.path, &area))
            .expect("the dependency is published");
        assert!(!mine.diagnostics.is_empty(), "{out:#?}");
    });
}

#[test]
fn renaming_a_dependencys_public_function_is_refused() {
    let (_d, app, _area) = workspace();
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&app, 1, APP.into());
        let r = s.rename(&app, 1, offset(APP, "rectangle", 0), "area_of");
        assert!(matches!(r, Err(Refusal::Exported)), "{r:?}");
    });
}
