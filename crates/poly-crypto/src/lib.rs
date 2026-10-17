//! Cipher implementations for Polyphonia (role 1).
//!
//! - [`FakeCipher`]: works today, NOT SECURE, for testing the pipeline.
//! - [`MlKemCipher`]: ML-KEM-768 + AES-256-GCM. Post-quantum.
//! - [`RsaCipher`]: RSA-2048-OAEP-SHA256 + AES-256-GCM. For comparison, not post-quantum.
//!
//! The format every cipher must produce is in `docs/SPEC.md` §Payload.

mod fake;
mod mlkem;
mod rsa;
mod session;
mod slots;

pub use fake::FakeCipher;
pub use mlkem::MlKemCipher;
pub use rsa::RsaCipher;

use poly_core::{Algorithm, Cipher};

/// One instance of every cipher, in [`Algorithm::ALL`] order.
pub fn all_ciphers() -> Vec<Box<dyn Cipher>> {
    Algorithm::ALL
        .iter()
        .map(|&a| -> Box<dyn Cipher> {
            match a {
                Algorithm::MlKem768 => Box::new(MlKemCipher),
                Algorithm::Rsa2048 => Box::new(RsaCipher),
                Algorithm::Fake => Box::new(FakeCipher),
            }
        })
        .collect()
}
