//! LSB steganography in a PNG (1 bit in each of R, G, B = 3 bits per pixel).
//!
//! Layout: the envelope bytes, most significant bit first, go into the least
//! significant bit of R, then G, then B of each pixel, pixels in row-major
//! order starting top-left. Fully transparent pixels (alpha = 0) are skipped
//! and left untouched, because some encoders zero their RGB. No length prefix
//! is stored: the envelope header's `payload_len` says how many bytes to read.
//! Bits after the envelope keep the cover's original values.
//!
//! Survives only lossless (file/document) sending: any JPEG step or resize
//! destroys the LSBs.

use std::io::Cursor;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use poly_core::envelope::{CRC_SIZE, HEADER_LEN, MAGIC, OFFSET_PAYLOAD_LEN, PAYLOAD_LEN_SIZE};
use poly_core::{Carrier, CarrierKind, CarrierOptions, PolyError};

use crate::load::{image_err, load_image};
use crate::rng::SplitMix64;

/// The 8-byte signature every PNG file starts with (PNG spec §5.2).
const PNG_SIGNATURE: [u8; 8] = *b"\x89PNG\r\n\x1a\n";

/// Colour channels used per pixel: R, G and B (alpha is never touched).
const CHANNELS_USED: usize = 3;
/// Bits in a byte.
const BITS_PER_BYTE: usize = 8;
/// Index of the alpha channel in an RGBA pixel.
const ALPHA: usize = 3;
/// Alpha of a fully transparent pixel; such pixels carry no data.
const TRANSPARENT: u8 = 0;
/// Alpha of a fully opaque pixel.
const OPAQUE: u8 = u8::MAX;

/// Side of the generated default cover: 512 x 512 x 3 bits = 98,304 bytes,
/// the capacity quoted in SPEC §5, and plenty for an ML-KEM envelope (≥ 1199 B).
const DEFAULT_COVER_SIDE: u32 = 512;
/// Seed for the default cover when `CarrierOptions::seed` is `None`, so
/// default output is reproducible. ASCII "PNG!".
const DEFAULT_COVER_SEED: u64 = 0x504E_4721;
/// Peak-to-peak per-pixel noise added to the default cover's gradient. A
/// little grain makes the cover look like a photo rather than a flat
/// gradient, whose LSBs would be suspiciously regular.
const DEFAULT_COVER_NOISE: u64 = 12;

/// LSB steganography in a PNG (1 bit in each of R, G, B = 3 bits per pixel).
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

    /// `None` when the cover can't be decoded, so the engine skips its size
    /// check and `encode` reports the real cover error instead of "too small".
    fn capacity(&self, opts: &CarrierOptions) -> Option<usize> {
        cover(opts).ok().map(|img| capacity_of(&img))
    }

    fn encode(&self, envelope: &[u8], opts: &CarrierOptions) -> Result<Vec<u8>, PolyError> {
        let mut img = cover(opts)?;
        let available = capacity_of(&img);
        if envelope.len() > available {
            return Err(PolyError::CarrierTooSmall {
                needed: envelope.len(),
                available,
            });
        }

        let mut bits = envelope
            .iter()
            .flat_map(|&byte| (0..BITS_PER_BYTE).rev().map(move |i| (byte >> i) & 1));
        'pixels: for px in img.pixels_mut().filter(|px| carries_data(px)) {
            for channel in &mut px.0[..CHANNELS_USED] {
                let Some(bit) = bits.next() else {
                    break 'pixels;
                };
                *channel = (*channel & !1) | bit;
            }
        }

        // Keep the file RGB when the cover has no transparency: smaller, and
        // indistinguishable from an ordinary photo.
        let out = if img.pixels().all(|px| px.0[ALPHA] == OPAQUE) {
            DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(img).to_rgb8())
        } else {
            DynamicImage::ImageRgba8(img)
        };
        let mut bytes = Vec::new();
        out.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .map_err(image_err)?;
        Ok(bytes)
    }

    fn decode(&self, file: &[u8]) -> Result<Vec<u8>, PolyError> {
        if !file.starts_with(&PNG_SIGNATURE) {
            return Err(PolyError::NotThisCarrier);
        }
        let img = load_image(file)
            .map_err(|_| PolyError::NotThisCarrier)?
            .to_rgba8();
        let mut bytes = lsb_bytes(&img);

        let header: Vec<u8> = bytes.by_ref().take(HEADER_LEN).collect();
        if !header.starts_with(&MAGIC) {
            return Err(PolyError::NotThisCarrier);
        }
        if header.len() < HEADER_LEN {
            return Err(PolyError::Truncated);
        }
        let len_field: [u8; PAYLOAD_LEN_SIZE] = header
            [OFFSET_PAYLOAD_LEN..OFFSET_PAYLOAD_LEN + PAYLOAD_LEN_SIZE]
            .try_into()
            .expect("slice has PAYLOAD_LEN_SIZE bytes");
        let rest = u32::from_be_bytes(len_field) as usize + CRC_SIZE;
        if rest > capacity_of(&img) - HEADER_LEN {
            // Random LSBs that happen to start with POLY, or a cropped file.
            return Err(PolyError::Truncated);
        }

        let mut envelope = header;
        envelope.extend(bytes.take(rest));
        Ok(envelope)
    }
}

/// Whether a pixel holds data (it isn't fully transparent).
fn carries_data(px: &Rgba<u8>) -> bool {
    px.0[ALPHA] != TRANSPARENT
}

/// Envelope bytes this image can hold.
fn capacity_of(img: &RgbaImage) -> usize {
    img.pixels().filter(|px| carries_data(px)).count() * CHANNELS_USED / BITS_PER_BYTE
}

/// The LSB stream of `img`, packed into bytes in the layout described above.
/// A final partial byte is dropped.
fn lsb_bytes(img: &RgbaImage) -> impl Iterator<Item = u8> + '_ {
    let mut bits = img
        .pixels()
        .filter(|px| carries_data(px))
        .flat_map(|px| px.0[..CHANNELS_USED].iter().map(|c| c & 1));
    std::iter::from_fn(move || {
        let mut byte = 0;
        for _ in 0..BITS_PER_BYTE {
            byte = (byte << 1) | bits.next()?;
        }
        Some(byte)
    })
}

/// The cover from the options, or the generated default.
fn cover(opts: &CarrierOptions) -> Result<RgbaImage, PolyError> {
    match &opts.cover_image {
        Some(file) => Ok(load_image(file)?.to_rgba8()),
        None => Ok(default_cover(opts.seed.unwrap_or(DEFAULT_COVER_SEED))),
    }
}

/// A soft diagonal blend between two seeded colours, with light grain.
fn default_cover(seed: u64) -> RgbaImage {
    let mut rng = SplitMix64(seed);
    let from = rng.color();
    let to = rng.color();
    let span = 2 * (DEFAULT_COVER_SIDE - 1);
    RgbaImage::from_fn(DEFAULT_COVER_SIDE, DEFAULT_COVER_SIDE, |x, y| {
        let t = x + y;
        let mut px = [0u8, 0, 0, OPAQUE];
        for c in 0..CHANNELS_USED {
            let base = (u32::from(from[c]) * (span - t) + u32::from(to[c]) * t) / span;
            let noise = (rng.next() % DEFAULT_COVER_NOISE) as u32;
            let noisy = (base + noise).saturating_sub((DEFAULT_COVER_NOISE / 2) as u32);
            px[c] = noisy.min(u32::from(u8::MAX)) as u8;
        }
        Rgba(px)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use poly_core::Envelope;
    use poly_core::envelope::ENVELOPE_OVERHEAD;

    /// Small cover side for tests that build their own covers.
    const TEST_SIDE: u32 = 64;

    fn png(img: DynamicImage) -> Vec<u8> {
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    fn envelope(payload_len: usize) -> Vec<u8> {
        Envelope::new(
            poly_core::Algorithm::Fake,
            0,
            [7; 8],
            vec![0xA5; payload_len],
        )
        .to_bytes()
        .unwrap()
    }

    #[test]
    fn roundtrip() {
        poly_core::test_utils::assert_carrier_roundtrip(&StealthPngCarrier);
    }

    #[test]
    fn default_capacity_matches_spec() {
        let side = DEFAULT_COVER_SIDE as usize;
        assert_eq!(
            StealthPngCarrier.capacity(&CarrierOptions::default()),
            Some(side * side * CHANNELS_USED / BITS_PER_BYTE)
        );
    }

    #[test]
    fn same_seed_same_file_different_seed_different_cover() {
        let env = envelope(10);
        let a = StealthPngCarrier.encode(&env, &CarrierOptions::with_seed(1));
        let b = StealthPngCarrier.encode(&env, &CarrierOptions::with_seed(1));
        let c = StealthPngCarrier.encode(&env, &CarrierOptions::with_seed(2));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn only_lsbs_of_cover_change() {
        let cover = RgbaImage::from_fn(TEST_SIDE, TEST_SIDE, |x, y| {
            Rgba([(x * 3) as u8, (y * 3) as u8, 200, OPAQUE])
        });
        let opts = CarrierOptions {
            cover_image: Some(png(DynamicImage::ImageRgba8(cover.clone()))),
            ..Default::default()
        };
        let env = envelope(100);
        let file = StealthPngCarrier.encode(&env, &opts).unwrap();
        let out = image::load_from_memory(&file).unwrap().to_rgba8();
        assert_eq!(out.dimensions(), cover.dimensions());
        for (a, b) in cover.pixels().zip(out.pixels()) {
            for c in 0..CHANNELS_USED {
                assert!(a.0[c].abs_diff(b.0[c]) <= 1);
            }
            assert_eq!(a.0[ALPHA], b.0[ALPHA]);
        }
        assert_eq!(StealthPngCarrier.decode(&file).unwrap(), env);
    }

    #[test]
    fn transparent_pixels_are_skipped_and_kept() {
        // Left half fully transparent, right half opaque.
        let cover = RgbaImage::from_fn(TEST_SIDE, TEST_SIDE, |x, _| {
            let alpha = if x < TEST_SIDE / 2 {
                TRANSPARENT
            } else {
                OPAQUE
            };
            Rgba([90, 120, 150, alpha])
        });
        let opts = CarrierOptions {
            cover_image: Some(png(DynamicImage::ImageRgba8(cover.clone()))),
            ..Default::default()
        };
        let half = (TEST_SIDE * TEST_SIDE / 2) as usize;
        let cap = half * CHANNELS_USED / BITS_PER_BYTE;
        assert_eq!(StealthPngCarrier.capacity(&opts), Some(cap));

        let env = envelope(cap - ENVELOPE_OVERHEAD);
        let file = StealthPngCarrier.encode(&env, &opts).unwrap();
        let out = image::load_from_memory(&file).unwrap().to_rgba8();
        for (a, b) in cover.pixels().zip(out.pixels()) {
            if a.0[ALPHA] == TRANSPARENT {
                assert_eq!(a, b);
            }
        }
        assert_eq!(StealthPngCarrier.decode(&file).unwrap(), env);
    }

    #[test]
    fn ordinary_png_is_not_this_carrier() {
        let file = png(DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            TEST_SIDE,
            TEST_SIDE,
            Rgba([10, 20, 30, OPAQUE]),
        )));
        assert_eq!(
            StealthPngCarrier.decode(&file),
            Err(PolyError::NotThisCarrier)
        );
        assert_eq!(
            StealthPngCarrier.decode(b"GIF89a..."),
            Err(PolyError::NotThisCarrier)
        );
    }

    #[test]
    fn jpeg_cover_works_and_output_is_png() {
        let mut jpeg = Vec::new();
        DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            TEST_SIDE,
            TEST_SIDE,
            image::Rgb([50, 100, 150]),
        ))
        .write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
        .unwrap();
        let opts = CarrierOptions {
            cover_image: Some(jpeg),
            ..Default::default()
        };
        let env = envelope(20);
        let file = StealthPngCarrier.encode(&env, &opts).unwrap();
        assert!(file.starts_with(&PNG_SIGNATURE));
        assert_eq!(StealthPngCarrier.decode(&file).unwrap(), env);
    }

    #[test]
    fn bad_cover_reports_cover_error() {
        let opts = CarrierOptions {
            cover_image: Some(b"not an image".to_vec()),
            ..Default::default()
        };
        assert_eq!(StealthPngCarrier.capacity(&opts), None);
        assert!(matches!(
            StealthPngCarrier.encode(&envelope(1), &opts),
            Err(PolyError::Carrier(_))
        ));
    }

    #[test]
    fn jpeg_damage_breaks_it() {
        let file = StealthPngCarrier
            .encode(&envelope(50), &CarrierOptions::default())
            .unwrap();
        let damaged = crate::damage_image(
            &file,
            crate::ImageDamage {
                jpeg_quality: Some(crate::damage::JPEG_QUALITY_MAX),
                max_side: None,
            },
        )
        .unwrap();
        assert!(StealthPngCarrier.decode(&damaged.bytes).is_err());
    }
}
