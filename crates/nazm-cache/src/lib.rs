//! Reusing a semantic check, soundly.
//!
//! # The claim, and the failure it must not have
//!
//! > If a module's own source and the interfaces it depends on have not changed, the
//! > result of checking it has not changed either.
//!
//! That is a soundness statement before it is a speed one. A cache that hits when it
//! should not is not a slow compiler; it is a compiler that accepted a program it should
//! have refused, silently, with a plausible explanation. `docs/architecture.md` §3 has
//! said since before there was a compiler that anything omitted from a key is a
//! miscompilation waiting to happen, and this crate is that sentence made into types.
//!
//! # The pieces
//!
//! ```text
//! SourceFingerprint   the exact bytes of one module's source
//! CheckerIdentity     which compiler's rules produced, or want, a result
//! CheckKey            those two plus the (ModuleKey, InterfaceHash) of each direct import
//! CheckArtifact       a module that checked cleanly, and the inputs that were true when it did
//! Store               a local directory of them, addressed by key, published atomically
//! ```
//!
//! [`InterfaceHash`](nazm_iface::InterfaceHash) is N3's and is unchanged: it answers *must
//! this module's dependents be re-checked*, and it is what one module contributes to
//! another's key.
//!
//! # Three keys, and the one that does not exist yet
//!
//! | | Over | Answers |
//! | --- | --- | --- |
//! | `InterfaceHash` | a module's published interface | must its **dependents** be re-checked |
//! | `CheckKey` | everything semantic checking of a module reads | may its **check** be skipped |
//! | `CodegenKey` | that plus implementation, target, CPU features, backend, optimisation level, ABI | may its **object code** be reused — **not built** |
//!
//! Collapsing the second into the third is the mistake worth naming. A `CheckKey` says
//! nothing about a body except through the module's own source fingerprint, and nothing
//! at all about a target; an object file keyed on one would be handed back for a
//! different machine. `docs/roadmap.md` records what the third needs.
//!
//! # What this crate is not
//!
//! Not a scheduler, not a query engine and not a build system. It computes keys, writes
//! files and validates them. Deciding *which* modules to check is the driver's, and the
//! checking itself is `nazm-core`'s — this crate cannot check anything, and does not
//! depend on the crate that can.
//!
//! It is also not a native-code cache. Nothing here knows what a target triple is, and
//! `docs/capability-matrix.md` is explicit that incremental **native** compilation does
//! not exist.

pub mod artifact;
mod digest;
pub mod identity;
pub mod key;
pub mod source;
pub mod store;

pub use artifact::{CheckArtifact, Invalid, Reusable, SCHEMA};
pub use identity::{CheckerIdentity, CHECKER_CODES, SEMANTIC_EPOCH};
pub use key::{CheckKey, Dependency};
pub use source::SourceFingerprint;
pub use store::{Found, Store};
