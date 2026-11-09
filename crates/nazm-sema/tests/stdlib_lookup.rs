//! A standard module is found by its whole name (N45): `@std/text` is `text`, never whichever
//! module of the same length is listed first, and a name no module has finds nothing.

use nazm_sema::stdlib::{module, MODULES};

#[test]
fn every_standard_module_is_found_by_its_whole_name_and_by_nothing_shorter_or_alike() {
    assert!(!MODULES.is_empty());
    for (name, source) in MODULES {
        let found = module(&format!("@std/{name}"));
        assert_eq!(found, Some((*name, *source)), "@std/{name}");
    }
    // Names of the same length as real modules, which are not modules.
    for unknown in ["zzzz", "texx", "sorx", "hashh", "nope", ""] {
        assert_eq!(module(&format!("@std/{unknown}")), None, "@std/{unknown}");
    }
    // Not a standard path at all.
    assert_eq!(module("text"), None);
    assert_eq!(module("@other/text"), None);
}
