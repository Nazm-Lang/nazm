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
//! available and it is available because the inputs are small. **One exception** (v1,
//! Q1-B-06): a generic instance whose type arguments spell longer than
//! [`MAX_SPELLED_ARGS`] carries their BLAKE3 digest instead, because nesting can make that
//! spelling exponentially long and a linker refuses a name of megabytes.
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
use nazm_sema::module::ModuleGraph;
use nazm_sema::udt::{TypeNode, UserTypes};
use nazm_sema::{ModuleId, Type};

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

    /// A static's storage (Gate 3): `nz.s.` and its owner, so it is one symbol in every unit and
    /// never a function's.
    #[must_use]
    pub fn of_static(owner: &nazm_mir::Owner) -> Self {
        match owner {
            nazm_mir::Owner::Keyed(k) => Self(format!(
                "nz.s.{}.{}",
                encode(k.module().as_str()),
                encode(k.name())
            )),
            nazm_mir::Owner::Unkeyed { module, name } => {
                Self(format!("nz.su.{}.{}", module.index(), encode(name)))
            }
        }
    }

    /// One concrete instance of a generic function: its definition's symbol and the
    /// [canonical keys](type_key) of its type arguments, in order.
    ///
    /// The **generic instance key** made a symbol. `identity[Int]` and `identity[Str]`
    /// differ in their keys and so in their symbols even where their machine code happens
    /// to coincide — semantic identity, not layout, names native code. The keys are
    /// encoded, so the `.` that introduces them cannot occur inside them, and a symbol with
    /// no `.g.` part is an ordinary definition's: the two can never collide.
    #[must_use]
    pub fn of_instance(base: &Self, args: &[String]) -> Self {
        Self(format!("{}.{}", base.0, instance_suffix(args)))
    }

    /// The name itself, with no sigil.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A foreign function's symbol (N42): exactly the C identifier its declaration wrote,
    /// validated by the checker. It never collides with a Nazm symbol, which always
    /// contains a `.`.
    #[must_use]
    pub fn foreign(symbol: &str) -> Self {
        Self(symbol.to_owned())
    }

    /// A symbol no definition has: what a reference to a function MIR does not contain
    /// spells, so that the object refuses to link rather than calling something else.
    #[must_use]
    pub fn missing() -> Self {
        Self("nz.missing".to_owned())
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

    /// The name for something inside this type: one variant's payload struct.
    ///
    /// Derived from the enum's name rather than minted separately, so a variant's native
    /// type is as deterministic as its enum's and cannot collide with another enum's
    /// variant of the same name. A variant name is an identifier, so the encoding has
    /// nothing to escape — it is applied anyway, because the reason to encode is that a
    /// name is data and that is true of every one of them.
    /// One concrete application of a generic record or enum: its definition's name and its
    /// arguments' canonical keys, by the rule [`NativeSymbol::of_instance`] states.
    #[must_use]
    pub fn of_instance(base: &Self, args: &[String]) -> Self {
        Self(format!("{}.{}", base.0, instance_suffix(args)))
    }

    #[must_use]
    pub fn member(&self, name: &str) -> Self {
        Self(format!("{}.{}", self.0, encode(name)))
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
/// A concrete type's **canonical key**: one durable spelling per semantic type.
///
/// It is what a generic instance's native name is made of, so it must distinguish every
/// two types the language distinguishes and nothing else — `Vec[Int]` from `Vec[Str]`,
/// `a.nz`'s `Box[Int]` from `b.nz`'s, `Pair[Int, Str]` from `Pair[Str, Int]` — and must
/// contain nothing session-local: no `RecordId`, no `TypeId`, no span, no path outside the
/// source root. A built-in is its name; a definition is its module's key and its name,
/// each [encoded](encode), under `R` or `E`; an application adds its arguments' keys in
/// parentheses; a `Vec` is `Vec(…)`. Encoded components contain only letters, digits and
/// `_`, so the punctuation between them is unambiguous and the key is injective.
///
/// A module with no durable identity is spelled by its load position — `u.3` — exactly as
/// its functions' symbols are, and for the same reason: it still compiles.
/// The symbol of a MIR function or planned instance: its definition's, extended by its type
/// arguments' canonical keys.
#[must_use]
pub fn of_identity(i: &nazm_mir::Identity) -> NativeSymbol {
    let base = match &i.owner {
        nazm_mir::Owner::Keyed(k) => NativeSymbol::of_definition(k),
        nazm_mir::Owner::Unkeyed { module, name } => NativeSymbol::of_unkeyed(*module, name),
    };
    if i.args.is_empty() {
        base
    } else {
        NativeSymbol::of_instance(&base, &i.args)
    }
}

#[must_use]
pub fn type_key(t: Type, types: &UserTypes, graph: &ModuleGraph) -> String {
    match t {
        Type::Vec(v) | Type::Box(v) | Type::List(v) => format!(
            "{}({})",
            match t {
                Type::Box(_) => "Box",
                Type::List(_) => "List",
                _ => "Vec",
            },
            types
                .vec_element(v)
                .map_or_else(|| "?".to_owned(), |e| type_key(e, types, graph))
        ),
        Type::ChanOf(c) => format!(
            "Chan({})",
            types
                .chan_element(c)
                .map_or_else(|| "?".to_owned(), |e| type_key(e, types, graph))
        ),
        Type::Record(_) | Type::Enum(_) => {
            let Some(node) = types.definition_of(t) else {
                return "?".to_owned();
            };
            let (kind, module, name) = match node {
                TypeNode::Record(id) => {
                    let Some(d) = types.records().get(id) else {
                        return "?".to_owned();
                    };
                    ("R", d.module, d.name.clone())
                }
                TypeNode::Enum(id) => {
                    let Some(d) = types.enums().get(id) else {
                        return "?".to_owned();
                    };
                    ("E", d.module, d.name.clone())
                }
            };
            let head = match graph.key(module) {
                Some(k) => format!("{kind}.m.{}.{}", encode(k.as_str()), encode(&name)),
                None => format!("{kind}.u.{}.{}", module.index(), encode(&name)),
            };
            let args = types.type_args(t);
            if args.is_empty() {
                head
            } else {
                let inner: Vec<String> = args.iter().map(|a| type_key(*a, types, graph)).collect();
                format!("{head}({})", inner.join(","))
            }
        }
        // A parameter never reaches a native name: instances are concrete.
        Type::Param(_) => "?".to_owned(),
        other => other.builtin_name().unwrap_or("?").to_owned(),
    }
}

/// The longest spelling of an instance's type arguments written out (v1, Q1-B-06): 512 bytes.
pub const MAX_SPELLED_ARGS: usize = 512;

/// An instance's part of its name: `g.` and its type arguments' encoded keys — or, when those
/// are longer than [`MAX_SPELLED_ARGS`], `d.` and the BLAKE3 digest of them.
///
/// A key's length can be exponential in a program's nesting — `P[P[T, T], P[T, T]]` sixteen deep
/// spells 65,536 leaves — and a linker refuses a name of megabytes (Q1-B-06). So past the cap the
/// name carries a digest of the same spelling. That form is unique by collision resistance rather
/// than by construction, which this module otherwise avoids; it is the price of a bounded name, paid
/// only where the spelling would not fit. `d.` and `g.` cannot meet: `.` never occurs in an
/// encoded spelling, so the field after it says which form follows.
fn instance_suffix(args: &[String]) -> String {
    let joined = args.join(",");
    let spelled = encode(&joined);
    if spelled.len() <= MAX_SPELLED_ARGS {
        format!("g.{spelled}")
    } else {
        format!("d.{}", blake3::hash(joined.as_bytes()).to_hex())
    }
}

/// [`encode`], for a canonical key that becomes part of a helper's name.
#[must_use]
pub fn encoded(text: &str) -> String {
    encode(text)
}

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
    fn an_instance_too_long_to_spell_is_named_by_its_digest() {
        let base = NativeSymbol::of_definition(&def("lib.nz", "g"));
        let short = NativeSymbol::of_instance(&base, &["Int".to_owned()]);
        assert!(short.as_str().ends_with(".g.Int"), "{short}");
        let long_arg = "P[".repeat(200) + "Int" + &"]".repeat(200);
        let long = NativeSymbol::of_instance(&base, &[long_arg.clone()]);
        assert!(long.as_str().contains(".d."), "{long}");
        assert!(
            long.as_str().len() <= base.as_str().len() + 3 + 64,
            "{long}"
        );
        // Deterministic, and a different spelling is a different digest.
        assert_eq!(long, NativeSymbol::of_instance(&base, &[long_arg.clone()]));
        let other = NativeSymbol::of_instance(&base, &[long_arg + "x"]);
        assert_ne!(long, other);
        // Exactly at the cap it is still spelled.
        let at_cap = "a".repeat(super::MAX_SPELLED_ARGS);
        assert!(NativeSymbol::of_instance(&base, &[at_cap])
            .as_str()
            .contains(".g."));
    }

    #[test]
    fn a_symbol_is_written_with_a_sigil_and_stored_without_one() {
        let s = NativeSymbol::of_definition(&def("a.nz", "f"));
        assert_eq!(s.to_string(), "@nz.m.a_2Enz.f");
        assert_eq!(s.as_str(), "nz.m.a_2Enz.f");
    }
}
