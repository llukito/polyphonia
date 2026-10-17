//! Prepare sample files, then check what a messenger did to them.
//!
//! `prepare` writes `sample.png`, `sample.wav`, `sample.mid` and a
//! `manifest.json` with their SHA-256 hashes. After sending them by hand and
//! saving what arrived under `<received_root>/<platform>/<method>/`, `compare`
//! reports for each received file whether it is byte-identical and, if not,
//! what changed. Owned by role 5. See `docs/platform-tests.md`.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use image::{ImageFormat, RgbImage};
use midly::num::{u4, u7, u15, u28};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Manifest format version.
pub const MANIFEST_VERSION: u32 = 1;
pub const MANIFEST_FILE: &str = "manifest.json";
pub const RESULTS_FILE: &str = "results.md";

pub const SAMPLE_PNG: &str = "sample.png";
pub const SAMPLE_WAV: &str = "sample.wav";
pub const SAMPLE_MID: &str = "sample.mid";

/// 512x512: big enough to look like a real photo to messengers and matches
/// the StealthPng example cover in SPEC §5.
pub const PNG_SIZE: u32 = 512;
/// Fixed seed so every `prepare` run produces the same files.
const NOISE_SEED: u64 = 0x504F_4C59; // "POLY"

/// CD-standard sample rate (also what MusicWav uses, SPEC §5).
pub const WAV_SAMPLE_RATE: u32 = 44_100;
pub const WAV_SECONDS: u32 = 2;
const WAV_BITS_PER_SAMPLE: u16 = 16;
const WAV_CHANNELS: u16 = 1;
/// Tones inside the 300-3400 Hz telephone voice band, a quarter of the clip each.
const WAV_TONES_HZ: [f64; 4] = [440.0, 660.0, 880.0, 1320.0];
/// Half of full scale, so the tones never clip.
const WAV_AMPLITUDE: f64 = 0.5;

/// MIDI ticks per quarter note (a common default resolution).
const MIDI_TICKS_PER_QUARTER: u16 = 480;
/// C major arpeggio: C4 E4 G4 C5 (MIDI note numbers).
const MIDI_NOTES: [u8; 4] = [60, 64, 67, 72];
/// Typical "mezzo-forte" note velocity.
const MIDI_VELOCITY: u8 = 80;

/// Received-file extensions mapped to the sample they came from. Messengers
/// may convert formats (e.g. PNG to JPEG), so related formats map too.
const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "gif", "heic"];
const AUDIO_EXTENSIONS: [&str; 7] = ["wav", "ogg", "opus", "m4a", "mp3", "aac", "caf"];
const MIDI_EXTENSIONS: [&str; 2] = ["mid", "midi"];

/// One prepared sample.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SampleInfo {
    pub name: String,
    pub sha256: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

/// `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub files: Vec<SampleInfo>,
}

impl Manifest {
    fn sample(&self, name: &str) -> Option<&SampleInfo> {
        self.files.iter().find(|f| f.name == name)
    }
}

/// Whether a received file is byte-identical to its sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Identical,
    Changed,
    /// No sample has this file's extension.
    Unmatched,
}

/// One row of the results table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub platform: String,
    pub method: String,
    /// Path relative to `<received_root>/<platform>/<method>/`.
    pub file: String,
    pub sample: Option<String>,
    pub verdict: Verdict,
    pub original_size: Option<u64>,
    pub received_size: u64,
    /// Image format detected from the bytes (images only).
    pub image_format: Option<String>,
    /// Received image dimensions, if readable.
    pub dimensions: Option<(u32, u32)>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn noise_png() -> Result<Vec<u8>> {
    let mut rng = StdRng::seed_from_u64(NOISE_SEED);
    let mut pixels = vec![0u8; (PNG_SIZE * PNG_SIZE * 3) as usize];
    rng.fill_bytes(&mut pixels);
    let img = RgbImage::from_raw(PNG_SIZE, PNG_SIZE, pixels).context("building noise image")?;
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png)?;
    Ok(out.into_inner())
}

fn sine_wav(path: &Path) -> Result<()> {
    let spec = hound::WavSpec {
        channels: WAV_CHANNELS,
        sample_rate: WAV_SAMPLE_RATE,
        bits_per_sample: WAV_BITS_PER_SAMPLE,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec)?;
    let total = WAV_SAMPLE_RATE * WAV_SECONDS;
    let per_tone = total / WAV_TONES_HZ.len() as u32;
    for i in 0..total {
        let freq = WAV_TONES_HZ[((i / per_tone) as usize).min(WAV_TONES_HZ.len() - 1)];
        let t = f64::from(i) / f64::from(WAV_SAMPLE_RATE);
        let v = (2.0 * std::f64::consts::PI * freq * t).sin() * WAV_AMPLITUDE;
        w.write_sample((v * f64::from(i16::MAX)) as i16)?;
    }
    w.finalize()?;
    Ok(())
}

fn arpeggio_midi() -> Result<Vec<u8>> {
    let channel = u4::new(0);
    let vel = u7::new(MIDI_VELOCITY);
    let mut track = Vec::new();
    for note in MIDI_NOTES {
        let key = u7::new(note);
        track.push(TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Midi {
                channel,
                message: MidiMessage::NoteOn { key, vel },
            },
        });
        track.push(TrackEvent {
            delta: u28::new(u32::from(MIDI_TICKS_PER_QUARTER)),
            kind: TrackEventKind::Midi {
                channel,
                message: MidiMessage::NoteOff { key, vel },
            },
        });
    }
    track.push(TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });
    let mut smf = Smf::new(Header::new(
        Format::SingleTrack,
        Timing::Metrical(u15::new(MIDI_TICKS_PER_QUARTER)),
    ));
    smf.tracks.push(track);
    let mut out = Vec::new();
    smf.write_std(&mut out)?;
    Ok(out)
}

fn sample_info(name: &str, bytes: &[u8]) -> SampleInfo {
    let dims = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .filter(|r| r.format().is_some())
        .and_then(|r| r.into_dimensions().ok());
    SampleInfo {
        name: name.into(),
        sha256: sha256_hex(bytes),
        size: bytes.len() as u64,
        width: dims.map(|d| d.0),
        height: dims.map(|d| d.1),
    }
}

/// Write the three samples and `manifest.json` into `dir` (created if missing).
pub fn prepare(dir: &Path) -> Result<Manifest> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    fs::write(dir.join(SAMPLE_PNG), noise_png()?)?;
    sine_wav(&dir.join(SAMPLE_WAV))?;
    fs::write(dir.join(SAMPLE_MID), arpeggio_midi()?)?;

    let mut files = Vec::new();
    for name in [SAMPLE_PNG, SAMPLE_WAV, SAMPLE_MID] {
        let bytes = fs::read(dir.join(name))?;
        files.push(sample_info(name, &bytes));
    }
    let manifest = Manifest {
        version: MANIFEST_VERSION,
        files,
    };
    fs::write(
        dir.join(MANIFEST_FILE),
        serde_json::to_string_pretty(&manifest)? + "\n",
    )?;
    Ok(manifest)
}

fn read_manifest(dir: &Path) -> Result<Manifest> {
    let path = dir.join(MANIFEST_FILE);
    let text = fs::read_to_string(&path).with_context(|| {
        format!(
            "reading {} (run `platform-check prepare` first)",
            path.display()
        )
    })?;
    let manifest: Manifest = serde_json::from_str(&text)?;
    if manifest.version != MANIFEST_VERSION {
        bail!("unsupported manifest version {}", manifest.version);
    }
    Ok(manifest)
}

fn sample_for_extension(ext: &str) -> Option<&'static str> {
    let ext = ext.to_ascii_lowercase();
    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        Some(SAMPLE_PNG)
    } else if AUDIO_EXTENSIONS.contains(&ext.as_str()) {
        Some(SAMPLE_WAV)
    } else if MIDI_EXTENSIONS.contains(&ext.as_str()) {
        Some(SAMPLE_MID)
    } else {
        None
    }
}

fn sorted_entries(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.retain(|p| {
        !p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.'))
    });
    entries.sort();
    Ok(entries)
}

/// All files under `dir`, recursively, sorted.
fn files_under(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for path in sorted_entries(dir)? {
        if path.is_dir() {
            out.extend(files_under(&path)?);
        } else {
            out.push(path);
        }
    }
    Ok(out)
}

fn check_file(
    manifest: &Manifest,
    platform: &str,
    method: &str,
    base: &Path,
    path: &Path,
) -> Result<Row> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let file = path
        .strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();
    let sample = path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(sample_for_extension)
        .and_then(|name| manifest.sample(name));

    let (image_format, dimensions) = if sample.is_some_and(|s| s.name == SAMPLE_PNG) {
        let format = image::guess_format(&bytes).ok();
        let dims = format.and_then(|f| {
            image::ImageReader::with_format(Cursor::new(&bytes), f)
                .into_dimensions()
                .ok()
        });
        (
            Some(format.map_or_else(
                || "unknown".to_string(),
                |f| format!("{f:?}").to_uppercase(),
            )),
            dims,
        )
    } else {
        (None, None)
    };

    Ok(Row {
        platform: platform.into(),
        method: method.into(),
        file,
        sample: sample.map(|s| s.name.clone()),
        verdict: match sample {
            None => Verdict::Unmatched,
            Some(s) if s.sha256 == sha256_hex(&bytes) => Verdict::Identical,
            Some(_) => Verdict::Changed,
        },
        original_size: sample.map(|s| s.size),
        received_size: bytes.len() as u64,
        image_format,
        dimensions,
    })
}

/// Compare every file under `<received_root>/<platform>/<method>/` with the
/// samples in `prepared_dir`. Files directly in `received_root` (such as
/// `results.md`) are ignored.
pub fn compare(prepared_dir: &Path, received_root: &Path) -> Result<Vec<Row>> {
    let manifest = read_manifest(prepared_dir)?;
    let mut rows = Vec::new();
    for platform_dir in sorted_entries(received_root)?
        .into_iter()
        .filter(|p| p.is_dir())
    {
        let platform = platform_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        for method_dir in sorted_entries(&platform_dir)?
            .into_iter()
            .filter(|p| p.is_dir())
        {
            let method = method_dir.file_name().unwrap_or_default().to_string_lossy();
            for path in files_under(&method_dir)? {
                rows.push(check_file(
                    &manifest,
                    &platform,
                    &method,
                    &method_dir,
                    &path,
                )?);
            }
        }
    }
    Ok(rows)
}

fn size_change(row: &Row) -> String {
    match row.original_size {
        Some(orig) if orig == row.received_size => format!("{orig} B (same)"),
        Some(orig) => {
            let pct = (row.received_size as f64 - orig as f64) / orig as f64 * 100.0;
            format!("{orig} → {} B ({pct:+.1}%)", row.received_size)
        }
        None => format!("{} B", row.received_size),
    }
}

/// The results as a Markdown table.
pub fn render_markdown(rows: &[Row]) -> String {
    let mut out = String::from(
        "| Platform | Method | File | Sample | Result | Size | Still PNG | Dimensions |\n\
         |---|---|---|---|---|---|---|---|\n",
    );
    for r in rows {
        let result = match r.verdict {
            Verdict::Identical => "identical",
            Verdict::Changed => "**changed**",
            Verdict::Unmatched => "no matching sample",
        };
        let still_png = match r.image_format.as_deref() {
            None => "".to_string(),
            Some("PNG") => "yes".to_string(),
            Some(other) => format!("no ({other})"),
        };
        let dims = r
            .dimensions
            .map_or_else(String::new, |(w, h)| format!("{w}×{h}"));
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            r.platform,
            r.method,
            r.file.replace('|', "\\|"),
            r.sample.as_deref().unwrap_or("-"),
            result,
            size_change(r),
            still_png,
            dims
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A fresh empty directory under the system temp dir.
    fn temp_dir(name: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "platform-check-{}-{}-{name}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn copy_samples(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for name in [SAMPLE_PNG, SAMPLE_WAV, SAMPLE_MID] {
            fs::copy(from.join(name), to.join(name)).unwrap();
        }
    }

    #[test]
    fn prepare_writes_samples_and_manifest() {
        let dir = temp_dir("prepare");
        let m = prepare(&dir).unwrap();
        assert_eq!(m.files.len(), 3);
        let png = m.sample(SAMPLE_PNG).unwrap();
        assert_eq!((png.width, png.height), (Some(PNG_SIZE), Some(PNG_SIZE)));
        assert_eq!(read_manifest(&dir).unwrap(), m);

        let wav = hound::WavReader::open(dir.join(SAMPLE_WAV)).unwrap();
        assert_eq!(wav.spec().sample_rate, WAV_SAMPLE_RATE);
        assert_eq!(wav.spec().channels, 1);
        assert_eq!(wav.duration(), WAV_SAMPLE_RATE * WAV_SECONDS);

        let mid = fs::read(dir.join(SAMPLE_MID)).unwrap();
        assert_eq!(Smf::parse(&mid).unwrap().tracks.len(), 1);
        // Deterministic: a second run gives the same hashes.
        assert_eq!(prepare(&temp_dir("prepare2")).unwrap(), m);
    }

    #[test]
    fn noise_png_does_not_compress() {
        let dir = temp_dir("noise");
        let m = prepare(&dir).unwrap();
        let raw = u64::from(PNG_SIZE * PNG_SIZE * 3);
        assert!(
            m.sample(SAMPLE_PNG).unwrap().size >= raw,
            "noise PNG should be at least raw size"
        );
    }

    #[test]
    fn copy_of_itself_is_identical() {
        let dir = temp_dir("identical");
        let prepared = dir.join("prepared");
        prepare(&prepared).unwrap();
        let received = dir.join("received");
        copy_samples(&prepared, &received.join("telegram").join("file"));

        let rows = compare(&prepared, &received).unwrap();
        assert_eq!(rows.len(), 3);
        for r in &rows {
            assert_eq!(r.verdict, Verdict::Identical, "{r:?}");
            assert_eq!(r.platform, "telegram");
            assert_eq!(r.method, "file");
        }
        let png = rows.iter().find(|r| r.file == SAMPLE_PNG).unwrap();
        assert_eq!(png.image_format.as_deref(), Some("PNG"));
        assert_eq!(png.dimensions, Some((PNG_SIZE, PNG_SIZE)));
        assert!(render_markdown(&rows).contains("identical"));
    }

    #[test]
    fn modified_copy_is_changed() {
        let dir = temp_dir("changed");
        let prepared = dir.join("prepared");
        prepare(&prepared).unwrap();
        let received = dir.join("received");
        let photo = received.join("whatsapp").join("photo");
        copy_samples(&prepared, &photo);

        // WAV: flip one byte. PNG: replace with a resized JPEG under a new name.
        let wav = photo.join(SAMPLE_WAV);
        let mut bytes = fs::read(&wav).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        fs::write(&wav, bytes).unwrap();
        fs::remove_file(photo.join(SAMPLE_PNG)).unwrap();
        let img = image::open(prepared.join(SAMPLE_PNG))
            .unwrap()
            .thumbnail(256, 256);
        img.save(photo.join("IMG_0001.jpg")).unwrap();
        fs::write(photo.join("notes.txt"), "unrelated").unwrap();

        let rows = compare(&prepared, &received).unwrap();
        let by = |f: &str| rows.iter().find(|r| r.file == f).unwrap();
        assert_eq!(by(SAMPLE_MID).verdict, Verdict::Identical);
        assert_eq!(by(SAMPLE_WAV).verdict, Verdict::Changed);
        let jpg = by("IMG_0001.jpg");
        assert_eq!(jpg.verdict, Verdict::Changed);
        assert_eq!(jpg.sample.as_deref(), Some(SAMPLE_PNG));
        assert_eq!(jpg.image_format.as_deref(), Some("JPEG"));
        assert_eq!(jpg.dimensions, Some((256, 256)));
        assert_eq!(by("notes.txt").verdict, Verdict::Unmatched);

        let md = render_markdown(&rows);
        assert!(md.contains("**changed**"));
        assert!(md.contains("no (JPEG)"));
        assert!(md.contains("256×256"));
    }

    #[test]
    fn compare_without_manifest_fails() {
        let dir = temp_dir("nomanifest");
        assert!(compare(&dir, &dir).is_err());
    }

    /// The Mac app relies on this rule (docs/platform-tests.md, "For the Mac app").
    #[test]
    fn platform_profiles_follow_the_rules() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../profiles/platforms.json");
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(v["version"], 1);
        let platforms = v["platforms"].as_array().unwrap();
        assert!(!platforms.is_empty());
        for p in platforms {
            let id = p["id"].as_str().unwrap();
            let lossless = p["lossless_file_mode"].as_bool().unwrap();
            let expected = if lossless { "stealth" } else { "robust" };
            assert_eq!(p["default_mode"], expected, "{id}: default_mode");
            assert!(
                ["research", "verified", "failed"]
                    .contains(&p["verified"]["status"].as_str().unwrap()),
                "{id}"
            );
            assert!(
                p["steps"]["stealth"].is_array() && p["steps"]["robust"].is_array(),
                "{id}"
            );
            assert!(!p["sources"].as_array().unwrap().is_empty(), "{id}");
        }
        assert!(v["other"]["default_mode"].is_null());
        assert!(v["other"]["explain"]["stealth"].is_string());
        assert!(v["other"]["explain"]["robust"].is_string());
    }
}
