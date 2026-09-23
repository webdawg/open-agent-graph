use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

/// Sign `domain_prefix || canonical_bytes` (spec sections 13, 27-28). Domain
/// separation happens here so the same canonical bytes hashed/signed under a
/// different object class never collide.
pub fn sign_with_domain(signing_key: &SigningKey, domain_prefix: &str, canonical_bytes: &[u8]) -> Signature {
    let mut message = Vec::with_capacity(domain_prefix.len() + canonical_bytes.len());
    message.extend_from_slice(domain_prefix.as_bytes());
    message.extend_from_slice(canonical_bytes);
    signing_key.sign(&message)
}

/// Verify a signature produced by [`sign_with_domain`].
pub fn verify_with_domain(
    verifying_key: &VerifyingKey,
    domain_prefix: &str,
    canonical_bytes: &[u8],
    signature: &Signature,
) -> Result<(), ed25519_dalek::SignatureError> {
    let mut message = Vec::with_capacity(domain_prefix.len() + canonical_bytes.len());
    message.extend_from_slice(domain_prefix.as_bytes());
    message.extend_from_slice(canonical_bytes);
    verifying_key.verify(&message, signature)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_round_trip() {
        let signing_key = SigningKey::generate(&mut rand::rng());
        let verifying_key = signing_key.verifying_key();
        let sig = sign_with_domain(&signing_key, "OAG:EVENT:v1:", b"hello");
        assert!(verify_with_domain(&verifying_key, "OAG:EVENT:v1:", b"hello", &sig).is_ok());
    }

    #[test]
    fn tampered_bytes_fail_verification() {
        let signing_key = SigningKey::generate(&mut rand::rng());
        let verifying_key = signing_key.verifying_key();
        let sig = sign_with_domain(&signing_key, "OAG:EVENT:v1:", b"hello");
        assert!(verify_with_domain(&verifying_key, "OAG:EVENT:v1:", b"goodbye", &sig).is_err());
    }

    #[test]
    fn wrong_domain_fails_verification() {
        let signing_key = SigningKey::generate(&mut rand::rng());
        let verifying_key = signing_key.verifying_key();
        let sig = sign_with_domain(&signing_key, "OAG:EVENT:v1:", b"hello");
        assert!(verify_with_domain(&verifying_key, "OAG:NODE:v1:", b"hello", &sig).is_err());
    }
}
