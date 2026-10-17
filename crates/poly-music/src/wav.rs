//! Sine-tone melody in a 44.1 kHz WAV file, decoded with FFT pitch detection.
//!
//! TODO(role 3): render the MIDI carrier's notes as pure sine tones between 300 and 3400 Hz (telephone voice band) at 44,100 Hz; decode with rustfft; then survive voice-note re-encoding.

use poly_core::{Carrier, CarrierKind, CarrierOptions, PolyError};

/// Sine-tone melody in a 44.1 kHz WAV file, decoded with FFT pitch detection. Not implemented yet (role 3).
#[derive(Debug, Default, Clone, Copy)]
pub struct MusicWavCarrier;

impl Carrier for MusicWavCarrier {
    fn kind(&self) -> CarrierKind {
        CarrierKind::MusicWav
    }

    fn file_extension(&self) -> &'static str {
        "wav"
    }

    fn mime_type(&self) -> &'static str {
        "audio/wav"
    }

    fn capacity(&self, _opts: &CarrierOptions) -> Option<usize> {
        // TODO(role 3): return the real capacity.
        None
    }

    fn encode(&self, _envelope: &[u8], _opts: &CarrierOptions) -> Result<Vec<u8>, PolyError> {
        Err(PolyError::NotImplemented("MusicWavCarrier (role 3)"))
    }

    fn decode(&self, _file: &[u8]) -> Result<Vec<u8>, PolyError> {
        Err(PolyError::NotImplemented("MusicWavCarrier (role 3)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "role 3: remove when implemented"]
    fn roundtrip() {
        poly_core::test_utils::assert_carrier_roundtrip(&MusicWavCarrier);
    }
}
