//! Melody in a MIDI file; exact decode.
//!
//! TODO(role 3): 5 bits per note = pitch from an 8-note scale (3 bits) + one of 4 durations (2 bits); 8 notes/s = 40 bits/s; data-free background chord progression; start marker.

use poly_core::{Carrier, CarrierKind, CarrierOptions, PolyError};

/// Melody in a MIDI file; exact decode. Not implemented yet (role 3).
#[derive(Debug, Default, Clone, Copy)]
pub struct MusicMidiCarrier;

impl Carrier for MusicMidiCarrier {
    fn kind(&self) -> CarrierKind {
        CarrierKind::MusicMidi
    }

    fn file_extension(&self) -> &'static str {
        "mid"
    }

    fn mime_type(&self) -> &'static str {
        "audio/midi"
    }

    fn capacity(&self, _opts: &CarrierOptions) -> Option<usize> {
        // TODO(role 3): return the real capacity.
        None
    }

    fn encode(&self, _envelope: &[u8], _opts: &CarrierOptions) -> Result<Vec<u8>, PolyError> {
        Err(PolyError::NotImplemented("MusicMidiCarrier (role 3)"))
    }

    fn decode(&self, _file: &[u8]) -> Result<Vec<u8>, PolyError> {
        Err(PolyError::NotImplemented("MusicMidiCarrier (role 3)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "role 3: remove when implemented"]
    fn roundtrip() {
        poly_core::test_utils::assert_carrier_roundtrip(&MusicMidiCarrier);
    }
}
