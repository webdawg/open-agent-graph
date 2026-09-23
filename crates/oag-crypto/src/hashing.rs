use sha2::{Digest, Sha256};

/// BLAKE3 digest of `bytes`, prefixed `"blake3:"` (spec section 24: BLAKE3 is
/// preferred internally for content hashing).
pub fn blake3_content_hash(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}

/// SHA-256 digest of `bytes`, prefixed `"sha256:"`. Spec section 24: MAY
/// additionally be stored when interchange with external systems benefits
/// from it.
pub fn sha256_content_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("sha256:{}", hex::encode(digest))
}
