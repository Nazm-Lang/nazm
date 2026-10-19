//! A fast hasher for the compiler's own tables (N106, `docs/internal/architecture.md` §7.107).
//!
//! The front end's maps are keyed by spans and by short identifiers, built and dropped once per
//! check. `std`'s default, SipHash-1-3 with a per-process random key, defends a map against
//! inputs chosen to collide; a compiler's maps are filled from the program it is checking, whose
//! author gains nothing by slowing their own build. Measured on the compiler written in Nazm, the
//! default's hashing was about a tenth of a check. This is the multiply-rotate hash rustc's own
//! tables use (FxHash), deterministic, with no key — which removes no order dependence, because a
//! randomly keyed map has no iteration order anything could have relied on.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

/// The multiply-rotate hasher.
#[derive(Debug, Default, Clone, Copy)]
pub struct FastHasher(u64);

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl FastHasher {
    #[inline]
    const fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FastHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let word = chunk
                .iter()
                .rev()
                .fold(0_u64, |w, b| (w << 8) | u64::from(*b));
            self.add(word);
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

/// The state a [`FastMap`] builds its hashers from.
pub type FastState = BuildHasherDefault<FastHasher>;

/// A `HashMap` with [`FastHasher`].
pub type FastMap<K, V> = HashMap<K, V, FastState>;

/// A `HashSet` with [`FastHasher`].
pub type FastSet<T> = HashSet<T, FastState>;

#[cfg(test)]
mod tests {
    use super::{FastHasher, FastMap};
    use crate::{FileId, Span};
    use std::hash::{Hash, Hasher};

    fn hash(s: Span) -> u64 {
        let mut h = FastHasher::default();
        s.hash(&mut h);
        h.finish()
    }

    #[test]
    fn spans_that_differ_anywhere_hash_apart_and_equal_ones_together() {
        let a = Span::new(FileId::ROOT, 10, 20);
        assert_eq!(hash(a), hash(Span::new(FileId::ROOT, 10, 20)));
        assert_ne!(hash(a), hash(Span::new(FileId::ROOT, 10, 21)));
        assert_ne!(hash(a), hash(Span::new(FileId::ROOT, 11, 20)));
        assert_ne!(hash(a), hash(Span::new(FileId::ROOT, 20, 10)));
        // A map of a hundred thousand neighbouring spans keeps them all apart.
        let mut m: FastMap<Span, u32> = FastMap::default();
        for i in 0..100_000_u32 {
            m.insert(Span::new(FileId::ROOT, i, i + 1), i);
        }
        assert_eq!(m.len(), 100_000);
        assert_eq!(m.get(&Span::new(FileId::ROOT, 777, 778)), Some(&777));
    }

    #[test]
    fn a_string_hashes_by_every_byte() {
        let h = |s: &str| {
            let mut h = FastHasher::default();
            s.hash(&mut h);
            h.finish()
        };
        assert_ne!(h("parse_expr"), h("parse_exps"));
        assert_ne!(h("ab"), h("ba"));
        assert_eq!(h("main"), h("main"));
    }
}
