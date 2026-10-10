//! Stable content fingerprint used by project cleanup and wave context.

pub(crate) fn fingerprint(body: &str) -> u64 {
    body.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3))
}
