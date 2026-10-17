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

/// A mutual receipt hash binding both parties' randomness to a specific
/// transfer's content (`spec/31-service-receipts.md`): `blake3("OAG:RECEIPT:v1:"
/// || client_nonce || server_nonce || content_hash)`. Neither `client_nonce`
/// nor `server_nonce` is predictable to the other party in advance, so neither
/// side can produce a matching hash for content it didn't actually send or
/// receive without the other party's real-time participation — this is the
/// "neither side could have fabricated it alone" property, not just a content
/// digest. `content_hash` is the `"blake3:..."`/`"sha256:..."`-prefixed string
/// as produced by [`blake3_content_hash`]/[`sha256_content_hash`], hashed as
/// its own UTF-8 bytes (no decoding) so this stays agnostic to which digest
/// algorithm produced it.
pub fn mutual_receipt_hash(client_nonce: &[u8; 32], server_nonce: &[u8; 32], content_hash: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"OAG:RECEIPT:v1:");
    hasher.update(client_nonce);
    hasher.update(server_nonce);
    hasher.update(content_hash.as_bytes());
    format!("blake3:{}", hasher.finalize().to_hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_for_identical_inputs() {
        let client_nonce = [1u8; 32];
        let server_nonce = [2u8; 32];
        let content_hash = blake3_content_hash(b"hello");
        assert_eq!(
            mutual_receipt_hash(&client_nonce, &server_nonce, &content_hash),
            mutual_receipt_hash(&client_nonce, &server_nonce, &content_hash)
        );
    }

    #[test]
    fn differs_when_client_nonce_differs() {
        let server_nonce = [2u8; 32];
        let content_hash = blake3_content_hash(b"hello");
        let a = mutual_receipt_hash(&[1u8; 32], &server_nonce, &content_hash);
        let b = mutual_receipt_hash(&[9u8; 32], &server_nonce, &content_hash);
        assert_ne!(a, b, "neither party can predict the other's nonce in advance");
    }

    #[test]
    fn differs_when_server_nonce_differs() {
        let client_nonce = [1u8; 32];
        let content_hash = blake3_content_hash(b"hello");
        let a = mutual_receipt_hash(&client_nonce, &[2u8; 32], &content_hash);
        let b = mutual_receipt_hash(&client_nonce, &[9u8; 32], &content_hash);
        assert_ne!(a, b);
    }

    #[test]
    fn differs_when_content_differs() {
        let client_nonce = [1u8; 32];
        let server_nonce = [2u8; 32];
        let a = mutual_receipt_hash(&client_nonce, &server_nonce, &blake3_content_hash(b"hello"));
        let b = mutual_receipt_hash(&client_nonce, &server_nonce, &blake3_content_hash(b"goodbye"));
        assert_ne!(a, b, "a tampered transfer must not produce a matching receipt");
    }
}
