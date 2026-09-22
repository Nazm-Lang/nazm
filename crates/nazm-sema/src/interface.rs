//! What one module offers the modules that import it.
//!
//! # The smallest thing that is enough
//!
//! A [`UnitInterface`] is a module's identity, the definitions it exports with their
//! signatures, and the modules it directly imports. That is the whole of it, and the
//! omissions are the design:
//!
//! - **no bodies**, which is the point — a module is checked against this instead of
//!   against its dependencies' implementations;
//! - **no private definitions**, because they are not offered;
//! - no locals, no control flow, no lowering, no ownership, no effects, no layout, and no
//!   content hash. Each of those is either a later milestone or a different question.
//!
//! The test it exists to make answerable is in `docs/spec.md`: *given a module's source
//! and the interfaces of the modules it imports, that module can be checked with no
//! access to those modules' bodies.* `nazm_core::check_unit` takes exactly these.
//!
//! # What it is not, yet
//!
//! Not a file, not a cache entry, and not a thing that crosses a process boundary. It is
//! built during a compilation from the modules' own declarations and dropped with it.
//! [`Export::def`] is a session-local [`FnId`], which is precisely the field that would
//! have to become a durable identity before an interface could be written to disk and
//! read back by a later build — see `docs/research-register.md` R2.

use nazm_span::Span;

use crate::module::{Dependency, ModuleId};
use crate::resolve::FnId;
use crate::types::Type;

/// Whether a definition leaves its module.
///
/// One bit, and the language has one keyword for it. `docs/spec.md` states the default
/// and why there is no `private`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Nameable only inside the module that defines it. The default, written by writing
    /// nothing.
    Private,
    /// Nameable by modules that **directly** import this one. Written `pub`.
    Exported,
}

impl Visibility {
    /// Derived from whether the parser saw the word.
    ///
    /// The syntax tree records `pub_span` — that `pub` was written, and where — and stops
    /// there. This is the one place that fact becomes a meaning.
    #[must_use]
    pub const fn from_keyword(written: Option<Span>) -> Self {
        match written {
            Some(_) => Self::Exported,
            None => Self::Private,
        }
    }

    #[must_use]
    pub const fn is_exported(self) -> bool {
        matches!(self, Self::Exported)
    }
}

/// One definition a module offers by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    pub name: String,
    pub params: Vec<Type>,
    pub ret: Type,
    /// Where the name is written in the **exporting** module.
    ///
    /// So that a diagnostic in the importing module can point at the definition it is
    /// about: "exported here", in a file the reader can open.
    pub name_span: Span,
    /// Which definition this is, in this compilation.
    ///
    /// Session-local. An interface that outlived its process would need something else
    /// here, and nothing here pretends otherwise.
    pub def: FnId,
}

/// A module's outward face: what it is, what it offers, and what it depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitInterface {
    module: ModuleId,
    exports: Vec<Export>,
    imports: Vec<Dependency>,
}

impl UnitInterface {
    #[must_use]
    pub fn new(module: ModuleId, exports: Vec<Export>, imports: Vec<Dependency>) -> Self {
        Self {
            module,
            exports,
            imports,
        }
    }

    #[must_use]
    pub fn module(&self) -> ModuleId {
        self.module
    }

    /// The exported definitions, in declaration order.
    #[must_use]
    pub fn exports(&self) -> &[Export] {
        &self.exports
    }

    /// The export with this name, if the module has one.
    ///
    /// A scan rather than a map: a module's exported surface is small by construction, and
    /// a second index over it is a second thing that can disagree with the first.
    #[must_use]
    pub fn export(&self, name: &str) -> Option<&Export> {
        self.exports.iter().find(|e| e.name == name)
    }

    /// The modules this one directly imports.
    #[must_use]
    pub fn imports(&self) -> &[Dependency] {
        &self.imports
    }
}
