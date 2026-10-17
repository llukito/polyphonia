//! RSA-2048-OAEP-SHA256 wrapping a random 32-byte S, then the same
//! HKDF / Argon2id / AES-256-GCM slots as ML-KEM. For comparison in the pitch.
//!
//! TODO(role 1): implement exactly per docs/SPEC.md §Payload and §Key derivation.
//! kem_block = RSA-OAEP ciphertext (256 bytes).

use poly_core::{Algorithm, Cipher, Identity, OpenRequest, Opened, PolyError, SealRequest};

/// Classical cipher for comparison. Not implemented yet (role 1).
#[derive(Debug, Default, Clone, Copy)]
pub struct RsaCipher;

impl Cipher for RsaCipher {
    fn algorithm(&self) -> Algorithm {
        Algorithm::Rsa2048
    }

    fn generate_identity(&self) -> Result<Identity, PolyError> {
        Err(PolyError::NotImplemented("RSA-2048 cipher (role 1)"))
    }

    fn seal(&self, _req: &SealRequest) -> Result<Vec<u8>, PolyError> {
        Err(PolyError::NotImplemented("RSA-2048 cipher (role 1)"))
    }

    fn open(&self, _req: &OpenRequest) -> Result<Opened, PolyError> {
        Err(PolyError::NotImplemented("RSA-2048 cipher (role 1)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "role 1: remove when implemented"]
    fn rsa_roundtrip() {
        poly_core::test_utils::assert_cipher_roundtrip(&RsaCipher);
    }
}
