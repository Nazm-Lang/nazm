//! `@std/hash`'s functions, exactly (L6): a derived hash is defined in their terms
//! (`docs/user/spec.md`, *Ordering and hashing are derived*), and the interpreter and the native
//! lowering compute it without calling them — so these are the library's text in Rust, and
//! the native lowering emits the same steps.

/// FNV-1a's offset basis.
pub const OFFSET: u64 = 14_695_981_039_346_656_037;
/// FNV-1a's prime.
pub const PRIME: u64 = 1_099_511_628_211;
/// SplitMix64's first multiplier.
pub const MIX1: u64 = 0xBF58_476D_1CE4_E5B9;
/// SplitMix64's second multiplier.
pub const MIX2: u64 = 0x94D0_49BB_1331_11EB;

/// `hash_finish`: SplitMix64's finaliser.
#[must_use]
pub const fn finish(x: u64) -> u64 {
    let a = (x ^ (x >> 30)).wrapping_mul(MIX1);
    let b = (a ^ (a >> 27)).wrapping_mul(MIX2);
    b ^ (b >> 31)
}

/// `hash_bytes`.
#[must_use]
pub fn bytes(s: &[u8]) -> u64 {
    let mut h = OFFSET;
    for b in s {
        h = (h ^ u64::from(*b)).wrapping_mul(PRIME);
    }
    finish(h)
}

/// `hash_u64`: a word's bytes, least significant first.
#[must_use]
pub const fn word(x: u64) -> u64 {
    let mut h = OFFSET;
    let mut w = x;
    let mut i = 0;
    while i < 8 {
        h = (h ^ (w & 255)).wrapping_mul(PRIME);
        w >>= 8;
        i += 1;
    }
    finish(h)
}

/// `hash_combine`.
#[must_use]
pub const fn combine(a: u64, b: u64) -> u64 {
    finish(a.wrapping_mul(PRIME).wrapping_add(b))
}
