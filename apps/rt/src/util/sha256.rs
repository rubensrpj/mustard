//! A dependency-free SHA-256 implementation.
//!
//! `sync-detect.js` used Node's `crypto.createHash('sha256')` to fingerprint a
//! subproject's source tree. The `mustard-rt` crate carries no crypto
//! dependency, so the algorithm (FIPS 180-4) is implemented here directly. It
//! is used only for change detection — a content fingerprint, not a security
//! primitive — but the digest is the genuine SHA-256 so a hash computed by the
//! JS script and by this port over the same bytes is identical.

/// SHA-256 round constants — the first 32 bits of the fractional parts of the
/// cube roots of the first 64 primes.
const K: [u32; 64] = [
    0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
    0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
    0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
    0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
    0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
    0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
    0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
    0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
    0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
    0xc671_78f2,
];

/// A streaming SHA-256 hasher — `update` repeatedly, then `hex_digest` once.
pub struct Sha256 {
    state: [u32; 8],
    buffer: Vec<u8>,
    total_len: u64,
}

impl Sha256 {
    /// Create a fresh hasher with the FIPS 180-4 initial hash values.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: [
                0x6a09_e667,
                0xbb67_ae85,
                0x3c6e_f372,
                0xa54f_f53a,
                0x510e_527f,
                0x9b05_688c,
                0x1f83_d9ab,
                0x5be0_cd19,
            ],
            buffer: Vec::with_capacity(64),
            total_len: 0,
        }
    }

    /// Feed `data` into the hash.
    pub fn update(&mut self, data: &[u8]) {
        self.total_len = self.total_len.wrapping_add(data.len() as u64);
        self.buffer.extend_from_slice(data);
        // Process every complete 64-byte block.
        let mut offset = 0;
        while self.buffer.len() - offset >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&self.buffer[offset..offset + 64]);
            self.process_block(&block);
            offset += 64;
        }
        if offset > 0 {
            self.buffer.drain(..offset);
        }
    }

    /// Finish the hash and return the lowercase hex digest (64 chars).
    #[must_use]
    pub fn hex_digest(mut self) -> String {
        let bit_len = self.total_len.wrapping_mul(8);
        // Pad: a 0x80 byte, then zeros, then the 64-bit big-endian length.
        self.buffer.push(0x80);
        while self.buffer.len() % 64 != 56 {
            self.buffer.push(0);
        }
        self.buffer.extend_from_slice(&bit_len.to_be_bytes());
        // Process the remaining (now block-aligned) buffer.
        let blocks = self.buffer.clone();
        for chunk in blocks.chunks_exact(64) {
            let mut block = [0u8; 64];
            block.copy_from_slice(chunk);
            self.process_block(&block);
        }
        let mut out = String::with_capacity(64);
        for word in self.state {
            out.push_str(&format!("{word:08x}"));
        }
        out
    }

    /// Apply the SHA-256 compression function to one 64-byte block.
    fn process_block(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 =
                w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 =
                w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        let deltas = [a, b, c, d, e, f, g, h];
        for (s, delta) in self.state.iter_mut().zip(deltas) {
            *s = s.wrapping_add(delta);
        }
    }
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compute the digest of `input` in one shot.
    fn digest(input: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(input);
        h.hex_digest()
    }

    #[test]
    fn empty_input_matches_known_vector() {
        assert_eq!(
            digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn abc_matches_known_vector() {
        assert_eq!(
            digest(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn long_input_matches_known_vector() {
        // The FIPS 180-4 two-block test vector.
        let input = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(
            digest(input),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn streaming_matches_one_shot() {
        let mut streamed = Sha256::new();
        streamed.update(b"abcdbcdecdefdefg");
        streamed.update(b"efghfghighijhijk");
        streamed.update(b"ijkljklmklmnlmnomnopnopq");
        assert_eq!(
            streamed.hex_digest(),
            digest(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")
        );
    }
}
