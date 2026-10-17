//! RSA-2048-OAEP-SHA256 wrapping a random 32-byte S, then the same
//! HKDF / Argon2id / AES-256-GCM slots as ML-KEM. For comparison in the pitch:
//! RSA is NOT post-quantum (Shor's algorithm breaks it).
//!
//! Implements docs/SPEC.md §3:
//! - public key = PKCS#1 DER, secret key = PKCS#8 DER, 2048-bit modulus;
//! - seal: S = 32 random bytes, `kem_block = RSA-OAEP-SHA256-Encrypt(pk, S)`
//!   (MGF1-SHA256, empty label), 256 bytes;
//! - open: `S = RSA-OAEP-SHA256-Decrypt(sk, kem_block)`, then the shared slot logic;
//! - sessions exactly as for ML-KEM (SPEC §3 "Session mode").
//!
//! Known weakness: the `rsa` 0.9 crate is affected by the "Marvin" timing
//! side channel in decryption (RUSTSEC-2023-0071, no fixed 0.9 release).
//! Decryption uses blinding, which helps but does not remove it. Acceptable
//! for a comparison cipher in a hackathon demo; not for production.

use poly_core::payload::{KEM_BLOCK_LEN_RSA2048, Payload, SHARED_SECRET_LEN};
use poly_core::{Algorithm, Cipher, Identity, OpenRequest, Opened, PolyError, SealRequest, Sealed};
use rand::CryptoRng;
use rsa::pkcs1::{DecodeRsaPublicKey, EncodeRsaPublicKey};
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use rsa::sha2::Sha256;
use rsa::traits::PublicKeyParts;
use rsa::{Oaep, RsaPrivateKey, RsaPublicKey};
use zeroize::Zeroizing;

use crate::session::{self, Keying};
use crate::slots::{self, SharedSecret};

/// RSA modulus size in bits (SPEC §3: RSA-2048).
pub const RSA_MODULUS_BITS: usize = 2048;
/// Bits per byte, to turn the modulus size into the ciphertext size.
const BITS_PER_BYTE: usize = 8;

// An RSA-2048 ciphertext is exactly one modulus long: 2048 / 8 = 256 bytes (SPEC §3).
const _: () = assert!(RSA_MODULUS_BITS / BITS_PER_BYTE == KEM_BLOCK_LEN_RSA2048);

/// Classical cipher for comparison: RSA-2048-OAEP-SHA256 + AES-256-GCM.
#[derive(Debug, Default, Clone, Copy)]
pub struct RsaCipher;

/// OAEP with SHA-256 for both the hash and MGF1, and the empty label (SPEC §3).
fn oaep() -> Oaep {
    Oaep::new::<Sha256>()
}

/// Lets the `rsa` crate (which uses rand_core 0.6) draw from the same RNG as
/// the rest of the cipher (rand 0.10), so tests can make RSA reproducible too.
struct RngAdapter<'a, R: ?Sized>(&'a mut R);

impl<R: CryptoRng + ?Sized> rsa::rand_core::RngCore for RngAdapter<'_, R> {
    fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }

    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0.fill_bytes(dest);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rsa::rand_core::Error> {
        self.0.fill_bytes(dest);
        Ok(())
    }
}

// The wrapped RNG is a CryptoRng, so the adapter is one as well.
impl<R: CryptoRng + ?Sized> rsa::rand_core::CryptoRng for RngAdapter<'_, R> {}

/// Parse a PKCS#1 DER public key and check it is RSA-2048.
fn public_key(bytes: &[u8]) -> Result<RsaPublicKey, PolyError> {
    let pk = RsaPublicKey::from_pkcs1_der(bytes)
        .map_err(|_| PolyError::InvalidKey("RSA public key must be PKCS#1 DER"))?;
    if pk.size() != KEM_BLOCK_LEN_RSA2048 {
        return Err(PolyError::InvalidKey("RSA public key must be 2048 bits"));
    }
    Ok(pk)
}

/// Parse a PKCS#8 DER secret key and check it is RSA-2048.
fn secret_key(bytes: &[u8]) -> Result<RsaPrivateKey, PolyError> {
    let sk = RsaPrivateKey::from_pkcs8_der(bytes)
        .map_err(|_| PolyError::InvalidKey("RSA secret key must be PKCS#8 DER"))?;
    if sk.size() != KEM_BLOCK_LEN_RSA2048 {
        return Err(PolyError::InvalidKey("RSA secret key must be 2048 bits"));
    }
    Ok(sk)
}

impl RsaCipher {
    /// [`Cipher::seal`] with a caller-chosen random number generator, so
    /// tests and golden vectors can be reproducible. Real use goes through
    /// [`Cipher::seal`], which uses the operating system's secure RNG.
    ///
    /// Randomness is drawn in this order (full messages): 32 bytes for S, then
    /// the OAEP padding seed, then the slot randomness (see `slots::seal_slots`).
    pub fn seal_with_rng<R: CryptoRng + ?Sized>(
        &self,
        req: &SealRequest,
        rng: &mut R,
    ) -> Result<Sealed, PolyError> {
        let mode = slots::mode(req)?;
        let (kem_block, shared, new_session_key) =
            match session::keying(req.flags, req.session_key.as_ref())? {
                Keying::Full => {
                    let pk = public_key(&req.recipient_public_key)?;
                    let mut shared: SharedSecret = Zeroizing::new([0u8; SHARED_SECRET_LEN]);
                    rng.fill_bytes(&mut shared[..]);
                    let kem_block = pk
                        .encrypt(&mut RngAdapter(&mut *rng), oaep(), &shared[..])
                        .map_err(|_| PolyError::Crypto("RSA-OAEP encryption failed".into()))?;
                    let new_session_key = session::derive_session_key(&shared)?;
                    (kem_block, shared, Some(new_session_key))
                }
                Keying::Session(key) => (key.id().to_vec(), session::session_secret(key), None),
            };
        let (slot_a, slot_b) = slots::seal_slots(&shared, req, mode, rng)?;
        Ok(Sealed {
            payload: Payload {
                kem_block,
                slot_a,
                slot_b,
            }
            .join(),
            new_session_key,
        })
    }
}

impl Cipher for RsaCipher {
    fn algorithm(&self) -> Algorithm {
        Algorithm::Rsa2048
    }

    fn generate_identity(&self) -> Result<Identity, PolyError> {
        let sk = RsaPrivateKey::new(&mut RngAdapter(&mut rand::rng()), RSA_MODULUS_BITS)
            .map_err(|_| PolyError::Crypto("RSA key generation failed".into()))?;
        let public_key = sk
            .to_public_key()
            .to_pkcs1_der()
            .map_err(|_| PolyError::Crypto("RSA public key encoding failed".into()))?;
        let secret_key = sk
            .to_pkcs8_der()
            .map_err(|_| PolyError::Crypto("RSA secret key encoding failed".into()))?;
        Ok(Identity {
            public_key: public_key.as_bytes().to_vec(),
            secret_key: secret_key.as_bytes().to_vec(),
        })
    }

    fn seal(&self, req: &SealRequest) -> Result<Sealed, PolyError> {
        self.seal_with_rng(req, &mut rand::rng())
    }

    fn open(&self, req: &OpenRequest) -> Result<Opened, PolyError> {
        let keying = session::keying(req.flags, req.session_key.as_ref())?;
        let payload = Payload::split(&req.payload, Algorithm::Rsa2048, req.flags)?;
        match keying {
            Keying::Full => {
                let sk = secret_key(&req.my_secret_key)?;
                // Any OAEP failure (wrong key, tampered kem_block) is just
                // "nothing opens"; blinding mitigates timing side channels.
                let decrypted = Zeroizing::new(
                    sk.decrypt_blinded(
                        &mut RngAdapter(&mut rand::rng()),
                        oaep(),
                        &payload.kem_block,
                    )
                    .map_err(|_| PolyError::DecryptFailed)?,
                );
                let shared: SharedSecret = Zeroizing::new(
                    decrypted
                        .as_slice()
                        .try_into()
                        .map_err(|_| PolyError::DecryptFailed)?,
                );
                let mut opened = slots::open_slots(&shared, &payload.slot_a, &payload.slot_b, req)?;
                opened.new_session_key = Some(session::derive_session_key(&shared)?);
                Ok(opened)
            }
            Keying::Session(key) => {
                // No long-term key needed: the session key replaces S.
                session::check_session_id(&payload.kem_block, key)?;
                let shared = session::session_secret(key);
                slots::open_slots(&shared, &payload.slot_a, &payload.slot_b, req)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poly_core::envelope::{FLAG_SESSION, header_aad};
    use poly_core::payload::{NONCE_LEN, TAG_LEN, padded_len, slot_pad};
    use poly_core::{Slot, contact_id};

    /// A key size SPEC does not allow, small enough to generate quickly.
    const SMALL_MODULUS_BITS: usize = 1024;

    #[test]
    fn rsa_roundtrip() {
        poly_core::test_utils::assert_cipher_roundtrip(&RsaCipher);
    }

    fn request(public_key: Vec<u8>) -> SealRequest {
        let cid = contact_id(&public_key);
        SealRequest {
            recipient_public_key: public_key,
            contact_id: cid,
            message: "the real text".into(),
            decoy: None,
            passphrase: None,
            aad: header_aad(Algorithm::Rsa2048, 0, &cid),
            flags: 0,
            session_key: None,
        }
    }

    #[test]
    fn key_encodings_follow_spec() {
        let me = RsaCipher.generate_identity().unwrap();
        let pk = RsaPublicKey::from_pkcs1_der(&me.public_key).expect("public key is PKCS#1 DER");
        let sk = RsaPrivateKey::from_pkcs8_der(&me.secret_key).expect("secret key is PKCS#8 DER");
        assert_eq!(pk.size() * BITS_PER_BYTE, RSA_MODULUS_BITS);
        assert_eq!(sk.to_public_key(), pk);
    }

    #[test]
    fn rejects_wrong_keys() {
        let small =
            RsaPrivateKey::new(&mut RngAdapter(&mut rand::rng()), SMALL_MODULUS_BITS).unwrap();
        let small_pk = small.to_public_key().to_pkcs1_der().unwrap();
        let small_sk = small.to_pkcs8_der().unwrap();
        assert!(matches!(
            public_key(small_pk.as_bytes()),
            Err(PolyError::InvalidKey(_))
        ));
        assert!(matches!(
            secret_key(small_sk.as_bytes()),
            Err(PolyError::InvalidKey(_))
        ));
        assert!(matches!(
            public_key(&[0x30, 0x00]),
            Err(PolyError::InvalidKey(_))
        ));
        assert!(matches!(
            secret_key(b"not a key"),
            Err(PolyError::InvalidKey(_))
        ));
        // A PKCS#8 secret key is not a PKCS#1 public key, and vice versa.
        let me = RsaCipher.generate_identity().unwrap();
        assert!(public_key(&me.secret_key).is_err());
        assert!(secret_key(&me.public_key).is_err());
    }

    /// OAEP decrypts fine but S is not 32 bytes: nothing opens.
    #[test]
    fn wrong_length_secret_is_decrypt_failed() {
        let me = RsaCipher.generate_identity().unwrap();
        let req = request(me.public_key.clone());
        let pk = public_key(&me.public_key).unwrap();
        let short_s = [7u8; SHARED_SECRET_LEN - 1];
        let kem_block = pk
            .encrypt(&mut RngAdapter(&mut rand::rng()), oaep(), &short_s)
            .unwrap();
        // Well-formed slot sizes so only the S length is wrong.
        let padded = padded_len(req.message.len());
        let slot = vec![0u8; NONCE_LEN + slot_pad(&req.message, padded).unwrap().len() + TAG_LEN];
        let payload = [kem_block.as_slice(), &slot, &slot].concat();
        let opened = RsaCipher.open(&OpenRequest {
            my_secret_key: me.secret_key,
            contact_id: req.contact_id,
            payload,
            passphrase: None,
            aad: req.aad,
            flags: 0,
            session_key: None,
        });
        assert_eq!(opened, Err(PolyError::DecryptFailed));
    }

    /// The session key replaces S, so a session message opens without the
    /// long-term secret key (SPEC §3 "Session mode").
    #[test]
    fn session_message_needs_no_long_term_key() {
        let me = RsaCipher.generate_identity().unwrap();
        let key = RsaCipher
            .seal(&request(me.public_key.clone()))
            .unwrap()
            .new_session_key
            .unwrap();
        let cid = contact_id(&me.public_key);
        let aad = header_aad(Algorithm::Rsa2048, FLAG_SESSION, &cid);
        let payload = RsaCipher
            .seal(&SealRequest {
                recipient_public_key: Vec::new(),
                aad,
                flags: FLAG_SESSION,
                session_key: Some(key.clone()),
                ..request(me.public_key.clone())
            })
            .unwrap()
            .payload;
        let opened = RsaCipher
            .open(&OpenRequest {
                my_secret_key: Vec::new(),
                contact_id: cid,
                payload,
                passphrase: None,
                aad,
                flags: FLAG_SESSION,
                session_key: Some(key),
            })
            .unwrap();
        assert_eq!(
            (opened.text.as_str(), opened.slot),
            ("the real text", Slot::A)
        );
    }
}
