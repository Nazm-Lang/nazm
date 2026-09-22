//! What a definition is called in the object file.
//!
//! # The problem this replaces
//!
//! Until N5 a function's symbol was `@nz.f<N>`, where `N` was its position in one
//! whole-program numbering. That is fine for one LLVM module and impossible for two: the
//! number depends on how many functions were collected before it, so module `b.nz`'s
//! `twice` is `@nz.f3` in one program and `@nz.f7` in another, and a separately generated
//! object cannot name it at all.
//!
//! # The basis is the durable identity, not a new one
//!
//! N3 settled what makes a definition the same definition across two builds:
//! [`DefKey`] — a module's durable key, a kind, and a declared name. That is exactly the
//! question a symbol has to answer, so a symbol is a **spelling** of it and nothing more:
//!
//! ```text
//! DefKey  (nazm-sema)   which definition is this
//!     ↓ this module
//! NativeSymbol (nazm-lir)   what the linker calls it
//! ```
//!
//! The direction matters. `DefKey` knows nothing about LLVM, so a second backend
//! consumes the same identity and spells it its own way; `docs/architecture.md` keeps
//! that boundary because the alternative — a semantic identity carrying an LLVM string —
//! is how a backend's spelling becomes a language's problem.
//!
//! # The encoding, and why it cannot collide
//!
//! A `ModuleKey` is a path and a name is an identifier, and neither is safe in an object
//! symbol as written. Each is encoded byte by byte:
//!
//! * `A`–`Z`, `a`–`z`, `0`–`9` stand for themselves;
//! * **everything else, including `_`, becomes `_XX`** with two uppercase hexadecimal
//!   digits.
//!
//! Because `_` is itself escaped it never appears literally, which makes the encoding
//! injective: every encoded string decodes to exactly one input. And `.` is never
//! produced, so it is free to separate the fields. A symbol is therefore **unique by
//! construction** rather than by a collision-resistance argument — there is no digest,
//! no truncation and no probability to reason about. That is the strongest guarantee
//! available and it is available because the inputs are small.
//!
//! ```text
//! @nz.m.lib_2Futil_2Enz.parse      lib/util.nz :: parse
//! @nz.m.a_2Enz.helper              a.nz :: helper
//! @nz.m.b_2Enz.helper              b.nz :: helper — a different symbol, by construction
//! ```
//!
//! # Why the kind is not in it, when it *is* in `DefKey`
//!
//! N3 put a kind in `DefKey` because a durable identity that did not say what it
//! identified would have to be **reinterpreted** when a second kind arrived, and
//! reinterpreting a durable identity is the failure that layer exists to prevent.
//!
//! A native symbol is the opposite kind of thing: it is regenerated from source on every
//! build, nothing reads one back, and nothing is compatible with one. Extending the
//! scheme when a second kind exists costs a recompile, which happens anyway. Carrying a
//! constant `f` today would be structure with no responsibility, which this project's
//! own ordering rule forbids. The two decisions differ because the two lifetimes do.
//!
//! # A module with no durable identity
//!
//! `docs/spec.md` refuses a `ModuleKey` to a module outside the source root, and to one a
//! symbolic link gave two names. Such a program still compiles — N4's rule, that a limit
//! on reuse may never become a refusal to compile — so its definitions still need
//! symbols. They get a **compilation-local** form instead, discriminated by its own
//! field so it cannot collide with a durable one:
//!
//! ```text
//! @nz.u.2.helper                   the third module loaded, which has no key
//! ```
//!
//! Unique within the program and deterministic within a compilation, because load order
//! is; **not** stable across compilations, because nothing about that module is. Saying
//! so is the honest version of giving it a durable-looking name.

use nazm_sema::key::DefKey;
use nazm_sema::ModuleId;

/// What the linker calls one definition. Compiler-private.
///
/// # Not an ABI
///
/// Nothing outside one build of this compiler may depend on this string. It is not a C
/// symbol, not a shared-library export and not a stable interface; `pub` in Nazm source
/// says which *Nazm modules* may name a definition and makes no promise about a binary.
/// `docs/spec.md` says so in the language's own terms, and every symbol here is emitted
/// with hidden or internal linkage so that the final image exports none of them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeSymbol(String);

impl NativeSymbol {
    /// The symbol for a definition that has a durable identity.
    #[must_use]
    pub fn of_definition(def: &DefKey) -> Self {
        Self(format!(
            "nz.m.{}.{}",
            encode(def.module().as_str()),
            encode(def.name())
        ))
    }

    /// The symbol for a definition in a module that has no durable identity.
    ///
    /// `module` is its position in this compilation's load order, which is deterministic
    /// within one run and means nothing outside it — exactly like the module it names.
    #[must_use]
    pub fn of_unkeyed(module: ModuleId, name: &str) -> Self {
        Self(format!("nz.u.{}.{}", module.index(), encode(name)))
    }

    /// The name itself, with no sigil.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// How a symbol is written where LLVM wants a global value.
impl std::fmt::Display for NativeSymbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "@{}", self.0)
    }
}

/// What a record's LLVM named type is called. Compiler-private.
///
/// # Derived from the durable identity, like a symbol
///
/// Two independently generated objects must agree on a record's layout (`docs/spec.md`,
/// *Separate code generation*), so they must first agree on its **name** — and the only
/// thing about a record that means the same in two compilations is its [`DefKey`]. A name
/// derived from a session `RecordId`, a declaration order or a load index would differ
/// between the two objects and LLVM would silently treat them as two types.
///
/// The encoding is [`encode`], exactly as for a symbol, so the same argument applies: `_`
/// is escaped, so nothing else can produce a `_`, so the encoding is injective and two
/// records cannot collide by construction. The field is `t` where a symbol's is `m`,
/// which is what keeps a record and a function of one name apart — they are in separate
/// namespaces in the language and must be in separate ones here.
///
/// ```text
/// %"nz.t.model_2Enz.Point"      model.nz :: record Point
/// %"nz.t.a_2Enz.Thing"          a.nz :: record Thing
/// %"nz.t.b_2Enz.Thing"          b.nz :: record Thing — a different type, by construction
/// ```
///
/// # Not an ABI
///
/// Nothing outside one build depends on this. `docs/spec.md` is explicit that `pub` on a
/// record says which Nazm modules may name it and promises nothing about a layout, a field
/// offset or a binary interface.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeTypeName(String);

impl NativeTypeName {
    /// The name for a record that has a durable identity.
    #[must_use]
    pub fn of_definition(def: &DefKey) -> Self {
        Self(format!(
            "nz.t.{}.{}",
            encode(def.module().as_str()),
            encode(def.name())
        ))
    }

    /// The name for a record in a module that has no durable identity.
    ///
    /// Load-order based, deterministic within one compilation and meaningless outside it —
    /// exactly like the module it names, and like [`NativeSymbol::of_unkeyed`].
    #[must_use]
    pub fn of_unkeyed(module: ModuleId, name: &str) -> Self {
        Self(format!("nz.tu.{}.{}", module.index(), encode(name)))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Quoted, because the encoding contains characters LLVM does not accept bare.
impl std::fmt::Display for NativeTypeName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "%\"{}\"", self.0)
    }
}

/// How much of the program may bind to a symbol.
///
/// Derived from the language's own visibility rule and nothing else, which is what makes
/// it a per-module decision: `docs/spec.md` says a private definition is nameable only
/// inside its module, so nothing in another object can possibly refer to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Linkage {
    /// Not in the object's symbol table. For a definition no other object can name.
    ///
    /// Worth keeping rather than making everything bindable: LLVM may inline, specialise
    /// and delete an internal function, and cannot do any of that to one an unknown
    /// caller might reach.
    Internal,
    /// Bindable by the linker, and **not exported from the final image**.
    ///
    /// For a definition another object names: an exported one, and the root module's
    /// entry, which the platform wrapper calls from a unit of its own.
    Hidden,
}

impl Linkage {
    /// The LLVM keyword, with its trailing space, for a definition line.
    #[must_use]
    pub const fn llvm(self) -> &'static str {
        match self {
            Self::Internal => "internal ",
            Self::Hidden => "hidden ",
        }
    }
}

/// One byte of a path or an identifier, made safe for an object symbol.
///
/// See the module documentation for why `_` is escaped along with everything else.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() {
            out.push(char::from(b));
        } else {
            out.push('_');
            out.push_str(&format!("{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use nazm_sema::key::{DefKind, ModuleKey};

    fn def(module: &str, name: &str) -> DefKey {
        DefKey::new(
            ModuleKey::relative(module).expect("a module key"),
            DefKind::Function,
            name,
        )
    }

    fn sym(module: &str, name: &str) -> String {
        NativeSymbol::of_definition(&def(module, name))
            .as_str()
            .to_owned()
    }

    #[test]
    fn a_symbol_is_the_module_and_the_name_and_nothing_else() {
        assert_eq!(sym("a.nz", "helper"), "nz.m.a_2Enz.helper");
        assert_eq!(sym("lib/util.nz", "parse"), "nz.m.lib_2Futil_2Enz.parse");
    }

    #[test]
    fn two_modules_may_define_one_name() {
        // The miscompile this replaces: before N2 both of these were `@nz.f0`, and clang
        // rejected the module as an invalid redefinition. The symbols differ because the
        // identities do.
        assert_ne!(sym("a.nz", "helper"), sym("b.nz", "helper"));
        assert_ne!(sym("lib/a.nz", "helper"), sym("lib/b.nz", "helper"));
    }

    #[test]
    fn the_same_definition_always_gets_the_same_symbol() {
        // Nothing about where a checkout sits, what order modules loaded in, or which
        // file the compilation began at reaches this. It is a function of the `DefKey`.
        assert_eq!(sym("lib/util.nz", "parse"), sym("lib/util.nz", "parse"));
    }

    #[test]
    fn a_rename_is_a_different_symbol_and_a_move_is_not() {
        assert_ne!(sym("a.nz", "parse"), sym("a.nz", "parse2"));
        // A body edit cannot reach this function: it takes no body. That is the property
        // stated as a test can state it — the input does not exist.
        assert_eq!(sym("a.nz", "parse"), sym("a.nz", "parse"));
    }

    #[test]
    fn the_encoding_cannot_make_two_inputs_one_symbol() {
        // The property that replaces a collision-resistance argument. `_` is escaped, so
        // it never appears literally, so no encoded string is the encoding of two inputs
        // — and `.` is never produced, so the fields cannot run together.
        let cases = [
            ("a_b.nz", "f"),
            ("a", "b.nz.f"),
            ("a.nz", "_b"),
            ("a.nz", "b"),
            ("a/b.nz", "c"),
            ("a.b.nz", "c"),
        ];
        let mut seen = std::collections::HashSet::new();
        for (m, n) in cases {
            let s = NativeSymbol::of_definition(&DefKey::new(
                ModuleKey::relative(m).expect("a key"),
                DefKind::Function,
                n,
            ));
            assert!(seen.insert(s.as_str().to_owned()), "{m}::{n} collided");
        }
        assert_eq!(encode("_"), "_5F");
        assert!(!encode("a/b.nz").contains('.'));
    }

    #[test]
    fn a_symbol_is_safe_to_write_into_an_object() {
        // Letters, digits and the two separators, and nothing else — no quoting rule to
        // get wrong, on any platform.
        for (m, n) in [("lib/a b.nz", "x"), ("çé.nz", "ñ")] {
            let s = NativeSymbol::of_definition(&DefKey::new(
                ModuleKey::relative(m).expect("a key"),
                DefKind::Function,
                n,
            ));
            assert!(
                s.as_str()
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_'),
                "{}",
                s.as_str()
            );
        }
    }

    #[test]
    fn a_module_with_no_durable_identity_gets_a_local_symbol() {
        // Unique and deterministic within the compilation; not durable, because nothing
        // about that module is. It cannot collide with a keyed one: the second field is
        // `u` rather than `m`.
        // Ids come from a graph, because that is the only thing entitled to mint one:
        // a `ModuleId` is an index into one, and conjuring one here would be the
        // session-local-identity confusion N3 exists to prevent, in a test.
        let mut graph = nazm_sema::module::ModuleGraph::default();
        let first = graph.add(nazm_span::FileId::ROOT, None);
        let second = graph.add(nazm_span::FileId::from_index(1), None);
        let a = NativeSymbol::of_unkeyed(first, "helper");
        let b = NativeSymbol::of_unkeyed(second, "helper");
        assert_eq!(a.as_str(), "nz.u.0.helper");
        assert_ne!(a, b);
        assert_ne!(a.as_str(), sym("a.nz", "helper"));
    }

    #[test]
    fn a_symbol_is_written_with_a_sigil_and_stored_without_one() {
        let s = NativeSymbol::of_definition(&def("a.nz", "f"));
        assert_eq!(s.to_string(), "@nz.m.a_2Enz.f");
        assert_eq!(s.as_str(), "nz.m.a_2Enz.f");
    }
}
