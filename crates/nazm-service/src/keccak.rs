//! Keccak-256 (N70): the hash the EVM's ABI selectors and this backend's storage slots are derived
//! from — the original Keccak padding (`0x01`), not SHA-3's (`0x06`). Small, dependency-free, and
//! held to known vectors by its tests.

const ROUNDS: [u64; 24] = [
    0x0000_0000_0000_0001,
    0x0000_0000_0000_8082,
    0x8000_0000_0000_808a,
    0x8000_0000_8000_8000,
    0x0000_0000_0000_808b,
    0x0000_0000_8000_0001,
    0x8000_0000_8000_8081,
    0x8000_0000_0000_8009,
    0x0000_0000_0000_008a,
    0x0000_0000_0000_0088,
    0x0000_0000_8000_8009,
    0x0000_0000_8000_000a,
    0x0000_0000_8000_808b,
    0x8000_0000_0000_008b,
    0x8000_0000_0000_8089,
    0x8000_0000_0000_8003,
    0x8000_0000_0000_8002,
    0x8000_0000_0000_0080,
    0x0000_0000_0000_800a,
    0x8000_0000_8000_000a,
    0x8000_0000_8000_8081,
    0x8000_0000_0000_8080,
    0x0000_0000_8000_0001,
    0x8000_0000_8000_8008,
];

const ROTATIONS: [u32; 25] = [
    0, 1, 62, 28, 27, 36, 44, 6, 55, 20, 3, 10, 43, 25, 39, 41, 45, 15, 21, 8, 18, 2, 61, 56, 14,
];

#[allow(clippy::indexing_slicing, reason = "x and y are below 5, so every lane index is below 25")]
fn permute(a: &mut [u64; 25]) {
    for rc in ROUNDS {
        // θ
        let mut c = [0u64; 5];
        for (x, cx) in c.iter_mut().enumerate() {
            *cx = a[x] ^ a[x + 5] ^ a[x + 10] ^ a[x + 15] ^ a[x + 20];
        }
        for x in 0..5 {
            let d = c[(x + 4) % 5] ^ c[(x + 1) % 5].rotate_left(1);
            for y in 0..5 {
                a[x + 5 * y] ^= d;
            }
        }
        // ρ and π
        let mut b = [0u64; 25];
        for x in 0..5 {
            for y in 0..5 {
                b[y + 5 * ((2 * x + 3 * y) % 5)] = a[x + 5 * y].rotate_left(ROTATIONS[x + 5 * y]);
            }
        }
        // χ
        for x in 0..5 {
            for y in 0..5 {
                a[x + 5 * y] = b[x + 5 * y] ^ (!b[(x + 1) % 5 + 5 * y] & b[(x + 2) % 5 + 5 * y]);
            }
        }
        // ι
        a[0] ^= rc;
    }
}

/// `keccak256(data)`.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    reason = "every index is below a fixed array's length"
)]
pub fn keccak256(data: &[u8]) -> [u8; 32] {
    const RATE: usize = 136;
    let mut state = [0u64; 25];
    let mut padded = data.to_vec();
    padded.push(0x01);
    while padded.len() % RATE != 0 {
        padded.push(0);
    }
    if let Some(last) = padded.last_mut() {
        *last |= 0x80;
    }
    for block in padded.chunks(RATE) {
        for (i, lane) in block.chunks(8).enumerate() {
            let mut w = [0u8; 8];
            w.copy_from_slice(lane);
            state[i] ^= u64::from_le_bytes(w);
        }
        permute(&mut state);
    }
    let mut out = [0u8; 32];
    for (i, lane) in state.iter().take(4).enumerate() {
        out[i * 8..i * 8 + 8].copy_from_slice(&lane.to_le_bytes());
    }
    out
}

/// Lower-case hexadecimal.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keccak256_matches_known_vectors() {
        assert_eq!(
            hex(&keccak256(b"")),
            "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"
        );
        assert_eq!(
            hex(&keccak256(b"abc")),
            "4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45"
        );
        // The ERC-20 `transfer` selector, the most quoted four bytes in the EVM.
        assert_eq!(
            hex(&keccak256(b"transfer(address,uint256)")[..4]),
            "a9059cbb"
        );
        // Longer than one 136-byte block: the reference is `eth_hash`'s, in `nazm-evm:n70`.
        assert_eq!(
            hex(&keccak256(&[b'a'; 200])),
            "96ea54061def936c4be90b518992fdc6f12f535068a256229aca54267b4d084d"
        );
    }
}
