//! Parse the golden vectors in tests/vectors/ (see its README for the fields).
//! They are produced by poly-engine's `golden` test with FakeCipher + RawDebug.

use poly_core::payload::{Payload, slot_unpad, split_slot};
use poly_core::{Algorithm, Envelope};

const ENVELOPE_MIN: &[u8] = include_bytes!("../../../tests/vectors/envelope_min.bin");
const ENVELOPE_HIDDEN: &[u8] = include_bytes!("../../../tests/vectors/envelope_hidden.bin");

/// contact_id of the vectors' fake public key (secret = 32 x 0x11).
const VECTOR_CONTACT_ID: [u8; 8] = [0xab, 0xec, 0xdb, 0xe8, 0xcf, 0x6c, 0xfa, 0x0b];

#[test]
fn envelope_min_fields() {
    assert_eq!(ENVELOPE_MIN.len(), poly_core::envelope::ENVELOPE_OVERHEAD);
    let env = Envelope::from_bytes(ENVELOPE_MIN).unwrap();
    assert_eq!(env.version, 1);
    assert_eq!(env.algorithm, Algorithm::Fake);
    assert_eq!(env.flags, 0);
    assert_eq!(env.contact_id, VECTOR_CONTACT_ID);
    assert!(env.payload.is_empty());
    assert_eq!(env.to_bytes().unwrap(), ENVELOPE_MIN);
}

#[test]
fn envelope_hidden_fields() {
    let env = Envelope::from_bytes(ENVELOPE_HIDDEN).unwrap();
    assert_eq!(env.algorithm, Algorithm::Fake);
    assert_eq!(env.flags, 0);
    assert_eq!(env.contact_id, VECTOR_CONTACT_ID);
    // 2 slots x (12 nonce + 32 padded plaintext + 16 tag)
    assert_eq!(env.payload.len(), 120);
    assert_eq!(env.to_bytes().unwrap(), ENVELOPE_HIDDEN);

    let p = Payload::split(&env.payload, env.algorithm, env.flags).unwrap();
    assert!(p.kem_block.is_empty());
    assert_eq!(p.slot_a.len(), 60);
    assert_eq!(p.slot_b.len(), 60);
    // FakeCipher stores the padded plaintext in the clear, so the slots unpad directly.
    let a = split_slot(&p.slot_a).unwrap();
    let b = split_slot(&p.slot_b).unwrap();
    assert_eq!(a.nonce, [0; 12]);
    assert_eq!(slot_unpad(a.ciphertext).unwrap(), "a harmless decoy");
    assert_eq!(slot_unpad(b.ciphertext).unwrap(), "the real message");
}
