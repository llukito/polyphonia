//! Color-grid PNG designed to survive messenger JPEG recompression.
//!
//! Layout (v1):
//! - 1024 x 1024 px, a grid of 64 x 64 cells of 16 x 16 px. Each cell covers
//!   2 x 2 whole 8 x 8 JPEG blocks, so its flat colour survives JPEG.
//! - A solid black 3 x 3-cell finder marker in each corner. Black appears
//!   nowhere else, so the decoder finds the markers after resizing, mild
//!   cropping or an aspect change, and maps the grid between their outer corners.
//! - The other 4060 cells carry 2 bits each in one of 4 well-separated
//!   colours, row-major, top-left first, most significant bits first.
//! - Data: the envelope zero-padded to 500 bytes, split into 4 blocks of 125,
//!   each Reed-Solomon encoded with 128 parity bytes (253 bytes, corrects 64
//!   byte errors per block). Blocks are interleaved byte by byte so a damaged
//!   area spreads over all four, then XOR-whitened with a fixed SplitMix64
//!   stream so padding doesn't paint flat areas. The last 3 bytes of the
//!   1015-byte cell stream are whitening only.
//!
//! The cover image option is ignored: the grid owns every pixel.

use std::io::Cursor;

use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use poly_core::envelope::MAGIC;
use poly_core::{Carrier, CarrierKind, CarrierOptions, PolyError};
use reed_solomon::{Decoder, Encoder};

use crate::load::{image_err, load_image};
use crate::rng::SplitMix64;

/// Output side: a power of two below the ~1200 px at which messengers start
/// resizing (docs/roles/2-image.md).
const SIDE_PX: u32 = 1024;
/// Cell side: 2 x 2 JPEG blocks of 8 x 8 px (JPEG, ITU-T T.81 §A.2).
const CELL_PX: u32 = 16;
/// Cells per row and column.
const GRID: usize = (SIDE_PX / CELL_PX) as usize;
/// Side of each corner finder marker, in cells.
const MARKER_CELLS: usize = 3;
/// One marker per corner.
const MARKERS: usize = 4;
/// Bits per cell: 4 colours.
const BITS_PER_CELL: usize = 2;
/// Bits in a byte.
const BITS_PER_BYTE: usize = 8;
/// Cells per byte.
const CELLS_PER_BYTE: usize = BITS_PER_BYTE / BITS_PER_CELL;
/// Cells that carry data: everything but the markers (4060).
const DATA_CELLS: usize = GRID * GRID - MARKERS * MARKER_CELLS * MARKER_CELLS;
/// Bytes the data cells hold (1015).
const RAW_BYTES: usize = DATA_CELLS / CELLS_PER_BYTE;

/// Reed-Solomon blocks; four fill the grid while staying under the GF(256)
/// codeword limit below.
const RS_BLOCKS: usize = 4;
/// Longest Reed-Solomon codeword over GF(256): 2^8 - 1 symbols.
const RS_MAX_BLOCK: usize = 255;
/// Codeword length per block: 4 x 253 = 1012 fits in the 1015 raw bytes.
const RS_BLOCK_LEN: usize = 253;
/// Parity bytes per block: about 50% (docs/roles/2-image.md), corrects up to
/// RS_PARITY / 2 = 64 wrong bytes per block.
const RS_PARITY: usize = 128;
/// Data bytes per block.
const RS_DATA: usize = RS_BLOCK_LEN - RS_PARITY;
/// Envelope bytes this carrier holds (500).
const CAPACITY: usize = RS_BLOCKS * RS_DATA;
const _: () = assert!(RS_BLOCK_LEN <= RS_MAX_BLOCK);
const _: () = assert!(RS_BLOCKS * RS_BLOCK_LEN <= RAW_BYTES);
const _: () = assert!(DATA_CELLS.is_multiple_of(CELLS_PER_BYTE));

/// Seed of the whitening stream. ASCII "ROBUST".
const WHITENING_SEED: u64 = 0x524F_4255_5354;

/// The 4 data colours, indexed by 2-bit symbol. Chosen far apart in RGB
/// (closest pair, red/yellow, is 166 apart) and all with Rec. 601 luma >= 94,
/// well above the black markers.
const PALETTE: [[u8; 3]; 4] = [
    [230, 50, 60],  // red
    [40, 170, 70],  // green
    [50, 90, 230],  // blue
    [245, 215, 60], // yellow
];
/// Finder marker colour.
const MARKER: [u8; 3] = [0, 0, 0];
/// Pixels darker than this count as marker: halfway between black and the
/// darkest palette colour's luma (blue, 94).
const DARK_LUMA: u32 = 47;
/// Rec. 601 luma weights, in thousandths (ITU-R BT.601).
const LUMA_WEIGHTS: [u32; 3] = [299, 587, 114];
const LUMA_SCALE: u32 = 1000;

/// Below this many pixels per cell the colours blur together; give up.
const MIN_CELL_PX: f32 = 3.0;
/// A row or column belongs to a marker when at least this fraction of the
/// marker's expected width is dark in it.
const MARKER_ROW_FILL: f32 = 0.5;
/// A found marker may differ from its expected size by this fraction
/// (covers mild cropping and aspect changes).
const MARKER_SIZE_TOLERANCE: f32 = 0.4;
/// Sample a box this fraction of a cell on each side of its centre, away
/// from edges where JPEG and resizing bleed colours.
const SAMPLE_RADIUS: f32 = 0.25;
/// Offset from a cell's corner to its centre, in cells.
const CELL_CENTRE: f32 = 0.5;

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

    fn capacity(&self, _opts: &CarrierOptions) -> Option<usize> {
        Some(CAPACITY)
    }

    fn encode(&self, envelope: &[u8], _opts: &CarrierOptions) -> Result<Vec<u8>, PolyError> {
        if envelope.len() > CAPACITY {
            return Err(PolyError::CarrierTooSmall {
                needed: envelope.len(),
                available: CAPACITY,
            });
        }
        let mut data = envelope.to_vec();
        data.resize(CAPACITY, 0);

        let rs = Encoder::new(RS_PARITY);
        let mut stream = vec![0u8; RAW_BYTES];
        for (b, chunk) in data.chunks(RS_DATA).enumerate() {
            for (j, &byte) in rs.encode(chunk).iter().enumerate() {
                stream[j * RS_BLOCKS + b] = byte;
            }
        }
        whiten(&mut stream);

        let mut img = RgbImage::from_pixel(SIDE_PX, SIDE_PX, Rgb(MARKER));
        for (i, (cx, cy)) in data_cells().enumerate() {
            fill_cell(
                &mut img,
                cx,
                cy,
                Rgb(PALETTE[usize::from(symbol(&stream, i))]),
            );
        }

        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .map_err(image_err)?;
        Ok(bytes)
    }

    /// Accepts any image format, since messengers usually deliver it as JPEG.
    ///
    /// When the grid is found but a block has too many errors to correct, the
    /// uncorrected bytes are returned anyway: the envelope CRC then fails and
    /// the app can say "compressed in transit".
    fn decode(&self, file: &[u8]) -> Result<Vec<u8>, PolyError> {
        if image::guess_format(file).is_err() {
            return Err(PolyError::NotThisCarrier);
        }
        let img = load_image(file)
            .map_err(|_| PolyError::NotThisCarrier)?
            .to_rgb8();
        let grid = find_grid(&img).ok_or(PolyError::NotThisCarrier)?;

        let mut stream = vec![0u8; RAW_BYTES];
        for (i, (cx, cy)) in data_cells().enumerate() {
            let sym = nearest_palette(grid.sample(&img, cx, cy));
            stream[i / CELLS_PER_BYTE] |= sym << symbol_shift(i);
        }
        whiten(&mut stream);

        let rs = Decoder::new(RS_PARITY);
        let mut out = Vec::with_capacity(CAPACITY);
        for b in 0..RS_BLOCKS {
            let block: Vec<u8> = (0..RS_BLOCK_LEN)
                .map(|j| stream[j * RS_BLOCKS + b])
                .collect();
            match rs.correct(&block, None) {
                Ok(fixed) => out.extend_from_slice(fixed.data()),
                Err(_) => out.extend_from_slice(&block[..RS_DATA]),
            }
        }
        if !out.starts_with(&MAGIC) {
            return Err(PolyError::NotThisCarrier);
        }
        Ok(out)
    }
}

/// Whether a cell is part of a corner marker.
fn is_marker_cell(cx: usize, cy: usize) -> bool {
    let edge = |c: usize| !(MARKER_CELLS..GRID - MARKER_CELLS).contains(&c);
    edge(cx) && edge(cy)
}

/// Data cells (column, row) in row-major order.
fn data_cells() -> impl Iterator<Item = (usize, usize)> {
    (0..GRID)
        .flat_map(|cy| (0..GRID).map(move |cx| (cx, cy)))
        .filter(|&(cx, cy)| !is_marker_cell(cx, cy))
}

fn fill_cell(img: &mut RgbImage, cx: usize, cy: usize, color: Rgb<u8>) {
    for y in 0..CELL_PX {
        for x in 0..CELL_PX {
            img.put_pixel(cx as u32 * CELL_PX + x, cy as u32 * CELL_PX + y, color);
        }
    }
}

/// Bit offset of the `i`-th 2-bit symbol inside its byte, most significant first.
fn symbol_shift(i: usize) -> usize {
    BITS_PER_CELL * (CELLS_PER_BYTE - 1 - i % CELLS_PER_BYTE)
}

/// The `i`-th 2-bit symbol of `stream`.
fn symbol(stream: &[u8], i: usize) -> u8 {
    (stream[i / CELLS_PER_BYTE] >> symbol_shift(i)) & ((1 << BITS_PER_CELL) - 1)
}

/// XORs `stream` with the fixed whitening stream (its own inverse).
fn whiten(stream: &mut [u8]) {
    let mut rng = SplitMix64(WHITENING_SEED);
    for chunk in stream.chunks_mut(size_of::<u64>()) {
        for (byte, mask) in chunk.iter_mut().zip(rng.next().to_le_bytes()) {
            *byte ^= mask;
        }
    }
}

fn luma(px: &Rgb<u8>) -> u32 {
    px.0.iter()
        .zip(LUMA_WEIGHTS)
        .map(|(&c, w)| u32::from(c) * w)
        .sum::<u32>()
        / LUMA_SCALE
}

/// Index of the palette colour closest to `rgb`.
fn nearest_palette(rgb: [f32; 3]) -> u8 {
    let dist = |p: &[u8; 3]| -> f32 {
        p.iter()
            .zip(rgb)
            .map(|(&a, b)| (f32::from(a) - b).powi(2))
            .sum()
    };
    let mut best = 0;
    for (i, p) in PALETTE.iter().enumerate() {
        if dist(p) < dist(&PALETTE[best]) {
            best = i;
        }
    }
    best as u8
}

/// A point in image pixels.
type Point = (f32, f32);

/// Where the grid's outer corners are in the image.
struct Grid {
    top_left: Point,
    top_right: Point,
    bottom_left: Point,
    bottom_right: Point,
}

impl Grid {
    /// Average colour around the centre of cell (cx, cy).
    fn sample(&self, img: &RgbImage, cx: usize, cy: usize) -> [f32; 3] {
        let lerp = |a: Point, b: Point, t: f32| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        let u = (cx as f32 + CELL_CENTRE) / GRID as f32;
        let v = (cy as f32 + CELL_CENTRE) / GRID as f32;
        let top = lerp(self.top_left, self.top_right, u);
        let bottom = lerp(self.bottom_left, self.bottom_right, u);
        let (px, py) = lerp(top, bottom, v);
        let cell_w = (self.top_right.0 - self.top_left.0) / GRID as f32;
        let r = (cell_w * SAMPLE_RADIUS) as i64;

        let (w, h) = (i64::from(img.width()), i64::from(img.height()));
        let (px, py) = (px as i64, py as i64);
        let mut sum = [0f32; 3];
        let mut n = 0f32;
        for y in (py - r).max(0)..=(py + r).min(h - 1) {
            for x in (px - r).max(0)..=(px + r).min(w - 1) {
                let p = img.get_pixel(x as u32, y as u32);
                for (s, &c) in sum.iter_mut().zip(&p.0) {
                    *s += f32::from(c);
                }
                n += 1.0;
            }
        }
        if n > 0.0 {
            sum.iter_mut().for_each(|s| *s /= n);
        }
        sum
    }
}

/// Finds the four corner markers and returns the grid's outer corners.
fn find_grid(img: &RgbImage) -> Option<Grid> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let cell = w.min(h) as f32 / GRID as f32;
    if cell < MIN_CELL_PX {
        return None;
    }
    let marker = cell * MARKER_CELLS as f32;
    let min_run = (marker * MARKER_ROW_FILL) as usize;
    let dark: Vec<bool> = img.pixels().map(|p| luma(p) < DARK_LUMA).collect();
    let is_dark = |x: usize, y: usize| dark[y * w + x];
    let fits = |len: usize| ((len as f32) - marker).abs() <= marker * MARKER_SIZE_TOLERANCE;

    // Bounding box (x0, y0, x1, y1), end-exclusive, of the marker in a quadrant.
    let find = |xs: std::ops::Range<usize>, ys: std::ops::Range<usize>| {
        let rows: Vec<usize> = ys
            .clone()
            .filter(|&y| xs.clone().filter(|&x| is_dark(x, y)).count() >= min_run)
            .collect();
        let cols: Vec<usize> = xs
            .filter(|&x| ys.clone().filter(|&y| is_dark(x, y)).count() >= min_run)
            .collect();
        let (y0, y1) = (*rows.first()?, *rows.last()? + 1);
        let (x0, x1) = (*cols.first()?, *cols.last()? + 1);
        (fits(x1 - x0) && fits(y1 - y0)).then_some([x0, y0, x1, y1].map(|v| v as f32))
    };

    let (mid_x, mid_y) = (w / 2, h / 2);
    let [tl_x0, tl_y0, _, _] = find(0..mid_x, 0..mid_y)?;
    let [_, tr_y0, tr_x1, _] = find(mid_x..w, 0..mid_y)?;
    let [bl_x0, _, _, bl_y1] = find(0..mid_x, mid_y..h)?;
    let [_, _, br_x1, br_y1] = find(mid_x..w, mid_y..h)?;
    Some(Grid {
        top_left: (tl_x0, tl_y0),
        top_right: (tr_x1, tr_y0),
        bottom_left: (bl_x0, bl_y1),
        bottom_right: (br_x1, br_y1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ImageDamage, damage_image};
    use image::imageops::{self, FilterType};
    use poly_core::envelope::{ENVELOPE_OVERHEAD, HEADER_LEN};
    use poly_core::{Algorithm, Envelope};

    /// Payload of a Fake "hi" envelope (111 bytes total, docs/roles/2-image.md).
    const FAKE_HI_PAYLOAD: usize = 88;
    /// Payload of an RSA-2048 "hi" envelope (367 bytes total).
    const RSA_HI_PAYLOAD: usize = 344;
    /// Smallest ML-KEM-768 envelope (SPEC §5).
    const MLKEM_MIN_ENVELOPE: usize = 1199;
    const WHITE: Rgb<u8> = Rgb([u8::MAX; 3]);

    fn envelope(payload_len: usize) -> Vec<u8> {
        let payload = (0..payload_len).map(|i| (i * 37 % 251) as u8).collect();
        Envelope::new(Algorithm::Fake, 0, [3; 8], payload)
            .to_bytes()
            .unwrap()
    }

    fn encode(payload_len: usize) -> (Vec<u8>, Vec<u8>) {
        let env = envelope(payload_len);
        let file = RobustPngCarrier
            .encode(&env, &CarrierOptions::default())
            .unwrap();
        (env, file)
    }

    fn rgb(file: &[u8]) -> RgbImage {
        image::load_from_memory(file).unwrap().to_rgb8()
    }

    fn png(img: RgbImage) -> Vec<u8> {
        let mut out = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    fn assert_decodes(file: &[u8], env: &[u8], what: &str) {
        let out = RobustPngCarrier
            .decode(file)
            .unwrap_or_else(|e| panic!("{what}: {e}"));
        assert_eq!(&out[..env.len()], env, "{what}");
    }

    #[test]
    fn roundtrip() {
        poly_core::test_utils::assert_carrier_roundtrip(&RobustPngCarrier);
    }

    #[test]
    fn layout_numbers_match_the_guide() {
        assert_eq!(GRID, 64);
        assert_eq!(DATA_CELLS, 4060);
        assert_eq!(CAPACITY, 500);
        let (_, file) = encode(FAKE_HI_PAYLOAD);
        assert_eq!(rgb(&file).dimensions(), (SIDE_PX, SIDE_PX));
    }

    #[test]
    fn mlkem_does_not_fit() {
        let env = vec![0u8; MLKEM_MIN_ENVELOPE];
        assert_eq!(
            RobustPngCarrier.encode(&env, &CarrierOptions::default()),
            Err(PolyError::CarrierTooSmall {
                needed: MLKEM_MIN_ENVELOPE,
                available: CAPACITY
            })
        );
    }

    /// The point of this carrier: what a messenger does to a photo.
    #[test]
    fn survives_jpeg_and_resize() {
        // Smallest realistic message and a full grid; the easier settings
        // (q90, 900 px) passed too but cost CI time in debug builds.
        for payload in [FAKE_HI_PAYLOAD, CAPACITY - ENVELOPE_OVERHEAD] {
            let (env, file) = encode(payload);
            for quality in [50, 70] {
                for max_side in [None, Some(800), Some(640), Some(512)] {
                    let damaged = damage_image(
                        &file,
                        ImageDamage {
                            jpeg_quality: Some(quality),
                            max_side,
                        },
                    )
                    .unwrap();
                    let what = format!("payload {payload}, q{quality}, resize {max_side:?}");
                    assert_decodes(&damaged.bytes, &env, &what);
                }
            }
        }
    }

    #[test]
    fn survives_aspect_change_crop_and_border() {
        let (env, file) = encode(RSA_HI_PAYLOAD);
        let img = rgb(&file);

        let squashed = imageops::resize(&img, 900, 760, FilterType::Triangle);
        assert_decodes(&png(squashed), &env, "aspect change");

        // Trim a few pixels off every edge, as some croppers do.
        let trim = 6;
        let side = SIDE_PX - 2 * trim;
        let cropped = imageops::crop_imm(&img, trim, trim, side, side).to_image();
        assert_decodes(&png(cropped), &env, "crop");

        // Or a white frame around it.
        let border = 40;
        let mut framed = RgbImage::from_pixel(SIDE_PX + 2 * border, SIDE_PX + 2 * border, WHITE);
        imageops::replace(&mut framed, &img, i64::from(border), i64::from(border));
        assert_decodes(&png(framed), &env, "border");
    }

    #[test]
    fn reed_solomon_repairs_a_sticker() {
        let (env, file) = encode(RSA_HI_PAYLOAD);
        let mut img = rgb(&file);
        // A 10 x 10-cell white sticker in the middle: 100 cells = 25 bytes.
        for cy in 27..37 {
            for cx in 27..37 {
                fill_cell(&mut img, cx, cy, WHITE);
            }
        }
        assert_decodes(&png(img), &env, "sticker");
    }

    #[test]
    fn hopeless_damage_reports_checksum_mismatch() {
        let (env, file) = encode(FAKE_HI_PAYLOAD);
        let mut img = rgb(&file);
        // Repaint every data cell with the wrong colour.
        for (i, (cx, cy)) in data_cells().enumerate() {
            let wrong = (i + 1) % PALETTE.len();
            fill_cell(&mut img, cx, cy, Rgb(PALETTE[wrong]));
        }
        // Then put the envelope header back (block 0's first bytes), so only
        // the payload is wrong.
        let mut stream = vec![0u8; RAW_BYTES];
        let mut data = env.clone();
        data.resize(CAPACITY, 0);
        let rs = Encoder::new(RS_PARITY);
        for (b, chunk) in data.chunks(RS_DATA).enumerate() {
            for (j, &byte) in rs.encode(chunk).iter().enumerate() {
                stream[j * RS_BLOCKS + b] = byte;
            }
        }
        whiten(&mut stream);
        let header_bytes: Vec<usize> = (0..HEADER_LEN).map(|j| j * RS_BLOCKS).collect();
        for (i, (cx, cy)) in data_cells().enumerate() {
            if header_bytes.contains(&(i / CELLS_PER_BYTE)) {
                fill_cell(
                    &mut img,
                    cx,
                    cy,
                    Rgb(PALETTE[usize::from(symbol(&stream, i))]),
                );
            }
        }
        let out = RobustPngCarrier.decode(&png(img)).unwrap();
        assert_eq!(Envelope::from_bytes(&out), Err(PolyError::ChecksumMismatch));
    }

    #[test]
    fn other_images_are_not_this_carrier() {
        let flat = png(RgbImage::from_pixel(256, 256, Rgb([200; 3])));
        let mut rng = SplitMix64(7);
        let noise = png(RgbImage::from_fn(512, 512, |_, _| Rgb(rng.color())));
        let stealth = crate::StealthPngCarrier
            .encode(&envelope(FAKE_HI_PAYLOAD), &CarrierOptions::default())
            .unwrap();
        for (name, file) in [("flat", flat), ("noise", noise), ("stealth", stealth)] {
            assert_eq!(
                RobustPngCarrier.decode(&file),
                Err(PolyError::NotThisCarrier),
                "{name}"
            );
        }
    }
}
