//! Identities that survive the process that computed them.
//!
//! # Why these are not the ids beside them
//!
//! [`ModuleId`](crate::ModuleId) and [`FnId`](crate::FnId) are indices into one
//! compilation: dense, cheap, exact, and meaningless to any other run. That is the right
//! shape for a checker and the wrong one for a file on disk. These are the other shape —
//! computed once per module, written down, and expected to mean the same thing next time.
//!
//! Both layers stay. Renaming the session ids to something durable-sounding would have
//! been the cheap version of this milestone and would have left every consumer holding a
//! load-order index under a new name.
//!
//! # What they are not
//!
//! Not content. `DefKey` answers *which definition is this*; whether its body changed is a
//! fingerprint's question, and `docs/spec.md` keeps the two apart because collapsing them
//! makes every edit create a new definition — the opposite of what durable identity is
//! for.
//!
//! Not a package path. There is no package system, and one was not invented to obtain a
//! namespace; the source root is derived from the root module's directory.

use std::fmt;

/// The directory-like prefix the toolchain reserves. No project key begins with it.
const CORE: &str = "@core";

/// The prefix of every standard module's key (N45). See [`ModuleKey::std`].
const STD: &str = "@std";

/// The core prelude's key. See [`ModuleKey::prelude`].
const PRELUDE: &str = "@core/prelude";

/// Which logical module this is: its path relative to the compilation's source root.
///
/// # What equality means
///
/// Two `ModuleKey`s are equal when the modules occupy the same place in the same project,
/// whatever directory that project was checked out into. `docs/spec.md`, *Durable
/// identity*, owns the rule; this owns its spelling:
///
/// * components separated by `/`, on every platform;
/// * no leading or trailing separator;
/// * `.` components removed and `..` folded away;
/// * no absolute path, no canonical path, no drive letter, and nothing above the root.
///
/// # When there is none
///
/// [`ModuleKey::relative`] returns `None` for a path that still escapes the source root
/// after folding. A module reached under two different names through a symbolic link also
/// has none, which the loader decides because only it can see the second name. Both
/// compile exactly as before; what they do not get is a durable identity, and the things
/// built on one decline rather than guess.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleKey(String);

impl ModuleKey {
    /// The key for a module at `path`, relative to the source root.
    ///
    /// `path` is the module's path **as the loader resolved it**, relative to the root
    /// module's directory — already relative, not yet normalised. Returns `None` when
    /// normalisation cannot bring it inside the root.
    #[must_use]
    pub fn relative(path: &str) -> Option<Self> {
        let key = Self::fold(path)?;
        // `@core` is the toolchain's, never a project's. A project file that happens to sit
        // under a directory of that name compiles exactly as before and has no durable
        // identity, so no project module can ever be keyed as the core prelude is.
        if key.0 == CORE || key.0.starts_with("@core/") {
            return None;
        }
        // Nor is `@std` (N45): a project file under a directory of that name has no durable
        // identity, so none can be keyed as a standard module is.
        if key.0 == STD || key.0.starts_with("@std/") {
            return None;
        }
        // Nor may its first component hold an `@` (N46): `NAME@VERSION/…` is a dependency
        // package's module, and a project path spelled that way would otherwise be keyed as
        // one. Deeper components may: `lib/@core/x.nz` is a project's own.
        if key.0.split('/').next().is_some_and(|c| c.contains('@')) {
            return None;
        }
        Some(key)
    }

    /// The core prelude's key: `@core/prelude`.
    ///
    /// Owned by the toolchain rather than computed from a path, so it is the same in every
    /// project, from every working directory and every checkout — the property a
    /// definition `?` recognises has to have. [`ModuleKey::relative`] can never produce
    /// it, which is what keeps it disjoint from every project module's key.
    #[must_use]
    pub fn prelude() -> Self {
        Self(PRELUDE.to_owned())
    }

    /// The key of the standard module `name`: `@std/name` (N45).
    ///
    /// Owned by the toolchain like the prelude's, so `@std/text` is one module in every
    /// project; [`ModuleKey::relative`] can never produce one.
    #[must_use]
    pub fn std(name: &str) -> Self {
        Self(format!("{STD}/{name}"))
    }

    /// The key of the module at `path` in the package `NAME@VERSION` (N46): `NAME@VERSION/path`,
    /// `path` relative to that package's source root and folded as any key is. `None` if it
    /// leaves the root.
    #[must_use]
    pub fn in_package(prefix: &str, path: &str) -> Option<Self> {
        let inner = Self::fold(path)?;
        Some(Self(format!("{prefix}/{}", inner.0)))
    }

    /// Whether this is a standard module's key.
    #[must_use]
    pub fn is_std(&self) -> bool {
        self.0.starts_with("@std/")
    }

    /// Whether this is the core prelude's key.
    #[must_use]
    pub fn is_prelude(&self) -> bool {
        self.0 == PRELUDE
    }

    fn fold(path: &str) -> Option<Self> {
        let mut out: Vec<&str> = Vec::new();
        for part in path.split(['/', '\\']) {
            match part {
                "" | "." => {}
                ".." => {
                    // Folding `..` against a real component keeps the key inside the
                    // root. Against nothing, it escapes — and a key that named a
                    // directory above the root would depend on where the root sits,
                    // which is the one thing this may not depend on.
                    out.pop()?;
                }
                other => out.push(other),
            }
        }
        if out.is_empty() {
            return None;
        }
        Some(Self(out.join("/")))
    }

    /// The key exactly as written, for reading one back from a persisted interface.
    ///
    /// Validating rather than trusting: a key read from a file is data, and one that is
    /// not in the canonical spelling is refused instead of being repaired into something
    /// that would then compare unequal to the key this compilation computes.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if text == PRELUDE {
            return Some(Self::prelude());
        }
        // A standard module's key (N45): `@std/` and a lowercase name.
        if let Some(name) = text.strip_prefix("@std/") {
            let ok = !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase());
            return ok.then(|| Self::std(name));
        }
        // A dependency package's module (N46): `NAME@VERSION/` and a relative key.
        if let Some((prefix, rest)) = text.split_once('/') {
            if let Some((name, version)) = prefix.split_once('@') {
                let name_ok = name.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
                let version_ok = version.split('.').count() == 3
                    && version
                        .split('.')
                        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
                let inner = Self::relative(rest)?;
                return (name_ok && version_ok && inner.0 == rest).then(|| Self(text.to_owned()));
            }
        }
        let candidate = Self(text.to_owned());
        let canonical = Self::relative(text)?;
        (candidate == canonical).then_some(canonical)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModuleKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What kind of thing a definition is.
///
/// A type rather than an implied constant, because a `DefKey` that did not say would have
/// to be reinterpreted when there is a second kind — and reinterpreting a durable identity
/// is the failure this whole layer exists to avoid. N3 wrote that down when there was one
/// variant; N9 is the milestone that added the second, and no key written before it
/// changed meaning, which is what the design was for.
///
/// The spelling is a word rather than a number for the same reason. `record` and not
/// `struct`: `docs/spec.md` uses `struct` for the source keyword and *record* for the
/// semantic category, and this is a semantic identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DefKind {
    Function,
    /// A user-defined record. One namespace apart from functions — see `docs/spec.md`,
    /// *Types and functions are separate namespaces* — so `a.nz::fn Point` and
    /// `a.nz::record Point` are two definitions and this is the field that says so.
    Record,
    /// A user-defined enum. Added by N10, which is the second time a key written under an
    /// earlier build kept its meaning — the property this field exists for.
    ///
    /// Records and enums share the **type** namespace, so `a.nz::record Item` and
    /// `a.nz::enum Item` are two kinds that cannot both exist in one module. The checker
    /// refuses the collision; this field is what makes the two identities distinguishable
    /// when they are in different modules.
    Enum,
}

impl DefKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Function => "fn",
            Self::Record => "record",
            // The source keyword, unlike `record`: `docs/spec.md` uses `struct` in source
            // and *record* for the category, and there is no such pair for an enum — the
            // keyword and the category are one word, so the identity uses it.
            Self::Enum => "enum",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "fn" => Some(Self::Function),
            "record" => Some(Self::Record),
            "enum" => Some(Self::Enum),
            _ => None,
        }
    }
}

/// Which logical definition this is: its module, its kind, and its declared name.
///
/// # What equality means
///
/// `docs/spec.md` owns the table. In short: a body edit, a signature edit, adding or
/// removing `pub`, and moving the declaration within its module all keep the key. A rename
/// does not — it is a removal and an addition. Deleting a definition and later declaring
/// one of the same name gives the same key, which is continuity of *a name in a module*
/// and not of an author's intent; lineage is a different question and this does not
/// pretend to answer it.
///
/// # What is not in it
///
/// No span, so moving a function is not a new definition. No body hash, so editing one is
/// not either. No signature, so changing a parameter type is the same definition with a
/// different contract. Each of those is a fingerprint's job.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DefKey {
    module: ModuleKey,
    kind: DefKind,
    name: String,
}

impl DefKey {
    #[must_use]
    pub fn new(module: ModuleKey, kind: DefKind, name: impl Into<String>) -> Self {
        Self {
            module,
            kind,
            name: name.into(),
        }
    }

    #[must_use]
    pub fn module(&self) -> &ModuleKey {
        &self.module
    }

    #[must_use]
    pub fn kind(&self) -> DefKind {
        self.kind
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// `module::kind name`, one line, unambiguous.
///
/// `::` and a space separate the three parts, and none of them may contain either: a
/// module key has no spaces by construction and an identifier has neither.
impl fmt::Display for DefKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{} {}", self.module, self.kind.as_str(), self.name)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn key(p: &str) -> Option<String> {
        ModuleKey::relative(p).map(|k| k.as_str().to_owned())
    }

    #[test]
    fn a_key_is_the_path_relative_to_the_root_and_nothing_else() {
        assert_eq!(key("main.nz").as_deref(), Some("main.nz"));
        assert_eq!(key("lib/util.nz").as_deref(), Some("lib/util.nz"));
        assert_eq!(key("./lib/./util.nz").as_deref(), Some("lib/util.nz"));
        assert_eq!(key("lib/sub/../util.nz").as_deref(), Some("lib/util.nz"));
        // Windows separators normalise to the one spelling, so a key computed on one
        // platform equals the key computed on another.
        assert_eq!(key("lib\\util.nz").as_deref(), Some("lib/util.nz"));
    }

    #[test]
    fn a_path_that_escapes_the_root_has_no_key() {
        // Not an error and not a guess: this compiles exactly as before, and gets no
        // durable identity because its identity would depend on where the root sits.
        assert_eq!(key("../outside.nz"), None);
        assert_eq!(key("lib/../../outside.nz"), None);
        assert_eq!(key(""), None);
        assert_eq!(key("."), None);
    }

    #[test]
    fn parsing_refuses_a_key_that_is_not_in_the_canonical_spelling() {
        // A key read back from a file is data. Repairing `./lib/util.nz` into
        // `lib/util.nz` would be worse than refusing it: the file would then claim an
        // identity its own bytes do not spell, and two writers could disagree.
        assert!(ModuleKey::parse("lib/util.nz").is_some());
        assert!(ModuleKey::parse("./lib/util.nz").is_none());
        assert!(ModuleKey::parse("lib//util.nz").is_none());
        assert!(ModuleKey::parse("lib/util.nz/").is_none());
        assert!(ModuleKey::parse("../outside.nz").is_none());
        assert!(ModuleKey::parse("lib\\util.nz").is_none());
    }

    #[test]
    fn no_project_path_can_take_the_cores_key() {
        // Disjoint by construction: a file under a directory called `@core` is compiled,
        // and keyed as nothing, rather than being mistaken for the toolchain's module.
        assert_eq!(key("@core/prelude"), None);
        assert_eq!(key("@core/prelude.nz"), None);
        assert_eq!(key("./@core/x.nz"), None);
        assert_eq!(key("lib/@core/x.nz").as_deref(), Some("lib/@core/x.nz"));
        assert!(ModuleKey::prelude().is_prelude());
        assert_eq!(
            ModuleKey::parse("@core/prelude"),
            Some(ModuleKey::prelude())
        );
        assert_eq!(ModuleKey::parse("@core/other"), None);
        // The toolchain's and the packages' keys read back as themselves (N45, N46).
        assert_eq!(ModuleKey::parse("@std/text"), Some(ModuleKey::std("text")));
        assert_eq!(ModuleKey::parse("@std/"), None);
        assert_eq!(ModuleKey::parse("@std/Text"), None);
        assert_eq!(
            ModuleKey::parse("geo@1.2.0/lib.nz"),
            ModuleKey::in_package("geo@1.2.0", "lib.nz")
        );
        assert!(ModuleKey::parse("geo@1.2/lib.nz").is_none());
        assert!(ModuleKey::parse("geo@1.2.0/../lib.nz").is_none());
        assert!(ModuleKey::parse("Geo@1.2.0/lib.nz").is_none());
        // And no project path is keyed as either.
        assert!(ModuleKey::relative("geo@1.2.0/lib.nz").is_none());
    }

    #[test]
    fn a_definition_is_its_module_its_kind_and_its_name() {
        let m = ModuleKey::relative("lib/util.nz").unwrap();
        let a = DefKey::new(m.clone(), DefKind::Function, "parse");
        let b = DefKey::new(m.clone(), DefKind::Function, "parse");
        assert_eq!(a, b);
        assert_ne!(a, DefKey::new(m, DefKind::Function, "compute"));
        assert_ne!(
            a,
            DefKey::new(
                ModuleKey::relative("other.nz").unwrap(),
                DefKind::Function,
                "parse"
            )
        );
        assert_eq!(a.to_string(), "lib/util.nz::fn parse");
    }

    #[test]
    fn two_modules_may_define_one_name_and_the_keys_differ() {
        // The durable form of what N2 made true: owning your names means the same
        // spelling in two modules is two definitions.
        let a = DefKey::new(
            ModuleKey::relative("a.nz").unwrap(),
            DefKind::Function,
            "helper",
        );
        let b = DefKey::new(
            ModuleKey::relative("b.nz").unwrap(),
            DefKind::Function,
            "helper",
        );
        assert_ne!(a, b);
    }
}
