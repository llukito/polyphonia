//! RawDebugCarrier: the file is the envelope bytes as-is. For debugging and tests.

use poly_core::envelope::MAGIC;
use poly_core::{Carrier, CarrierKind, CarrierOptions, PolyError};

/// Writes the envelope unchanged to a `.bin` file. Unlimited capacity.
#[derive(Debug, Default, Clone, Copy)]
pub struct RawDebugCarrier;

impl Carrier for RawDebugCarrier {
    fn kind(&self) -> CarrierKind {
        CarrierKind::RawDebug
    }

    fn file_extension(&self) -> &'static str {
        "bin"
    }

    fn mime_type(&self) -> &'static str {
        "application/octet-stream"
    }

    fn capacity(&self, _opts: &CarrierOptions) -> Option<usize> {
        None
    }

    fn encode(&self, envelope: &[u8], _opts: &CarrierOptions) -> Result<Vec<u8>, PolyError> {
        Ok(envelope.to_vec())
    }

    fn decode(&self, file: &[u8]) -> Result<Vec<u8>, PolyError> {
        if file.starts_with(&MAGIC) {
            Ok(file.to_vec())
        } else {
            Err(PolyError::NotThisCarrier)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_roundtrip() {
        poly_core::test_utils::assert_carrier_roundtrip(&RawDebugCarrier);
    }
}
