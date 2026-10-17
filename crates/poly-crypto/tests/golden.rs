//! ML-KEM-768 golden vectors in tests/vectors/: a fixed key and a fixed RNG
//! must always produce exactly these bytes, so a silent format change fails.
//! (That the bytes follow SPEC is checked independently in spec_reference.rs.)
//!
//! Regenerate (only when the SPEC changes, with the lead's approval) with:
//!
//!     POLY_REGEN_VECTORS=1 cargo test -p poly-crypto --test golden

mod common;

use std::path::PathBuf;

use common::{
    HIDDEN_DECOY, HIDDEN_MESSAGE, HIDDEN_PASSPHRASE, HIDDEN_RNG_DOMAIN, NORMAL_MESSAGE,
    NORMAL_RNG_DOMAIN, TestRng, VECTOR_SEED,
};
use poly_core::envelope::header_aad;
use poly_core::{Algorithm, Cipher, Envelope, OpenRequest, SealRequest, Slot, contact_id};
use poly_crypto::MlKemCipher;

/// Envelope flags of the vectors: MVP envelopes have no SESSION flag (SPEC §2).
const FLAGS: u8 = 0;

fn vectors_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/vectors")
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

/// Seal a message to the vector key with a fixed RNG and wrap it in an envelope.
fn envelope(message: &str, hidden: Option<(&str, &str)>, rng_domain: &[u8]) -> Vec<u8> {
    let me = MlKemCipher::identity_from_seed(&VECTOR_SEED).unwrap();
    let cid = contact_id(&me.public_key);
    let req = SealRequest {
        recipient_public_key: me.public_key,
        contact_id: cid,
        message: message.into(),
        decoy: hidden.map(|(decoy, _)| decoy.into()),
        passphrase: hidden.map(|(_, pass)| pass.into()),
        aad: header_aad(Algorithm::MlKem768, FLAGS, &cid),
        flags: FLAGS,
        session_key: None,
    };
    let payload = MlKemCipher
        .seal_with_rng(&req, &mut TestRng::new(rng_domain))
        .unwrap()
        .payload;
    Envelope::new(Algorithm::MlKem768, FLAGS, cid, payload)
        .to_bytes()
        .unwrap()
}

/// Open a vector file from disk with the vector key; returns (text, slot).
fn open_vector(name: &str, passphrase: Option<&str>) -> (String, Slot) {
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
            session_key: None,
        })
        .map(|o| (o.text, o.slot))
        .unwrap()
}

fn opened(text: &str, slot: Slot) -> (String, Slot) {
    (text.into(), slot)
}

#[test]
fn mlkem_envelope_hidden() {
    let bytes = envelope(
        HIDDEN_MESSAGE,
        Some((HIDDEN_DECOY, HIDDEN_PASSPHRASE)),
        HIDDEN_RNG_DOMAIN,
    );
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
    let bytes = envelope(NORMAL_MESSAGE, None, NORMAL_RNG_DOMAIN);
    check_or_write("mlkem_envelope_normal.bin", &bytes);

    let name = "mlkem_envelope_normal.bin";
    assert_eq!(open_vector(name, None), opened(NORMAL_MESSAGE, Slot::A));
    assert_eq!(
        open_vector(name, Some("any")),
        opened(NORMAL_MESSAGE, Slot::A)
    );
}

#[test]
fn identity_from_seed_is_deterministic() {
    let a = MlKemCipher::identity_from_seed(&VECTOR_SEED).unwrap();
    let b = MlKemCipher::identity_from_seed(&VECTOR_SEED).unwrap();
    assert_eq!(a, b);
    assert!(MlKemCipher::identity_from_seed(&VECTOR_SEED[..63]).is_err());
}
