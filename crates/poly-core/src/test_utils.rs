//! Shared conformance tests for [`Carrier`] and [`Cipher`] implementations.
//!
//! Enable with `poly-core = { workspace = true, features = ["test-utils"] }`
//! in `[dev-dependencies]`. Every implementation's test module calls one of
//! these; when it passes, the implementation meets the contract.
//!
//! Randomness is seeded so failures reproduce.

use rand::rngs::StdRng;
use rand::{Rng, RngExt, SeedableRng};

use crate::envelope::{ENVELOPE_OVERHEAD, Envelope, contact_id, header_aad};
use crate::payload::{Payload, payload_len};
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
}
