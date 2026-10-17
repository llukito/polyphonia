//! The payload inside an envelope: `kem_block || slot_A || slot_B`.
//!
//! See `docs/SPEC.md` §Payload. This module fixes the layout and the slot
//! padding; the actual encryption is done by a [`crate::Cipher`].

use crate::PolyError;
use crate::envelope::FLAG_SESSION;
use crate::types::Algorithm;

/// ML-KEM-768 ciphertext length (FIPS 203, Table 3: c = 1088 bytes).
pub const KEM_BLOCK_LEN_MLKEM768: usize = 1088;
/// RSA-2048-OAEP ciphertext length: 2048-bit modulus / 8 = 256 bytes.
pub const KEM_BLOCK_LEN_RSA2048: usize = 256;
/// The fake cipher has no KEM block.
pub const KEM_BLOCK_LEN_FAKE: usize = 0;

/// AES-GCM standard nonce size (NIST SP 800-38D recommends 96 bits).
pub const NONCE_LEN: usize = 12;
/// AES-GCM full tag size (128 bits).
pub const TAG_LEN: usize = 16;
/// Bytes a slot adds around its ciphertext: nonce + tag.
pub const SLOT_OVERHEAD: usize = NONCE_LEN + TAG_LEN;

/// Size of the u16 text-length prefix in a slot plaintext.
pub const TEXT_LEN_PREFIX: usize = 2;
/// Slot plaintexts are padded to a multiple of this (the AES block size),
/// which hides the exact message length a little.
pub const SLOT_BLOCK: usize = 16;
/// Longest text a slot can hold (u16 length prefix).
pub const MAX_TEXT_LEN: usize = u16::MAX as usize;
/// Shared secret length from KEM decapsulation / RSA unwrap.
pub const SHARED_SECRET_LEN: usize = 32;

/// HKDF info strings (SPEC §Key derivation).
pub const HKDF_INFO_SLOT_A: &[u8] = b"polyphonia/v1/slot-a";
pub const HKDF_INFO_SLOT_B: &[u8] = b"polyphonia/v1/slot-b";

impl Algorithm {
    /// Length of the KEM block this algorithm puts at the start of the payload.
    pub fn kem_block_len(self) -> usize {
        match self {
            Algorithm::MlKem768 => KEM_BLOCK_LEN_MLKEM768,
            Algorithm::Rsa2048 => KEM_BLOCK_LEN_RSA2048,
            Algorithm::Fake => KEM_BLOCK_LEN_FAKE,
        }
    }
}

/// KEM block length for an algorithm with these envelope flags
/// (0 when the SESSION flag is set).
pub fn kem_block_len(algorithm: Algorithm, flags: u8) -> usize {
    if flags & FLAG_SESSION != 0 {
        0
    } else {
        algorithm.kem_block_len()
    }
}

/// Padded slot plaintext length for a text of `text_len` bytes:
/// `2 + text_len` rounded up to a multiple of 16.
pub fn padded_len(text_len: usize) -> usize {
    (TEXT_LEN_PREFIX + text_len).div_ceil(SLOT_BLOCK) * SLOT_BLOCK
}

/// Encrypted slot length for a padded plaintext of `padded` bytes.
pub fn slot_len(padded: usize) -> usize {
    NONCE_LEN + padded + TAG_LEN
}

/// Total payload length for the given algorithm/flags when the longer of the
/// two slot texts is `max_text_len` bytes. Lets the engine check carrier
/// capacity before doing any (slow) crypto.
pub fn payload_len(algorithm: Algorithm, flags: u8, max_text_len: usize) -> usize {
    kem_block_len(algorithm, flags) + 2 * slot_len(padded_len(max_text_len))
}

/// Build a slot plaintext: `u16 BE length || UTF-8 text || zero padding`,
/// exactly `padded` bytes long. Use [`padded_len`] of the longer of the two
/// texts so both slots end up the same size.
pub fn slot_pad(text: &str, padded: usize) -> Result<Vec<u8>, PolyError> {
    let len = text.len();
    if len > MAX_TEXT_LEN {
        return Err(PolyError::MessageTooLong {
            len,
            max: MAX_TEXT_LEN,
        });
    }
    if !padded.is_multiple_of(SLOT_BLOCK) || padded < TEXT_LEN_PREFIX + len {
        return Err(PolyError::InvalidRequest(
            "slot padding must be a multiple of 16 and fit the text",
        ));
    }
    let mut out = Vec::with_capacity(padded);
    out.extend_from_slice(&(len as u16).to_be_bytes());
    out.extend_from_slice(text.as_bytes());
    out.resize(padded, 0);
    Ok(out)
}

/// Reverse of [`slot_pad`]. Checks the length prefix, UTF-8 and that the
/// padding is all zeros.
pub fn slot_unpad(plaintext: &[u8]) -> Result<String, PolyError> {
    let prefix = plaintext
        .get(..TEXT_LEN_PREFIX)
        .ok_or(PolyError::MalformedPayload(
            "slot shorter than length prefix",
        ))?;
    let len = u16::from_be_bytes([prefix[0], prefix[1]]) as usize;
    let end = TEXT_LEN_PREFIX + len;
    let text = plaintext
        .get(TEXT_LEN_PREFIX..end)
        .ok_or(PolyError::MalformedPayload("slot text length exceeds slot"))?;
    if plaintext[end..].iter().any(|&b| b != 0) {
        return Err(PolyError::MalformedPayload("slot padding is not zero"));
    }
    String::from_utf8(text.to_vec())
        .map_err(|_| PolyError::MalformedPayload("slot text is not UTF-8"))
}

/// A payload split into its three parts. Each slot is `nonce || ciphertext || tag`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
    pub kem_block: Vec<u8>,
    pub slot_a: Vec<u8>,
    pub slot_b: Vec<u8>,
}

impl Payload {
    /// Split payload bytes. Checks that the KEM block has the algorithm's
    /// length and that the rest is two equal, well-formed slots.
    pub fn split(bytes: &[u8], algorithm: Algorithm, flags: u8) -> Result<Payload, PolyError> {
        let kem_len = kem_block_len(algorithm, flags);
        if bytes.len() < kem_len {
            return Err(PolyError::MalformedPayload("shorter than KEM block"));
        }
        let (kem_block, slots) = bytes.split_at(kem_len);
        if slots.len() % 2 != 0 {
            return Err(PolyError::MalformedPayload("slots have unequal length"));
        }
        let slot = slots.len() / 2;
        if slot < SLOT_OVERHEAD + SLOT_BLOCK || !(slot - SLOT_OVERHEAD).is_multiple_of(SLOT_BLOCK) {
            return Err(PolyError::MalformedPayload("bad slot length"));
        }
        let (a, b) = slots.split_at(slot);
        Ok(Payload {
            kem_block: kem_block.to_vec(),
            slot_a: a.to_vec(),
            slot_b: b.to_vec(),
        })
    }

    /// Concatenate `kem_block || slot_a || slot_b`.
    pub fn join(&self) -> Vec<u8> {
        let mut out =
            Vec::with_capacity(self.kem_block.len() + self.slot_a.len() + self.slot_b.len());
        out.extend_from_slice(&self.kem_block);
        out.extend_from_slice(&self.slot_a);
        out.extend_from_slice(&self.slot_b);
        out
    }
}

/// The three parts of one encrypted slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotParts<'a> {
    pub nonce: &'a [u8],
    pub ciphertext: &'a [u8],
    pub tag: &'a [u8],
}

/// Split a slot into nonce, ciphertext and tag. The slot must be at least
/// [`SLOT_OVERHEAD`] bytes (guaranteed after [`Payload::split`]).
pub fn split_slot(slot: &[u8]) -> Result<SlotParts<'_>, PolyError> {
    if slot.len() < SLOT_OVERHEAD {
        return Err(PolyError::MalformedPayload("slot shorter than nonce + tag"));
    }
    let (nonce, rest) = slot.split_at(NONCE_LEN);
    let (ciphertext, tag) = rest.split_at(rest.len() - TAG_LEN);
    Ok(SlotParts {
        nonce,
        ciphertext,
        tag,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn padded_len_rounds_to_16() {
        assert_eq!(padded_len(0), 16);
        assert_eq!(padded_len(14), 16);
        assert_eq!(padded_len(15), 32);
        assert_eq!(padded_len(30), 32);
        assert_eq!(padded_len(31), 48);
    }

    #[test]
    fn pad_unpad_roundtrip() {
        for text in ["", "hi", "გამარჯობა 👋", &"x".repeat(1000)] {
            let p = slot_pad(text, padded_len(text.len())).unwrap();
            assert_eq!(p.len() % SLOT_BLOCK, 0);
            assert_eq!(slot_unpad(&p).unwrap(), text);
        }
    }

    #[test]
    fn pad_to_larger_size() {
        let p = slot_pad("hi", 64).unwrap();
        assert_eq!(p.len(), 64);
        assert_eq!(&p[..4], &[0, 2, b'h', b'i']);
        assert!(p[4..].iter().all(|&b| b == 0));
        assert_eq!(slot_unpad(&p).unwrap(), "hi");
    }

    #[test]
    fn pad_rejects_bad_sizes() {
        assert!(slot_pad("hello", 4).is_err());
        assert!(slot_pad("hi", 17).is_err());
        let long = "x".repeat(MAX_TEXT_LEN + 1);
        assert!(matches!(
            slot_pad(&long, padded_len(long.len())),
            Err(PolyError::MessageTooLong { .. })
        ));
    }

    #[test]
    fn unpad_rejects_garbage() {
        assert!(slot_unpad(&[]).is_err());
        assert!(slot_unpad(&[0]).is_err());
        assert!(slot_unpad(&[0, 5, b'a']).is_err()); // length past end
        assert!(slot_unpad(&[0, 1, b'a', 1]).is_err()); // nonzero padding
        assert!(slot_unpad(&[0, 1, 0xFF, 0]).is_err()); // invalid UTF-8
    }

    #[test]
    fn kem_lengths() {
        assert_eq!(Algorithm::MlKem768.kem_block_len(), 1088);
        assert_eq!(Algorithm::Rsa2048.kem_block_len(), 256);
        assert_eq!(Algorithm::Fake.kem_block_len(), 0);
        assert_eq!(kem_block_len(Algorithm::MlKem768, FLAG_SESSION), 0);
    }

    #[test]
    fn payload_len_formula() {
        // 1088 + 2 * (12 + 16 + 16)
        assert_eq!(payload_len(Algorithm::MlKem768, 0, 5), 1088 + 2 * 44);
        assert_eq!(payload_len(Algorithm::Fake, 0, 0), 88);
    }

    fn sample(algorithm: Algorithm, slot: usize) -> Payload {
        Payload {
            kem_block: vec![0xAA; algorithm.kem_block_len()],
            slot_a: vec![0xBB; slot],
            slot_b: vec![0xCC; slot],
        }
    }

    #[test]
    fn split_join_roundtrip() {
        for algorithm in Algorithm::ALL {
            for slot in [44, 60, 76, 1000 * 16 + SLOT_OVERHEAD] {
                let p = sample(algorithm, slot);
                let bytes = p.join();
                assert_eq!(bytes.len(), algorithm.kem_block_len() + 2 * slot);
                assert_eq!(Payload::split(&bytes, algorithm, 0).unwrap(), p);
            }
        }
    }

    #[test]
    fn split_session_has_no_kem_block() {
        let p = sample(Algorithm::Fake, 44);
        let parsed = Payload::split(&p.join(), Algorithm::MlKem768, FLAG_SESSION).unwrap();
        assert_eq!(parsed, p);
    }

    #[test]
    fn split_rejects_bad_layouts() {
        let a = Algorithm::Rsa2048;
        assert!(Payload::split(&[0; 100], a, 0).is_err()); // shorter than KEM
        assert!(Payload::split(&[0; 256 + 89], a, 0).is_err()); // odd
        assert!(Payload::split(&[0; 256 + 2 * 43], a, 0).is_err()); // slot too short
        assert!(Payload::split(&[0; 256 + 2 * 45], a, 0).is_err()); // not block aligned
        assert!(Payload::split(&[0; 256], a, 0).is_err()); // no slots
    }

    #[test]
    fn split_slot_parts() {
        let slot: Vec<u8> = (0..44).collect();
        let p = split_slot(&slot).unwrap();
        assert_eq!(
            (p.nonce.len(), p.ciphertext.len(), p.tag.len()),
            (12, 16, 16)
        );
        assert_eq!(p.nonce[0], 0);
        assert_eq!(p.ciphertext[0], 12);
        assert_eq!(p.tag[0], 28);
        assert!(split_slot(&[0; 27]).is_err());
    }

    proptest! {
        #[test]
        fn split_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..2000), alg in 0usize..3, flags in any::<u8>()) {
            let _ = Payload::split(&bytes, Algorithm::ALL[alg], flags);
        }

        #[test]
        fn unpad_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
            let _ = slot_unpad(&bytes);
        }
    }
}
