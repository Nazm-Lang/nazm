//! Unambiguous framing for everything this crate hashes.
//!
//! # Why a framing at all
//!
//! A key computed by concatenating its inputs is a key two different input sets can
//! share. `("ab", "c")` and `("a", "bc")` are different dependency lists and the same
//! bytes, and a cache that cannot tell them apart hands one module's result to another.
//! That is a miscompilation, and it is the sort that reproduces.
//!
//! So every piece is written **length-prefixed**: `<n>:<bytes>` where `n` is the decimal
//! byte length. Nothing needs escaping, no value is forbidden, and there is exactly one
//! encoding of any sequence of fields. A file name containing a newline, an identifier
//! containing a colon — neither can make two different keys collide, because the length
//! is read before the bytes are.
//!
//! # And a domain
//!
//! Each digest starts with the name of what it is a digest **of**. A source fingerprint
//! and a check key computed over the same bytes are then different numbers, so one can
//! never be compared against, or mistaken for, the other.

use std::fmt;

/// A digest under construction: a domain, then a sequence of length-prefixed fields.
///
/// Small fields are gathered and given to the hasher together: BLAKE3 hashes the
/// concatenation of what it is given however it is divided, so this is the same digest as
/// giving it every length, colon and value apart — which, for an identity of a thousand short
/// fields, was most of the work of computing it (performance leadership, `nazm check`).
#[derive(Debug)]
pub(crate) struct Framed {
    hasher: blake3::Hasher,
    pending: Vec<u8>,
}

/// Fields up to this long are gathered; longer ones go to the hasher directly.
const GATHER: usize = 4096;

impl Framed {
    /// Begin a digest of `domain`.
    pub(crate) fn new(domain: &str) -> Self {
        let mut f = Self {
            hasher: blake3::Hasher::new(),
            pending: Vec::with_capacity(GATHER),
        };
        f.push(domain.as_bytes());
        f
    }

    /// Add one field.
    pub(crate) fn field(&mut self, value: &str) -> &mut Self {
        self.push(value.as_bytes());
        self
    }

    /// Add one field of raw bytes — for source text, which is not required to be
    /// anything in particular.
    pub(crate) fn bytes(&mut self, value: &[u8]) -> &mut Self {
        self.push(value);
        self
    }

    /// Add a number, in its decimal spelling.
    pub(crate) fn number(&mut self, value: u32) -> &mut Self {
        self.field(&value.to_string())
    }

    pub(crate) fn finish(&self) -> [u8; 32] {
        let mut h = self.hasher.clone();
        h.update(&self.pending);
        *h.finalize().as_bytes()
    }

    /// `<n>:<bytes>`, `n` the decimal byte length.
    fn push(&mut self, value: &[u8]) {
        use std::io::Write as _;
        let _ = write!(self.pending, "{}:", value.len());
        if value.len() <= GATHER {
            self.pending.extend_from_slice(value);
            if self.pending.len() >= GATHER {
                self.hasher.update(&self.pending);
                self.pending.clear();
            }
        } else {
            self.hasher.update(&self.pending);
            self.pending.clear();
            self.hasher.update(value);
        }
    }
}

/// Read a digest back from its hexadecimal form.
///
/// Refuses anything that is not exactly 64 lowercase hexadecimal digits. A digest is
/// compared; one repaired from a near-miss would compare equal to nothing and unequal to
/// everything, which is worse than refusing it.
pub(crate) fn from_hex(text: &str) -> Option<[u8; 32]> {
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
    Some(out)
}

/// Rendered as lowercase hexadecimal wherever one of these appears. It is what a person
/// compares and what a file name can hold.
pub(crate) fn write_hex(f: &mut fmt::Formatter<'_>, bytes: &[u8; 32]) -> fmt::Result {
    for b in bytes {
        write!(f, "{b:02x}")?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// The `Display` every digest in this crate uses, reachable from a test.
    struct Hex<'a>(&'a [u8; 32]);

    impl fmt::Display for Hex<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write_hex(f, self.0)
        }
    }

    #[test]
    fn gathering_fields_is_the_digest_of_their_framing() {
        // The framing written out by hand, given to BLAKE3 in one piece, against `Framed`'s
        // gathering — short fields, one longer than it gathers, and a run that fills a batch.
        let long = "x".repeat(GATHER + 7);
        let many: Vec<String> = (0..3000).map(|i| i.to_string()).collect();
        let mut fields: Vec<&str> = vec!["a", "", "bc", &long, "d"];
        fields.extend(many.iter().map(String::as_str));
        let mut framed = Framed::new("domain");
        let mut by_hand = b"6:domain".to_vec();
        for x in &fields {
            framed.field(x);
            by_hand.extend_from_slice(format!("{}:{x}", x.len()).as_bytes());
        }
        framed.bytes(b"\x00\xff").number(42);
        by_hand.extend_from_slice(b"2:\x00\xff2:42");
        assert_eq!(framed.finish(), *blake3::hash(&by_hand).as_bytes());
    }

    fn digest(domain: &str, fields: &[&str]) -> [u8; 32] {
        let mut f = Framed::new(domain);
        for x in fields {
            f.field(x);
        }
        f.finish()
    }

    #[test]
    fn the_same_fields_give_the_same_digest() {
        assert_eq!(digest("d", &["a", "b"]), digest("d", &["a", "b"]));
    }

    #[test]
    fn a_field_boundary_cannot_be_moved() {
        // The property the framing exists for. Without it these are one byte string and
        // one digest, and two different dependency lists share a cache entry.
        assert_ne!(digest("d", &["ab", "c"]), digest("d", &["a", "bc"]));
        assert_ne!(digest("d", &["", "abc"]), digest("d", &["abc", ""]));
        assert_ne!(digest("d", &["a"]), digest("d", &["a", ""]));
    }

    #[test]
    fn a_separator_inside_a_value_is_just_a_value() {
        // Nothing is escaped and nothing has to be, because the length is read first.
        assert_ne!(digest("d", &["3:abc"]), digest("d", &["abc"]));
        assert_ne!(digest("d", &["a:b"]), digest("d", &["a", "b"]));
    }

    #[test]
    fn the_domain_keeps_two_kinds_of_digest_apart() {
        assert_ne!(digest("one", &["x"]), digest("two", &["x"]));
    }

    #[test]
    fn hexadecimal_round_trips_and_refuses_anything_else() {
        let d = digest("d", &["x"]);
        let text = format!("{}", Hex(&d));
        assert_eq!(text.len(), 64);
        assert_eq!(from_hex(&text), Some(d));
        assert!(from_hex(&text[..63]).is_none());
        assert!(from_hex(&text.to_uppercase()).is_none());
        assert!(from_hex(&format!("{}g", &text[..63])).is_none());
    }
}
