//! Simulated messenger damage for images: JPEG recompression and resizing.
//!
//! Used by `poly damage` so carriers can be tested against what WhatsApp,
//! Telegram & co. do to photos, without sending anything.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use poly_core::PolyError;

/// Largest width or height accepted from an input file. 16384 px is far above
/// any messenger photo limit (WhatsApp/Telegram cap around 1600-2560 px) and
/// keeps a hostile header from making us allocate gigabytes.
const MAX_INPUT_SIDE: u32 = 16_384;

/// Upper bound on decoder allocations: a 16384 x 16384 RGBA8 image is 1 GiB,
/// so allow exactly that (4 bytes per pixel).
const MAX_DECODE_ALLOC: u64 = MAX_INPUT_SIDE as u64 * MAX_INPUT_SIDE as u64 * 4;

/// Lowest JPEG quality the `image` crate's encoder accepts.
pub const JPEG_QUALITY_MIN: u8 = 1;
/// Highest JPEG quality the `image` crate's encoder accepts.
pub const JPEG_QUALITY_MAX: u8 = 100;

/// Resampling filter for resizing. Messengers' image pipelines (libjpeg-turbo
/// scaling, Android `Bitmap.createScaledBitmap`, iOS Core Graphics) are
/// bilinear-class, which `Triangle` matches.
const RESIZE_FILTER: FilterType = FilterType::Triangle;

/// What to do to an image. Resizing happens first, then JPEG recompression,
/// which is the order messengers use.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ImageDamage {
    /// Recompress as JPEG at this quality (1..=100). `None` keeps PNG output.
    pub jpeg_quality: Option<u8>,
    /// Scale so the longest side is at most this many pixels (aspect kept).
    /// Images already that small are left alone, as messengers do.
    pub max_side: Option<u32>,
}

/// The damaged image file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamagedImage {
    pub bytes: Vec<u8>,
    /// "jpg" or "png".
    pub extension: &'static str,
    pub width: u32,
    pub height: u32,
}

/// Applies `damage` to an image file in any format the `image` crate reads.
pub fn damage_image(file: &[u8], damage: ImageDamage) -> Result<DamagedImage, PolyError> {
    if let Some(q) = damage.jpeg_quality
        && !(JPEG_QUALITY_MIN..=JPEG_QUALITY_MAX).contains(&q)
    {
        return Err(PolyError::InvalidRequest(
            "JPEG quality must be between 1 and 100",
        ));
    }
    if damage.max_side == Some(0) {
        return Err(PolyError::InvalidRequest(
            "resize target must be at least 1 px",
        ));
    }

    let mut img = load(file)?;
    if let Some(max_side) = damage.max_side {
        img = shrink_to_fit(img, max_side);
    }

    let mut bytes = Vec::new();
    let extension = match damage.jpeg_quality {
        Some(quality) => {
            // JPEG has no alpha channel; messengers drop it the same way.
            JpegEncoder::new_with_quality(&mut bytes, quality)
                .encode_image(&img.to_rgb8())
                .map_err(carrier_err)?;
            "jpg"
        }
        None => {
            img.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
                .map_err(carrier_err)?;
            "png"
        }
    };
    Ok(DamagedImage {
        bytes,
        extension,
        width: img.width(),
        height: img.height(),
    })
}

fn load(file: &[u8]) -> Result<DynamicImage, PolyError> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_INPUT_SIDE);
    limits.max_image_height = Some(MAX_INPUT_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);

    let mut reader = ImageReader::new(Cursor::new(file))
        .with_guessed_format()
        .map_err(|e| PolyError::Carrier(e.to_string()))?;
    if reader.format().is_none() {
        return Err(PolyError::Carrier("not a recognized image format".into()));
    }
    reader.limits(limits);
    reader.decode().map_err(carrier_err)
}

fn shrink_to_fit(img: DynamicImage, max_side: u32) -> DynamicImage {
    if img.width().max(img.height()) <= max_side {
        return img;
    }
    // `resize` keeps the aspect ratio and fits inside the box.
    img.resize(max_side, max_side, RESIZE_FILTER)
}

fn carrier_err(e: image::ImageError) -> PolyError {
    PolyError::Carrier(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    /// Test image size: not square, so aspect-ratio handling is exercised.
    const TEST_WIDTH: u32 = 64;
    const TEST_HEIGHT: u32 = 32;
    /// A typical messenger photo quality (WhatsApp is reported around 70-80).
    const TEST_QUALITY: u8 = 70;

    fn test_png() -> Vec<u8> {
        let img = RgbaImage::from_fn(TEST_WIDTH, TEST_HEIGHT, |x, y| {
            Rgba([(x * 4) as u8, (y * 8) as u8, 128, 255])
        });
        let mut out = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn jpeg_recompression_outputs_jpeg() {
        let d = damage_image(
            &test_png(),
            ImageDamage {
                jpeg_quality: Some(TEST_QUALITY),
                max_side: None,
            },
        )
        .unwrap();
        assert_eq!(d.extension, "jpg");
        assert_eq!(image::guess_format(&d.bytes).unwrap(), ImageFormat::Jpeg);
        assert_eq!((d.width, d.height), (TEST_WIDTH, TEST_HEIGHT));
    }

    #[test]
    fn resize_keeps_aspect_ratio() {
        let d = damage_image(
            &test_png(),
            ImageDamage {
                jpeg_quality: None,
                max_side: Some(TEST_WIDTH / 2),
            },
        )
        .unwrap();
        assert_eq!(d.extension, "png");
        assert_eq!((d.width, d.height), (TEST_WIDTH / 2, TEST_HEIGHT / 2));
        let back = image::load_from_memory(&d.bytes).unwrap();
        assert_eq!((back.width(), back.height()), (d.width, d.height));
    }

    #[test]
    fn small_images_are_not_upscaled() {
        let d = damage_image(
            &test_png(),
            ImageDamage {
                jpeg_quality: None,
                max_side: Some(TEST_WIDTH * 4),
            },
        )
        .unwrap();
        assert_eq!((d.width, d.height), (TEST_WIDTH, TEST_HEIGHT));
    }

    #[test]
    fn png_without_damage_is_pixel_identical() {
        let input = test_png();
        let d = damage_image(&input, ImageDamage::default()).unwrap();
        let a = image::load_from_memory(&input).unwrap().to_rgba8();
        let b = image::load_from_memory(&d.bytes).unwrap().to_rgba8();
        assert_eq!(a, b);
    }

    #[test]
    fn rejects_bad_options_and_junk() {
        let png = test_png();
        for bad in [0, 101] {
            let r = damage_image(
                &png,
                ImageDamage {
                    jpeg_quality: Some(bad),
                    max_side: None,
                },
            );
            assert!(matches!(r, Err(PolyError::InvalidRequest(_))), "q={bad}");
        }
        let r = damage_image(
            &png,
            ImageDamage {
                jpeg_quality: None,
                max_side: Some(0),
            },
        );
        assert!(matches!(r, Err(PolyError::InvalidRequest(_))));
        assert!(damage_image(b"not an image", ImageDamage::default()).is_err());
        assert!(damage_image(&png[..png.len() / 2], ImageDamage::default()).is_err());
    }
}
