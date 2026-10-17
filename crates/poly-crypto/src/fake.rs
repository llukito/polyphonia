//! FakeCipher: NOT SECURE. NO REAL ENCRYPTION. FOR TESTING ONLY.
//!
//! Produces payloads with the exact SPEC layout so every other crate can be
//! built and tested before the real ciphers exist:
//!
//! - kem_block: empty (Fake has a 0-byte KEM block).
//! - nonce: 12 zero bytes.
//! - "ciphertext": the padded plaintext, in the clear.
//! - slot A "tag": first 16 bytes of SHA-256(domain || public key || aad || nonce || ct),
//!   so tampering and wrong keys are detected like with AES-GCM.
//! - slot B "tag" (hidden mode): first 16 bytes of SHA-256(passphrase), so the
//!   passphrase logic behaves like the real thing.
//! - slot B in normal mode: random bytes.
//!
//! Keys: secret = 32 random bytes, public = SHA-256(domain || secret).
//!
//! Sessions: a full message's session key = SHA-256(domain || public key);
//! session messages carry the session id and use the same slots as above.

use poly_core::payload::{
    Payload, SLOT_BLOCK, TAG_LEN, padded_len, slot_len, slot_pad, slot_unpad, split_slot,
};
use poly_core::{
    Algorithm, Cipher, Identity, OpenRequest, Opened, PolyError, SealRequest, Sealed, SessionKey,
    Slot,
};
use rand::Rng;
use sha2::{Digest, Sha256};

use crate::session::{self, Keying};

/// Fake secret key length (arbitrary; matches a 256-bit key).
pub const FAKE_SECRET_KEY_LEN: usize = 32;
/// Fake public key length (SHA-256 output).
pub const FAKE_PUBLIC_KEY_LEN: usize = 32;
/// Fake nonce: all zeros (the real cipher uses random nonces).
const FAKE_NONCE: [u8; poly_core::payload::NONCE_LEN] = [0; poly_core::payload::NONCE_LEN];
/// Domain separators so the fake hashes can't be confused with each other.
const DOMAIN_PUBLIC_KEY: &[u8] = b"polyphonia/fake/public-key";
const DOMAIN_SLOT_A: &[u8] = b"polyphonia/fake/slot-a";
const DOMAIN_SESSION: &[u8] = b"polyphonia/fake/session";

// The padded plaintext is always block aligned, like AES-GCM ciphertext here.
const _: () = assert!(SLOT_BLOCK == 16);

/// NOT SECURE. Testing-only cipher with the real payload layout but no encryption.
#[derive(Debug, Default, Clone, Copy)]
pub struct FakeCipher;

impl FakeCipher {
    /// Deterministic identity from a 32-byte secret (used for golden vectors).
    pub fn identity_from_secret(secret: &[u8]) -> Result<Identity, PolyError> {
        Ok(Identity {
            public_key: public_from_secret(secret)?.to_vec(),
            secret_key: secret.to_vec(),
        })
    }
}

fn public_from_secret(secret: &[u8]) -> Result<[u8; FAKE_PUBLIC_KEY_LEN], PolyError> {
    if secret.len() != FAKE_SECRET_KEY_LEN {
        return Err(PolyError::InvalidKey("fake secret key must be 32 bytes"));
    }
    Ok(Sha256::new()
        .chain_update(DOMAIN_PUBLIC_KEY)
        .chain_update(secret)
        .finalize()
        .into())
}

fn tag_a(public_key: &[u8], aad: &[u8], nonce: &[u8], ct: &[u8]) -> [u8; TAG_LEN] {
    let digest = Sha256::new()
        .chain_update(DOMAIN_SLOT_A)
        .chain_update(public_key)
        .chain_update(aad)
        .chain_update(nonce)
        .chain_update(ct)
        .finalize();
    let mut tag = [0u8; TAG_LEN];
    tag.copy_from_slice(&digest[..TAG_LEN]);
    tag
}

/// The (fake, deterministic) session key a full message to `public_key` starts.
fn session_key(public_key: &[u8]) -> SessionKey {
    SessionKey::from_bytes(
        Sha256::new()
            .chain_update(DOMAIN_SESSION)
            .chain_update(public_key)
            .finalize()
            .into(),
    )
}

fn tag_b(passphrase: &str) -> [u8; TAG_LEN] {
    let digest = Sha256::digest(passphrase.as_bytes());
    let mut tag = [0u8; TAG_LEN];
    tag.copy_from_slice(&digest[..TAG_LEN]);
    tag
}

fn build_slot(ct: &[u8], tag: &[u8]) -> Vec<u8> {
    let mut slot = Vec::with_capacity(FAKE_NONCE.len() + ct.len() + tag.len());
    slot.extend_from_slice(&FAKE_NONCE);
    slot.extend_from_slice(ct);
    slot.extend_from_slice(tag);
    slot
}

impl Cipher for FakeCipher {
    fn algorithm(&self) -> Algorithm {
        Algorithm::Fake
    }

    fn generate_identity(&self) -> Result<Identity, PolyError> {
        let mut secret = [0u8; FAKE_SECRET_KEY_LEN];
        rand::rng().fill_bytes(&mut secret);
        Self::identity_from_secret(&secret)
    }

    fn seal(&self, req: &SealRequest) -> Result<Sealed, PolyError> {
        let keying = session::keying(req.flags, req.session_key.as_ref())?;
        if req.recipient_public_key.len() != FAKE_PUBLIC_KEY_LEN {
            return Err(PolyError::InvalidKey("fake public key must be 32 bytes"));
        }
        let hidden = match (&req.decoy, &req.passphrase) {
            (Some(decoy), Some(pass)) => Some((decoy.as_str(), pass.as_str())),
            (Some(_), None) => return Err(PolyError::MissingPassphrase),
            (None, Some(_)) => {
                return Err(PolyError::InvalidRequest("passphrase given without decoy"));
            }
            (None, None) => None,
        };
        let slot_a_text = hidden.map_or(req.message.as_str(), |(decoy, _)| decoy);
        let longest = slot_a_text.len().max(req.message.len());
        let padded = padded_len(longest);

        let ct_a = slot_pad(slot_a_text, padded)?;
        let slot_a = build_slot(
            &ct_a,
            &tag_a(&req.recipient_public_key, &req.aad, &FAKE_NONCE, &ct_a),
        );
        let slot_b = match hidden {
            Some((_, pass)) => build_slot(&slot_pad(&req.message, padded)?, &tag_b(pass)),
            None => {
                let mut random = vec![0u8; slot_len(padded)];
                rand::rng().fill_bytes(&mut random);
                random
            }
        };
        let (kem_block, new_session_key) = match keying {
            Keying::Full => (Vec::new(), Some(session_key(&req.recipient_public_key))),
            Keying::Session(key) => (key.id().to_vec(), None),
        };
        Ok(Sealed {
            payload: Payload {
                kem_block,
                slot_a,
                slot_b,
            }
            .join(),
            new_session_key,
        })
    }

    fn open(&self, req: &OpenRequest) -> Result<Opened, PolyError> {
        let keying = session::keying(req.flags, req.session_key.as_ref())?;
        let public_key = public_from_secret(&req.my_secret_key)?;
        let payload = Payload::split(&req.payload, Algorithm::Fake, req.flags)?;
        let new_session_key = match keying {
            Keying::Full => Some(session_key(&public_key)),
            Keying::Session(key) => {
                session::check_session_id(&payload.kem_block, key)?;
                None
            }
        };

        // Always attempt both slots (mirrors the real cipher's timing rule).
        let a = split_slot(&payload.slot_a).ok().and_then(|s| {
            (s.tag == tag_a(&public_key, &req.aad, s.nonce, s.ciphertext))
                .then(|| slot_unpad(s.ciphertext).ok())
                .flatten()
        });
        let b = split_slot(&payload.slot_b).ok().and_then(|s| {
            let pass = req.passphrase.as_deref()?;
            (s.tag == tag_b(pass))
                .then(|| slot_unpad(s.ciphertext).ok())
                .flatten()
        });

        match (b, a) {
            (Some(text), _) => Ok(Opened {
                text,
                slot: Slot::B,
                new_session_key,
            }),
            (None, Some(text)) => Ok(Opened {
                text,
                slot: Slot::A,
                new_session_key,
            }),
            (None, None) => Err(PolyError::DecryptFailed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_roundtrip() {
        poly_core::test_utils::assert_cipher_roundtrip(&FakeCipher);
    }

    #[test]
    fn identity_is_deterministic() {
        let a = FakeCipher::identity_from_secret(&[7; 32]).unwrap();
        let b = FakeCipher::identity_from_secret(&[7; 32]).unwrap();
        assert_eq!(a, b);
        assert_ne!(a.public_key, a.secret_key);
        assert!(FakeCipher::identity_from_secret(&[7; 31]).is_err());
    }

    #[test]
    fn hidden_mode_is_deterministic() {
        let me = FakeCipher::identity_from_secret(&[1; 32]).unwrap();
        let req = SealRequest {
            recipient_public_key: me.public_key,
            contact_id: [0; 8],
            message: "real".into(),
            decoy: Some("decoy".into()),
            passphrase: Some("pw".into()),
            aad: [0; 15],
            flags: 0,
            session_key: None,
        };
        assert_eq!(FakeCipher.seal(&req), FakeCipher.seal(&req));
    }
}
