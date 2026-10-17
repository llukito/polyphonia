//! An independent re-implementation of docs/SPEC.md §2–§3 for ML-KEM-768,
//! written from the spec text only. It shares no code or constants with
//! `src/` or poly-core's payload/envelope helpers (every value below is
//! copied from SPEC with its section), and calls the crypto crates directly.
//!
//! Round-trip tests only show that MlKemCipher agrees with itself. These
//! tests show it agrees with the spec, in both directions:
//! - whatever MlKemCipher writes, this reference can read;
//! - whatever this reference writes, MlKemCipher can read;
//! - the golden vectors in tests/vectors/ decode with this reference;
//! - the same for sessions (§3 "Session mode").

mod common;

use std::path::PathBuf;

use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{Aead, KeyInit, Nonce, Payload as AeadInput};
use argon2::{Argon2, Params, Version};
use common::{
    HIDDEN_DECOY, HIDDEN_MESSAGE, HIDDEN_PASSPHRASE, HIDDEN_RNG_DOMAIN, NORMAL_MESSAGE,
    NORMAL_RNG_DOMAIN, SESSION_RNG_DOMAIN, TestRng, VECTOR_SEED,
};
use hkdf::Hkdf;
use ml_kem::ml_kem_768::{Ciphertext, DecapsulationKey, EncapsulationKey};
use ml_kem::{Decapsulate, Encapsulate, Key, KeyExport, Seed};
use poly_core::{Cipher, OpenRequest, Opened, SealRequest, SessionKey, Slot};
use poly_crypto::MlKemCipher;
use rand::Rng;
use sha2::{Digest, Sha256};

// ---- Values from docs/SPEC.md --------------------------------------------

/// §2: magic.
const MAGIC: &[u8; 4] = b"POLY";
/// §2: version.
const VERSION: u8 = 1;
/// §2: algorithm byte for ML-KEM-768.
const ALG_MLKEM768: u8 = 1;
/// §2: header = magic (4) + version + algorithm + flags + contact_id (8).
const HEADER_LEN: usize = 15;
/// §2: payload_len is a big-endian u32 at offset 15.
const PAYLOAD_LEN_SIZE: usize = 4;
/// §2: CRC-32 trailer size.
const CRC_LEN: usize = 4;
/// §2: contact_id = first 8 bytes of SHA-256(public key).
const CONTACT_ID_LEN: usize = 8;
/// §3: ML-KEM-768 kem_block (FIPS 203 ciphertext).
const KEM_BLOCK_LEN: usize = 1088;
/// §3 key encodings: 1184-byte encapsulation key.
const PUBLIC_KEY_LEN: usize = 1184;
/// §3: nonce and tag sizes.
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
/// §3: slot plaintext = u16 length || text || zeros, padded to a multiple of 16.
const LEN_PREFIX: usize = 2;
const PAD_TO: usize = 16;
/// §3 key derivation: HKDF info strings.
const INFO_SLOT_A: &[u8] = b"polyphonia/v1/slot-a";
const INFO_SLOT_B: &[u8] = b"polyphonia/v1/slot-b";
/// §3 "Session mode": HKDF info strings for the session key and id.
const INFO_SESSION_KEY: &[u8] = b"polyphonia/v1/session-key";
const INFO_SESSION_ID: &[u8] = b"polyphonia/v1/session-id";
/// §3 "Session mode": a session message's payload starts with an 8-byte id.
const SESSION_ID_LEN: usize = 8;
/// §2: flags bit 0 = SESSION.
const FLAG_SESSION: u8 = 0b0000_0001;
/// §3: Argon2id v0x13, m = 19,456 KiB, t = 2, p = 1, 32-byte output.
const ARGON2_M_KIB: u32 = 19_456;
const ARGON2_T: u32 = 2;
const ARGON2_P: u32 = 1;
const ARGON2_OUT: usize = 32;
/// §3: AES-256 key size; also the shared secret S size.
const KEY_LEN: usize = 32;
/// IEEE CRC-32 polynomial, reflected form (§2 uses "IEEE CRC-32").
const CRC32_POLY: u32 = 0xEDB8_8320;

// ---- Reference implementation --------------------------------------------

fn contact_id(public_key: &[u8]) -> [u8; CONTACT_ID_LEN] {
    Sha256::digest(public_key)[..CONTACT_ID_LEN]
        .try_into()
        .unwrap()
}

/// Header bytes 0..15 for an ML-KEM full message (flags 0): the AES-GCM AAD (§3).
fn header(public_key: &[u8]) -> [u8; HEADER_LEN] {
    header_with_flags(public_key, 0)
}

/// Header bytes 0..15 for an ML-KEM message with these flags.
fn header_with_flags(public_key: &[u8], flags: u8) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[..4].copy_from_slice(MAGIC);
    h[4] = VERSION;
    h[5] = ALG_MLKEM768;
    h[6] = flags;
    h[7..].copy_from_slice(&contact_id(public_key));
    h
}

/// §3: session_key = HKDF-SHA256(S, info "polyphonia/v1/session-key").
fn session_key_from(s: &[u8; KEY_LEN]) -> [u8; KEY_LEN] {
    hkdf(None, s, INFO_SESSION_KEY)
}

/// §3: session_id = first 8 bytes of HKDF-SHA256(session_key, info "polyphonia/v1/session-id").
/// (HKDF-Expand output is a prefix-stable stream, so the first 8 bytes of a
/// 32-byte output equal an 8-byte output.)
fn session_id(session_key: &[u8; KEY_LEN]) -> [u8; SESSION_ID_LEN] {
    hkdf(None, session_key, INFO_SESSION_ID)[..SESSION_ID_LEN]
        .try_into()
        .unwrap()
}

fn padded_len(longest_text: usize) -> usize {
    (LEN_PREFIX + longest_text).div_ceil(PAD_TO) * PAD_TO
}

fn hkdf(salt: Option<&[u8]>, ikm: &[u8], info: &[u8]) -> [u8; KEY_LEN] {
    let mut okm = [0u8; KEY_LEN];
    Hkdf::<Sha256>::new(salt, ikm)
        .expand(info, &mut okm)
        .unwrap();
    okm
}

fn key_a(s: &[u8; KEY_LEN]) -> [u8; KEY_LEN] {
    hkdf(None, s, INFO_SLOT_A)
}

fn key_b(s: &[u8; KEY_LEN], passphrase: &str, contact_id: &[u8]) -> [u8; KEY_LEN] {
    let params = Params::new(ARGON2_M_KIB, ARGON2_T, ARGON2_P, Some(ARGON2_OUT)).unwrap();
    let mut salt = [0u8; ARGON2_OUT];
    Argon2::new(argon2::Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.as_bytes(), contact_id, &mut salt)
        .unwrap();
    hkdf(Some(&salt), s, INFO_SLOT_B)
}

fn decapsulate(seed: &[u8], kem_block: &[u8]) -> [u8; KEY_LEN] {
    let dk = DecapsulationKey::from_seed(Seed::try_from(seed).unwrap());
    dk.decapsulate(&Ciphertext::try_from(kem_block).unwrap())
        .into()
}

fn pad(text: &str, padded: usize) -> Vec<u8> {
    let mut p = (text.len() as u16).to_be_bytes().to_vec();
    p.extend_from_slice(text.as_bytes());
    p.resize(padded, 0);
    p
}

/// Parse a slot plaintext strictly per §3.
fn unpad(plaintext: &[u8]) -> String {
    assert_eq!(plaintext.len() % PAD_TO, 0, "padded to a multiple of 16");
    let len = u16::from_be_bytes([plaintext[0], plaintext[1]]) as usize;
    let end = LEN_PREFIX + len;
    assert!(plaintext[end..].iter().all(|&b| b == 0), "zero padding");
    String::from_utf8(plaintext[LEN_PREFIX..end].to_vec()).unwrap()
}

fn encrypt_slot(key: &[u8; KEY_LEN], plaintext: &[u8], aad: &[u8]) -> Vec<u8> {
    let mut nonce = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce);
    let ct = Aes256Gcm::new_from_slice(key)
        .unwrap()
        .encrypt(
            &Nonce::<Aes256Gcm>::from(nonce),
            AeadInput {
                msg: plaintext,
                aad,
            },
        )
        .unwrap();
    [nonce.as_slice(), &ct].concat()
}

/// Decrypt one slot `nonce || ciphertext || tag`; None if the tag fails.
fn decrypt_slot(key: &[u8; KEY_LEN], slot: &[u8], aad: &[u8]) -> Option<String> {
    let (nonce, ct) = slot.split_at(NONCE_LEN);
    let plaintext = Aes256Gcm::new_from_slice(key)
        .unwrap()
        .decrypt(
            &Nonce::<Aes256Gcm>::try_from(nonce).unwrap(),
            AeadInput { msg: ct, aad },
        )
        .ok()?;
    Some(unpad(&plaintext))
}

/// Split a payload per §3 and check its layout for a given longest text.
fn split(payload: &[u8], longest_text: usize) -> (&[u8], &[u8], &[u8]) {
    let slot_len = NONCE_LEN + padded_len(longest_text) + TAG_LEN;
    assert_eq!(
        payload.len(),
        KEM_BLOCK_LEN + 2 * slot_len,
        "payload length"
    );
    let (kem, slots) = payload.split_at(KEM_BLOCK_LEN);
    let (a, b) = slots.split_at(slot_len);
    (kem, a, b)
}

/// Split a session payload per §3: session_id (8) || slot_A || slot_B.
fn split_session(payload: &[u8], longest_text: usize) -> (&[u8], &[u8], &[u8]) {
    let slot_len = NONCE_LEN + padded_len(longest_text) + TAG_LEN;
    assert_eq!(
        payload.len(),
        SESSION_ID_LEN + 2 * slot_len,
        "session payload length"
    );
    let (id, slots) = payload.split_at(SESSION_ID_LEN);
    let (a, b) = slots.split_at(slot_len);
    (id, a, b)
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ CRC32_POLY
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

// ---- Helpers to call the cipher under test --------------------------------

struct Recipient {
    seed: Vec<u8>,
    public_key: Vec<u8>,
}

fn recipient() -> Recipient {
    let me = MlKemCipher.generate_identity().unwrap();
    Recipient {
        seed: me.secret_key,
        public_key: me.public_key,
    }
}

fn cipher_seal(to: &Recipient, message: &str, hidden: Option<(&str, &str)>) -> Vec<u8> {
    MlKemCipher
        .seal(&SealRequest {
            recipient_public_key: to.public_key.clone(),
            contact_id: contact_id(&to.public_key),
            message: message.into(),
            decoy: hidden.map(|(d, _)| d.into()),
            passphrase: hidden.map(|(_, p)| p.into()),
            aad: header(&to.public_key),
            flags: 0,
            session_key: None,
        })
        .unwrap()
        .payload
}

fn cipher_open(to: &Recipient, payload: &[u8], passphrase: Option<&str>) -> Opened {
    MlKemCipher
        .open(&OpenRequest {
            my_secret_key: to.seed.clone(),
            contact_id: contact_id(&to.public_key),
            payload: payload.to_vec(),
            passphrase: passphrase.map(Into::into),
            aad: header(&to.public_key),
            flags: 0,
            session_key: None,
        })
        .unwrap()
}

// ---- Tests ----------------------------------------------------------------

#[test]
fn header_matches_poly_core() {
    // Sanity check of this reference itself against the code under test.
    let to = recipient();
    let cid = contact_id(&to.public_key);
    assert_eq!(cid, poly_core::contact_id(&to.public_key));
    assert_eq!(
        header(&to.public_key),
        poly_core::envelope::header_aad(poly_core::Algorithm::MlKem768, 0, &cid)
    );
}

#[test]
fn key_encoding_follows_spec() {
    // §3: public key = 1184-byte encapsulation key derived from the 64-byte seed.
    let to = recipient();
    assert_eq!(to.public_key.len(), PUBLIC_KEY_LEN);
    let dk = DecapsulationKey::from_seed(Seed::try_from(to.seed.as_slice()).unwrap());
    assert_eq!(
        dk.encapsulation_key().to_bytes().as_slice(),
        to.public_key.as_slice()
    );
}

#[test]
fn cipher_normal_message_follows_spec() {
    let to = recipient();
    let aad = header(&to.public_key);
    let message = "hello, spec";
    let payload = cipher_seal(&to, message, None);
    let (kem, a, b) = split(&payload, message.len());

    let s = decapsulate(&to.seed, kem);
    assert_eq!(decrypt_slot(&key_a(&s), a, &aad).as_deref(), Some(message));
    // Slot B is random bytes: it opens under no key.
    assert_eq!(decrypt_slot(&key_a(&s), b, &aad), None);
    let cid = contact_id(&to.public_key);
    assert_eq!(decrypt_slot(&key_b(&s, "anything", &cid), b, &aad), None);
    // ...and is fresh every time, never a fixed pattern such as zeros.
    let again = cipher_seal(&to, message, None);
    let (_, _, b_again) = split(&again, message.len());
    assert_ne!(b, b_again, "normal-mode slot B must be random bytes");
    // The AAD is really bound: any other header fails.
    let mut other = aad;
    other[6] = 1;
    assert_eq!(decrypt_slot(&key_a(&s), a, &other), None);
}

#[test]
fn cipher_hidden_message_follows_spec() {
    let to = recipient();
    let aad = header(&to.public_key);
    let cid = contact_id(&to.public_key);
    let pass = "orchid";
    // Both orders, so padding follows the longer of the two texts.
    for (real, decoy) in [
        ("short", "a decoy much longer than the real message"),
        ("a real message much longer than the decoy", "hi"),
    ] {
        let payload = cipher_seal(&to, real, Some((decoy, pass)));
        let (kem, a, b) = split(&payload, real.len().max(decoy.len()));
        assert_ne!(&a[..NONCE_LEN], &b[..NONCE_LEN], "fresh nonce per slot");

        let s = decapsulate(&to.seed, kem);
        assert_eq!(decrypt_slot(&key_a(&s), a, &aad).as_deref(), Some(decoy));
        assert_eq!(
            decrypt_slot(&key_b(&s, pass, &cid), b, &aad).as_deref(),
            Some(real)
        );
        assert_eq!(decrypt_slot(&key_b(&s, "tulip", &cid), b, &aad), None);
        assert_eq!(decrypt_slot(&key_a(&s), b, &aad), None);
    }
}

#[test]
fn reference_payload_opens_in_cipher() {
    let to = recipient();
    let aad = header(&to.public_key);
    let cid = contact_id(&to.public_key);
    let ek = EncapsulationKey::new(
        &Key::<EncapsulationKey>::try_from(to.public_key.as_slice()).unwrap(),
    )
    .unwrap();

    // Normal mode: slot A = message, slot B = random bytes.
    let message = "written by the reference";
    let (kem, s) = ek.encapsulate_with_rng(&mut rand::rng());
    let s: [u8; KEY_LEN] = s.into();
    let padded = padded_len(message.len());
    let mut random_b = vec![0u8; NONCE_LEN + padded + TAG_LEN];
    rand::rng().fill_bytes(&mut random_b);
    let payload = [
        kem.as_slice(),
        &encrypt_slot(&key_a(&s), &pad(message, padded), &aad),
        &random_b,
    ]
    .concat();
    assert_eq!(cipher_open(&to, &payload, None).text, message);
    assert_eq!(cipher_open(&to, &payload, Some("x")).slot, Slot::A);

    // Hidden mode: slot A = decoy, slot B = real.
    let (real, decoy, pass) = ("the hidden one", "the decoy", "orchid");
    let (kem, s) = ek.encapsulate_with_rng(&mut TestRng::new(b"reference-hidden"));
    let s: [u8; KEY_LEN] = s.into();
    let padded = padded_len(real.len().max(decoy.len()));
    let payload = [
        kem.as_slice(),
        &encrypt_slot(&key_a(&s), &pad(decoy, padded), &aad),
        &encrypt_slot(&key_b(&s, pass, &cid), &pad(real, padded), &aad),
    ]
    .concat();
    let b = cipher_open(&to, &payload, Some(pass));
    assert_eq!((b.text.as_str(), b.slot), (real, Slot::B));
    let a = cipher_open(&to, &payload, None);
    assert_eq!((a.text.as_str(), a.slot), (decoy, Slot::A));
}

/// Parse a golden envelope per §2, check every header field and the CRC,
/// and return the payload.
fn parse_vector(name: &str, public_key: &[u8]) -> Vec<u8> {
    parse_vector_with_flags(name, public_key, 0)
}

fn parse_vector_with_flags(name: &str, public_key: &[u8], flags: u8) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/vectors")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(
        &bytes[..HEADER_LEN],
        &header_with_flags(public_key, flags),
        "{name}: header"
    );
    let len_field = &bytes[HEADER_LEN..HEADER_LEN + PAYLOAD_LEN_SIZE];
    let payload_len = u32::from_be_bytes(len_field.try_into().unwrap()) as usize;
    let crc_at = HEADER_LEN + PAYLOAD_LEN_SIZE + payload_len;
    assert_eq!(bytes.len(), crc_at + CRC_LEN, "{name}: total length");
    let crc = u32::from_be_bytes(bytes[crc_at..].try_into().unwrap());
    assert_eq!(crc, crc32(&bytes[..crc_at]), "{name}: CRC-32");
    bytes[HEADER_LEN + PAYLOAD_LEN_SIZE..crc_at].to_vec()
}

#[test]
fn golden_vectors_follow_spec() {
    let public_key = DecapsulationKey::from_seed(Seed::try_from(VECTOR_SEED.as_slice()).unwrap())
        .encapsulation_key()
        .to_bytes()
        .to_vec();
    let aad = header(&public_key);
    let cid = contact_id(&public_key);

    let payload = parse_vector("mlkem_envelope_hidden.bin", &public_key);
    let (kem, a, b) = split(&payload, HIDDEN_MESSAGE.len().max(HIDDEN_DECOY.len()));
    let s = decapsulate(&VECTOR_SEED, kem);
    assert_eq!(
        decrypt_slot(&key_a(&s), a, &aad).as_deref(),
        Some(HIDDEN_DECOY)
    );
    assert_eq!(
        decrypt_slot(&key_b(&s, HIDDEN_PASSPHRASE, &cid), b, &aad).as_deref(),
        Some(HIDDEN_MESSAGE)
    );

    let payload = parse_vector("mlkem_envelope_normal.bin", &public_key);
    let (kem, a, _) = split(&payload, NORMAL_MESSAGE.len());
    let s = decapsulate(&VECTOR_SEED, kem);
    assert_eq!(
        decrypt_slot(&key_a(&s), a, &aad).as_deref(),
        Some(NORMAL_MESSAGE)
    );

    // The session vector belongs to the session started by the hidden vector:
    // derive its key from that vector's S, independently of the cipher.
    let payload = parse_vector("mlkem_envelope_hidden.bin", &public_key);
    let (kem, _, _) = split(&payload, HIDDEN_MESSAGE.len().max(HIDDEN_DECOY.len()));
    let session_key = session_key_from(&decapsulate(&VECTOR_SEED, kem));
    let session_aad = header_with_flags(&public_key, FLAG_SESSION);
    let payload = parse_vector_with_flags("mlkem_envelope_session.bin", &public_key, FLAG_SESSION);
    let (id, a, b) = split_session(&payload, HIDDEN_MESSAGE.len().max(HIDDEN_DECOY.len()));
    assert_eq!(id, session_id(&session_key));
    assert_eq!(
        decrypt_slot(&key_a(&session_key), a, &session_aad).as_deref(),
        Some(HIDDEN_DECOY)
    );
    assert_eq!(
        decrypt_slot(
            &key_b(&session_key, HIDDEN_PASSPHRASE, &cid),
            b,
            &session_aad
        )
        .as_deref(),
        Some(HIDDEN_MESSAGE)
    );

    // The RNG domains are part of the vector definition; keep them referenced
    // here so a change to them is noticed in review.
    assert_ne!(HIDDEN_RNG_DOMAIN, NORMAL_RNG_DOMAIN);
    assert_ne!(HIDDEN_RNG_DOMAIN, SESSION_RNG_DOMAIN);
}

// ---- Sessions (§3 "Session mode") -------------------------------------------

/// Seal through the cipher with any flags/session key; returns (payload, new session key).
fn cipher_seal_with(
    to: &Recipient,
    message: &str,
    hidden: Option<(&str, &str)>,
    session_key: Option<&SessionKey>,
) -> (Vec<u8>, Option<SessionKey>) {
    let flags = if session_key.is_some() {
        FLAG_SESSION
    } else {
        0
    };
    let sealed = MlKemCipher
        .seal(&SealRequest {
            recipient_public_key: to.public_key.clone(),
            contact_id: contact_id(&to.public_key),
            message: message.into(),
            decoy: hidden.map(|(d, _)| d.into()),
            passphrase: hidden.map(|(_, p)| p.into()),
            aad: header_with_flags(&to.public_key, flags),
            flags,
            session_key: session_key.cloned(),
        })
        .unwrap();
    (sealed.payload, sealed.new_session_key)
}

fn cipher_open_session(
    to: &Recipient,
    payload: &[u8],
    passphrase: Option<&str>,
    key: &SessionKey,
) -> Opened {
    MlKemCipher
        .open(&OpenRequest {
            my_secret_key: to.seed.clone(),
            contact_id: contact_id(&to.public_key),
            payload: payload.to_vec(),
            passphrase: passphrase.map(Into::into),
            aad: header_with_flags(&to.public_key, FLAG_SESSION),
            flags: FLAG_SESSION,
            session_key: Some(key.clone()),
        })
        .unwrap()
}

#[test]
fn full_message_starts_the_spec_session() {
    let to = recipient();
    let (payload, sender_key) = cipher_seal_with(&to, "start", None, None);
    let (kem, _, _) = split(&payload, "start".len());
    let expected = session_key_from(&decapsulate(&to.seed, kem));

    let sender_key = sender_key.expect("a full message starts a session");
    assert_eq!(sender_key.as_bytes(), &expected, "sender's session key");
    let receiver_key = cipher_open(&to, &payload, None)
        .new_session_key
        .expect("opening a full message returns its session");
    assert_eq!(receiver_key.as_bytes(), &expected, "receiver's session key");
    assert_eq!(sender_key.id(), session_id(&expected), "session id");
}

#[test]
fn cipher_session_message_follows_spec() {
    let to = recipient();
    let cid = contact_id(&to.public_key);
    let aad = header_with_flags(&to.public_key, FLAG_SESSION);
    let key = cipher_seal_with(&to, "start", None, None).1.unwrap();
    let k = *key.as_bytes();

    // Normal mode.
    let message = "short and sweet";
    let (payload, new_key) = cipher_seal_with(&to, message, None, Some(&key));
    assert!(new_key.is_none(), "a session message starts no session");
    let (id, a, b) = split_session(&payload, message.len());
    assert_eq!(id, session_id(&k));
    assert_eq!(decrypt_slot(&key_a(&k), a, &aad).as_deref(), Some(message));
    assert_eq!(decrypt_slot(&key_a(&k), b, &aad), None, "slot B is random");
    // The flags byte is in the AAD: the full-message header doesn't verify.
    assert_eq!(decrypt_slot(&key_a(&k), a, &header(&to.public_key)), None);

    // Hidden mode.
    let (real, decoy, pass) = ("the real one", "a decoy", "orchid");
    let (payload, _) = cipher_seal_with(&to, real, Some((decoy, pass)), Some(&key));
    let (id, a, b) = split_session(&payload, real.len().max(decoy.len()));
    assert_eq!(id, session_id(&k));
    assert_eq!(decrypt_slot(&key_a(&k), a, &aad).as_deref(), Some(decoy));
    assert_eq!(
        decrypt_slot(&key_b(&k, pass, &cid), b, &aad).as_deref(),
        Some(real)
    );
}

#[test]
fn reference_session_payload_opens_in_cipher() {
    let to = recipient();
    let cid = contact_id(&to.public_key);
    let aad = header_with_flags(&to.public_key, FLAG_SESSION);
    // Start the session from a reference-built full message's S.
    let ek = EncapsulationKey::new(
        &Key::<EncapsulationKey>::try_from(to.public_key.as_slice()).unwrap(),
    )
    .unwrap();
    let (_, s) = ek.encapsulate_with_rng(&mut rand::rng());
    let s: [u8; KEY_LEN] = s.into();
    let k = session_key_from(&s);
    let key = SessionKey::from_bytes(k);

    let (real, decoy, pass) = ("built by the reference", "decoy", "orchid");
    let padded = padded_len(real.len().max(decoy.len()));
    let payload = [
        session_id(&k).as_slice(),
        &encrypt_slot(&key_a(&k), &pad(decoy, padded), &aad),
        &encrypt_slot(&key_b(&k, pass, &cid), &pad(real, padded), &aad),
    ]
    .concat();
    let b = cipher_open_session(&to, &payload, Some(pass), &key);
    assert_eq!((b.text.as_str(), b.slot), (real, Slot::B));
    let a = cipher_open_session(&to, &payload, None, &key);
    assert_eq!((a.text.as_str(), a.slot), (decoy, Slot::A));
    assert!(a.new_session_key.is_none());
}
