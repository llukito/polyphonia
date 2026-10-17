//! The two extension points: [`Cipher`] (role 1) and [`Carrier`] (roles 2 and 3).

use crate::PolyError;
use crate::types::{
    Algorithm, CarrierKind, CarrierOptions, Identity, OpenRequest, Opened, SealRequest,
};

/// Encrypts a message into a payload and back. Stateless: keys come in and
/// go out as bytes.
///
/// Implementations must follow `docs/SPEC.md` §Payload and §Key derivation
/// exactly, and pass `poly_core::test_utils::assert_cipher_roundtrip`.
pub trait Cipher: Send + Sync {
    /// The algorithm this cipher implements (written to the envelope).
    fn algorithm(&self) -> Algorithm;

    /// Generate a fresh key pair.
    fn generate_identity(&self) -> Result<Identity, PolyError>;

    /// Build the full payload (kem_block + both slots) for the recipient.
    ///
    /// - Normal mode (`decoy` is None): slot A = `message`, slot B = random bytes.
    /// - Hidden mode (`decoy` is Some): slot A = decoy, slot B = `message`,
    ///   protected additionally by `passphrase`.
    /// - `decoy` Some and `passphrase` None => [`PolyError::MissingPassphrase`].
    /// - `passphrase` Some and `decoy` None => [`PolyError::InvalidRequest`].
    /// - Both slots are padded to the same length (the longer text decides).
    fn seal(&self, req: &SealRequest) -> Result<Vec<u8>, PolyError>;

    /// Open a payload; returns the text and which slot opened.
    ///
    /// If a passphrase is given, slot B is returned when its tag verifies;
    /// otherwise slot A. Both slots are always attempted so timing does not
    /// reveal which one opened. If nothing opens: [`PolyError::DecryptFailed`].
    fn open(&self, req: &OpenRequest) -> Result<Opened, PolyError>;
}

/// Hides envelope bytes inside a file (PNG, WAV, MIDI, ...) and gets them back.
///
/// Implementations must pass `poly_core::test_utils::assert_carrier_roundtrip`.
pub trait Carrier: Send + Sync {
    /// Which carrier this is.
    fn kind(&self) -> CarrierKind;

    /// File extension without the dot: "bin", "png", "wav", "mid".
    fn file_extension(&self) -> &'static str;

    /// MIME type of the produced file.
    fn mime_type(&self) -> &'static str;

    /// Max envelope bytes this carrier can hold with these options (None = unlimited).
    fn capacity(&self, opts: &CarrierOptions) -> Option<usize>;

    /// Hide `envelope` in a new file. Return [`PolyError::CarrierTooSmall`]
    /// if it doesn't fit.
    fn encode(&self, envelope: &[u8], opts: &CarrierOptions) -> Result<Vec<u8>, PolyError>;

    /// Extract the hidden bytes. The result must start with the envelope;
    /// trailing padding is allowed (the envelope parser ignores it).
    ///
    /// Must return `Err(PolyError::NotThisCarrier)` quickly when the file
    /// isn't its format, so auto-detect moves on. Must never panic.
    fn decode(&self, file: &[u8]) -> Result<Vec<u8>, PolyError>;
}
