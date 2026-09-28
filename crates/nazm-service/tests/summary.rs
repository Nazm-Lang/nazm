//! Command summaries through the service: references linked to N30 only when the command
//! read exactly the sources N30 reads. N31.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use nazm_core::__syntax::PARSE_STACK_BYTES;
use nazm_service::summary::{summarize, Command, Outcome, Status};

#[test]
fn a_reference_is_linked_only_to_the_sources_the_command_read() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("main.nz");
    std::fs::write(&root, "fn main() -> Int { 1 + true }\n").unwrap();
    let decided = nazm_service::root::decide(&root, None).unwrap();
    let loaded = nazm_service::load::load(&root, decided.path())
        .ok()
        .unwrap();
    let diagnostics = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(PARSE_STACK_BYTES)
            .spawn_scoped(scope, || nazm_core::check_map(&loaded.map, &loaded.graph))
            .unwrap()
            .join()
            .unwrap()
    });
    assert_eq!(diagnostics.len(), 1);
    let outcome = Outcome {
        command: Command::Check,
        status: Status::Rejected,
        exit_code: 1,
        diagnostics: &diagnostics,
        map: Some(&loaded.map),
        artifact: None,
    };
    let run = |outcome: &Outcome<'_>| {
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(PARSE_STACK_BYTES)
                .spawn_scoped(scope, || summarize(outcome, &root, None))
                .unwrap()
                .join()
                .unwrap()
        })
    };
    let linked = run(&outcome);
    assert!(linked.state.is_some());
    assert!(linked.diagnostics[0].id.is_some());
    // The file changes after the command read it: the command's diagnostic is still
    // reported, with no N30 id and no state — never an id of the new sources.
    std::fs::write(&root, "// moved\nfn main() -> Int { 1 + true }\n").unwrap();
    let unlinked = run(&outcome);
    assert_eq!(unlinked.state, None);
    assert_eq!(unlinked.diagnostics.len(), 1);
    assert_eq!(unlinked.diagnostics[0].id, None);
    assert_eq!(unlinked.diagnostics[0].scope, "command");
    assert_eq!(unlinked.diagnostics[0].code, "N0304");
    // And the status is the command's, whatever the references.
    assert_eq!(unlinked.status, Status::Rejected);
}
