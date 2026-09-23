use std::fmt;
use std::str::FromStr;

use data_encoding::BASE32_NOPAD;
use ed25519_dalek::VerifyingKey;

/// A peer's public identity: `BLAKE3(public_key)`, human-encoded as lowercase
/// Base32 with an `oagp_` prefix (spec section 12).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PeerId([u8; 32]);

#[derive(Debug, thiserror::Error)]
pub enum PeerIdParseError {
    #[error("missing 'oagp_' prefix")]
    MissingPrefix,
    #[error("invalid base32 encoding: {0}")]
    Base32(#[from] data_encoding::DecodeError),
    #[error("expected 32 bytes, got {0}")]
    WrongLength(usize),
}

impl PeerId {
    pub fn from_public_key(key: &VerifyingKey) -> Self {
        Self(*blake3::hash(&key.to_bytes()).as_bytes())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "oagp_{}", BASE32_NOPAD.encode(&self.0).to_lowercase())
    }
}

impl fmt::Debug for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PeerId({})", self)
    }
}

impl FromStr for PeerId {
    type Err = PeerIdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let rest = s.strip_prefix("oagp_").ok_or(PeerIdParseError::MissingPrefix)?;
        let bytes = BASE32_NOPAD.decode(rest.to_uppercase().as_bytes())?;
        let len = bytes.len();
        let array: [u8; 32] = bytes
            .try_into()
            .map_err(|_| PeerIdParseError::WrongLength(len))?;
        Ok(Self(array))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    #[test]
    fn round_trips_through_display() {
        let signing_key = SigningKey::generate(&mut rand::rng());
        let peer_id = PeerId::from_public_key(&signing_key.verifying_key());
        let rendered = peer_id.to_string();
        assert!(rendered.starts_with("oagp_"));
        let parsed: PeerId = rendered.parse().unwrap();
        assert_eq!(peer_id, parsed);
    }
}
