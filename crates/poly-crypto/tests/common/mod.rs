//! Shared by the poly-crypto integration tests: a portable deterministic RNG
//! and the fixed inputs of the ML-KEM golden vectors.

use std::convert::Infallible;

use rand::{TryCryptoRng, TryRng};
use sha2::{Digest, Sha256};

/// Fixed ML-KEM-768 secret key (64-byte seed d || z) for the golden vectors
/// (documented in tests/vectors/README.md).
pub const VECTOR_SEED: [u8; 64] = [0x22; 64];
/// TestRng domain for `mlkem_envelope_hidden.bin`.
pub const HIDDEN_RNG_DOMAIN: &[u8] = b"polyphonia/test-rng/mlkem-hidden";
/// TestRng domain for `mlkem_envelope_normal.bin`.
pub const NORMAL_RNG_DOMAIN: &[u8] = b"polyphonia/test-rng/mlkem-normal";
/// TestRng domain for `mlkem_envelope_session.bin`, a hidden-mode session
/// message in the session started by `mlkem_envelope_hidden.bin`.
pub const SESSION_RNG_DOMAIN: &[u8] = b"polyphonia/test-rng/mlkem-session";
pub const HIDDEN_MESSAGE: &str = "the real message";
pub const HIDDEN_DECOY: &str = "a harmless decoy";
pub const HIDDEN_PASSPHRASE: &str = "correct horse";
pub const NORMAL_MESSAGE: &str = "lunch at noon?";

/// SHA-256 output length, the size of one block of the TestRng stream.
const BLOCK_LEN: usize = 32;

/// Deterministic RNG for tests and golden vectors. NOT SECURE, never use it
/// for real messages.
///
/// The output stream is `SHA-256(domain || 0u64 BE) || SHA-256(domain || 1u64 BE) || ...`.
/// Every call takes the next bytes of that stream; `next_u32`/`next_u64` read
/// them little-endian. Unlike `rand::rngs::StdRng`, whose algorithm may change
/// between rand versions, this is fully specified, so the vectors stay valid.
pub struct TestRng {
    domain: Vec<u8>,
    counter: u64,
    block: [u8; BLOCK_LEN],
    used: usize,
}

impl TestRng {
    pub fn new(domain: &[u8]) -> Self {
        TestRng {
            domain: domain.to_vec(),
            counter: 0,
            block: [0; BLOCK_LEN],
            used: BLOCK_LEN,
        }
    }

    fn fill(&mut self, dst: &mut [u8]) {
        for byte in dst {
            if self.used == BLOCK_LEN {
                self.block = Sha256::new()
                    .chain_update(&self.domain)
                    .chain_update(self.counter.to_be_bytes())
                    .finalize()
                    .into();
                self.counter += 1;
                self.used = 0;
            }
            *byte = self.block[self.used];
            self.used += 1;
        }
    }
}

impl TryRng for TestRng {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        let mut bytes = [0; 4];
        self.fill(&mut bytes);
        Ok(u32::from_le_bytes(bytes))
    }

    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        let mut bytes = [0; 8];
        self.fill(&mut bytes);
        Ok(u64::from_le_bytes(bytes))
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
        self.fill(dst);
        Ok(())
    }
}

impl TryCryptoRng for TestRng {}
