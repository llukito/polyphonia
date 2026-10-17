//! ML-KEM-768 and RSA-2048 golden vectors in tests/vectors/: a fixed key and
//! a fixed RNG must always produce exactly these bytes, so a silent format
//! change fails.
//! (That the bytes follow SPEC is checked independently in spec_reference.rs.)
//!
//! Regenerate (only when the SPEC changes, with the lead's approval) with:
//!
//!     POLY_REGEN_VECTORS=1 cargo test -p poly-crypto --test golden

mod common;

use std::path::PathBuf;

use common::{
    HIDDEN_DECOY, HIDDEN_MESSAGE, HIDDEN_PASSPHRASE, HIDDEN_RNG_DOMAIN, NORMAL_MESSAGE,
    NORMAL_RNG_DOMAIN, RSA_HIDDEN_RNG_DOMAIN, RSA_KEY_RNG_DOMAIN, RSA_PUBLIC_KEY_FILE,
    RSA_SECRET_KEY_FILE, SESSION_RNG_DOMAIN, TestRng, VECTOR_SEED,
};
use poly_core::envelope::{FLAG_SESSION, header_aad};
use poly_core::{
    Algorithm, Cipher, Envelope, OpenRequest, SealRequest, SessionKey, Slot, contact_id,
};
use poly_crypto::{MlKemCipher, RsaCipher};
use rsa::RsaPrivateKey;
use rsa::pkcs1::EncodeRsaPublicKey;
use rsa::pkcs8::EncodePrivateKey;

fn vectors_dir() -> PathBuf {
    common::vectors_dir()
}

fn check_or_write(name: &str, bytes: &[u8]) {
    let path = vectors_dir().join(name);
    if std::env::var_os("POLY_REGEN_VECTORS").is_some() {
        std::fs::write(&path, bytes).unwrap();
        return;
    }
    let golden = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(
        golden, bytes,
        "{name} differs from what the code produces now"
    );
}

/// Seal a message to the vector key with a fixed RNG and wrap it in an
/// envelope (a session message if `session_key` is Some). Returns the
/// envelope bytes and the session a full message starts.
fn envelope(
    message: &str,
    hidden: Option<(&str, &str)>,
    rng_domain: &[u8],
    session_key: Option<&SessionKey>,
) -> (Vec<u8>, Option<SessionKey>) {
    let me = MlKemCipher::identity_from_seed(&VECTOR_SEED).unwrap();
    let cid = contact_id(&me.public_key);
    let flags = if session_key.is_some() {
        FLAG_SESSION
    } else {
        0
    };
    let req = SealRequest {
        recipient_public_key: me.public_key,
        contact_id: cid,
        message: message.into(),
        decoy: hidden.map(|(decoy, _)| decoy.into()),
        passphrase: hidden.map(|(_, pass)| pass.into()),
        aad: header_aad(Algorithm::MlKem768, flags, &cid),
        flags,
        session_key: session_key.cloned(),
    };
    let sealed = MlKemCipher
        .seal_with_rng(&req, &mut TestRng::new(rng_domain))
        .unwrap();
    let bytes = Envelope::new(Algorithm::MlKem768, flags, cid, sealed.payload)
        .to_bytes()
        .unwrap();
    (bytes, sealed.new_session_key)
}

/// The hidden vector and the session it starts.
fn hidden_vector() -> (Vec<u8>, SessionKey) {
    let (bytes, key) = envelope(
        HIDDEN_MESSAGE,
        Some((HIDDEN_DECOY, HIDDEN_PASSPHRASE)),
        HIDDEN_RNG_DOMAIN,
        None,
    );
    (bytes, key.expect("a full message starts a session"))
}

/// Open a vector file from disk with the vector key; returns (text, slot).
fn open_vector(name: &str, passphrase: Option<&str>) -> (String, Slot) {
    open_vector_with(name, passphrase, None)
}

fn open_vector_with(
    name: &str,
    passphrase: Option<&str>,
    session_key: Option<&SessionKey>,
) -> (String, Slot) {
    let bytes = std::fs::read(vectors_dir().join(name)).unwrap();
    let env = Envelope::from_bytes(&bytes).unwrap();
    MlKemCipher
        .open(&OpenRequest {
            my_secret_key: VECTOR_SEED.to_vec(),
            contact_id: env.contact_id,
            payload: env.payload.clone(),
            passphrase: passphrase.map(Into::into),
            aad: env.aad(),
            flags: env.flags,
            session_key: session_key.cloned(),
        })
        .map(|o| (o.text, o.slot))
        .unwrap()
}

fn opened(text: &str, slot: Slot) -> (String, Slot) {
    (text.into(), slot)
}

#[test]
fn mlkem_envelope_hidden() {
    let (bytes, _) = hidden_vector();
    check_or_write("mlkem_envelope_hidden.bin", &bytes);

    let name = "mlkem_envelope_hidden.bin";
    assert_eq!(
        open_vector(name, Some(HIDDEN_PASSPHRASE)),
        opened(HIDDEN_MESSAGE, Slot::B)
    );
    assert_eq!(open_vector(name, None), opened(HIDDEN_DECOY, Slot::A));
    assert_eq!(
        open_vector(name, Some("wrong")),
        opened(HIDDEN_DECOY, Slot::A)
    );
}

#[test]
fn mlkem_envelope_normal() {
    let (bytes, _) = envelope(NORMAL_MESSAGE, None, NORMAL_RNG_DOMAIN, None);
    check_or_write("mlkem_envelope_normal.bin", &bytes);

    let name = "mlkem_envelope_normal.bin";
    assert_eq!(open_vector(name, None), opened(NORMAL_MESSAGE, Slot::A));
    assert_eq!(
        open_vector(name, Some("any")),
        opened(NORMAL_MESSAGE, Slot::A)
    );
}

/// A hidden-mode session message in the session the hidden vector starts.
#[test]
fn mlkem_envelope_session() {
    let (_, key) = hidden_vector();
    let (bytes, new_key) = envelope(
        HIDDEN_MESSAGE,
        Some((HIDDEN_DECOY, HIDDEN_PASSPHRASE)),
        SESSION_RNG_DOMAIN,
        Some(&key),
    );
    assert!(new_key.is_none());
    check_or_write("mlkem_envelope_session.bin", &bytes);

    let name = "mlkem_envelope_session.bin";
    assert_eq!(
        open_vector_with(name, Some(HIDDEN_PASSPHRASE), Some(&key)),
        opened(HIDDEN_MESSAGE, Slot::B)
    );
    assert_eq!(
        open_vector_with(name, None, Some(&key)),
        opened(HIDDEN_DECOY, Slot::A)
    );
}

/// The receiver of the hidden vector derives the same session key as its sender.
#[test]
fn hidden_vector_session_key_matches_on_open() {
    let (_, sender_key) = hidden_vector();
    let bytes = std::fs::read(vectors_dir().join("mlkem_envelope_hidden.bin")).unwrap();
    let env = Envelope::from_bytes(&bytes).unwrap();
    let opened = MlKemCipher
        .open(&OpenRequest {
            my_secret_key: VECTOR_SEED.to_vec(),
            contact_id: env.contact_id,
            payload: env.payload.clone(),
            passphrase: None,
            aad: env.aad(),
            flags: env.flags,
            session_key: None,
        })
        .unwrap();
    assert_eq!(opened.new_session_key, Some(sender_key));
}

#[test]
fn identity_from_seed_is_deterministic() {
    let a = MlKemCipher::identity_from_seed(&VECTOR_SEED).unwrap();
    let b = MlKemCipher::identity_from_seed(&VECTOR_SEED).unwrap();
    assert_eq!(a, b);
    assert!(MlKemCipher::identity_from_seed(&VECTOR_SEED[..63]).is_err());
}

// ---- RSA-2048 ---------------------------------------------------------------

/// RSA modulus size for the vector key (SPEC §3).
const RSA_MODULUS_BITS: usize = 2048;

/// The RSA vector key (public PKCS#1 DER, secret PKCS#8 DER), read from
/// tests/vectors/. With POLY_REGEN_VECTORS set and no key files yet, it is
/// generated from RSA_KEY_RNG_DOMAIN and written first.
fn rsa_vector_key() -> (Vec<u8>, Vec<u8>) {
    let public = vectors_dir().join(RSA_PUBLIC_KEY_FILE);
    let secret = vectors_dir().join(RSA_SECRET_KEY_FILE);
    if std::env::var_os("POLY_REGEN_VECTORS").is_some() && !secret.exists() {
        let sk =
            RsaPrivateKey::new(&mut TestRng::new(RSA_KEY_RNG_DOMAIN), RSA_MODULUS_BITS).unwrap();
        std::fs::write(
            &public,
            sk.to_public_key().to_pkcs1_der().unwrap().as_bytes(),
        )
        .unwrap();
        std::fs::write(&secret, sk.to_pkcs8_der().unwrap().as_bytes()).unwrap();
    }
    (
        std::fs::read(public).unwrap(),
        std::fs::read(secret).unwrap(),
    )
}

#[test]
fn rsa_envelope_hidden() {
    let (public_key, secret_key) = rsa_vector_key();
    let cid = contact_id(&public_key);
    let req = SealRequest {
        recipient_public_key: public_key,
        contact_id: cid,
        message: HIDDEN_MESSAGE.into(),
        decoy: Some(HIDDEN_DECOY.into()),
        passphrase: Some(HIDDEN_PASSPHRASE.into()),
        aad: header_aad(Algorithm::Rsa2048, 0, &cid),
        flags: 0,
        session_key: None,
    };
    let payload = RsaCipher
        .seal_with_rng(&req, &mut TestRng::new(RSA_HIDDEN_RNG_DOMAIN))
        .unwrap()
        .payload;
    let bytes = Envelope::new(Algorithm::Rsa2048, 0, cid, payload)
        .to_bytes()
        .unwrap();
    check_or_write("rsa_envelope_hidden.bin", &bytes);

    let golden = std::fs::read(vectors_dir().join("rsa_envelope_hidden.bin")).unwrap();
    let env = Envelope::from_bytes(&golden).unwrap();
    let open = |passphrase: Option<&str>| {
        RsaCipher
            .open(&OpenRequest {
                my_secret_key: secret_key.clone(),
                contact_id: env.contact_id,
                payload: env.payload.clone(),
                passphrase: passphrase.map(Into::into),
                aad: env.aad(),
                flags: env.flags,
                session_key: None,
            })
            .map(|o| (o.text, o.slot))
            .unwrap()
    };
    assert_eq!(
        open(Some(HIDDEN_PASSPHRASE)),
        opened(HIDDEN_MESSAGE, Slot::B)
    );
    assert_eq!(open(None), opened(HIDDEN_DECOY, Slot::A));
}
