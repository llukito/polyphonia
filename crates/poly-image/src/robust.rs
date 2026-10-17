//! Color-grid PNG designed to survive messenger JPEG recompression.
//!
//! TODO(role 2): v0 = QR code via a crate; v1 = 1024x1024 image, 16x16 px cells, 4 colors (2 bits/cell), corner finder markers, Reed-Solomon with ~50% parity (~500 usable bytes).

use poly_core::{Carrier, CarrierKind, CarrierOptions, PolyError};

/// Color-grid PNG designed to survive messenger JPEG recompression. Not implemented yet (role 2).
#[derive(Debug, Default, Clone, Copy)]
pub struct RobustPngCarrier;

impl Carrier for RobustPngCarrier {
    fn kind(&self) -> CarrierKind {
        CarrierKind::RobustPng
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
        Err(PolyError::NotImplemented("RobustPngCarrier (role 2)"))
    }

    fn decode(&self, _file: &[u8]) -> Result<Vec<u8>, PolyError> {
        Err(PolyError::NotImplemented("RobustPngCarrier (role 2)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "role 2: remove when implemented"]
    fn roundtrip() {
        poly_core::test_utils::assert_carrier_roundtrip(&RobustPngCarrier);
    }
}
