//! LSB steganography in a PNG (1 bit in each of R, G, B = 3 bits per pixel).
//!
//! TODO(role 2): hide the envelope in the least significant bit of R, G and B; cover image from `CarrierOptions::cover_image` or a generated default. Survives only lossless (file/document) sending.

use poly_core::{Carrier, CarrierKind, CarrierOptions, PolyError};

/// LSB steganography in a PNG (1 bit in each of R, G, B = 3 bits per pixel). Not implemented yet (role 2).
#[derive(Debug, Default, Clone, Copy)]
pub struct StealthPngCarrier;

impl Carrier for StealthPngCarrier {
    fn kind(&self) -> CarrierKind {
        CarrierKind::StealthPng
    }

    fn file_extension(&self) -> &'static str {
        "png"
    }

    fn mime_type(&self) -> &'static str {
        "image/png"
    }

    fn capacity(&self, _opts: &CarrierOptions) -> Option<usize> {
        // TODO(role 2): return the real capacity.
        None
    }

    fn encode(&self, _envelope: &[u8], _opts: &CarrierOptions) -> Result<Vec<u8>, PolyError> {
        Err(PolyError::NotImplemented("StealthPngCarrier (role 2)"))
    }

    fn decode(&self, _file: &[u8]) -> Result<Vec<u8>, PolyError> {
        Err(PolyError::NotImplemented("StealthPngCarrier (role 2)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "role 2: remove when implemented"]
    fn roundtrip() {
        poly_core::test_utils::assert_carrier_roundtrip(&StealthPngCarrier);
    }
}
