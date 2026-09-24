use std::fs;
use std::io;
use std::path::Path;

use ed25519_dalek::{SigningKey, VerifyingKey};

use crate::peer_id::PeerId;

/// A peer's persistent Ed25519 identity (spec section 12). Generated on
/// first startup and stored at `identity.key` in the data directory. The
/// private key never leaves the node except through an explicit
/// `identity backup`.
#[derive(Clone)]
pub struct PeerIdentity {
    signing_key: SigningKey,
}

impl PeerIdentity {
    /// Load the identity from `path`, or generate and persist a new one if
    /// the file doesn't exist yet.
    pub fn load_or_generate(path: &Path) -> io::Result<Self> {
        if path.exists() {
            Self::load(path)
        } else {
            let identity = Self::generate();
            identity.save(path)?;
            Ok(identity)
        }
    }

    pub fn generate() -> Self {
        Self {
            signing_key: SigningKey::generate(&mut rand::rng()),
        }
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let bytes = fs::read(path)?;
        let array: [u8; 32] = bytes.try_into().map_err(|bytes: Vec<u8>| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("identity.key must be 32 bytes, got {}", bytes.len()),
            )
        })?;
        Ok(Self {
            signing_key: SigningKey::from_bytes(&array),
        })
    }

    /// Persist the private key to `path` with owner-only permissions.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let bytes = self.signing_key.to_bytes();
        fs::write(path, bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    pub fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    pub fn peer_id(&self) -> PeerId {
        PeerId::from_public_key(&self.verifying_key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_then_reload_yields_same_peer_id() {
        let dir = tempdir();
        let path = dir.join("identity.key");
        let identity = PeerIdentity::load_or_generate(&path).unwrap();
        let peer_id = identity.peer_id();

        let reloaded = PeerIdentity::load_or_generate(&path).unwrap();
        assert_eq!(peer_id, reloaded.peer_id());
    }

    fn tempdir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("oag-crypto-test-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn uuid_like() -> u128 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    }
}
