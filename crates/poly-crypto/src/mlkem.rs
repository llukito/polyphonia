//! ML-KEM-768 (FIPS 203) + HKDF-SHA256 + Argon2id + AES-256-GCM.
//!
//! Implements docs/SPEC.md §3:
//! - public key = 1184-byte encapsulation key, secret key = 64-byte seed (d || z);
//! - seal: `(kem_block, S) = Encapsulate(public key)`, kem_block = 1088 bytes;
//! - open: `S = Decapsulate(seed, kem_block)`, then the shared slot logic in [`crate::slots`].

use ml_kem::array::{AssocArraySize, typenum::Unsigned};
use ml_kem::ml_kem_768::{Ciphertext, DecapsulationKey, EncapsulationKey};
use ml_kem::{
    Decapsulate, Encapsulate, Kem, Key, KeyExport, KeySizeUser, MlKem768, Seed, SharedKey,
};
use poly_core::payload::{KEM_BLOCK_LEN_MLKEM768, Payload};
use poly_core::{Algorithm, Cipher, Identity, OpenRequest, Opened, PolyError, SealRequest, Sealed};
use rand::{CryptoRng, Rng};
use zeroize::{Zeroize, Zeroizing};

use crate::slots::{self, SharedSecret};

/// ML-KEM-768 encapsulation (public) key length (FIPS 203, Table 3: ek = 1184 bytes).
pub const MLKEM768_PUBLIC_KEY_LEN: usize = 1184;
/// ML-KEM secret key stored as the seed d || z, 32 + 32 bytes (FIPS 203, §7.1 ML-KEM.KeyGen).
pub const MLKEM768_SEED_LEN: usize = 64;

/// MVP envelopes always have flags = 0, so the payload has a KEM block (SPEC §2).
const MVP_FLAGS: u8 = 0;

// The ml-kem crate's sizes must match SPEC; checked when compiling.
const _: () = {
    assert!(<EncapsulationKey as KeySizeUser>::KeySize::USIZE == MLKEM768_PUBLIC_KEY_LEN);
    assert!(<MlKem768 as Kem>::CiphertextSize::USIZE == KEM_BLOCK_LEN_MLKEM768);
    assert!(<Seed as AssocArraySize>::Size::USIZE == MLKEM768_SEED_LEN);
};

/// Post-quantum cipher: ML-KEM-768 + AES-256-GCM.
#[derive(Debug, Default, Clone, Copy)]
pub struct MlKemCipher;

impl MlKemCipher {
    /// Deterministic identity from a 64-byte seed (used for golden vectors).
    pub fn identity_from_seed(seed: &[u8]) -> Result<Identity, PolyError> {
        let dk = secret_key(seed)?;
        Ok(Identity {
            public_key: dk.encapsulation_key().to_bytes().to_vec(),
            secret_key: seed.to_vec(),
        })
    }

    /// [`Cipher::seal`] with a caller-chosen random number generator, so
    /// tests and golden vectors can be reproducible. Real use goes through
    /// [`Cipher::seal`], which uses the operating system's secure RNG.
    ///
    /// Randomness is drawn in this order: 32 bytes for ML-KEM encapsulation,
    /// then the slot randomness (see `slots::seal_slots`).
    pub fn seal_with_rng<R: CryptoRng + ?Sized>(
        &self,
        req: &SealRequest,
        rng: &mut R,
    ) -> Result<Sealed, PolyError> {
        let mode = slots::mode(req)?;
        if req.flags != MVP_FLAGS || req.session_key.is_some() {
            return Err(PolyError::NotImplemented("SESSION mode in MlKemCipher"));
        }
        let (kem_block, shared) = public_key(&req.recipient_public_key)?.encapsulate_with_rng(rng);
        let shared = shared_secret(shared);
        let (slot_a, slot_b) = slots::seal_slots(&shared, req, mode, rng)?;
        Ok(Sealed {
            payload: Payload {
                kem_block: kem_block.to_vec(),
                slot_a,
                slot_b,
            }
            .join(),
            new_session_key: None,
        })
    }
}

/// Parse a 1184-byte encapsulation key.
fn public_key(bytes: &[u8]) -> Result<EncapsulationKey, PolyError> {
    let bytes = Key::<EncapsulationKey>::try_from(bytes)
        .map_err(|_| PolyError::InvalidKey("ML-KEM-768 public key must be 1184 bytes"))?;
    EncapsulationKey::new(&bytes)
        .map_err(|_| PolyError::InvalidKey("ML-KEM-768 public key is not well-formed"))
}

/// Rebuild the decapsulation key from its 64-byte seed.
/// (With ml-kem's `zeroize` feature the key wipes itself when dropped.)
fn secret_key(bytes: &[u8]) -> Result<DecapsulationKey, PolyError> {
    let seed = Zeroizing::new(
        Seed::try_from(bytes)
            .map_err(|_| PolyError::InvalidKey("ML-KEM-768 secret key must be a 64-byte seed"))?,
    );
    Ok(DecapsulationKey::from_seed(*seed))
}

/// Copy the KEM's shared key into a wiping [`SharedSecret`] and wipe the original.
fn shared_secret(mut key: SharedKey) -> SharedSecret {
    let shared: SharedSecret = Zeroizing::new(key.into());
    key.zeroize();
    shared
}

impl Cipher for MlKemCipher {
    fn algorithm(&self) -> Algorithm {
        Algorithm::MlKem768
    }

    fn generate_identity(&self) -> Result<Identity, PolyError> {
        let mut seed = Zeroizing::new([0u8; MLKEM768_SEED_LEN]);
        rand::rng().fill_bytes(&mut seed[..]);
        Self::identity_from_seed(&seed[..])
    }

    fn seal(&self, req: &SealRequest) -> Result<Sealed, PolyError> {
        self.seal_with_rng(req, &mut rand::rng())
    }

    fn open(&self, req: &OpenRequest) -> Result<Opened, PolyError> {
        if req.flags != MVP_FLAGS || req.session_key.is_some() {
            return Err(PolyError::NotImplemented("SESSION mode in MlKemCipher"));
        }
        let dk = secret_key(&req.my_secret_key)?;
        let payload = Payload::split(&req.payload, Algorithm::MlKem768, MVP_FLAGS)?;
        let kem_block = Ciphertext::try_from(payload.kem_block.as_slice())
            .map_err(|_| PolyError::MalformedPayload("ML-KEM-768 kem_block must be 1088 bytes"))?;
        // Never fails: a tampered kem_block gives a different S (implicit
        // rejection), so slot A then fails to open with DecryptFailed.
        let shared = shared_secret(dk.decapsulate(&kem_block));
        slots::open_slots(&shared, &payload.slot_a, &payload.slot_b, req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poly_core::Slot;

    #[test]
    fn mlkem_roundtrip() {
        poly_core::test_utils::assert_cipher_roundtrip(&MlKemCipher);
    }

    #[test]
    fn mlkem_kem_block_is_1088_bytes() {
        let me = MlKemCipher.generate_identity().unwrap();
        let cid = poly_core::contact_id(&me.public_key);
        let payload = MlKemCipher
            .seal(&SealRequest {
                recipient_public_key: me.public_key.clone(),
                contact_id: cid,
                message: "hi".into(),
                decoy: None,
                passphrase: None,
                aad: poly_core::envelope::header_aad(Algorithm::MlKem768, 0, &cid),
                flags: 0,
                session_key: None,
            })
            .unwrap()
            .payload;
        assert_eq!(
            payload.len(),
            poly_core::payload::payload_len(Algorithm::MlKem768, 0, 2)
        );
    }

    #[test]
    fn key_sizes_match_spec() {
        let me = MlKemCipher.generate_identity().unwrap();
        assert_eq!(me.public_key.len(), MLKEM768_PUBLIC_KEY_LEN);
        assert_eq!(me.secret_key.len(), MLKEM768_SEED_LEN);
        assert!(MlKemCipher.seal(&request(vec![0; 10], None, None)).is_err());
        assert!(secret_key(&[0; 63]).is_err());
    }

    fn request(public_key: Vec<u8>, decoy: Option<&str>, pass: Option<&str>) -> SealRequest {
        let cid = poly_core::contact_id(&public_key);
        SealRequest {
            recipient_public_key: public_key,
            contact_id: cid,
            message: "the real text".into(),
            decoy: decoy.map(Into::into),
            passphrase: pass.map(Into::into),
            aad: poly_core::envelope::header_aad(Algorithm::MlKem768, 0, &cid),
            flags: 0,
            session_key: None,
        }
    }

    /// A passphrase on decrypt must never cause an error, or it would reveal
    /// whether a hidden message exists: it silently returns slot A.
    #[test]
    fn passphrase_on_normal_message_returns_slot_a() {
        let me = MlKemCipher.generate_identity().unwrap();
        let req = request(me.public_key.clone(), None, None);
        let payload = MlKemCipher.seal(&req).unwrap().payload;
        let opened = MlKemCipher
            .open(&OpenRequest {
                my_secret_key: me.secret_key,
                contact_id: req.contact_id,
                payload,
                passphrase: Some("any passphrase".into()),
                aad: req.aad,
                flags: 0,
                session_key: None,
            })
            .unwrap();
        assert_eq!(opened.text, "the real text");
        assert_eq!(opened.slot, Slot::A);
    }
}
