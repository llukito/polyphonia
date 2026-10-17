//! Color-grid PNG designed to survive messenger JPEG recompression.
//!
//! v0: the envelope as a black-and-white QR code (byte mode, error
//! correction level Q), drawn as large as fits in 1024 x 1024. QR finder
//! patterns and Reed-Solomon let it survive JPEG and resizing.
//!
//! TODO(role 2): v1 = 1024x1024 image, 16x16 px cells, 4 colors (2 bits/cell), corner finder markers, Reed-Solomon with ~50% parity (~500 usable bytes).

use std::io::Cursor;

use image::{DynamicImage, GrayImage, ImageFormat, Luma};
use poly_core::envelope::MAGIC;
use poly_core::{Carrier, CarrierKind, CarrierOptions, PolyError};
use qrcode::{Color, EcLevel, QrCode};

use crate::load::{image_err, load_image};

/// Error correction level: Q restores up to 25% damaged codewords
/// (ISO/IEC 18004 §6.5.1), a margin for JPEG artefacts at the cost of capacity.
const EC_LEVEL: EcLevel = EcLevel::Q;
/// Byte-mode capacity of the largest QR code (version 40) at level Q
/// (ISO/IEC 18004 Table 7).
const QR_CAPACITY: usize = 1663;
/// Blank border around the code, in modules (ISO/IEC 18004 §6.3.8 asks for 4).
const QUIET_ZONE_MODULES: usize = 4;
/// Largest output side: a power of two below the ~1200 px at which
/// messengers start resizing (see docs/roles/2-image.md).
const MAX_SIDE_PX: usize = 1024;
const BLACK: Luma<u8> = Luma([u8::MIN]);
const WHITE: Luma<u8> = Luma([u8::MAX]);
/// Halfway between black and white: darker pixels read as dark modules.
const MID_GREY: u8 = u8::MAX / 2;

/// Color-grid PNG designed to survive messenger JPEG recompression.
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

    /// Fixed: the cover image is ignored (the code has to own every pixel).
    fn capacity(&self, _opts: &CarrierOptions) -> Option<usize> {
        Some(QR_CAPACITY)
    }

    fn encode(&self, envelope: &[u8], _opts: &CarrierOptions) -> Result<Vec<u8>, PolyError> {
        if envelope.len() > QR_CAPACITY {
            return Err(PolyError::CarrierTooSmall {
                needed: envelope.len(),
                available: QR_CAPACITY,
            });
        }
        let code = QrCode::with_error_correction_level(envelope, EC_LEVEL)
            .map_err(|e| PolyError::Carrier(e.to_string()))?;

        let modules = code.width();
        let total = modules + 2 * QUIET_ZONE_MODULES;
        let scale = MAX_SIDE_PX / total;
        let side = total * scale;
        let colors = code.to_colors();
        let img = GrayImage::from_fn(side as u32, side as u32, |x, y| {
            let mx = (x as usize / scale).wrapping_sub(QUIET_ZONE_MODULES);
            let my = (y as usize / scale).wrapping_sub(QUIET_ZONE_MODULES);
            if mx < modules && my < modules && colors[my * modules + mx] == Color::Dark {
                BLACK
            } else {
                WHITE
            }
        });

        let mut bytes = Vec::new();
        DynamicImage::ImageLuma8(img)
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .map_err(image_err)?;
        Ok(bytes)
    }

    /// Accepts any image format, since messengers usually deliver it as JPEG.
    fn decode(&self, file: &[u8]) -> Result<Vec<u8>, PolyError> {
        if image::guess_format(file).is_err() {
            return Err(PolyError::NotThisCarrier);
        }
        let gray = load_image(file)
            .map_err(|_| PolyError::NotThisCarrier)?
            .to_luma8();
        // The code is pure black on white, so a fixed mid-grey threshold is
        // more reliable than rqrr's adaptive one on the soft edges resizing leaves.
        let mut prepared = rqrr::PreparedImage::prepare_from_bitmap(
            gray.width() as usize,
            gray.height() as usize,
            |x, y| gray.get_pixel(x as u32, y as u32).0[0] < MID_GREY,
        );
        for grid in prepared.detect_grids() {
            let mut data = Vec::new();
            if grid.decode_to(&mut data).is_ok() && data.starts_with(&MAGIC) {
                return Ok(data);
            }
        }
        Err(PolyError::NotThisCarrier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ImageDamage, damage_image};
    use poly_core::{Algorithm, Envelope};

    /// Payload of a Fake "hi" envelope (111 bytes total, docs/roles/2-image.md).
    const FAKE_HI_PAYLOAD: usize = 88;
    /// Payload of an RSA-2048 "hi" envelope (367 bytes total).
    const RSA_HI_PAYLOAD: usize = 344;

    fn envelope(payload_len: usize) -> Vec<u8> {
        let payload = (0..payload_len).map(|i| (i * 37 % 251) as u8).collect();
        Envelope::new(Algorithm::Fake, 0, [3; 8], payload)
            .to_bytes()
            .unwrap()
    }

    #[test]
    fn roundtrip() {
        poly_core::test_utils::assert_carrier_roundtrip(&RobustPngCarrier);
    }

    #[test]
    fn output_fits_messenger_limit() {
        let file = RobustPngCarrier
            .encode(&envelope(QR_CAPACITY - 23), &CarrierOptions::default())
            .unwrap();
        let img = image::load_from_memory(&file).unwrap();
        assert!(img.width() as usize <= MAX_SIDE_PX);
        assert_eq!(img.width(), img.height());
    }

    /// The point of this carrier: what a messenger does to a photo.
    #[test]
    fn survives_jpeg_and_resize() {
        for payload in [FAKE_HI_PAYLOAD, RSA_HI_PAYLOAD] {
            let env = envelope(payload);
            let file = RobustPngCarrier
                .encode(&env, &CarrierOptions::default())
                .unwrap();
            for (quality, max_side) in [(70, None), (70, Some(800)), (50, Some(640))] {
                let damaged = damage_image(
                    &file,
                    ImageDamage {
                        jpeg_quality: Some(quality),
                        max_side,
                    },
                )
                .unwrap();
                assert_eq!(
                    RobustPngCarrier.decode(&damaged.bytes).as_deref(),
                    Ok(&env[..]),
                    "payload {payload}, q{quality}, resize {max_side:?}"
                );
            }
        }
    }

    #[test]
    fn plain_images_are_not_this_carrier() {
        let mut png = Vec::new();
        DynamicImage::ImageLuma8(GrayImage::from_pixel(64, 64, WHITE))
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        assert_eq!(
            RobustPngCarrier.decode(&png),
            Err(PolyError::NotThisCarrier)
        );
    }
}
