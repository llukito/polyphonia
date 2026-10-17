//! Golden vectors in tests/vectors/. Regenerate (only when the SPEC changes,
//! with the lead's approval) with:
//!
//!     POLY_REGEN_VECTORS=1 cargo test -p poly-engine --test golden

use std::path::PathBuf;

use poly_core::{CarrierKind, CarrierOptions, Envelope, Slot};
use poly_crypto::FakeCipher;
use poly_engine::{Algorithm, DecryptRequest, EncryptRequest, Engine, contact_id};

/// Fixed fake secret key for the vectors (documented in tests/vectors/README.md).
const VECTOR_SECRET: [u8; 32] = [0x11; 32];
const HIDDEN_MESSAGE: &str = "the real message";
const HIDDEN_DECOY: &str = "a harmless decoy";
const HIDDEN_PASSPHRASE: &str = "correct horse";

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

#[test]
fn envelope_min() {
    let me = FakeCipher::identity_from_secret(&VECTOR_SECRET).unwrap();
    let env = Envelope::new(Algorithm::Fake, 0, contact_id(&me.public_key), vec![]);
    check_or_write("envelope_min.bin", &env.to_bytes().unwrap());
}

#[test]
fn envelope_hidden() {
    let me = FakeCipher::identity_from_secret(&VECTOR_SECRET).unwrap();
    let engine = Engine::default();
    let file = engine
        .encrypt(&EncryptRequest {
            message: HIDDEN_MESSAGE.into(),
            decoy: Some(HIDDEN_DECOY.into()),
            passphrase: Some(HIDDEN_PASSPHRASE.into()),
            recipient_public_key: me.public_key.clone(),
            algorithm: Algorithm::Fake,
            carrier: CarrierKind::RawDebug,
            options: CarrierOptions::default(),
            session_key: None,
        })
        .unwrap();
    check_or_write("envelope_hidden.bin", &file.bytes);

    let golden = std::fs::read(vectors_dir().join("envelope_hidden.bin")).unwrap();
    let d = engine
        .decrypt(&DecryptRequest {
            file: golden,
            my_secret_key: me.secret_key.clone(),
            my_public_key: me.public_key.clone(),
            passphrase: Some(HIDDEN_PASSPHRASE.into()),
            session_keys: Vec::new(),
        })
        .unwrap();
    assert_eq!((d.text.as_str(), d.slot), (HIDDEN_MESSAGE, Slot::B));
}
