//! What a completed semantic check leaves behind: `nazm.check/1`.
//!
//! # What is in it, and what is not
//!
//! A module's **published interface**, and enough of the inputs to prove the entry is the
//! one that was asked for. Nothing else: no resolution, no tree, no slot assignment, no
//! diagnostic, no body.
//!
//! `Resolution` in particular is deliberately absent. It is the checker's answer to every
//! name mention in every body, it is addressed by session-local `FnId`s and slot numbers,
//! and it has no stable written form. Persisting it would mean designing one — and
//! `docs/architecture.md` §7.4 says why that is a later decision rather than a convenient
//! one: a cached interface is already enough for a dependent to be checked without its
//! dependency's body, which is the reuse this milestone is for.
//!
//! # Only clean checks are written
//!
//! An entry means *this module checked, and offers this*. A module that produced a
//! diagnostic produces no entry, so no entry can ever be a cached refusal, and a cache
//! hit can never suppress an error. The refusal is recomputed every run, from source,
//! which is the only authority there is.
//!
//! # It carries its own inputs
//!
//! The file name is the key, which is a digest; a digest match is evidence and not proof.
//! So the artefact repeats the inputs it was keyed on, and [`CheckArtifact::validate`]
//! recomputes the key from them and requires it to be the one asked for. A truncated,
//! edited or mis-filed entry fails that and is a miss. `docs/architecture.md` §7.4 keeps
//! the claim where it belongs: BLAKE3 is collision-resistant in practice for a compiler,
//! which is a reason to trust a match and not a reason to skip the check.

use serde::{Deserialize, Serialize};

use nazm_iface::{InterfaceHash, PersistentFacts, PersistentUnitInterface};
use nazm_sema::key::ModuleKey;

use crate::identity::CheckerIdentity;
use crate::key::{CheckKey, Dependency};
use crate::source::SourceFingerprint;

/// The identifier every persisted check result carries.
///
/// `name/major`, the rule `docs/diagnostics.md` states and `nazm.interface/1` follows: the
/// major number changes when a reader written against the previous one would be **wrong**.
/// An unknown version is refused, never reinterpreted — a cache is the one place where
/// reinterpreting a future file's fields as the current ones fails silently and
/// successfully.
///
/// `/2` since N38: an entry also carries the module's provenance facts, which a run that
/// reuses the module needs for the whole program's summaries. A `/1` entry has none, so it
/// is refused and the module checked again — never read as a module with no flows.
pub const SCHEMA: &str = "nazm.check/2";

/// One dependency, as the artefact records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentDependency {
    /// Its durable key.
    pub module: String,
    /// The fingerprint of what it offered when this module was checked against it.
    pub interface: String,
}

/// A module that checked cleanly, and the inputs that were true when it did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckArtifact {
    /// The shape these bytes are in. First field, so a reader can refuse before the rest
    /// means anything.
    pub schema: String,
    /// Which checker produced it. An entry from another is unreachable, not merely
    /// distrusted: the identity is an input to the key it is filed under.
    pub checker: String,
    /// The key it is filed under, repeated so that a match can be verified rather than
    /// assumed.
    pub key: String,
    /// Which module this is.
    pub module: String,
    /// The fingerprint of the source that was checked.
    pub source: String,
    /// What it was checked against, sorted by module key.
    pub dependencies: Vec<PersistentDependency>,
    /// What it offers. The reason the entry is worth having.
    pub interface: PersistentUnitInterface,
    /// The fingerprint of that interface, so a dependent can compute its own key from
    /// this entry without re-deriving one.
    pub interface_hash: String,
    /// Its bodies reduced to provenance flows (N38): a function of exactly the inputs above.
    pub provenance: PersistentFacts,
}

/// Why a stored check result could not be used.
///
/// Every one of these is a **miss**: the module is checked from source, which is the only
/// authority. None is a compilation error, because a cache is derived data and a damaged
/// one may not change what a program means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalid {
    /// Not JSON, or not this shape — including a truncated file.
    Malformed(String),
    /// A schema this build does not implement.
    UnknownSchema(String),
    /// A key, fingerprint or identity that is not in the canonical written form.
    BadField(&'static str),
    /// The interface inside it is not a valid `nazm.interface/1` interface.
    BadInterface(String),
    /// The interface is not the one the recorded fingerprint describes.
    InterfaceMoved,
    /// The interface belongs to a different module than the entry claims.
    ForeignInterface,
    /// The dependencies are not sorted, or name one module twice.
    NotCanonical,
    /// The inputs recorded in the entry do not produce the key it is filed under. A
    /// truncated write, an edited file, or an entry moved to the wrong name.
    KeyDisagrees,
    /// It is a valid entry, for something else.
    WrongKey,
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(e) => write!(f, "not a `{SCHEMA}` entry: {e}"),
            Self::UnknownSchema(s) => write!(f, "entry schema `{s}` is not `{SCHEMA}`"),
            Self::BadField(w) => write!(f, "`{w}` is not in canonical form"),
            Self::BadInterface(e) => write!(f, "the interface it carries is not valid: {e}"),
            Self::InterfaceMoved => {
                f.write_str("the interface does not match the fingerprint beside it")
            }
            Self::ForeignInterface => f.write_str("the interface belongs to another module"),
            Self::NotCanonical => f.write_str("its dependencies are not in canonical order"),
            Self::KeyDisagrees => {
                f.write_str("its recorded inputs do not produce the key it is filed under")
            }
            Self::WrongKey => f.write_str("it is an entry for something else"),
        }
    }
}

impl std::error::Error for Invalid {}

/// What a validated entry offers its reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reusable {
    pub module: ModuleKey,
    pub interface: PersistentUnitInterface,
    pub interface_hash: InterfaceHash,
    pub provenance: PersistentFacts,
}

impl CheckArtifact {
    /// Record a module that checked cleanly.
    ///
    /// # Errors
    ///
    /// Only if the interface cannot be serialised, which for its shape it cannot fail to
    /// be.
    pub fn of(
        checker: CheckerIdentity,
        module: &ModuleKey,
        source: SourceFingerprint,
        dependencies: &[Dependency],
        interface: PersistentUnitInterface,
    ) -> Result<Self, serde_json::Error> {
        let mut deps = dependencies.to_vec();
        // The same canonicalisation the key performs, so that the entry records the
        // dependency set the key was computed over rather than the order a caller
        // happened to pass.
        deps.sort();
        deps.dedup();
        let key = CheckKey::of(checker, module, source, &deps);
        let interface_hash = InterfaceHash::of(&interface)?;
        Ok(Self {
            schema: SCHEMA.to_owned(),
            checker: checker.to_string(),
            key: key.to_string(),
            module: module.as_str().to_owned(),
            source: source.to_string(),
            dependencies: deps
                .into_iter()
                .map(|d| PersistentDependency {
                    module: d.module.as_str().to_owned(),
                    interface: d.interface.to_string(),
                })
                .collect(),
            interface,
            interface_hash: interface_hash.to_string(),
            provenance: PersistentFacts::default(),
        })
    }

    /// The same entry, carrying `facts` as the module's provenance facts.
    #[must_use]
    pub fn with_provenance(mut self, facts: PersistentFacts) -> Self {
        self.provenance = facts;
        self
    }

    /// The canonical bytes of this entry.
    ///
    /// # Errors
    ///
    /// Only if serialisation itself fails, which for this shape it does not.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Read one back and establish that it is the entry that was asked for.
    ///
    /// # Errors
    ///
    /// [`Invalid`] for anything this build may not act on. Every variant is a miss.
    pub fn read(text: &str, wanted: &CheckKey) -> Result<Reusable, Invalid> {
        // The schema on its own and first: a future version may not have the shape this
        // build's `Deserialize` expects, and "failed to parse" would be the wrong thing to
        // conclude about a newer file.
        #[derive(Deserialize)]
        struct Envelope {
            schema: String,
        }
        let envelope: Envelope =
            serde_json::from_str(text).map_err(|e| Invalid::Malformed(e.to_string()))?;
        if envelope.schema != SCHEMA {
            return Err(Invalid::UnknownSchema(envelope.schema));
        }

        let parsed: Self =
            serde_json::from_str(text).map_err(|e| Invalid::Malformed(e.to_string()))?;
        parsed.validate(wanted)
    }

    /// Everything that must be true of an entry beyond its bytes parsing.
    fn validate(&self, wanted: &CheckKey) -> Result<Reusable, Invalid> {
        let checker = CheckerIdentity::parse(&self.checker).ok_or(Invalid::BadField("checker"))?;
        let filed = CheckKey::parse(&self.key).ok_or(Invalid::BadField("key"))?;
        let module = ModuleKey::parse(&self.module).ok_or(Invalid::BadField("module"))?;
        let source = SourceFingerprint::parse(&self.source).ok_or(Invalid::BadField("source"))?;
        let interface_hash = InterfaceHash::parse(&self.interface_hash)
            .ok_or(Invalid::BadField("interface_hash"))?;

        let mut deps = Vec::with_capacity(self.dependencies.len());
        for d in &self.dependencies {
            deps.push(Dependency {
                module: ModuleKey::parse(&d.module).ok_or(Invalid::BadField("dependency"))?,
                interface: InterfaceHash::parse(&d.interface)
                    .ok_or(Invalid::BadField("dependency"))?,
            });
        }
        if deps.windows(2).any(|w| w.first() >= w.last()) {
            return Err(Invalid::NotCanonical);
        }

        // The interface is revalidated through the reader that owns the format rather
        // than trusted because it deserialised. An entry is a file, and a file is data.
        let bytes = self
            .interface
            .to_json()
            .map_err(|e| Invalid::BadInterface(e.to_string()))?;
        let interface = PersistentUnitInterface::from_json(&bytes)
            .map_err(|e| Invalid::BadInterface(e.to_string()))?;
        if interface.module_key().as_ref() != Some(&module) {
            return Err(Invalid::ForeignInterface);
        }
        if InterfaceHash::of(&interface).map_err(|e| Invalid::BadInterface(e.to_string()))?
            != interface_hash
        {
            return Err(Invalid::InterfaceMoved);
        }

        // The inputs must produce the key the entry claims, and that key must be the one
        // asked for. Two separate failures: the first says the file disagrees with
        // itself, the second that it is somebody else's entry.
        if CheckKey::of(checker, &module, source, &deps) != filed {
            return Err(Invalid::KeyDisagrees);
        }
        if filed != *wanted {
            return Err(Invalid::WrongKey);
        }

        Ok(Reusable {
            module,
            interface,
            interface_hash,
            provenance: self.provenance.clone(),
        })
    }
}
