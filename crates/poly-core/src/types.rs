//! Plain data types shared by the traits and the engine.

use crate::PolyError;
use crate::session::SessionKey;

/// Length of a contact id: the first 8 bytes of SHA-256(public key).
/// 64 bits makes a collision between two contacts practically impossible
/// while costing few bytes in the slow music carrier (SPEC §Envelope).
pub const CONTACT_ID_LEN: usize = 8;

/// Identifies the recipient of a message.
pub type ContactId = [u8; CONTACT_ID_LEN];

/// Envelope algorithm byte (SPEC §Envelope, offset 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Algorithm {
    /// ML-KEM-768 (FIPS 203) + AES-256-GCM. Post-quantum.
    MlKem768 = 1,
    /// RSA-2048-OAEP-SHA256 + AES-256-GCM. For comparison.
    Rsa2048 = 2,
    /// Fake cipher for testing. NOT SECURE.
    Fake = 0xFF,
}

impl Algorithm {
    /// Every algorithm, in registry order.
    pub const ALL: [Algorithm; 3] = [Algorithm::MlKem768, Algorithm::Rsa2048, Algorithm::Fake];

    /// Wire value of this algorithm.
    pub fn id(self) -> u8 {
        self as u8
    }

    /// Parse the wire value.
    pub fn from_id(id: u8) -> Result<Self, PolyError> {
        match id {
            1 => Ok(Algorithm::MlKem768),
            2 => Ok(Algorithm::Rsa2048),
            0xFF => Ok(Algorithm::Fake),
            other => Err(PolyError::UnknownAlgorithm(other)),
        }
    }

    /// Short lowercase name used by the CLI and key files.
    pub fn name(self) -> &'static str {
        match self {
            Algorithm::MlKem768 => "mlkem",
            Algorithm::Rsa2048 => "rsa",
            Algorithm::Fake => "fake",
        }
    }
}

/// Which carrier a file uses (SPEC §Carriers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CarrierKind {
    /// The envelope bytes as-is (`.bin`). For debugging.
    RawDebug = 0,
    /// LSB steganography in a PNG. Survives only lossless sending.
    StealthPng = 1,
    /// Color-grid PNG designed to survive messenger JPEG recompression.
    RobustPng = 2,
    /// Sine-tone melody in a WAV file.
    MusicWav = 3,
    /// Melody in a MIDI file (exact decode).
    MusicMidi = 4,
}

impl CarrierKind {
    /// Every carrier kind, in auto-detect order.
    pub const ALL: [CarrierKind; 5] = [
        CarrierKind::RawDebug,
        CarrierKind::StealthPng,
        CarrierKind::RobustPng,
        CarrierKind::MusicWav,
        CarrierKind::MusicMidi,
    ];

    /// Short lowercase name used by the CLI.
    pub fn name(self) -> &'static str {
        match self {
            CarrierKind::RawDebug => "raw",
            CarrierKind::StealthPng => "stealth-png",
            CarrierKind::RobustPng => "robust-png",
            CarrierKind::MusicWav => "wav",
            CarrierKind::MusicMidi => "midi",
        }
    }
}

/// Which of the two slots a message was opened from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    /// Opens with the normal key.
    A,
    /// Also needs the passphrase agreed in person.
    B,
}

/// A key pair. The core is stateless: storing these is the app's job.
#[derive(Clone, PartialEq, Eq)]
pub struct Identity {
    pub public_key: Vec<u8>,
    pub secret_key: Vec<u8>,
}

impl std::fmt::Debug for Identity {
    // Never print the secret key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("public_key_len", &self.public_key.len())
            .field("secret_key", &"<redacted>")
            .finish()
    }
}

/// Length of the associated data: envelope header bytes 0..15 (magic through contact_id).
pub const AAD_LEN: usize = 15;

/// Input to [`crate::Cipher::seal`].
#[derive(Clone)]
pub struct SealRequest {
    pub recipient_public_key: Vec<u8>,
    pub contact_id: ContactId,
    /// The real message. In hidden mode it goes to slot B, otherwise slot A.
    pub message: String,
    /// Hidden mode: harmless text for slot A. Requires `passphrase`.
    pub decoy: Option<String>,
    /// Hidden mode: passphrase protecting slot B. Requires `decoy`.
    pub passphrase: Option<String>,
    /// Envelope header bytes 0..15, used as AES-GCM associated data.
    pub aad: [u8; AAD_LEN],
    /// Envelope flags. With the SESSION flag this is a session message.
    pub flags: u8,
    /// The session to use. Some exactly when `flags` has the SESSION flag.
    pub session_key: Option<SessionKey>,
}

/// Result of [`crate::Cipher::seal`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sealed {
    /// The envelope payload (kem_block or session_id || slot_A || slot_B).
    pub payload: Vec<u8>,
    /// The session this full message starts (None for a session message).
    pub new_session_key: Option<SessionKey>,
}

/// Input to [`crate::Cipher::open`].
#[derive(Clone)]
pub struct OpenRequest {
    pub my_secret_key: Vec<u8>,
    pub contact_id: ContactId,
    /// The envelope payload (kem_block || slot_A || slot_B).
    pub payload: Vec<u8>,
    /// If given, slot B is tried first.
    pub passphrase: Option<String>,
    /// Envelope header bytes 0..15, used as AES-GCM associated data.
    pub aad: [u8; AAD_LEN],
    /// Envelope flags. With the SESSION flag this is a session message.
    pub flags: u8,
    /// The session to use. Some exactly when `flags` has the SESSION flag.
    pub session_key: Option<SessionKey>,
}

/// Result of [`crate::Cipher::open`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub text: String,
    pub slot: Slot,
    /// The session this full message started (None for a session message).
    pub new_session_key: Option<SessionKey>,
}

/// Carrier-specific options. Add new optional fields here; every carrier
/// must work with `CarrierOptions::default()`. Construct with
/// `CarrierOptions { seed: Some(1), ..Default::default() }` so new fields
/// don't break callers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CarrierOptions {
    /// Cover image (any format the `image` crate reads) for image carriers.
    pub cover_image: Option<Vec<u8>>,
    /// Cover audio (16-bit PCM WAV) whose sample LSBs carry the envelope,
    /// for the `MusicWav` carrier; `None` synthesizes a melody instead.
    pub cover_audio: Option<Vec<u8>>,
    /// Music style name for music carriers (carrier-defined; None = default).
    pub music_style: Option<String>,
    /// Seed for any randomness in the carrier, for reproducible files.
    pub seed: Option<u64>,
}

impl CarrierOptions {
    /// Options with only a seed set.
    pub fn with_seed(seed: u64) -> Self {
        CarrierOptions {
            seed: Some(seed),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn algorithm_ids_roundtrip() {
        for a in Algorithm::ALL {
            assert_eq!(Algorithm::from_id(a.id()), Ok(a));
        }
        assert_eq!(Algorithm::from_id(0), Err(PolyError::UnknownAlgorithm(0)));
    }

    #[test]
    fn identity_debug_hides_secret() {
        let id = Identity {
            public_key: vec![1, 2],
            secret_key: vec![0xAB; 4],
        };
        let s = format!("{id:?}");
        assert!(!s.contains("171"), "{s}");
        assert!(s.contains("redacted"));
    }
}
