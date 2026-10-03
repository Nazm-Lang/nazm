//! The core prelude: one toolchain-owned module every other module imports implicitly.
//!
//! # What it is
//!
//! Ordinary Nazm source — `library/core/prelude.nz` — declaring `Result[T, E]` and
//! `Option[T]` as ordinary generic enums. It is loaded into every compilation as one more
//! module, checked by the same checker, published through the same interface, and
//! depended on through the same import edge a `use` would create. Nothing downstream has
//! a special case for it; what is special is only **where it comes from** and **what it
//! is called**:
//!
//! * its [`ModuleKey`] is [`ModuleKey::prelude`], fixed by the toolchain rather than
//!   computed from a path, so `Result` is the same definition in every project;
//! * every other module of a compilation imports it, with no `use` written.
//!
//! # What it is not
//!
//! Not a standard library, a package, or a namespace mechanism. There is one such module,
//! it has two declarations, and nothing can add a second one. `docs/spec.md`, *The core
//! prelude*, owns the rules; `docs/architecture.md` §7.13 says why the boundary is here.
//!
//! # The bytes
//!
//! The file is the definition. This compiler carries a copy of its bytes so that a
//! compilation needs no installation directory, which is a packaging decision and not a
//! semantic one: what is checked is those bytes, parsed as source, and nothing here
//! describes the two enums in Rust.

use crate::key::ModuleKey;

/// The core prelude's source, exactly as the file in the repository holds it.
pub const SOURCE: &str = include_str!("../../../library/core/prelude.nz");

/// The name the prelude's file is shown under in a diagnostic.
///
/// Not a path to open: the file is part of the toolchain. It spells the module's key so a
/// reader can see which module a secondary span points into.
pub const FILE_NAME: &str = "@core/prelude.nz";

/// The prelude's durable identity.
#[must_use]
pub fn key() -> ModuleKey {
    ModuleKey::prelude()
}

/// The name of the definition `?` propagates through.
pub const RESULT: &str = "Result";

/// The name of the definition a nullable C result is (N85).
pub const OPTION: &str = "Option";
