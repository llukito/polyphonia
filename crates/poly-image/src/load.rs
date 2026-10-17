//! Bounded image decoding shared by the carriers and `damage`.

use std::io::Cursor;

use image::{DynamicImage, ImageReader, Limits};
use poly_core::PolyError;

/// Largest width or height accepted from an input file. 16384 px is far above
/// any messenger photo limit (WhatsApp/Telegram cap around 1600-2560 px) and
/// keeps a hostile header from making us allocate gigabytes.
pub(crate) const MAX_INPUT_SIDE: u32 = 16_384;

/// Bytes per RGBA8 pixel, the widest layout we decode into.
const RGBA8_BYTES_PER_PIXEL: u64 = 4;

/// Upper bound on decoder allocations: a 16384 x 16384 RGBA8 image is 1 GiB,
/// so allow exactly that.
const MAX_DECODE_ALLOC: u64 = MAX_INPUT_SIDE as u64 * MAX_INPUT_SIDE as u64 * RGBA8_BYTES_PER_PIXEL;

/// Decodes an image in any format the `image` crate reads, refusing
/// dimensions or allocations above the limits above.
pub(crate) fn load_image(file: &[u8]) -> Result<DynamicImage, PolyError> {
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
    reader.decode().map_err(image_err)
}

/// Wraps an `image` crate error.
pub(crate) fn image_err(e: image::ImageError) -> PolyError {
    PolyError::Carrier(e.to_string())
}
