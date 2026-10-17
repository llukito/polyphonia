//! The single error type shared by every Polyphonia crate.

use thiserror::Error;

/// Every error any Polyphonia crate can return.
///
/// Error messages never contain plaintext, keys or passphrases.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PolyError {
    // ---- Envelope parsing -------------------------------------------------
    /// The bytes do not start with the `POLY` magic.
    #[error("not a Polyphonia envelope (bad magic)")]
    BadMagic,
    /// The envelope version is not one this build understands.
    #[error("unsupported envelope version {0}")]
    UnsupportedVersion(u8),
    /// The algorithm byte is not a known [`crate::Algorithm`].
    #[error("unknown algorithm id {0:#04x}")]
    UnknownAlgorithm(u8),
    /// A reserved flag bit is set.
    #[error("reserved flag bits set: {0:#010b}")]
    ReservedFlags(u8),
    /// The input ends before the structure it claims to contain.
    #[error("data is truncated")]
    Truncated,
    /// The CRC-32 does not match: the file was damaged (e.g. recompressed) in transit.
    #[error("checksum mismatch: the file was damaged or compressed in transit")]
    ChecksumMismatch,
    /// The payload length does not fit the u32 `payload_len` field.
    #[error("payload too large: {0} bytes")]
    PayloadTooLarge(usize),

    // ---- Payload / slots --------------------------------------------------
    /// The payload does not have the layout required by its algorithm and flags.
    #[error("malformed payload: {0}")]
    MalformedPayload(&'static str),
    /// The message does not fit the u16 length prefix of a slot.
    #[error("message too long: {len} bytes (max {max})")]
    MessageTooLong { len: usize, max: usize },

    // ---- Crypto -----------------------------------------------------------
    /// A decoy was given without a passphrase (hidden mode needs both).
    #[error("hidden mode needs a passphrase")]
    MissingPassphrase,
    /// A request is inconsistent (e.g. a passphrase without a decoy).
    #[error("invalid request: {0}")]
    InvalidRequest(&'static str),
    /// A key has the wrong length or encoding for its algorithm.
    #[error("invalid key: {0}")]
    InvalidKey(&'static str),
    /// No slot could be decrypted (wrong key, or the payload was tampered with).
    #[error("decryption failed")]
    DecryptFailed,
    /// The message is addressed to a different contact (contact_id mismatch).
    #[error("this message is not addressed to this key")]
    WrongRecipient,
    /// A cryptographic primitive failed unexpectedly.
    #[error("crypto error: {0}")]
    Crypto(String),

    // ---- Carriers ---------------------------------------------------------
    /// The file is not in this carrier's format; auto-detect should try the next one.
    #[error("file is not in this carrier's format")]
    NotThisCarrier,
    /// The envelope does not fit in the carrier.
    #[error("carrier too small: need {needed} bytes, carrier holds {available}")]
    CarrierTooSmall { needed: usize, available: usize },
    /// No registered carrier recognized the file.
    #[error("no carrier recognized this file")]
    UnrecognizedFile,
    /// Encoding or decoding a carrier file failed.
    #[error("carrier error: {0}")]
    Carrier(String),

    // ---- General ----------------------------------------------------------
    /// The feature exists in the contract but is not implemented yet.
    #[error("not implemented yet: {0}")]
    NotImplemented(&'static str),
}
