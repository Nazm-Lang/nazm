//! N49: the service over qualified names.
//!
//! A qualified name resolves to the same definition an unqualified import would, recorded at
//! the item's own span, so the service's answers need no case of their own: a definition is
//! found from `g::area`, and a reference to `area` is the item's bytes alone, never the alias.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use nazm_service::{LanguageService, Target};

mod common;
use common::on_the_service_stack;

const GEOMETRY: &str =
    "pub struct Point { x: Int, y: Int, }\npub fn area(p: Point) -> Int { p.x * p.y }\n";
const MAIN: &str =
    "use \"geometry.nz\" as g;\nfn main() -> Int { g::area(g::Point(x: 2, y: 3)) }\n";

#[test]
fn a_qualified_use_is_a_reference_at_the_items_name_and_finds_its_definition() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("geometry.nz"), GEOMETRY).unwrap();
    std::fs::write(dir.path().join("main.nz"), MAIN).unwrap();
    let main = dir.path().join("main.nz");
    let geometry = dir.path().join("geometry.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, MAIN.into());
        let use_at = u32::try_from(MAIN.find("area(").unwrap()).unwrap();

        let d = s.definition(&main, 1, use_at + 1).expect("a definition");
        assert_eq!(d.kind, "function");
        let Target::File(loc) = d.target else {
            panic!("a file location: {d:?}");
        };
        assert_eq!(loc.path.canonicalize().ok(), geometry.canonicalize().ok());
        assert_eq!(
            &GEOMETRY[loc.start as usize..loc.end as usize],
            "area",
            "{loc:?}"
        );

        let r = s.references(&main, 1, use_at + 1).expect("references");
        let here: Vec<_> = r
            .uses
            .iter()
            .filter(|l| l.path.canonicalize().ok() == main.canonicalize().ok())
            .map(|l| &MAIN[l.start as usize..l.end as usize])
            .collect();
        assert_eq!(here, ["area"], "the item's bytes, not `g::area`: {r:?}");

        // The record named through the same alias is the same record.
        let point_at = u32::try_from(MAIN.find("Point(").unwrap()).unwrap();
        let d = s.definition(&main, 1, point_at + 1).expect("a definition");
        let Target::File(loc) = d.target else {
            panic!("a file location: {d:?}");
        };
        assert_eq!(&GEOMETRY[loc.start as usize..loc.end as usize], "Point");
    });
}
