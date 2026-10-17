//! ML-KEM-768 (FIPS 203) + HKDF-SHA256 + Argon2id + AES-256-GCM.
//!
//! TODO(role 1): implement exactly per docs/SPEC.md §Payload and §Key derivation.
//! kem_block = ML-KEM-768 ciphertext (1088 bytes); shared secret S = 32 bytes.

use poly_core::{Algorithm, Cipher, Identity, OpenRequest, Opened, PolyError, SealRequest};

/// Post-quantum cipher. Not implemented yet (role 1).
#[derive(Debug, Default, Clone, Copy)]
pub struct MlKemCipher;

impl Cipher for MlKemCipher {
    fn algorithm(&self) -> Algorithm {
        Algorithm::MlKem768
    }

    fn generate_identity(&self) -> Result<Identity, PolyError> {
        Err(PolyError::NotImplemented("ML-KEM-768 cipher (role 1)"))
    }

    fn seal(&self, _req: &SealRequest) -> Result<Vec<u8>, PolyError> {
        Err(PolyError::NotImplemented("ML-KEM-768 cipher (role 1)"))
    }

    fn open(&self, _req: &OpenRequest) -> Result<Opened, PolyError> {
        Err(PolyError::NotImplemented("ML-KEM-768 cipher (role 1)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "role 1: remove when implemented"]
    fn mlkem_roundtrip() {
        poly_core::test_utils::assert_cipher_roundtrip(&MlKemCipher);
    }

    #[test]
    #[ignore = "role 1: remove when implemented"]
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
            })
            .unwrap();
        assert_eq!(
            payload.len(),
            poly_core::payload::payload_len(Algorithm::MlKem768, 0, 2)
        );
    }
}
