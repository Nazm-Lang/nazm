//! A fingerprint over a module's published interface.
//!
//! # What it is for
//!
//! One question: **must this module's dependents be re-checked?** A dependent is checked
//! against an interface and nothing else (`docs/spec.md`), so if the interface is the
//! same, the answer is no. That is the whole value of the thing, and it is why a body is
//! deliberately not an input — a module whose private helper changed publishes the same
//! interface and its dependents are unaffected.
//!
//! # What it is not for
//!
//! Deciding whether a module's **own** output can be reused. That needs the bodies, the
//! compiler's identity, the target and the flags, and `architecture.md` §7.3 calls it a
//! `BuildKey` precisely so this cannot be quietly promoted into one. An `InterfaceHash`
//! used as a codegen cache key would hand back an object file compiled from a body that
//! has since changed.
//!
//! # The algorithm, and the words used about it
//!
//! BLAKE3 over the interface's canonical bytes. It is a cryptographic digest, and what is
//! claimed here is **collision resistance in practice for a compiler**, not mathematical
//! uniqueness: two different interfaces producing one fingerprint is possible in the sense
//! that any 256-bit digest permits, and not in any sense a build has to plan for. A
//! non-cryptographic checksum would not do — the failure it invites is an invalid cache
//! hit, which is a miscompilation that comes with a plausible explanation.
//!
//! The schema is an input, not an assumption: the bytes hashed begin with
//! `nazm.interface/1`, so the same exported signatures under a future schema fingerprint
//! differently rather than colliding with what this one wrote.

use std::fmt;

use crate::wire::PersistentUnitInterface;

/// A digest of a module's published interface.
///
/// Rendered as lowercase hexadecimal, which is what a person compares and what a file can
/// hold. Constructed only from canonical bytes, so equal fingerprints mean the two
/// interfaces serialise identically — which, since serialisation is deterministic and
/// total over the interface, means they offer the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InterfaceHash([u8; 32]);

impl InterfaceHash {
    /// The fingerprint of these canonical bytes.
    #[must_use]
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(*blake3::hash(bytes).as_bytes())
    }

    /// The fingerprint of this interface.
    ///
    /// # Errors
    ///
    /// Only if the interface cannot be serialised, which for this shape it cannot fail to
    /// be.
    pub fn of(interface: &PersistentUnitInterface) -> Result<Self, serde_json::Error> {
        Ok(Self::of_bytes(interface.to_json()?.as_bytes()))
    }

    /// The algorithm, named in the artefact rather than assumed by the reader.
    pub const ALGORITHM: &'static str = "blake3-256";

    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Read one back from its hexadecimal form.
    ///
    /// Refuses anything that is not exactly 64 lowercase hexadecimal digits: a fingerprint
    /// is compared, and one that had been repaired from a near-miss would compare equal to
    /// nothing and unequal to everything.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() != 64 {
            return None;
        }
        let mut out = [0u8; 32];
        for (i, byte) in out.iter_mut().enumerate() {
            let pair = text.get(i * 2..i * 2 + 2)?;
            if pair
                .bytes()
                .any(|b| !b.is_ascii_digit() && !(b'a'..=b'f').contains(&b))
            {
                return None;
            }
            *byte = u8::from_str_radix(pair, 16).ok()?;
        }
        Some(Self(out))
    }
}

impl fmt::Display for InterfaceHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in &self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}
