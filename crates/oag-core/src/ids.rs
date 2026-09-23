use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A 32-byte BLAKE3 digest, domain-separated at construction time.
///
/// Every OAG identifier (`NodeId`, `EdgeId`, `EventId`, `ActorId`) is one of
/// these under the hood. Domain separation happens once, in [`Hash32::derive`]
/// and friends — nothing else in the codebase should call `blake3::hash`
/// directly on identifier material.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hash32([u8; 32]);

impl Hash32 {
    /// Hash `"{domain_prefix}{input}"` with BLAKE3. `domain_prefix` should be
    /// one of the `OAG:<KIND>:v1:` constants so different object classes never
    /// share a hash space (spec section 13).
    pub fn derive(domain_prefix: &str, input: &[u8]) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(domain_prefix.as_bytes());
        hasher.update(input);
        Self(*hasher.finalize().as_bytes())
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Debug for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash32({})", self.to_hex())
    }
}

impl fmt::Display for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Hash32ParseError {
    #[error("invalid hex encoding: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("expected 32 bytes, got {0}")]
    WrongLength(usize),
}

impl FromStr for Hash32 {
    type Err = Hash32ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(s)?;
        let len = bytes.len();
        let array: [u8; 32] = bytes
            .try_into()
            .map_err(|_| Hash32ParseError::WrongLength(len))?;
        Ok(Self(array))
    }
}

/// Macro to define a newtype identifier wrapping [`Hash32`] with consistent
/// Display/Debug/Serde/FromStr behavior, and a domain-separation prefix.
macro_rules! define_id {
    ($name:ident, $prefix:expr) => {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Hash32);

        impl $name {
            pub const DOMAIN_PREFIX: &'static str = $prefix;

            pub fn derive(input: &[u8]) -> Self {
                Self(Hash32::derive(Self::DOMAIN_PREFIX, input))
            }

            pub fn from_hash(hash: Hash32) -> Self {
                Self(hash)
            }

            pub fn as_hash(&self) -> Hash32 {
                self.0
            }

            pub fn to_hex(&self) -> String {
                self.0.to_hex()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0.to_hex())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0.to_hex())
            }
        }

        impl FromStr for $name {
            type Err = Hash32ParseError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(Hash32::from_str(s)?))
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0.to_hex())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let s = String::deserialize(deserializer)?;
                Self::from_str(&s).map_err(serde::de::Error::custom)
            }
        }
    };
}

define_id!(NodeId, "OAG:NODE:v1:");
define_id!(EdgeId, "OAG:EDGE:v1:");
define_id!(EventId, "OAG:EVENT:v1:");
define_id!(ActorId, "OAG:ACTOR:v1:");

/// An `Assertion`'s ID is always the ID of the `ASSERT_RELATION` event that
/// created it (spec section 21) — this is a type alias, not a new hash space.
pub type AssertionId = EventId;

impl NodeId {
    /// Derive a Node ID from its canonical identifier string, e.g.
    /// `"url:https://www.rust-lang.org/"` or `"uuid:<uuidv7>"` for entities
    /// with no stable canonical identity (spec sections 14-15).
    pub fn from_canonical_identifier(identifier: &str) -> Self {
        Self::derive(identifier.as_bytes())
    }
}

impl EdgeId {
    /// Derive an Edge ID from its (subject, predicate, object) triple (spec
    /// section 18). All peers computing this triple get the same ID.
    pub fn from_triple(subject: NodeId, predicate: &str, object: NodeId) -> Self {
        let mut buf = Vec::with_capacity(64 + predicate.len() + 2);
        buf.extend_from_slice(subject.to_hex().as_bytes());
        buf.push(b':');
        buf.extend_from_slice(predicate.as_bytes());
        buf.push(b':');
        buf.extend_from_slice(object.to_hex().as_bytes());
        Self::derive(&buf)
    }
}
