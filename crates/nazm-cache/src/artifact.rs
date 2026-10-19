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
//! `docs/internal/architecture.md` §7.4 says why that is a later decision rather than a convenient
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
//! edited or mis-filed entry fails that and is a miss. `docs/internal/architecture.md` §7.4 keeps
//! the claim where it belongs: BLAKE3 is collision-resistant in practice for a compiler,
//! which is a reason to trust a match and not a reason to skip the check.

use serde::{Deserialize, Serialize};

use nazm_iface::{InterfaceHash, PersistentFacts, PersistentUnitInterface, ResolvedUnit};
use nazm_sema::key::ModuleKey;

use crate::identity::CheckerIdentity;
use crate::key::{CheckKey, Dependency};
use crate::source::SourceFingerprint;

/// The identifier every persisted check result carries.
///
/// `name/major`, the rule `docs/user/diagnostics.md` states and `nazm.interface/1` follows: the
/// major number changes when a reader written against the previous one would be **wrong**.
/// An unknown version is refused, never reinterpreted — a cache is the one place where
/// reinterpreting a future file's fields as the current ones fails silently and
/// successfully.
///
/// `/2` since N38: an entry also carries the module's provenance facts, which a run that
/// reuses the module needs for the whole program's summaries. A `/1` entry has none, so it
/// is refused and the module checked again — never read as a module with no flows.
///
/// `/3` since N77: an entry also carries the module's resolved unit (`nazm.resolved/1`),
/// which a reader of a reused module's names needs. A `/2` entry has none and is a miss.
///
/// `/4` since N80: the provenance facts gain container cells, indirect calls and the
/// function values a module makes. A `/3` entry has none, so it would read as a module that
/// writes no container and makes no value — wrong, not merely incomplete — and is a miss.
pub const SCHEMA: &str = "nazm.check/5";

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
    /// What checking the module decided about its names (N77): a function of exactly the
    /// inputs the key covers, so it is filed under the same key.
    pub resolved: Option<ResolvedUnit>,
    /// BLAKE3 over the provenance facts and the resolved unit, as written. The key and the
    /// interface are re-derived on read; these two cannot be, so a parseable edit to either —
    /// a sink list emptied — would otherwise be believed and turn a refusal into `ok`
    /// (Q1-M-07). Filled by [`Self::to_json`]; `/5` since Q1.
    #[serde(default)]
    pub facts_digest: String,
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
    /// The resolved unit is missing, unreadable, or of another module or source text.
    ForeignResolution,
    /// The provenance facts or the resolved unit differ from the digest filed beside them.
    FactsMoved,
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
            Self::ForeignResolution => {
                f.write_str("its resolved unit is not this module's, of this source")
            }
            Self::FactsMoved => f.write_str(
                "its provenance facts or resolved unit do not match the digest beside them",
            ),
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
    /// `None` when the reader did not ask for it ([`CheckArtifact::read_expecting`]).
    pub resolved: Option<ResolvedUnit>,
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
            resolved: None,
            facts_digest: String::new(),
        })
    }

    /// The same entry, carrying the module's resolved unit (N77).
    #[must_use]
    pub fn with_resolved(mut self, unit: ResolvedUnit) -> Self {
        self.resolved = Some(unit);
        self
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
        let mut sealed = self.clone();
        sealed.facts_digest = self.facts_of()?;
        serde_json::to_string(&sealed)
    }

    /// The digest [`Self::facts_digest`] records: the two payloads' canonical JSON.
    fn facts_of(&self) -> Result<String, serde_json::Error> {
        let unit = serde_json::to_string(&self.resolved)?;
        Ok(facts_digest(
            serde_json::to_string(&self.provenance)?.as_bytes(),
            unit.as_bytes(),
        ))
    }

    /// Read one back and establish that it is the entry that was asked for.
    ///
    /// # Errors
    ///
    /// [`Invalid`] for anything this build may not act on. Every variant is a miss.
    pub fn read(text: &str, wanted: &CheckKey) -> Result<Reusable, Invalid> {
        let parsed: Self = parse_entry(text)?;
        let (module, interface_hash) = head(
            &Head {
                checker: &parsed.checker,
                key: &parsed.key,
                module: &parsed.module,
                source: &parsed.source,
                dependencies: &parsed.dependencies,
                interface_hash: &parsed.interface_hash,
            },
            wanted,
            || reread(&parsed.interface),
        )?;
        // The unit, of this module's exact source: a unit of another module or another text
        // names ranges that mean nothing here. It was deserialised with the entry, so writing
        // it and reading it back through its own reader gives the same value; what that
        // reader adds is its schema check, made here directly.
        let unit_fits = parsed.resolved.as_ref().is_some_and(|u| {
            u.schema == nazm_iface::resolved::SCHEMA
                && u.module == parsed.module
                && u.source == parsed.source
        });
        if !unit_fits {
            return Err(Invalid::ForeignResolution);
        }
        if parsed.facts_of().ok().as_deref() != Some(parsed.facts_digest.as_str()) {
            return Err(Invalid::FactsMoved);
        }
        Ok(Reusable {
            module,
            interface: parsed.interface,
            interface_hash,
            provenance: parsed.provenance,
            resolved: parsed.resolved,
        })
    }

    /// As [`Self::read`], by a checker that holds the interface this build publishes for the
    /// module — `offered`, its canonical bytes `offered_json` and their fingerprint — and will
    /// reuse no entry offering anything else, and that never reads the resolved unit.
    ///
    /// Such an entry is read without building its interface or its unit: the interface's bytes
    /// as written must be `offered_json` (anything else is [`Invalid::InterfaceMoved`]), and
    /// the unit and the facts must have the digest recorded beside them over their bytes as
    /// written. This build writes all three canonically, so an entry it wrote reads the same
    /// way it would through [`Self::read`]; an entry spelling an equal value some other way is
    /// a miss, and its module is checked again. [`Reusable::resolved`] is `None`.
    ///
    /// # Errors
    ///
    /// As [`Self::read`].
    pub fn read_expecting(
        text: &str,
        wanted: &CheckKey,
        offered: &PersistentUnitInterface,
        offered_json: &str,
        fingerprint: InterfaceHash,
    ) -> Result<Reusable, Invalid> {
        let parsed: Unread<'_> = parse_entry(text)?;
        let (module, interface_hash) = head(
            &Head {
                checker: &parsed.checker,
                key: &parsed.key,
                module: &parsed.module,
                source: &parsed.source,
                dependencies: &parsed.dependencies,
                interface_hash: &parsed.interface_hash,
            },
            wanted,
            || {
                if parsed.interface.get() != offered_json {
                    return Err(Invalid::InterfaceMoved);
                }
                // The bytes are `offered`'s own, which this build's reader accepts exactly when
                // `offered.readable()` holds: they are canonical, and they read back as
                // `offered` (`an_interface_round_trips_through_its_own_bytes`). So an interface
                // that reader would refuse is still refused, and its module checked again.
                offered
                    .readable()
                    .map_err(|err| Invalid::BadInterface(err.to_string()))?;
                Ok((offered.module_key(), fingerprint))
            },
        )?;
        // The unit's head is checked as the full reader checks it, on the bytes the digest
        // covers: its schema, module and source are its first three members.
        let raw = parsed.resolved.ok_or(Invalid::ForeignResolution)?;
        let unit = raw.get();
        let quoted =
            |s: &str| serde_json::to_string(s).map_err(|e| Invalid::Malformed(e.to_string()));
        let unit_head = format!(
            "{{\"schema\":{},\"module\":{},\"source\":{},",
            quoted(nazm_iface::resolved::SCHEMA)?,
            quoted(&parsed.module)?,
            quoted(&parsed.source)?
        );
        if !unit.starts_with(&unit_head) {
            return Err(Invalid::ForeignResolution);
        }
        if facts_digest(parsed.provenance.get().as_bytes(), unit.as_bytes()) != parsed.facts_digest
        {
            return Err(Invalid::FactsMoved);
        }
        let provenance: PersistentFacts = serde_json::from_str(parsed.provenance.get())
            .map_err(|e| Invalid::Malformed(e.to_string()))?;
        Ok(Reusable {
            module,
            interface: offered.clone(),
            interface_hash,
            provenance,
            resolved: None,
        })
    }
}

/// BLAKE3 over the provenance facts' and the resolved unit's JSON: [`CheckArtifact::facts_digest`].
fn facts_digest(provenance: &[u8], unit: &[u8]) -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"nazm.check-facts/1\0");
    h.update(provenance);
    h.update(b"\0");
    h.update(unit);
    format!("blake3:{}", h.finalize().to_hex())
}

/// An entry as a checker reads it ([`CheckArtifact::read_expecting`]): its interface, its
/// provenance facts and its resolved unit left as their bytes, as written.
#[derive(Deserialize)]
struct Unread<'a> {
    schema: String,
    checker: String,
    key: String,
    module: String,
    source: String,
    dependencies: Vec<PersistentDependency>,
    #[serde(borrow)]
    interface: &'a serde_json::value::RawValue,
    interface_hash: String,
    #[serde(borrow)]
    provenance: &'a serde_json::value::RawValue,
    #[serde(borrow)]
    resolved: Option<&'a serde_json::value::RawValue>,
    #[serde(default)]
    facts_digest: String,
}

/// What every reading of an entry has a `schema` for.
trait Schema {
    fn schema(&self) -> &str;
}

impl Schema for CheckArtifact {
    fn schema(&self) -> &str {
        &self.schema
    }
}

impl Schema for Unread<'_> {
    fn schema(&self) -> &str {
        &self.schema
    }
}

/// An entry's bytes as `T`, of this build's schema.
///
/// The schema on its own and first: a future version may not have the shape this build's
/// `Deserialize` expects, and "failed to parse" would be the wrong thing to conclude about a
/// newer file. Parsed once on the way that succeeds (performance leadership, a warm check):
/// the envelope is read only when the whole does not parse, to tell a future schema from a
/// damaged entry — the same answer as reading it first, since a whole that parses has the
/// envelope's `schema`.
fn parse_entry<'a, T: Deserialize<'a> + Schema>(text: &'a str) -> Result<T, Invalid> {
    #[derive(Deserialize)]
    struct Envelope {
        schema: String,
    }
    match serde_json::from_str::<T>(text) {
        Ok(parsed) if parsed.schema() != SCHEMA => {
            Err(Invalid::UnknownSchema(parsed.schema().to_owned()))
        }
        Ok(parsed) => Ok(parsed),
        Err(whole) => {
            let envelope: Envelope =
                serde_json::from_str(text).map_err(|e| Invalid::Malformed(e.to_string()))?;
            if envelope.schema != SCHEMA {
                return Err(Invalid::UnknownSchema(envelope.schema));
            }
            Err(Invalid::Malformed(whole.to_string()))
        }
    }
}

/// The fields of an entry every reading validates the same way.
struct Head<'a> {
    checker: &'a str,
    key: &'a str,
    module: &'a str,
    source: &'a str,
    dependencies: &'a [PersistentDependency],
    interface_hash: &'a str,
}

/// Everything that must be true of an entry's inputs and interface beyond its bytes parsing:
/// its module and its interface's fingerprint. `interface` checks the entry's interface and
/// returns the module key it names and its fingerprint, in the place of the order of checks
/// where the interface is read.
fn head(
    e: &Head<'_>,
    wanted: &CheckKey,
    interface: impl FnOnce() -> Result<(Option<ModuleKey>, InterfaceHash), Invalid>,
) -> Result<(ModuleKey, InterfaceHash), Invalid> {
    let checker = CheckerIdentity::parse(e.checker).ok_or(Invalid::BadField("checker"))?;
    let filed = CheckKey::parse(e.key).ok_or(Invalid::BadField("key"))?;
    let module = ModuleKey::parse(e.module).ok_or(Invalid::BadField("module"))?;
    let source = SourceFingerprint::parse(e.source).ok_or(Invalid::BadField("source"))?;
    let interface_hash =
        InterfaceHash::parse(e.interface_hash).ok_or(Invalid::BadField("interface_hash"))?;

    let mut deps = Vec::with_capacity(e.dependencies.len());
    for d in e.dependencies {
        deps.push(Dependency {
            module: ModuleKey::parse(&d.module).ok_or(Invalid::BadField("dependency"))?,
            interface: InterfaceHash::parse(&d.interface).ok_or(Invalid::BadField("dependency"))?,
        });
    }
    if deps.windows(2).any(|w| w.first() >= w.last()) {
        return Err(Invalid::NotCanonical);
    }

    let (named, fingerprint) = interface()?;
    if named.as_ref() != Some(&module) {
        return Err(Invalid::ForeignInterface);
    }
    if fingerprint != interface_hash {
        return Err(Invalid::InterfaceMoved);
    }

    // The inputs must produce the key the entry claims, and that key must be the one asked
    // for. Two separate failures: the first says the file disagrees with itself, the second
    // that it is somebody else's entry.
    if CheckKey::of(checker, &module, source, &deps) != filed {
        return Err(Invalid::KeyDisagrees);
    }
    if filed != *wanted {
        return Err(Invalid::WrongKey);
    }
    Ok((module, interface_hash))
}

/// An entry's interface read through the reader that owns the format rather than trusted
/// because it deserialised — an entry is a file, and a file is data: the module it names and
/// its fingerprint.
fn reread(
    interface: &PersistentUnitInterface,
) -> Result<(Option<ModuleKey>, InterfaceHash), Invalid> {
    let bytes = interface
        .to_json()
        .map_err(|err| Invalid::BadInterface(err.to_string()))?;
    PersistentUnitInterface::from_json(&bytes)
        .map_err(|err| Invalid::BadInterface(err.to_string()))?;
    // `from_json` has just required `bytes` to be the interface's own canonical form, so they
    // are what `InterfaceHash::of` would serialise it to again.
    Ok((
        interface.module_key(),
        InterfaceHash::of_bytes(bytes.as_bytes()),
    ))
}
