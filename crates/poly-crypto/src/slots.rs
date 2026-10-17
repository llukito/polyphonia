//! Slot encryption shared by the real ciphers (SPEC §3 "Key derivation" and "Slots").
//!
//! A real cipher (ML-KEM-768, RSA-2048) only produces the kem_block and the
//! 32-byte shared secret S. Everything after that is identical for every
//! algorithm and lives here:
//!
//! ```text
//! key_A = HKDF-SHA256(ikm = S, salt = none, info = "polyphonia/v1/slot-a")
//! key_B = HKDF-SHA256(ikm = S, salt = Argon2id(passphrase, contact_id), info = "polyphonia/v1/slot-b")
//! slot  = nonce (12, random) || AES-256-GCM(key, nonce, padded text, aad) (ciphertext || tag)
//! ```

use aes_gcm::Aes256Gcm;
use aes_gcm::aead::array::typenum::Unsigned;
use aes_gcm::aead::{Aead, Key, KeyInit, KeySizeUser, Nonce, Payload as AeadInput};
use argon2::{Argon2, Params, Version};
use hkdf::Hkdf;
use poly_core::payload::{
    HKDF_INFO_SLOT_A, HKDF_INFO_SLOT_B, NONCE_LEN, SHARED_SECRET_LEN, padded_len, slot_len,
};
use poly_core::{
    ContactId, OpenRequest, Opened, PolyError, SealRequest, Slot, slot_pad, slot_unpad,
};
use rand::CryptoRng;
use sha2::Sha256;
use zeroize::Zeroizing;

/// AES-256 key length: 256 bits = 32 bytes (FIPS 197).
const AES_KEY_LEN: usize = 32;
const _: () = assert!(<Aes256Gcm as KeySizeUser>::KeySize::USIZE == AES_KEY_LEN);

/// Argon2id memory cost in KiB: 19,456 KiB = 19 MiB (SPEC §3, OWASP minimum recommendation).
const ARGON2_MEMORY_KIB: u32 = 19_456;
/// Argon2id iterations (SPEC §3, OWASP minimum recommendation).
const ARGON2_ITERATIONS: u32 = 2;
/// Argon2id parallelism / lanes (SPEC §3, OWASP minimum recommendation).
const ARGON2_PARALLELISM: u32 = 1;
/// Argon2id output length: 32 bytes, used as the HKDF salt for key_B (SPEC §3).
const ARGON2_OUTPUT_LEN: usize = 32;

/// The 32-byte shared secret S from the KEM (or the RSA-wrapped random value).
/// Wiped from memory when dropped.
pub(crate) type SharedSecret = Zeroizing<[u8; SHARED_SECRET_LEN]>;

/// A 32-byte AES-256-GCM key derived from S, in the cipher's own key type so
/// it is never copied into an unwiped buffer. Wiped from memory when dropped.
type SlotKey = Zeroizing<Key<Aes256Gcm>>;

/// What goes into the two slots, after checking the request.
#[derive(Clone, Copy)]
pub(crate) enum Mode<'a> {
    /// Slot A = message, slot B = random bytes.
    Normal,
    /// Slot A = decoy, slot B = message (needs the passphrase).
    Hidden { decoy: &'a str, passphrase: &'a str },
}

/// Check the decoy/passphrase combination of a seal request (SPEC §3 "Seal and open rules").
///
/// Ciphers call this before any public-key work, so a bad request fails fast.
pub(crate) fn mode(req: &SealRequest) -> Result<Mode<'_>, PolyError> {
    match (&req.decoy, &req.passphrase) {
        (None, None) => Ok(Mode::Normal),
        (Some(decoy), Some(passphrase)) => Ok(Mode::Hidden { decoy, passphrase }),
        (Some(_), None) => Err(PolyError::MissingPassphrase),
        (None, Some(_)) => Err(PolyError::InvalidRequest("passphrase given without decoy")),
    }
}

/// key_A = HKDF-SHA256(ikm = S, salt = none, info = "polyphonia/v1/slot-a").
fn key_a(shared: &SharedSecret) -> Result<SlotKey, PolyError> {
    let mut key = SlotKey::default();
    Hkdf::<Sha256>::new(None, &shared[..])
        .expand(HKDF_INFO_SLOT_A, &mut key[..])
        .map_err(|_| PolyError::Crypto("HKDF expand failed".into()))?;
    Ok(key)
}

/// key_B = HKDF-SHA256(ikm = S, salt = Argon2id(passphrase, salt = contact_id), info = "polyphonia/v1/slot-b").
fn key_b(
    shared: &SharedSecret,
    passphrase: &str,
    contact_id: &ContactId,
) -> Result<SlotKey, PolyError> {
    let params = Params::new(
        ARGON2_MEMORY_KIB,
        ARGON2_ITERATIONS,
        ARGON2_PARALLELISM,
        Some(ARGON2_OUTPUT_LEN),
    )
    .map_err(|_| PolyError::Crypto("bad Argon2id parameters".into()))?;
    let mut stretched = Zeroizing::new([0u8; ARGON2_OUTPUT_LEN]);
    Argon2::new(argon2::Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.as_bytes(), contact_id, &mut stretched[..])
        .map_err(|_| PolyError::Crypto("Argon2id failed".into()))?;

    let mut key = SlotKey::default();
    Hkdf::<Sha256>::new(Some(&stretched[..]), &shared[..])
        .expand(HKDF_INFO_SLOT_B, &mut key[..])
        .map_err(|_| PolyError::Crypto("HKDF expand failed".into()))?;
    Ok(key)
}

/// Encrypt one padded plaintext into `nonce || ciphertext || tag` with a fresh random nonce.
fn encrypt_slot<R: CryptoRng + ?Sized>(
    key: &SlotKey,
    padded: &[u8],
    aad: &[u8],
    rng: &mut R,
) -> Result<Vec<u8>, PolyError> {
    // A new random nonce for every slot: never reuse a nonce with the same key.
    let mut nonce = [0u8; NONCE_LEN];
    rng.fill_bytes(&mut nonce);

    let ciphertext_and_tag = Aes256Gcm::new(key)
        .encrypt(
            &Nonce::<Aes256Gcm>::from(nonce),
            AeadInput { msg: padded, aad },
        )
        .map_err(|_| PolyError::Crypto("AES-GCM encryption failed".into()))?;

    let mut slot = Vec::with_capacity(NONCE_LEN + ciphertext_and_tag.len());
    slot.extend_from_slice(&nonce);
    slot.extend_from_slice(&ciphertext_and_tag);
    Ok(slot)
}

/// Decrypt one slot. `None` if the tag doesn't verify or the plaintext is malformed.
/// The text is returned in a wiping wrapper so a slot that isn't used is wiped.
fn open_slot(key: &SlotKey, slot: &[u8], aad: &[u8]) -> Option<Zeroizing<String>> {
    let (nonce, ciphertext_and_tag) = slot.split_at_checked(NONCE_LEN)?;
    let nonce = Nonce::<Aes256Gcm>::try_from(nonce).ok()?;
    // AES-GCM checks the tag in constant time before returning any plaintext.
    let padded = Aes256Gcm::new(key)
        .decrypt(
            &nonce,
            AeadInput {
                msg: ciphertext_and_tag,
                aad,
            },
        )
        .map(Zeroizing::new)
        .ok()?;
    slot_unpad(&padded).ok().map(Zeroizing::new)
}

/// Build slot A and slot B (equal length) for a checked request.
///
/// Randomness is drawn from `rng` in this order: slot A nonce, then slot B
/// nonce (hidden mode) or the random slot B bytes (normal mode).
pub(crate) fn seal_slots<R: CryptoRng + ?Sized>(
    shared: &SharedSecret,
    req: &SealRequest,
    mode: Mode<'_>,
    rng: &mut R,
) -> Result<(Vec<u8>, Vec<u8>), PolyError> {
    let slot_a_text = match mode {
        Mode::Normal => req.message.as_str(),
        Mode::Hidden { decoy, .. } => decoy,
    };
    // Both slots are padded to the size of the longer text, so they are equal length.
    let padded = padded_len(slot_a_text.len().max(req.message.len()));

    let plaintext_a = Zeroizing::new(slot_pad(slot_a_text, padded)?);
    let slot_a = encrypt_slot(&key_a(shared)?, &plaintext_a, &req.aad, rng)?;
    let slot_b = match mode {
        Mode::Hidden { passphrase, .. } => {
            let plaintext_b = Zeroizing::new(slot_pad(&req.message, padded)?);
            encrypt_slot(
                &key_b(shared, passphrase, &req.contact_id)?,
                &plaintext_b,
                &req.aad,
                rng,
            )?
        }
        Mode::Normal => {
            // Random bytes look exactly like a real encrypted slot of the same length.
            let mut random = vec![0u8; slot_len(padded)];
            rng.fill_bytes(&mut random);
            random
        }
    };
    Ok((slot_a, slot_b))
}

/// Open the two slots (SPEC §3 "Seal and open rules").
///
/// With a passphrase, both slots are decrypted before deciding, so timing
/// doesn't reveal which one opened. A wrong or unneeded passphrase is never
/// an error: slot B just doesn't open and slot A is returned.
pub(crate) fn open_slots(
    shared: &SharedSecret,
    slot_a: &[u8],
    slot_b: &[u8],
    req: &OpenRequest,
) -> Result<Opened, PolyError> {
    // Any failure on the slot B side counts as "B didn't open", never as an error.
    let b = req.passphrase.as_deref().and_then(|passphrase| {
        let key = key_b(shared, passphrase, &req.contact_id).ok()?;
        open_slot(&key, slot_b, &req.aad)
    });
    let a = open_slot(&key_a(shared)?, slot_a, &req.aad);

    // The winning text moves out of its wrapper into `Opened` (a poly-core
    // type, not wiped); the other slot's text is wiped when `a`/`b` drop.
    match (b, a) {
        (Some(mut text), _) => Ok(Opened {
            text: std::mem::take(&mut *text),
            slot: Slot::B,
        }),
        (None, Some(mut text)) => Ok(Opened {
            text: std::mem::take(&mut *text),
            slot: Slot::A,
        }),
        (None, None) => Err(PolyError::DecryptFailed),
    }
}
