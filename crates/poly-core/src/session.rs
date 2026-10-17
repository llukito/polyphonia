//! Session keys (SPEC §3 "Session mode").
//!
//! A full message starts a one-directional session: both sides derive the
//! same 32-byte session key from its shared secret S, and later session
//! messages carry the key's 8-byte id instead of the KEM block.

use hkdf::Hkdf;
use sha2::Sha256;

use crate::PolyError;
use crate::payload::{HKDF_INFO_SESSION_ID, SESSION_ID_LEN, SESSION_KEY_LEN};

/// Identifies a session in a session message's payload (first 8 bytes).
pub type SessionId = [u8; SESSION_ID_LEN];

/// A 32-byte session key. Storing it is the app's job (Keychain).
///
/// `Debug` never prints the bytes, so the key can't leak into logs or test output.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionKey([u8; SESSION_KEY_LEN]);

impl SessionKey {
    /// Wrap 32 key bytes.
    pub fn from_bytes(bytes: [u8; SESSION_KEY_LEN]) -> Self {
        SessionKey(bytes)
    }

    /// The raw key bytes, e.g. to store them.
    pub fn as_bytes(&self) -> &[u8; SESSION_KEY_LEN] {
        &self.0
    }

    /// This key's session id, see [`session_id`].
    pub fn id(&self) -> SessionId {
        session_id(self)
    }
}

impl TryFrom<&[u8]> for SessionKey {
    type Error = PolyError;

    fn try_from(bytes: &[u8]) -> Result<Self, PolyError> {
        bytes
            .try_into()
            .map(SessionKey)
            .map_err(|_| PolyError::InvalidKey("session key must be 32 bytes"))
    }
}

impl std::fmt::Debug for SessionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionKey(<redacted>)")
    }
}

/// `session_id = HKDF-SHA256(ikm = session_key, salt = none, info = "polyphonia/v1/session-id")[..8]`.
pub fn session_id(key: &SessionKey) -> SessionId {
    let mut id = [0u8; SESSION_ID_LEN];
    Hkdf::<Sha256>::new(None, key.as_bytes())
        .expand(HKDF_INFO_SESSION_ID, &mut id)
        .expect("8 bytes is a valid HKDF-SHA256 output length");
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_the_key() {
        let key = SessionKey::from_bytes([0xAB; SESSION_KEY_LEN]);
        let s = format!("{key:?}");
        assert!(
            !s.contains("171") && !s.to_lowercase().contains("ab,"),
            "{s}"
        );
        assert!(s.contains("redacted"));
    }

    #[test]
    fn try_from_checks_length() {
        assert!(SessionKey::try_from(&[0u8; SESSION_KEY_LEN][..]).is_ok());
        assert!(matches!(
            SessionKey::try_from(&[0u8; SESSION_KEY_LEN - 1][..]),
            Err(PolyError::InvalidKey(_))
        ));
    }

    #[test]
    fn id_is_deterministic_and_key_dependent() {
        let a = SessionKey::from_bytes([1; SESSION_KEY_LEN]);
        let b = SessionKey::from_bytes([2; SESSION_KEY_LEN]);
        assert_eq!(a.id(), session_id(&a.clone()));
        assert_ne!(a.id(), b.id());
    }
}
