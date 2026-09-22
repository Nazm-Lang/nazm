//! One module, checked from its own source and its dependencies' **interfaces**.
//!
//! # What this file is for
//!
//! `docs/spec.md` claims that a module is a compilation unit in a sense stronger than
//! having an id: *given module A's source and the interfaces of the modules A imports, A
//! can be checked with no access to those modules' bodies.*
//!
//! Every other test reaches the checker through a whole program, where every module's tree
//! happens to be present — so none of them can tell the claim from a coincidence. These
//! do: the dependency **does not exist**. There is no source for it, no tree, no body, and
//! nothing to parse. There is an interface, constructed by hand, and it is the only thing
//! the checker is given about the module it names.
//!
//! The negative case is the other half: with an empty interface the same source is
//! refused. Together they establish that the interface is what made checking possible
//! rather than something else in reach.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use nazm_core::sema::interface::{Export, UnitInterface, Visibility};
use nazm_core::sema::key::ModuleKey;
use nazm_core::sema::module::ModuleGraph;
use nazm_core::sema::resolve::{FnId, Resolution};
use nazm_core::sema::types::Type;
use nazm_core::{check_unit, declare_unit, Unit};
use nazm_span::{FileId, Span};

/// The graph the cases below share: module A, module B, and A imports B.
///
/// B's *file* is in it, because a graph is built while loading and loading is what
/// produced both. Its **text** is not: nothing below ever reads it, which is the point.
fn two_modules() -> ModuleGraph {
    let mut g = ModuleGraph::default();
    let a = g.add(FileId::ROOT, ModuleKey::relative("a.nz"));
    let b = g.add(FileId::from_index(1), ModuleKey::relative("b.nz"));
    g.import(a, b, Span::new(FileId::ROOT, 4, 12));
    g
}

/// What B offers: `twice(Int) -> Int`, and nothing else.
///
/// `def` is a definition id that this compilation never declared, which is exactly the
/// situation a separately compiled dependency would present. The checker records it and
/// does not dereference it.
fn interface_of_b(module: nazm_core::sema::ModuleId) -> UnitInterface {
    UnitInterface::new(
        module,
        vec![Export {
            name: "twice".to_owned(),
            params: vec![Type::Int],
            ret: Type::Int,
            name_span: Span::new(FileId::from_index(1), 7, 12),
            def: FnId::from_index(4096),
        }],
        Vec::new(),
        Vec::new(),
    )
}

const A: &str = "use \"b.nz\";\nfn main() -> Int { twice(3) }\n";

#[test]
fn a_module_checks_against_an_interface_whose_bodies_do_not_exist() {
    let graph = two_modules();
    let (program, parse_errors) = nazm_core::__syntax::parse_source(A);
    assert!(parse_errors.is_empty(), "{parse_errors:#?}");

    let unit = Unit::of(&program, &graph, graph.root());
    let mut res = Resolution::default();
    let mut errors = Vec::new();

    // Stage 1 for A alone. It needs nothing from B, which is what lets the two stages be
    // ordered freely — including around a cycle.
    let own = declare_unit(&unit, &[], &mut res, &mut errors);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(own.module(), graph.root());
    assert!(
        own.exports().is_empty(),
        "nothing in A is `pub`, so A exports nothing"
    );

    // Stage 2 for A, given B's interface and nothing else about B.
    let b = interface_of_b(graph.ids().nth(1).unwrap());
    check_unit(&unit, &[&b], &mut res, &mut errors);
    assert!(
        errors.is_empty(),
        "A should check against B's interface alone: {errors:#?}"
    );

    // And the call was resolved to the definition the interface named — the id from the
    // interface, not one this compilation minted.
    let call = res
        .callee(Span::new(FileId::ROOT, 31, 36))
        .expect("the call to `twice` should be resolved");
    assert_eq!(
        call,
        nazm_core::sema::Callee::Function(FnId::from_index(4096))
    );
}

#[test]
fn without_that_interface_the_same_module_is_refused() {
    let graph = two_modules();
    let (program, _) = nazm_core::__syntax::parse_source(A);
    let unit = Unit::of(&program, &graph, graph.root());
    let mut res = Resolution::default();
    let mut errors = Vec::new();

    declare_unit(&unit, &[], &mut res, &mut errors);
    // B exists and exports nothing.
    let empty = UnitInterface::new(
        graph.ids().nth(1).unwrap(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    check_unit(&unit, &[&empty], &mut res, &mut errors);

    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_eq!(errors[0].code, nazm_diag::codes::UNDEFINED_FUNCTION);
}

#[test]
fn a_dependencys_private_names_are_not_in_its_interface_and_so_are_not_in_scope() {
    // The importer's view of privacy, expressed structurally: a private definition is
    // simply not in the value the importer is given. There is no filtering step at the
    // point of use that could be forgotten.
    let graph = two_modules();
    let b = graph.ids().nth(1).unwrap();
    let mut res = Resolution::default();
    let mut errors = Vec::new();

    let (b_program, _) = nazm_core::__syntax::parse_file(
        FileId::from_index(1),
        "pub fn shown() -> Int { hidden() }\nfn hidden() -> Int { 1 }\n",
    );
    let b_unit = Unit::of(&b_program, &graph, b);
    let iface = declare_unit(&b_unit, &[], &mut res, &mut errors);
    assert!(errors.is_empty(), "{errors:#?}");

    assert_eq!(iface.exports().len(), 1);
    assert_eq!(iface.exports()[0].name, "shown");
    assert!(iface.export("hidden").is_none());

    // The definition still exists, and still belongs to B. It is unexported, not absent.
    let hidden = res.definition(b, "hidden").expect("B defines `hidden`");
    let def = res.function(hidden).unwrap();
    assert_eq!(def.module, b);
    assert_eq!(def.visibility, Visibility::Private);
}
