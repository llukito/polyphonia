//! Top-level Polyphonia API: encrypt a message into a carrier file and back.
//!
//! The engine owns the registry of every [`Cipher`] and [`Carrier`], builds
//! the envelope, checks capacity and auto-detects the carrier on decrypt.
//! Owned by role 1 (lead): change only through a PR the lead approves.

mod raw;

pub use poly_core;
pub use poly_core::{
    Algorithm, CarrierKind, CarrierOptions, Envelope, Identity, PolyError, Slot, contact_id,
};
pub use raw::RawDebugCarrier;

use poly_core::envelope::header_aad;
use poly_core::payload::payload_len;
use poly_core::{Carrier, Cipher, OpenRequest, SealRequest};

/// Flags written by this version: SESSION mode is reserved, so always 0.
const MVP_FLAGS: u8 = 0;

/// What to encrypt and how.
#[derive(Clone)]
pub struct EncryptRequest {
    pub message: String,
    /// Hidden mode: decoy text for slot A. Requires `passphrase`.
    pub decoy: Option<String>,
    /// Hidden mode: passphrase for slot B. Requires `decoy`.
    pub passphrase: Option<String>,
    pub recipient_public_key: Vec<u8>,
    pub algorithm: Algorithm,
    pub carrier: CarrierKind,
    pub options: CarrierOptions,
}

/// A finished carrier file, ready to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFile {
    pub bytes: Vec<u8>,
    /// Without the dot, e.g. "png".
    pub extension: &'static str,
    pub mime: &'static str,
    pub carrier: CarrierKind,
    pub algorithm: Algorithm,
}

/// A received file to open.
#[derive(Clone)]
pub struct DecryptRequest {
    pub file: Vec<u8>,
    pub my_secret_key: Vec<u8>,
    /// Used to check the envelope's contact_id.
    pub my_public_key: Vec<u8>,
    /// If given, slot B is tried first.
    pub passphrase: Option<String>,
}

/// The opened message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decrypted {
    pub text: String,
    pub slot: Slot,
    pub carrier: CarrierKind,
    pub algorithm: Algorithm,
}

/// A file's detected carrier and envelope, found without any key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspected {
    pub carrier: CarrierKind,
    pub envelope: Envelope,
}

/// The registry of ciphers and carriers plus the encrypt/decrypt pipeline.
pub struct Engine {
    ciphers: Vec<Box<dyn Cipher>>,
    carriers: Vec<Box<dyn Carrier>>,
}

impl Default for Engine {
    /// Every cipher and carrier, carriers in [`CarrierKind::ALL`] (auto-detect) order.
    fn default() -> Self {
        Engine::with(poly_crypto::all_ciphers(), all_carriers())
    }
}

/// One instance of every carrier, in auto-detect order.
pub fn all_carriers() -> Vec<Box<dyn Carrier>> {
    CarrierKind::ALL
        .iter()
        .map(|&k| -> Box<dyn Carrier> {
            match k {
                CarrierKind::RawDebug => Box::new(RawDebugCarrier),
                CarrierKind::StealthPng => Box::new(poly_image::StealthPngCarrier),
                CarrierKind::RobustPng => Box::new(poly_image::RobustPngCarrier),
                CarrierKind::MusicWav => Box::new(poly_music::MusicWavCarrier),
                CarrierKind::MusicMidi => Box::new(poly_music::MusicMidiCarrier),
            }
        })
        .collect()
}

impl Engine {
    /// An engine with a custom registry (useful in tests). Carriers are
    /// tried in the given order during auto-detect.
    pub fn with(ciphers: Vec<Box<dyn Cipher>>, carriers: Vec<Box<dyn Carrier>>) -> Self {
        Engine { ciphers, carriers }
    }

    /// Look up a cipher.
    pub fn cipher(&self, algorithm: Algorithm) -> Result<&dyn Cipher, PolyError> {
        self.ciphers
            .iter()
            .find(|c| c.algorithm() == algorithm)
            .map(|c| c.as_ref())
            .ok_or(PolyError::InvalidRequest("algorithm not registered"))
    }

    /// Look up a carrier.
    pub fn carrier(&self, kind: CarrierKind) -> Result<&dyn Carrier, PolyError> {
        self.carriers
            .iter()
            .find(|c| c.kind() == kind)
            .map(|c| c.as_ref())
            .ok_or(PolyError::InvalidRequest("carrier not registered"))
    }

    /// Generate a key pair for an algorithm.
    pub fn generate_identity(&self, algorithm: Algorithm) -> Result<Identity, PolyError> {
        self.cipher(algorithm)?.generate_identity()
    }

    /// Envelope size a request will produce (computed without doing crypto).
    pub fn envelope_len(req: &EncryptRequest) -> usize {
        let longest = req
            .message
            .len()
            .max(req.decoy.as_ref().map_or(0, String::len));
        Envelope::encoded_len(payload_len(req.algorithm, MVP_FLAGS, longest))
    }

    /// Encrypt a message into a carrier file. Checks capacity before any crypto.
    pub fn encrypt(&self, req: &EncryptRequest) -> Result<EncodedFile, PolyError> {
        let cipher = self.cipher(req.algorithm)?;
        let carrier = self.carrier(req.carrier)?;

        let needed = Self::envelope_len(req);
        if let Some(available) = carrier.capacity(&req.options)
            && needed > available
        {
            return Err(PolyError::CarrierTooSmall { needed, available });
        }

        let contact_id = contact_id(&req.recipient_public_key);
        let payload = cipher.seal(&SealRequest {
            recipient_public_key: req.recipient_public_key.clone(),
            contact_id,
            message: req.message.clone(),
            decoy: req.decoy.clone(),
            passphrase: req.passphrase.clone(),
            aad: header_aad(req.algorithm, MVP_FLAGS, &contact_id),
        })?;
        let envelope = Envelope::new(req.algorithm, MVP_FLAGS, contact_id, payload).to_bytes()?;
        debug_assert_eq!(envelope.len(), needed, "payload size formula out of sync");

        Ok(EncodedFile {
            bytes: carrier.encode(&envelope, &req.options)?,
            extension: carrier.file_extension(),
            mime: carrier.mime_type(),
            carrier: carrier.kind(),
            algorithm: req.algorithm,
        })
    }

    /// Auto-detect the carrier and parse the envelope, without any key.
    ///
    /// Tries each carrier's decode in order; the first result that parses as
    /// an envelope wins. If some carrier decoded but the CRC failed and
    /// nothing else works, returns [`PolyError::ChecksumMismatch`] so the app
    /// can say "compressed in transit". Otherwise [`PolyError::UnrecognizedFile`].
    pub fn inspect(&self, file: &[u8]) -> Result<Inspected, PolyError> {
        let mut damaged = false;
        for carrier in &self.carriers {
            let Ok(bytes) = carrier.decode(file) else {
                continue;
            };
            match Envelope::from_bytes(&bytes) {
                Ok(envelope) => {
                    return Ok(Inspected {
                        carrier: carrier.kind(),
                        envelope,
                    });
                }
                Err(PolyError::ChecksumMismatch) => damaged = true,
                Err(_) => {}
            }
        }
        Err(if damaged {
            PolyError::ChecksumMismatch
        } else {
            PolyError::UnrecognizedFile
        })
    }

    /// Detect the carrier, check the recipient and open the message.
    pub fn decrypt(&self, req: &DecryptRequest) -> Result<Decrypted, PolyError> {
        let Inspected { carrier, envelope } = self.inspect(&req.file)?;
        if envelope.contact_id != contact_id(&req.my_public_key) {
            return Err(PolyError::WrongRecipient);
        }
        let aad = envelope.aad();
        let opened = self.cipher(envelope.algorithm)?.open(&OpenRequest {
            my_secret_key: req.my_secret_key.clone(),
            contact_id: envelope.contact_id,
            payload: envelope.payload,
            passphrase: req.passphrase.clone(),
            aad,
        })?;
        Ok(Decrypted {
            text: opened.text,
            slot: opened.slot,
            carrier,
            algorithm: envelope.algorithm,
        })
    }
}

/// [`Engine::encrypt`] with the default registry.
pub fn encrypt(req: &EncryptRequest) -> Result<EncodedFile, PolyError> {
    Engine::default().encrypt(req)
}

/// [`Engine::decrypt`] with the default registry.
pub fn decrypt(req: &DecryptRequest) -> Result<Decrypted, PolyError> {
    Engine::default().decrypt(req)
}

/// [`Engine::inspect`] with the default registry.
pub fn inspect(file: &[u8]) -> Result<Inspected, PolyError> {
    Engine::default().inspect(file)
}

/// [`Engine::generate_identity`] with the default registry.
pub fn generate_identity(algorithm: Algorithm) -> Result<Identity, PolyError> {
    Engine::default().generate_identity(algorithm)
}
