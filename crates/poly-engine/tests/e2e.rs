//! End-to-end tests with FakeCipher + RawDebugCarrier.

use poly_core::{Carrier, CarrierKind, CarrierOptions, PolyError, Slot};
use poly_crypto::FakeCipher;
use poly_engine::{
    Algorithm, DecryptRequest, EncryptRequest, Engine, Identity, RawDebugCarrier, generate_identity,
};

fn me() -> Identity {
    generate_identity(Algorithm::Fake).unwrap()
}

fn request(to: &Identity, message: &str) -> EncryptRequest {
    EncryptRequest {
        message: message.into(),
        decoy: None,
        passphrase: None,
        recipient_public_key: to.public_key.clone(),
        algorithm: Algorithm::Fake,
        carrier: CarrierKind::RawDebug,
        options: CarrierOptions::default(),
    }
}

fn open(
    engine: &Engine,
    me: &Identity,
    file: &[u8],
    passphrase: Option<&str>,
) -> Result<(String, Slot), PolyError> {
    engine
        .decrypt(&DecryptRequest {
            file: file.to_vec(),
            my_secret_key: me.secret_key.clone(),
            my_public_key: me.public_key.clone(),
            passphrase: passphrase.map(Into::into),
        })
        .map(|d| (d.text, d.slot))
}

#[test]
fn normal_mode() {
    let engine = Engine::default();
    let me = me();
    let file = engine.encrypt(&request(&me, "hello bob")).unwrap();
    assert_eq!(file.extension, "bin");
    assert_eq!(file.carrier, CarrierKind::RawDebug);
    assert_eq!(file.algorithm, Algorithm::Fake);
    assert_eq!(
        file.bytes.len(),
        Engine::envelope_len(&request(&me, "hello bob"))
    );

    let d = engine
        .decrypt(&DecryptRequest {
            file: file.bytes.clone(),
            my_secret_key: me.secret_key.clone(),
            my_public_key: me.public_key.clone(),
            passphrase: None,
        })
        .unwrap();
    assert_eq!(d.text, "hello bob");
    assert_eq!(d.slot, Slot::A);
    assert_eq!(d.carrier, CarrierKind::RawDebug);
    assert_eq!(d.algorithm, Algorithm::Fake);
    // A passphrase on a normal message still opens slot A.
    assert_eq!(
        open(&engine, &me, &file.bytes, Some("x")).unwrap(),
        ("hello bob".into(), Slot::A)
    );
}

#[test]
fn hidden_mode_and_wrong_passphrase() {
    let engine = Engine::default();
    let me = me();
    let mut req = request(&me, "the real plan");
    req.decoy = Some("see you at dinner".into());
    req.passphrase = Some("orchid".into());
    let file = engine.encrypt(&req).unwrap().bytes;

    assert_eq!(
        open(&engine, &me, &file, Some("orchid")).unwrap(),
        ("the real plan".into(), Slot::B)
    );
    assert_eq!(
        open(&engine, &me, &file, Some("tulip")).unwrap(),
        ("see you at dinner".into(), Slot::A)
    );
    assert_eq!(
        open(&engine, &me, &file, None).unwrap(),
        ("see you at dinner".into(), Slot::A)
    );
}

#[test]
fn hidden_and_normal_files_have_same_size() {
    let engine = Engine::default();
    let me = me();
    let normal = engine.encrypt(&request(&me, "same length")).unwrap();
    let mut req = request(&me, "same length");
    req.decoy = Some("other text!".into());
    req.passphrase = Some("pw".into());
    let hidden = engine.encrypt(&req).unwrap();
    assert_eq!(normal.bytes.len(), hidden.bytes.len());
}

#[test]
fn decoy_without_passphrase() {
    let me = me();
    let mut req = request(&me, "x");
    req.decoy = Some("y".into());
    assert_eq!(
        Engine::default().encrypt(&req),
        Err(PolyError::MissingPassphrase)
    );
}

#[test]
fn junk_file() {
    let engine = Engine::default();
    let me = me();
    for junk in [
        &b""[..],
        b"hello",
        &[0u8; 1000],
        b"\x89PNG\r\n\x1a\nnot really",
    ] {
        assert_eq!(
            open(&engine, &me, junk, None),
            Err(PolyError::UnrecognizedFile)
        );
    }
}

#[test]
fn damaged_file_reports_checksum_mismatch() {
    let engine = Engine::default();
    let me = me();
    let file = engine.encrypt(&request(&me, "fragile")).unwrap().bytes;
    for i in [6, 20, file.len() / 2, file.len() - 1] {
        let mut damaged = file.clone();
        damaged[i] ^= 0x01;
        assert_eq!(
            open(&engine, &me, &damaged, None),
            Err(PolyError::ChecksumMismatch),
            "byte {i}"
        );
    }
}

#[test]
fn wrong_recipient() {
    let engine = Engine::default();
    let alice = me();
    let bob = me();
    let file = engine.encrypt(&request(&alice, "for alice")).unwrap().bytes;
    assert_eq!(
        open(&engine, &bob, &file, None),
        Err(PolyError::WrongRecipient)
    );
}

#[test]
fn inspect_without_key() {
    let engine = Engine::default();
    let me = me();
    let file = engine.encrypt(&request(&me, "peek")).unwrap().bytes;
    let i = engine.inspect(&file).unwrap();
    assert_eq!(i.carrier, CarrierKind::RawDebug);
    assert_eq!(i.envelope.algorithm, Algorithm::Fake);
    assert_eq!(
        i.envelope.contact_id,
        poly_engine::contact_id(&me.public_key)
    );
}

#[test]
fn not_implemented_parts_report_it() {
    let engine = Engine::default();
    let me = me();
    let mut req = request(&me, "x");
    req.carrier = CarrierKind::MusicMidi;
    assert!(matches!(
        engine.encrypt(&req),
        Err(PolyError::NotImplemented(_))
    ));
}

/// A carrier with a tiny fixed capacity, to test the capacity check.
struct TinyCarrier;

/// Capacity of [`TinyCarrier`]: smaller than any Fake envelope (23 + 88 bytes).
const TINY_CAPACITY: usize = 50;

impl Carrier for TinyCarrier {
    fn kind(&self) -> CarrierKind {
        CarrierKind::StealthPng
    }
    fn file_extension(&self) -> &'static str {
        "tiny"
    }
    fn mime_type(&self) -> &'static str {
        "application/x-tiny"
    }
    fn capacity(&self, _opts: &CarrierOptions) -> Option<usize> {
        Some(TINY_CAPACITY)
    }
    fn encode(&self, _envelope: &[u8], _opts: &CarrierOptions) -> Result<Vec<u8>, PolyError> {
        panic!("encode must not be called when capacity is exceeded")
    }
    fn decode(&self, _file: &[u8]) -> Result<Vec<u8>, PolyError> {
        Err(PolyError::NotThisCarrier)
    }
}

#[test]
fn carrier_too_small() {
    let engine = Engine::with(vec![Box::new(FakeCipher)], vec![Box::new(TinyCarrier)]);
    let me = me();
    let mut req = request(&me, "hi");
    req.carrier = CarrierKind::StealthPng;
    let needed = Engine::envelope_len(&req);
    assert_eq!(
        engine.encrypt(&req),
        Err(PolyError::CarrierTooSmall {
            needed,
            available: TINY_CAPACITY
        })
    );
}

/// A carrier that stores the envelope reversed, to test auto-detect order.
struct ReversedCarrier;

impl Carrier for ReversedCarrier {
    fn kind(&self) -> CarrierKind {
        CarrierKind::MusicMidi
    }
    fn file_extension(&self) -> &'static str {
        "rev"
    }
    fn mime_type(&self) -> &'static str {
        "application/x-rev"
    }
    fn capacity(&self, _opts: &CarrierOptions) -> Option<usize> {
        None
    }
    fn encode(&self, envelope: &[u8], _opts: &CarrierOptions) -> Result<Vec<u8>, PolyError> {
        Ok(envelope.iter().rev().copied().collect())
    }
    fn decode(&self, file: &[u8]) -> Result<Vec<u8>, PolyError> {
        Ok(file.iter().rev().copied().collect())
    }
}

#[test]
fn auto_detect_skips_carriers_that_do_not_match() {
    let engine = Engine::with(
        vec![Box::new(FakeCipher)],
        vec![Box::new(RawDebugCarrier), Box::new(ReversedCarrier)],
    );
    let me = me();
    let mut req = request(&me, "backwards");
    req.carrier = CarrierKind::MusicMidi;
    let file = engine.encrypt(&req).unwrap();
    assert_eq!(file.extension, "rev");
    let d = engine
        .decrypt(&DecryptRequest {
            file: file.bytes,
            my_secret_key: me.secret_key.clone(),
            my_public_key: me.public_key.clone(),
            passphrase: None,
        })
        .unwrap();
    assert_eq!(
        (d.text.as_str(), d.carrier),
        ("backwards", CarrierKind::MusicMidi)
    );
}

#[test]
fn checksum_mismatch_is_remembered_across_carriers() {
    // Raw decodes the damaged file but the CRC fails; Reversed decodes junk.
    let engine = Engine::with(
        vec![Box::new(FakeCipher)],
        vec![Box::new(RawDebugCarrier), Box::new(ReversedCarrier)],
    );
    let me = me();
    let mut file = engine.encrypt(&request(&me, "x")).unwrap().bytes;
    file[30] ^= 0xFF;
    assert_eq!(engine.inspect(&file), Err(PolyError::ChecksumMismatch));
}
