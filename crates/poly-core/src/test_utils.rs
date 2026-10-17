//! Shared conformance tests for [`Carrier`] and [`Cipher`] implementations.
//!
//! Enable with `poly-core = { workspace = true, features = ["test-utils"] }`
//! in `[dev-dependencies]`. Every implementation's test module calls one of
//! these; when it passes, the implementation meets the contract.
//!
//! Randomness is seeded so failures reproduce.

use rand::rngs::StdRng;
use rand::{Rng, RngExt, SeedableRng};

use crate::envelope::{ENVELOPE_OVERHEAD, Envelope, FLAG_SESSION, contact_id, header_aad};
use crate::payload::{Payload, SESSION_ID_LEN, SESSION_KEY_LEN, payload_len};
use crate::session::SessionKey;
use crate::types::{Algorithm, CarrierOptions, OpenRequest, SealRequest, Slot};
use crate::{Carrier, Cipher, PolyError};

/// Seed for all helper randomness.
const SEED: u64 = 0x504F_4C59; // "POLY"

/// Payload sizes tried by [`assert_carrier_roundtrip`] (filtered by capacity).
const CARRIER_PAYLOAD_SIZES: [usize; 6] = [0, 1, 45, 200, 1_200, 20_000];

/// File prefixes used to build junk that looks like a real format, to make
/// sure decoders reject it instead of panicking.
const JUNK_PREFIXES: [&[u8]; 5] = [
    b"",
    b"\x89PNG\r\n\x1a\n", // PNG signature
    b"RIFF\x24\x00\x00\x00WAVEfmt ",
    b"MThd\x00\x00\x00\x06",
    b"\xFF\xD8\xFF", // JPEG SOI
];

fn random_bytes(rng: &mut StdRng, len: usize) -> Vec<u8> {
    let mut v = vec![0u8; len];
    rng.fill_bytes(&mut v);
    v
}

fn random_envelope(rng: &mut StdRng, payload_len: usize) -> Envelope {
    let algorithm = Algorithm::ALL[rng.random_range(0..Algorithm::ALL.len())];
    let mut cid = [0u8; 8];
    rng.fill_bytes(&mut cid);
    Envelope::new(algorithm, 0, cid, random_bytes(rng, payload_len))
}

/// Check a carrier against the contract:
///
/// - random envelopes of several sizes up to `capacity` come back
///   byte-for-byte (trailing padding allowed) with default options;
/// - an envelope one byte over capacity gives `CarrierTooSmall`;
/// - junk input (random bytes, fake format headers) returns an error;
/// - truncated real files never panic.
pub fn assert_carrier_roundtrip(c: &dyn Carrier) {
    let mut rng = StdRng::seed_from_u64(SEED);
    let opts = CarrierOptions::default();
    let cap = c.capacity(&opts);
    let name = c.kind().name();

    let mut sizes: Vec<usize> = CARRIER_PAYLOAD_SIZES
        .iter()
        .map(|p| p + ENVELOPE_OVERHEAD)
        .filter(|&n| cap.is_none_or(|cap| n <= cap))
        .collect();
    if let Some(cap) = cap {
        assert!(
            cap >= ENVELOPE_OVERHEAD,
            "{name}: capacity {cap} cannot hold even an empty envelope"
        );
        sizes.push(cap);
    }

    let mut first_file = None;
    for size in sizes {
        let env = random_envelope(&mut rng, size - ENVELOPE_OVERHEAD);
        let bytes = env.to_bytes().expect("envelope serializes");
        let file = c
            .encode(&bytes, &opts)
            .unwrap_or_else(|e| panic!("{name}: encode {size} bytes failed: {e}"));
        let decoded = c
            .decode(&file)
            .unwrap_or_else(|e| panic!("{name}: decode {size} bytes failed: {e}"));
        assert!(
            decoded.len() >= bytes.len() && decoded[..bytes.len()] == bytes[..],
            "{name}: {size}-byte envelope did not come back byte-for-byte"
        );
        assert_eq!(
            Envelope::from_bytes(&decoded).as_ref(),
            Ok(&env),
            "{name}: decoded bytes do not parse back to the envelope"
        );
        first_file.get_or_insert(file);
    }

    if let Some(cap) = cap {
        let too_big = vec![0u8; cap + 1];
        assert_eq!(
            c.encode(&too_big, &opts),
            Err(PolyError::CarrierTooSmall {
                needed: cap + 1,
                available: cap
            }),
            "{name}: over-capacity encode must return CarrierTooSmall"
        );
    }

    for prefix in JUNK_PREFIXES {
        for len in [0usize, 1, 16, 100, 4096] {
            let mut junk = prefix.to_vec();
            junk.extend(random_bytes(&mut rng, len));
            assert!(
                c.decode(&junk).is_err(),
                "{name}: junk input ({} bytes, prefix {prefix:?}) must return an error",
                junk.len()
            );
        }
    }

    if let Some(file) = first_file {
        for cut in [0, 1, 8, file.len() / 4, file.len() / 2, file.len() - 1] {
            let _ = c.decode(&file[..cut.min(file.len())]);
        }
    }
}

/// Check a cipher against the contract:
///
/// - normal mode: slot A opens with or without a passphrase;
/// - hidden mode: right passphrase -> slot B, wrong passphrase -> slot A,
///   no passphrase -> slot A;
/// - both slots are equal length and the payload has the SPEC layout;
/// - decoy without passphrase -> `MissingPassphrase`, passphrase without
///   decoy -> `InvalidRequest`;
/// - a flipped byte in the KEM block or slot A, a wrong AAD or a wrong key
///   -> `DecryptFailed`; a truncated payload -> an error.
pub fn assert_cipher_roundtrip(c: &dyn Cipher) {
    let alg = c.algorithm();
    let me = c.generate_identity().expect("generate_identity");
    let cid = contact_id(&me.public_key);
    let aad = header_aad(alg, 0, &cid);

    let seal = |message: &str, decoy: Option<&str>, passphrase: Option<&str>| {
        c.seal(&SealRequest {
            recipient_public_key: me.public_key.clone(),
            contact_id: cid,
            message: message.into(),
            decoy: decoy.map(Into::into),
            passphrase: passphrase.map(Into::into),
            aad,
            flags: 0,
            session_key: None,
        })
        .map(|sealed| sealed.payload)
    };
    // Compared on (text, slot); the session a full message starts is checked separately.
    let open_with = |payload: &[u8], passphrase: Option<&str>, aad, key: &[u8]| {
        c.open(&OpenRequest {
            my_secret_key: key.to_vec(),
            contact_id: cid,
            payload: payload.to_vec(),
            passphrase: passphrase.map(Into::into),
            aad,
            flags: 0,
            session_key: None,
        })
        .map(|o| (o.text, o.slot))
    };
    let open = |payload: &[u8], passphrase: Option<&str>| {
        open_with(payload, passphrase, aad, &me.secret_key)
    };
    let opened = |text: &str, slot| Ok((text.to_string(), slot));

    // Normal mode.
    let long = "long message ".repeat(40);
    for msg in ["", "hi", "გამარჯობა, quantum world 👋", long.as_str()] {
        let p = seal(msg, None, None).expect("seal normal");
        assert_eq!(p.len(), payload_len(alg, 0, msg.len()), "payload length");
        let parts = Payload::split(&p, alg, 0).expect("payload splits");
        assert_eq!(parts.slot_a.len(), parts.slot_b.len());
        assert_eq!(
            open(&p, None),
            opened(msg, Slot::A),
            "normal, no passphrase"
        );
        assert_eq!(
            open(&p, Some("some passphrase")),
            opened(msg, Slot::A),
            "normal, with passphrase"
        );
    }
    assert_ne!(
        seal("same", None, None).unwrap(),
        seal("same", None, None).unwrap(),
        "sealing must be randomized"
    );

    // Hidden mode.
    let pass = "correct horse battery staple";
    for (real, decoy) in [
        ("meet at 6 by the old bridge", "happy birthday!"),
        ("short", "a much longer decoy message than the real one"),
    ] {
        let p = seal(real, Some(decoy), Some(pass)).expect("seal hidden");
        assert_eq!(p.len(), payload_len(alg, 0, real.len().max(decoy.len())));
        let parts = Payload::split(&p, alg, 0).expect("payload splits");
        assert_eq!(parts.slot_a.len(), parts.slot_b.len());
        assert_eq!(
            open(&p, Some(pass)),
            opened(real, Slot::B),
            "right passphrase"
        );
        assert_eq!(
            open(&p, Some("wrong")),
            opened(decoy, Slot::A),
            "wrong passphrase"
        );
        assert_eq!(open(&p, None), opened(decoy, Slot::A), "no passphrase");
    }

    // Request validation.
    assert_eq!(
        seal("x", Some("decoy"), None),
        Err(PolyError::MissingPassphrase)
    );
    assert!(matches!(
        seal("x", None, Some(pass)),
        Err(PolyError::InvalidRequest(_))
    ));

    // Tampering.
    let p = seal("tamper with me", None, None).unwrap();
    let kem = alg.kem_block_len();
    let slot = (p.len() - kem) / 2;
    let mut positions = vec![kem, kem + 12, kem + slot - 1];
    if kem > 0 {
        positions.extend([0, kem - 1]);
    }
    for i in positions {
        let mut t = p.clone();
        t[i] ^= 0x01;
        assert_eq!(
            open(&t, None),
            Err(PolyError::DecryptFailed),
            "flipped byte {i} must fail"
        );
    }
    let mut bad_aad = aad;
    bad_aad[14] ^= 0x01;
    assert_eq!(
        open_with(&p, None, bad_aad, &me.secret_key),
        Err(PolyError::DecryptFailed),
        "wrong AAD must fail"
    );
    let other = c.generate_identity().unwrap();
    assert!(
        open_with(&p, None, aad, &other.secret_key).is_err(),
        "wrong key must fail"
    );
    for cut in [0, 1, p.len() / 2, p.len() - 1] {
        assert!(
            open(&p[..cut], None).is_err(),
            "truncated to {cut} must fail"
        );
    }

    assert_cipher_sessions(c);
}

/// Check a cipher's session mode (SPEC §3 "Session mode"); called by
/// [`assert_cipher_roundtrip`]:
///
/// - a full message starts a session: seal and open return the same key;
/// - session messages start with the key's id, have the SPEC length and open
///   with that key in normal and hidden mode (right / wrong / no passphrase),
///   returning no new session key;
/// - a wrong session key or a changed session id -> `DecryptFailed`;
/// - SESSION flag without key, key without flag, unknown flag bits ->
///   `InvalidRequest`; a truncated session payload -> an error.
pub fn assert_cipher_sessions(c: &dyn Cipher) {
    let alg = c.algorithm();
    let me = c.generate_identity().expect("generate_identity");
    let cid = contact_id(&me.public_key);
    let full_aad = header_aad(alg, 0, &cid);
    let session_aad = header_aad(alg, FLAG_SESSION, &cid);

    let request = |message: &str, hidden: Option<(&str, &str)>, flags, key: Option<&SessionKey>| {
        SealRequest {
            recipient_public_key: me.public_key.clone(),
            contact_id: cid,
            message: message.into(),
            decoy: hidden.map(|(d, _)| d.into()),
            passphrase: hidden.map(|(_, p)| p.into()),
            aad: if flags & FLAG_SESSION != 0 {
                session_aad
            } else {
                full_aad
            },
            flags,
            session_key: key.cloned(),
        }
    };
    let open = |payload: &[u8], passphrase: Option<&str>, flags, key: Option<&SessionKey>| {
        c.open(&OpenRequest {
            my_secret_key: me.secret_key.clone(),
            contact_id: cid,
            payload: payload.to_vec(),
            passphrase: passphrase.map(Into::into),
            aad: if flags & FLAG_SESSION != 0 {
                session_aad
            } else {
                full_aad
            },
            flags,
            session_key: key.cloned(),
        })
    };
    let session_open = |payload: &[u8], passphrase: Option<&str>, key: &SessionKey| {
        open(payload, passphrase, FLAG_SESSION, Some(key))
            .map(|o| (o.text, o.slot, o.new_session_key))
    };

    // A full message starts a session; both sides get the same key.
    let full = c
        .seal(&request("start", None, 0, None))
        .expect("seal full message");
    let key = full
        .new_session_key
        .clone()
        .expect("a full message must start a session");
    let opened = open(&full.payload, None, 0, None).expect("open full message");
    assert_eq!(opened.text, "start");
    assert_eq!(
        opened.new_session_key.as_ref(),
        Some(&key),
        "sender and receiver must derive the same session key"
    );

    // Normal-mode session messages.
    for msg in ["", "hi", "a session message, much shorter on the wire"] {
        let sealed = c
            .seal(&request(msg, None, FLAG_SESSION, Some(&key)))
            .expect("seal session message");
        assert_eq!(
            sealed.new_session_key, None,
            "session messages start no session"
        );
        let p = &sealed.payload;
        assert_eq!(
            p.len(),
            payload_len(alg, FLAG_SESSION, msg.len()),
            "session payload length"
        );
        assert_eq!(
            p[..SESSION_ID_LEN],
            key.id(),
            "session payload starts with the id"
        );
        let parts = Payload::split(p, alg, FLAG_SESSION).expect("session payload splits");
        assert_eq!(parts.slot_a.len(), parts.slot_b.len());
        assert_eq!(session_open(p, None, &key), Ok((msg.into(), Slot::A, None)));
        assert_eq!(
            session_open(p, Some("some passphrase"), &key),
            Ok((msg.into(), Slot::A, None))
        );
    }

    // Hidden-mode session message.
    let pass = "orchid";
    let hidden = c
        .seal(&request(
            "real",
            Some(("decoy", pass)),
            FLAG_SESSION,
            Some(&key),
        ))
        .expect("seal hidden session message")
        .payload;
    assert_eq!(
        session_open(&hidden, Some(pass), &key),
        Ok(("real".into(), Slot::B, None))
    );
    assert_eq!(
        session_open(&hidden, Some("tulip"), &key),
        Ok(("decoy".into(), Slot::A, None))
    );
    assert_eq!(
        session_open(&hidden, None, &key),
        Ok(("decoy".into(), Slot::A, None))
    );

    // Wrong key, changed id, truncation.
    let p = c
        .seal(&request("tamper", None, FLAG_SESSION, Some(&key)))
        .unwrap()
        .payload;
    let other = SessionKey::from_bytes([0x5A; SESSION_KEY_LEN]);
    assert_eq!(
        session_open(&p, None, &other),
        Err(PolyError::DecryptFailed),
        "wrong session key"
    );
    let mut t = p.clone();
    t[0] ^= 0x01;
    assert_eq!(
        session_open(&t, None, &key),
        Err(PolyError::DecryptFailed),
        "changed session id"
    );
    for cut in [0, 1, SESSION_ID_LEN, p.len() / 2, p.len() - 1] {
        assert!(
            session_open(&p[..cut], None, &key).is_err(),
            "truncated to {cut} must fail"
        );
    }

    // Request rules.
    for (flags, key) in [
        (FLAG_SESSION, None),
        (0, Some(&key)),
        (FLAG_SESSION | 0b10, Some(&key)),
    ] {
        assert!(
            matches!(
                c.seal(&request("x", None, flags, key)),
                Err(PolyError::InvalidRequest(_))
            ),
            "seal with flags {flags:#b} and key {:?}",
            key.is_some()
        );
        assert!(
            matches!(
                open(&p, None, flags, key),
                Err(PolyError::InvalidRequest(_))
            ),
            "open with flags {flags:#b} and key {:?}",
            key.is_some()
        );
    }
}
