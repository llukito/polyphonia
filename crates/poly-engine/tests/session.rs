//! End-to-end tests of SESSION mode (SPEC §3 "Session mode", §6) through the
//! default engine with real carriers.

use poly_core::{CarrierKind, CarrierOptions, PolyError, SessionKey, Slot};
use poly_engine::{
    Algorithm, DecryptRequest, Decrypted, EncodedFile, EncryptRequest, Engine, Identity,
    generate_identity,
};

/// Session messages are sent over the carriers a full ML-KEM message can't use.
const SMALL_CARRIERS: [CarrierKind; 3] = [
    CarrierKind::RobustPng,
    CarrierKind::MusicMidi,
    CarrierKind::MusicWav,
];

/// SPEC §3 example: a 2-byte message as a session message is 23 + 8 + 2×44 bytes.
const SESSION_ENVELOPE_LEN_HI: usize = 119;

/// Any 32 bytes that are not the real session key.
const OTHER_KEY: [u8; 32] = [0x42; 32];

fn request(
    to: &Identity,
    algorithm: Algorithm,
    carrier: CarrierKind,
    message: &str,
    session_key: Option<&SessionKey>,
) -> EncryptRequest {
    EncryptRequest {
        message: message.into(),
        decoy: None,
        passphrase: None,
        recipient_public_key: to.public_key.clone(),
        algorithm,
        carrier,
        options: CarrierOptions::default(),
        session_key: session_key.cloned(),
    }
}

fn decrypt(
    me: &Identity,
    file: &EncodedFile,
    passphrase: Option<&str>,
    session_keys: &[SessionKey],
) -> Result<Decrypted, PolyError> {
    Engine::default().decrypt(&DecryptRequest {
        file: file.bytes.clone(),
        my_secret_key: me.secret_key.clone(),
        my_public_key: me.public_key.clone(),
        passphrase: passphrase.map(Into::into),
        session_keys: session_keys.to_vec(),
    })
}

/// Send a full message to `to` and return the session key both sides got.
fn start_session(to: &Identity, algorithm: Algorithm) -> SessionKey {
    let carrier = CarrierKind::RawDebug;
    let file = Engine::default()
        .encrypt(&request(to, algorithm, carrier, "full message", None))
        .unwrap();
    let sender_key = file
        .session_key
        .clone()
        .expect("a full message starts a session");
    let opened = decrypt(to, &file, None, &[]).unwrap();
    assert_eq!(opened.text, "full message");
    assert_eq!(opened.session_key.as_ref(), Some(&sender_key));
    sender_key
}

#[test]
fn mlkem_session_messages_fit_the_small_carriers() {
    let engine = Engine::default();
    let giorgi = generate_identity(Algorithm::MlKem768).unwrap();

    // A full ML-KEM message doesn't fit the small carriers...
    for carrier in SMALL_CARRIERS {
        let full = request(&giorgi, Algorithm::MlKem768, carrier, "hi", None);
        assert!(
            matches!(
                engine.encrypt(&full),
                Err(PolyError::CarrierTooSmall { .. })
            ),
            "{carrier:?}: a full ML-KEM message should not fit"
        );
    }

    // ...so Ana sends one over StealthPng, which starts the session...
    let first = engine
        .encrypt(&request(
            &giorgi,
            Algorithm::MlKem768,
            CarrierKind::StealthPng,
            "hi Giorgi",
            None,
        ))
        .unwrap();
    let ana_key = first
        .session_key
        .clone()
        .expect("a full message starts a session");
    let opened = decrypt(&giorgi, &first, None, &[]).unwrap();
    assert_eq!(opened.text, "hi Giorgi");
    let giorgi_key = opened
        .session_key
        .expect("the receiver gets the session too");
    assert_eq!(giorgi_key, ana_key);

    // ...and her next messages are session messages over the small carriers.
    let stored = [SessionKey::from_bytes(OTHER_KEY), giorgi_key];
    for carrier in SMALL_CARRIERS {
        let file = engine
            .encrypt(&request(
                &giorgi,
                Algorithm::MlKem768,
                carrier,
                "see you at 6",
                Some(&ana_key),
            ))
            .unwrap_or_else(|e| panic!("{carrier:?}: {e}"));
        assert_eq!(
            file.session_key, None,
            "a session message starts no session"
        );
        let inspected = engine.inspect(&file.bytes).unwrap();
        assert!(inspected.envelope.is_session());
        assert_eq!(inspected.envelope.session_id(), Some(ana_key.id()));

        let d =
            decrypt(&giorgi, &file, None, &stored).unwrap_or_else(|e| panic!("{carrier:?}: {e}"));
        assert_eq!(d.text, "see you at 6");
        assert_eq!(d.slot, Slot::A);
        assert_eq!(d.carrier, carrier);
        assert_eq!(d.algorithm, Algorithm::MlKem768);
        assert_eq!(d.session_key, None);
    }
}

#[test]
fn hidden_mode_in_a_session() {
    let giorgi = generate_identity(Algorithm::MlKem768).unwrap();
    let key = start_session(&giorgi, Algorithm::MlKem768);
    let file = Engine::default()
        .encrypt(&EncryptRequest {
            decoy: Some("happy birthday!".into()),
            passphrase: Some("orchid".into()),
            ..request(
                &giorgi,
                Algorithm::MlKem768,
                CarrierKind::MusicMidi,
                "the real plan",
                Some(&key),
            )
        })
        .unwrap();
    let keys = [key];
    let real = decrypt(&giorgi, &file, Some("orchid"), &keys).unwrap();
    assert_eq!((real.text.as_str(), real.slot), ("the real plan", Slot::B));
    for pass in [None, Some("tulip")] {
        let decoy = decrypt(&giorgi, &file, pass, &keys).unwrap();
        assert_eq!(
            (decoy.text.as_str(), decoy.slot),
            ("happy birthday!", Slot::A)
        );
    }
}

#[test]
fn unknown_session() {
    let giorgi = generate_identity(Algorithm::MlKem768).unwrap();
    let key = start_session(&giorgi, Algorithm::MlKem768);
    let file = Engine::default()
        .encrypt(&request(
            &giorgi,
            Algorithm::MlKem768,
            CarrierKind::RawDebug,
            "x",
            Some(&key),
        ))
        .unwrap();
    assert_eq!(
        decrypt(&giorgi, &file, None, &[]).map(|d| d.text),
        Err(PolyError::UnknownSession),
        "no stored keys"
    );
    assert_eq!(
        decrypt(&giorgi, &file, None, &[SessionKey::from_bytes(OTHER_KEY)]).map(|d| d.text),
        Err(PolyError::UnknownSession),
        "only other keys"
    );
}

#[test]
fn wrong_recipient_is_checked_before_the_session() {
    let giorgi = generate_identity(Algorithm::MlKem768).unwrap();
    let key = start_session(&giorgi, Algorithm::MlKem768);
    let file = Engine::default()
        .encrypt(&request(
            &giorgi,
            Algorithm::MlKem768,
            CarrierKind::RawDebug,
            "x",
            Some(&key),
        ))
        .unwrap();
    let eve = generate_identity(Algorithm::MlKem768).unwrap();
    assert_eq!(
        decrypt(&eve, &file, None, &[key]).map(|d| d.text),
        Err(PolyError::WrongRecipient)
    );
}

#[test]
fn session_envelope_length() {
    for algorithm in [Algorithm::MlKem768, Algorithm::Fake] {
        let me = generate_identity(algorithm).unwrap();
        let key = start_session(&me, algorithm);
        let req = request(&me, algorithm, CarrierKind::RawDebug, "hi", Some(&key));
        assert_eq!(Engine::envelope_len(&req), SESSION_ENVELOPE_LEN_HI);
        let file = Engine::default().encrypt(&req).unwrap();
        assert_eq!(file.bytes.len(), SESSION_ENVELOPE_LEN_HI);
    }
}

#[test]
fn fake_sessions_work_through_the_engine() {
    // Lets carrier authors test small session envelopes without real crypto.
    let me = generate_identity(Algorithm::Fake).unwrap();
    let key = start_session(&me, Algorithm::Fake);
    let file = Engine::default()
        .encrypt(&request(
            &me,
            Algorithm::Fake,
            CarrierKind::RobustPng,
            "fake session",
            Some(&key),
        ))
        .unwrap();
    let d = decrypt(&me, &file, None, &[key]).unwrap();
    assert_eq!(
        (d.text.as_str(), d.carrier),
        ("fake session", CarrierKind::RobustPng)
    );
}
