//! Session-mode rules shared by every cipher (SPEC §3 "Session mode").

use hkdf::Hkdf;
use poly_core::envelope::FLAG_SESSION;
use poly_core::payload::{HKDF_INFO_SESSION_KEY, SESSION_KEY_LEN};
use poly_core::{PolyError, SessionKey};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::slots::SharedSecret;

/// Where a message's slot keys come from.
pub(crate) enum Keying<'a> {
    /// Full message: a fresh S from the KEM block; it starts a new session.
    Full,
    /// Session message: S is replaced by this session key.
    Session(&'a SessionKey),
}

/// Check a request's flags and session key (SPEC §3 "Session mode", request rules).
pub(crate) fn keying(flags: u8, session_key: Option<&SessionKey>) -> Result<Keying<'_>, PolyError> {
    if flags & !FLAG_SESSION != 0 {
        return Err(PolyError::InvalidRequest("unknown envelope flag bits"));
    }
    match (flags & FLAG_SESSION != 0, session_key) {
        (false, None) => Ok(Keying::Full),
        (true, Some(key)) => Ok(Keying::Session(key)),
        _ => Err(PolyError::InvalidRequest(
            "the SESSION flag and a session key must be given together",
        )),
    }
}

/// `session_key = HKDF-SHA256(ikm = S, salt = none, info = "polyphonia/v1/session-key")`.
pub(crate) fn derive_session_key(shared: &SharedSecret) -> Result<SessionKey, PolyError> {
    let mut key = Zeroizing::new([0u8; SESSION_KEY_LEN]);
    Hkdf::<Sha256>::new(None, &shared[..])
        .expand(HKDF_INFO_SESSION_KEY, &mut key[..])
        .map_err(|_| PolyError::Crypto("HKDF expand failed".into()))?;
    Ok(SessionKey::from_bytes(*key))
}

/// The session key in the role of S for the slot key derivation.
pub(crate) fn session_secret(key: &SessionKey) -> SharedSecret {
    Zeroizing::new(*key.as_bytes())
}

/// A session message's payload must start with the id of the key used to open it.
pub(crate) fn check_session_id(payload_id: &[u8], key: &SessionKey) -> Result<(), PolyError> {
    if payload_id == key.id() {
        Ok(())
    } else {
        Err(PolyError::DecryptFailed)
    }
}
