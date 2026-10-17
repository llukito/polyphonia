//! Property tests for MlKemCipher::open: it never panics, random input never
//! decrypts, and a single flipped bit anywhere follows the SPEC §3 open rules,
//! for full messages and for session messages, and for RsaCipher full messages.

use std::sync::OnceLock;

use poly_core::envelope::{FLAG_SESSION, header_aad};
use poly_core::payload::{
    KEM_BLOCK_LEN_MLKEM768, KEM_BLOCK_LEN_RSA2048, SESSION_ID_LEN, SLOT_BLOCK, SLOT_OVERHEAD,
};
use poly_core::{
    Algorithm, Cipher, OpenRequest, PolyError, SealRequest, SessionKey, Slot, contact_id,
};
use poly_crypto::{MlKemCipher, RsaCipher};
use proptest::prelude::*;

/// Cases for tests that run Argon2id (~19 MiB, slow in debug builds) per case.
const SLOW_CASES: u32 = 24;
/// Cases for tests without Argon2id.
const FAST_CASES: u32 = 256;
/// Cases for RSA tests without Argon2id (each case does one RSA decryption,
/// ~tens of ms in debug builds).
const RSA_CASES: u32 = 64;
/// Longest arbitrary payload tried: a bit more than a short ML-KEM payload.
const MAX_RANDOM_PAYLOAD: usize = 1_400;
/// Well-formed payloads use 1..=MAX_BLOCKS padded 16-byte blocks per slot.
const MAX_BLOCKS: usize = 4;
/// Longest arbitrary secret key tried (the real one is 64 bytes).
const MAX_RANDOM_KEY: usize = 100;
/// The real ML-KEM seed length (SPEC §3).
const SEED_LEN: usize = 64;

const REAL: &str = "the real message";
const DECOY: &str = "a harmless decoy";
const PASS: &str = "correct horse";

struct Fixture {
    seed: Vec<u8>,
    contact_id: [u8; 8],
    aad: [u8; 15],
    hidden_payload: Vec<u8>,
}

/// One identity and one hidden-mode payload, shared by every case.
fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let me = MlKemCipher.generate_identity().unwrap();
        let cid = contact_id(&me.public_key);
        let aad = header_aad(Algorithm::MlKem768, 0, &cid);
        let hidden_payload = MlKemCipher
            .seal(&SealRequest {
                recipient_public_key: me.public_key,
                contact_id: cid,
                message: REAL.into(),
                decoy: Some(DECOY.into()),
                passphrase: Some(PASS.into()),
                aad,
                flags: 0,
                session_key: None,
            })
            .unwrap()
            .payload;
        Fixture {
            seed: me.secret_key,
            contact_id: cid,
            aad,
            hidden_payload,
        }
    })
}

/// Open with the fixture's contact id and AAD; returns (text, slot).
fn open(key: &[u8], payload: &[u8], passphrase: Option<&str>) -> Result<(String, Slot), PolyError> {
    let f = fixture();
    MlKemCipher
        .open(&OpenRequest {
            my_secret_key: key.to_vec(),
            contact_id: f.contact_id,
            payload: payload.to_vec(),
            passphrase: passphrase.map(Into::into),
            aad: f.aad,
            flags: 0,
            session_key: None,
        })
        .map(|o| (o.text, o.slot))
}

/// Arbitrary bytes, or random bytes with exactly the shape of an ML-KEM
/// payload (so decapsulation and both slot decryptions really run).
fn payloads() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..MAX_RANDOM_PAYLOAD),
        (1..=MAX_BLOCKS).prop_flat_map(|blocks| {
            let slot = SLOT_OVERHEAD + blocks * SLOT_BLOCK;
            proptest::collection::vec(any::<u8>(), KEM_BLOCK_LEN_MLKEM768 + 2 * slot)
        }),
    ]
}

/// A valid-length random seed, or a key of arbitrary length.
fn keys() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        proptest::collection::vec(any::<u8>(), SEED_LEN),
        proptest::collection::vec(any::<u8>(), 0..MAX_RANDOM_KEY),
    ]
}

/// What SPEC §3 says opening the hidden payload gives after flipping a bit
/// at `index`, with or without the right passphrase.
fn expected_after_flip(index: usize, with_pass: bool) -> Result<(String, Slot), PolyError> {
    let len = fixture().hidden_payload.len();
    let slot_a_end = KEM_BLOCK_LEN_MLKEM768 + (len - KEM_BLOCK_LEN_MLKEM768) / 2;
    let a = Ok((DECOY.to_string(), Slot::A));
    let b = Ok((REAL.to_string(), Slot::B));
    if index < KEM_BLOCK_LEN_MLKEM768 {
        // Wrong S (implicit rejection): neither slot opens.
        Err(PolyError::DecryptFailed)
    } else if index < slot_a_end {
        // Slot A broken: only slot B can open, and only with the passphrase.
        if with_pass {
            b
        } else {
            Err(PolyError::DecryptFailed)
        }
    } else {
        // Slot B broken: slot A (the decoy) opens, silently.
        a
    }
}

struct SessionFixture {
    key: SessionKey,
    aad: [u8; 15],
    hidden_payload: Vec<u8>,
}

/// A session started by a full message to the fixture identity, and one
/// hidden-mode session message in it.
fn session_fixture() -> &'static SessionFixture {
    static FIXTURE: OnceLock<SessionFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let f = fixture();
        let key = MlKemCipher
            .open(&OpenRequest {
                my_secret_key: f.seed.clone(),
                contact_id: f.contact_id,
                payload: f.hidden_payload.clone(),
                passphrase: None,
                aad: f.aad,
                flags: 0,
                session_key: None,
            })
            .unwrap()
            .new_session_key
            .unwrap();
        let aad = header_aad(Algorithm::MlKem768, FLAG_SESSION, &f.contact_id);
        let hidden_payload = MlKemCipher
            .seal(&SealRequest {
                recipient_public_key: Vec::new(),
                contact_id: f.contact_id,
                message: REAL.into(),
                decoy: Some(DECOY.into()),
                passphrase: Some(PASS.into()),
                aad,
                flags: FLAG_SESSION,
                session_key: Some(key.clone()),
            })
            .unwrap()
            .payload;
        SessionFixture {
            key,
            aad,
            hidden_payload,
        }
    })
}

/// Open a session message with the session fixture's key; returns (text, slot).
fn open_session(payload: &[u8], passphrase: Option<&str>) -> Result<(String, Slot), PolyError> {
    let sf = session_fixture();
    MlKemCipher
        .open(&OpenRequest {
            my_secret_key: Vec::new(),
            contact_id: fixture().contact_id,
            payload: payload.to_vec(),
            passphrase: passphrase.map(Into::into),
            aad: sf.aad,
            flags: FLAG_SESSION,
            session_key: Some(sf.key.clone()),
        })
        .map(|o| (o.text, o.slot))
}

/// What SPEC §3 says opening the hidden session message gives after flipping
/// a bit at `index`.
fn expected_after_session_flip(index: usize, with_pass: bool) -> Result<(String, Slot), PolyError> {
    let len = session_fixture().hidden_payload.len();
    let slot_a_end = SESSION_ID_LEN + (len - SESSION_ID_LEN) / 2;
    if index < SESSION_ID_LEN {
        // Changed session id: it no longer matches the key.
        Err(PolyError::DecryptFailed)
    } else if index < slot_a_end {
        if with_pass {
            Ok((REAL.to_string(), Slot::B))
        } else {
            Err(PolyError::DecryptFailed)
        }
    } else {
        Ok((DECOY.to_string(), Slot::A))
    }
}

/// Random slots behind the right session id (so both slot decryptions run).
fn session_payloads() -> impl Strategy<Value = Vec<u8>> {
    (1..=MAX_BLOCKS).prop_flat_map(|blocks| {
        let slot = SLOT_OVERHEAD + blocks * SLOT_BLOCK;
        proptest::collection::vec(any::<u8>(), 2 * slot)
            .prop_map(|slots| [session_fixture().key.id().as_slice(), &slots].concat())
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(FAST_CASES))]

    #[test]
    fn random_session_slots_never_open(payload in session_payloads()) {
        prop_assert_eq!(open_session(&payload, None), Err(PolyError::DecryptFailed));
    }

    #[test]
    fn flipped_bit_in_session_message_without_passphrase(
        index in 0..session_fixture().hidden_payload.len(),
        bit in 0..8u8,
    ) {
        let mut payload = session_fixture().hidden_payload.clone();
        payload[index] ^= 1 << bit;
        prop_assert_eq!(open_session(&payload, None), expected_after_session_flip(index, false));
    }

    #[test]
    fn random_input_never_opens_without_passphrase(payload in payloads(), key in keys()) {
        prop_assert!(open(&key, &payload, None).is_err());
    }

    #[test]
    fn flipped_bit_without_passphrase(index in 0..fixture().hidden_payload.len(), bit in 0..8u8) {
        let mut payload = fixture().hidden_payload.clone();
        payload[index] ^= 1 << bit;
        prop_assert_eq!(open(&fixture().seed, &payload, None), expected_after_flip(index, false));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(SLOW_CASES))]

    #[test]
    fn random_input_never_opens_with_passphrase(payload in payloads(), key in keys(), pass in "[a-z]{0,12}") {
        prop_assert!(open(&key, &payload, Some(&pass)).is_err());
    }

    #[test]
    fn flipped_bit_in_session_message_with_passphrase(
        index in 0..session_fixture().hidden_payload.len(),
        bit in 0..8u8,
    ) {
        let mut payload = session_fixture().hidden_payload.clone();
        payload[index] ^= 1 << bit;
        prop_assert_eq!(open_session(&payload, Some(PASS)), expected_after_session_flip(index, true));
    }

    #[test]
    fn flipped_bit_with_passphrase(index in 0..fixture().hidden_payload.len(), bit in 0..8u8) {
        let mut payload = fixture().hidden_payload.clone();
        payload[index] ^= 1 << bit;
        prop_assert_eq!(open(&fixture().seed, &payload, Some(PASS)), expected_after_flip(index, true));
    }
}

// ---- RSA-2048 ---------------------------------------------------------------

struct RsaFixture {
    secret_key: Vec<u8>,
    contact_id: [u8; 8],
    aad: [u8; 15],
    hidden_payload: Vec<u8>,
}

/// One RSA identity and one hidden-mode payload, shared by every case.
fn rsa_fixture() -> &'static RsaFixture {
    static FIXTURE: OnceLock<RsaFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let me = RsaCipher.generate_identity().unwrap();
        let cid = contact_id(&me.public_key);
        let aad = header_aad(Algorithm::Rsa2048, 0, &cid);
        let hidden_payload = RsaCipher
            .seal(&SealRequest {
                recipient_public_key: me.public_key,
                contact_id: cid,
                message: REAL.into(),
                decoy: Some(DECOY.into()),
                passphrase: Some(PASS.into()),
                aad,
                flags: 0,
                session_key: None,
            })
            .unwrap()
            .payload;
        RsaFixture {
            secret_key: me.secret_key,
            contact_id: cid,
            aad,
            hidden_payload,
        }
    })
}

fn open_rsa(payload: &[u8], passphrase: Option<&str>) -> Result<(String, Slot), PolyError> {
    let f = rsa_fixture();
    RsaCipher
        .open(&OpenRequest {
            my_secret_key: f.secret_key.clone(),
            contact_id: f.contact_id,
            payload: payload.to_vec(),
            passphrase: passphrase.map(Into::into),
            aad: f.aad,
            flags: 0,
            session_key: None,
        })
        .map(|o| (o.text, o.slot))
}

/// What SPEC §3 says opening the RSA hidden payload gives after flipping a bit.
fn expected_after_rsa_flip(index: usize, with_pass: bool) -> Result<(String, Slot), PolyError> {
    let len = rsa_fixture().hidden_payload.len();
    let slot_a_end = KEM_BLOCK_LEN_RSA2048 + (len - KEM_BLOCK_LEN_RSA2048) / 2;
    if index < KEM_BLOCK_LEN_RSA2048 {
        // OAEP rejects the changed ciphertext: no S, nothing opens.
        Err(PolyError::DecryptFailed)
    } else if index < slot_a_end {
        if with_pass {
            Ok((REAL.to_string(), Slot::B))
        } else {
            Err(PolyError::DecryptFailed)
        }
    } else {
        Ok((DECOY.to_string(), Slot::A))
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(RSA_CASES))]

    #[test]
    fn rsa_flipped_bit_without_passphrase(
        index in 0..rsa_fixture().hidden_payload.len(),
        bit in 0..8u8,
    ) {
        let mut payload = rsa_fixture().hidden_payload.clone();
        payload[index] ^= 1 << bit;
        prop_assert_eq!(open_rsa(&payload, None), expected_after_rsa_flip(index, false));
    }

    #[test]
    fn rsa_random_kem_block_never_opens(kem in proptest::collection::vec(any::<u8>(), KEM_BLOCK_LEN_RSA2048)) {
        let mut payload = rsa_fixture().hidden_payload.clone();
        payload[..KEM_BLOCK_LEN_RSA2048].copy_from_slice(&kem);
        prop_assert_eq!(open_rsa(&payload, None), Err(PolyError::DecryptFailed));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(SLOW_CASES))]

    #[test]
    fn rsa_flipped_bit_with_passphrase(
        index in 0..rsa_fixture().hidden_payload.len(),
        bit in 0..8u8,
    ) {
        let mut payload = rsa_fixture().hidden_payload.clone();
        payload[index] ^= 1 << bit;
        prop_assert_eq!(open_rsa(&payload, Some(PASS)), expected_after_rsa_flip(index, true));
    }
}
