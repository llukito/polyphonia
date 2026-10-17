//! End-to-end tests of RSA-2048 through the default engine and real carriers.

use poly_core::{CarrierKind, CarrierOptions, PolyError, Slot};
use poly_engine::{
    Algorithm, DecryptRequest, Decrypted, EncodedFile, EncryptRequest, Engine, Identity,
    generate_identity,
};

/// SPEC §3 example: "hi" as a full RSA-2048 message is 23 + 256 + 2×44 bytes.
const RSA_ENVELOPE_LEN_HI: usize = 367;

fn decrypt(
    me: &Identity,
    file: &EncodedFile,
    passphrase: Option<&str>,
) -> Result<Decrypted, PolyError> {
    Engine::default().decrypt(&DecryptRequest {
        file: file.bytes.clone(),
        my_secret_key: me.secret_key.clone(),
        my_public_key: me.public_key.clone(),
        passphrase: passphrase.map(Into::into),
        session_keys: Vec::new(),
    })
}

fn request(to: &Identity, carrier: CarrierKind, message: &str) -> EncryptRequest {
    EncryptRequest {
        message: message.into(),
        decoy: None,
        passphrase: None,
        recipient_public_key: to.public_key.clone(),
        algorithm: Algorithm::Rsa2048,
        carrier,
        options: CarrierOptions::default(),
        session_key: None,
    }
}

#[test]
fn full_rsa_message_fits_robust_png() {
    // Unlike a full ML-KEM message (1199 bytes), RSA's 367 fit RobustPng's ~500.
    let giorgi = generate_identity(Algorithm::Rsa2048).unwrap();
    let req = request(&giorgi, CarrierKind::RobustPng, "hi");
    assert_eq!(Engine::envelope_len(&req), RSA_ENVELOPE_LEN_HI);
    let file = Engine::default().encrypt(&req).unwrap();
    let d = decrypt(&giorgi, &file, None).unwrap();
    assert_eq!(
        (d.text.as_str(), d.slot, d.carrier, d.algorithm),
        ("hi", Slot::A, CarrierKind::RobustPng, Algorithm::Rsa2048)
    );
    assert_eq!(
        d.session_key, file.session_key,
        "both sides start the same session"
    );
    assert!(d.session_key.is_some());
}

#[test]
fn rsa_hidden_mode_and_session_through_the_engine() {
    let engine = Engine::default();
    let giorgi = generate_identity(Algorithm::Rsa2048).unwrap();
    let first = engine
        .encrypt(&EncryptRequest {
            decoy: Some("happy birthday!".into()),
            passphrase: Some("orchid".into()),
            ..request(&giorgi, CarrierKind::StealthPng, "the real plan")
        })
        .unwrap();
    let real = decrypt(&giorgi, &first, Some("orchid")).unwrap();
    assert_eq!((real.text.as_str(), real.slot), ("the real plan", Slot::B));
    let decoy = decrypt(&giorgi, &first, None).unwrap();
    assert_eq!(
        (decoy.text.as_str(), decoy.slot),
        ("happy birthday!", Slot::A)
    );

    // The session that hidden message started carries the next one over MIDI.
    let key = real.session_key.expect("a full message starts a session");
    let file = engine
        .encrypt(&EncryptRequest {
            session_key: Some(key.clone()),
            ..request(&giorgi, CarrierKind::MusicMidi, "see you at 6")
        })
        .unwrap();
    let d = engine
        .decrypt(&DecryptRequest {
            file: file.bytes,
            my_secret_key: giorgi.secret_key.clone(),
            my_public_key: giorgi.public_key.clone(),
            passphrase: None,
            session_keys: vec![key],
        })
        .unwrap();
    assert_eq!(
        (d.text.as_str(), d.algorithm),
        ("see you at 6", Algorithm::Rsa2048)
    );
}

#[test]
fn rsa_key_for_the_wrong_algorithm_is_rejected() {
    // An ML-KEM public key is not an RSA key.
    let mlkem = generate_identity(Algorithm::MlKem768).unwrap();
    let req = request(&mlkem, CarrierKind::RawDebug, "x");
    assert!(matches!(
        Engine::default().encrypt(&req),
        Err(PolyError::InvalidKey(_))
    ));
}
