use rand::Rng;

/// 32 bytes of OS-backed randomness, e.g. for generating a raw API key
/// before hashing it for storage.
pub fn random_bytes_32() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes
}
