//! A fingerprint of the source a module's checking was performed on.

use std::fmt;

use crate::digest::{self, Framed};

/// The exact bytes of one module's source.
///
/// # Exact, and conservatively so
///
/// Every byte of the file as it was read, with **no normalisation at all**: no newline
/// translation, no whitespace folding, no comment stripping, no BOM removal, no Unicode
/// normalisation. A comment edit, a re-indent and a line-ending change each move this
/// fingerprint and each cause the module itself to be checked again.
///
/// That is the wrong trade for a cache that wanted the highest hit rate and the right one
/// for a cache that must never hit wrongly. Normalising means deciding that two byte
/// sequences check identically, and every such decision is a place the compiler and the
/// normaliser can come to disagree — after which a real change looks like a comment. The
/// cost of being wrong here is a stale accept; the cost of being conservative is re-doing
/// work that was cheap.
///
/// **And it costs nothing at the boundary that matters.** A module whose comment changed
/// is re-checked and publishes the same interface, so its `InterfaceHash` does not move
/// and none of its importers is affected. The conservatism stops at the module it
/// belongs to, which is `docs/internal/architecture.md` §7.4's whole point.
///
/// # What it is not
///
/// Not a semantic hash, not a body hash and not a definition hash. It says *these bytes*,
/// and nothing about what they mean; `docs/internal/research-register.md` R2 is the hypothesis
/// about a normalised form of a definition, and this is deliberately not a down payment
/// on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceFingerprint([u8; 32]);

impl SourceFingerprint {
    /// The algorithm, named in the artefact rather than assumed by the reader.
    pub const ALGORITHM: &'static str = "blake3-256";

    /// The fingerprint of these bytes.
    #[must_use]
    pub fn of(source: &[u8]) -> Self {
        Self(Framed::new("nazm.source/1").bytes(source).finish())
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

impl fmt::Display for SourceFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        digest::write_hex(f, &self.0)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_same_bytes_give_the_same_fingerprint() {
        let text = b"pub fn v() -> Int { 1 }\n";
        assert_eq!(SourceFingerprint::of(text), SourceFingerprint::of(text));
    }

    #[test]
    fn anything_at_all_moves_it() {
        // Stated as a test because the conservatism is a decision, not an accident: a
        // comment and a line ending are both source, and this is a fingerprint of source.
        let base = SourceFingerprint::of(b"pub fn v() -> Int { 1 }\n");
        for other in [
            &b"pub fn v() -> Int { 1 }\r\n"[..],
            &b"pub fn v() -> Int { 1 }\n// a note\n"[..],
            &b"pub  fn v() -> Int { 1 }\n"[..],
            &b"pub fn v() -> Int { 2 }\n"[..],
            &b""[..],
        ] {
            assert_ne!(base, SourceFingerprint::of(other));
        }
    }

    #[test]
    fn it_round_trips_through_its_written_form() {
        let f = SourceFingerprint::of(b"x");
        assert_eq!(SourceFingerprint::parse(&f.to_string()), Some(f));
        assert!(SourceFingerprint::parse("not a digest").is_none());
    }
}
