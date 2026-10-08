//! Everything semantic checking of one module depends on, as one number.
//!
//! # The question a `CheckKey` answers
//!
//! > Are all the inputs capable of affecting the semantic checking of this module
//! > identical to those of a module that was already checked?
//!
//! Not "is this module unchanged" — a module whose dependency's exported signature moved
//! is unchanged and must be checked again. Not "is this build the same" — a target triple
//! and an optimisation level change nothing about whether a program type-checks. The key
//! covers exactly what checking reads, which is why what it **excludes** is stated as
//! carefully as what it includes.
//!
//! # What is deliberately **not** in it, and why
//!
//! An omission is the failure mode: `docs/architecture.md` §3 has said since before there
//! was a compiler that anything left out of a cache key is a silent miscompile waiting to
//! happen. Each of these was checked against the implementation rather than assumed.
//!
//! | Left out | Why it cannot affect checking this module |
//! | --- | --- |
//! | target triple, CPU features, ABI | `crates/nazm-core/src/check/` mentions none of them. `Int` is 64-bit in every configuration and no rule consults the host. **A codegen key is a different key and will need all of them** |
//! | optimisation level, backend | `nazm build`'s subset refusals live in `nazm-lir`, after checking, and are not cached |
//! | `--max-iterations`, `--max-depth` | interpreter budgets. `docs/spec.md` places them in the interpreter rather than in the language |
//! | the source root, the checkout directory, the working directory | they decide what a [`ModuleKey`] *is*; once it exists they are gone, which is what lets two checkouts share an entry |
//! | the cache's own location | a store holds entries; it does not name them |
//! | whether the compilation is a program | `N0204` is a fact about a *compilation*, not about a module, and it is recomputed from declarations every run — never cached, never skipped |
//! | a dependency's source, body, or private definitions | none is readable from `check_unit`, which is the property N2 established |
//! | a dependency's own dependencies | imports are not transitive |
//! | load order, session ids, spans, timestamps | not semantics |
//!
//! The one that will be wrong first is the first row: it is right **today**, and a rule
//! that depended on a target would have to be added to the key on the day it was added to
//! the checker.

use std::fmt;

use nazm_iface::InterfaceHash;
use nazm_sema::key::ModuleKey;

use crate::digest::{self, Framed};
use crate::identity::CheckerIdentity;
use crate::source::SourceFingerprint;

/// One direct dependency, as a check key sees it: which module, and what it offers.
///
/// Its *interface*, not its source. That is the whole of the invalidation model: a
/// dependency whose private body changed offers the same thing, so this pair is unchanged
/// and the dependent keeps its key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Dependency {
    pub module: ModuleKey,
    pub interface: InterfaceHash,
}

/// The inputs to checking one module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CheckKey([u8; 32]);

impl CheckKey {
    /// The key for a module with this source, under this checker, over these
    /// dependencies.
    ///
    /// # Order
    ///
    /// `dependencies` is sorted and deduplicated here, so the order the loader happened
    /// to walk the import graph in cannot reach the key. That is safe **because the order
    /// the imports were written in is part of the module's own source**, and the source
    /// is an input: two modules differing only in which `use` comes first have different
    /// [`SourceFingerprint`]s and therefore different keys, which is what preserves the
    /// distinction `N0207` draws between them. Sorting removes an incidental difference
    /// without removing a real one, and the two tests below check both halves of that
    /// sentence.
    #[must_use]
    pub fn of(
        checker: CheckerIdentity,
        module: &ModuleKey,
        source: SourceFingerprint,
        dependencies: &[Dependency],
    ) -> Self {
        let mut deps = dependencies.to_vec();
        deps.sort();
        deps.dedup();

        let mut f = Framed::new("nazm.check-key/1");
        f.field(&checker.to_string())
            .field(module.as_str())
            .field(&source.to_string())
            .number(u32::try_from(deps.len()).unwrap_or(u32::MAX));
        for d in &deps {
            f.field(d.module.as_str()).field(&d.interface.to_string());
        }
        Self(f.finish())
    }

    /// Read one back from its hexadecimal form.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        digest::from_hex(text).map(Self)
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for CheckKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        digest::write_hex(f, &self.0)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn m(p: &str) -> ModuleKey {
        ModuleKey::relative(p).expect("a key")
    }

    fn iface(n: u8) -> InterfaceHash {
        InterfaceHash::of_bytes(&[n])
    }

    fn key(module: &str, source: &[u8], deps: &[(&str, u8)]) -> CheckKey {
        let deps: Vec<Dependency> = deps
            .iter()
            .map(|(p, n)| Dependency {
                module: m(p),
                interface: iface(*n),
            })
            .collect();
        CheckKey::of(
            CheckerIdentity::current(),
            &m(module),
            SourceFingerprint::of(source),
            &deps,
        )
    }

    #[test]
    fn the_same_inputs_give_the_same_key() {
        assert_eq!(
            key("a.nz", b"x", &[("b.nz", 1)]),
            key("a.nz", b"x", &[("b.nz", 1)])
        );
    }

    #[test]
    fn every_input_moves_it() {
        // One test per field, because a key missing a field is exactly a wrong hit.
        let base = key("a.nz", b"x", &[("b.nz", 1)]);
        assert_ne!(base, key("other.nz", b"x", &[("b.nz", 1)]));
        assert_ne!(base, key("a.nz", b"y", &[("b.nz", 1)]));
        assert_ne!(base, key("a.nz", b"x", &[("b.nz", 2)]));
        assert_ne!(base, key("a.nz", b"x", &[("c.nz", 1)]));
        assert_ne!(base, key("a.nz", b"x", &[]));
        assert_ne!(base, key("a.nz", b"x", &[("b.nz", 1), ("c.nz", 2)]));
    }

    #[test]
    fn the_order_the_loader_walked_the_graph_in_does_not() {
        // Incidental, so it is canonicalised away. What is **not** incidental — which
        // `use` the author wrote first — is in the source, and the test below says so.
        assert_eq!(
            key("a.nz", b"x", &[("b.nz", 1), ("c.nz", 2)]),
            key("a.nz", b"x", &[("c.nz", 2), ("b.nz", 1)])
        );
    }

    #[test]
    fn the_order_the_imports_were_written_in_does() {
        // `N0207` names the *second* import that brings a colliding name in, so two
        // programs differing only in which `use` came first are two programs. The
        // distinction survives because the source is an input, not because the
        // dependency list preserved an order.
        let one = key(
            "a.nz",
            b"use \"b.nz\";\nuse \"c.nz\";\n",
            &[("b.nz", 1), ("c.nz", 2)],
        );
        let two = key(
            "a.nz",
            b"use \"c.nz\";\nuse \"b.nz\";\n",
            &[("b.nz", 1), ("c.nz", 2)],
        );
        assert_ne!(one, two);
    }

    #[test]
    fn a_repeated_dependency_is_one_dependency() {
        assert_eq!(
            key("a.nz", b"x", &[("b.nz", 1)]),
            key("a.nz", b"x", &[("b.nz", 1), ("b.nz", 1)])
        );
    }

    #[test]
    fn it_round_trips_through_its_written_form() {
        let k = key("a.nz", b"x", &[]);
        assert_eq!(CheckKey::parse(&k.to_string()), Some(k));
        assert!(CheckKey::parse("short").is_none());
    }
}
