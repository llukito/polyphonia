//! Sine-tone melody in a 44.1 kHz WAV file, decoded with FFT pitch
//! detection (role 3).
//!
//! The same 5-bit notes as [`crate::MusicMidiCarrier`] are rendered as pure
//! sine tones in the 300–1000 Hz core of the telephone band, so a voice
//! codec (Opus and friends) keeps them. Notes are separated by short
//! fade-outs, which lets the decoder find note boundaries even when two
//! consecutive notes share a pitch. See `crate::codec`.

use std::f32::consts::PI;
use std::io::Cursor;

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use poly_core::{Carrier, CarrierKind, CarrierOptions, PolyError};
use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use crate::codec::{
    DURATION_COUNT, DURATION_UNITS, MARKER_NOTES, MAX_ENVELOPE_BYTES, MAX_NOTES, Note, SCALE_HZ,
    UNIT_MS, envelope_to_notes, notes_to_envelope,
};

/// CD-standard sample rate (SPEC §Carriers).
pub(crate) const SAMPLE_RATE: u32 = 44_100;
/// 16-bit PCM, one channel: enough for pitch detection; smaller files.
const BITS_PER_SAMPLE: u16 = 16;
const CHANNELS: u16 = 1;
/// Melody peak amplitude, about -6 dBFS.
const AMPLITUDE: f32 = 0.5;

/// Raised-cosine fade-in at the start of every note: avoids clicks and
/// gives the FFT a settled window.
const FADE_IN_MS: u32 = 5;
/// Raised-cosine fade-out at the end of every note's sound.
const FADE_OUT_MS: u32 = 10;
/// Silence at the end of every note span. It is part of the note's 50 ms
/// duration units (so it costs no bit rate) and is what makes boundaries
/// unambiguous even between two notes of the same pitch: the RMS window is
/// 10 ms, so a 25 ms gap always contains several fully silent frames.
const NOTE_GAP_MS: u32 = 25;

/// RMS frame used to find notes.
const ENERGY_WINDOW_MS: u32 = 10;
/// Frame step; 5 ms is fine enough to measure 50 ms units.
const ENERGY_HOP_MS: u32 = 5;
/// A frame counts as "sound" at 20% of the loudest frame. Fades and codec
/// noise fall below it; the steady part of every note stays above it.
const SOUND_THRESHOLD_RATIO: f32 = 0.2;
/// Sound blobs separated by a shorter dip than this are one note (codec
/// noise); the true silence gaps above are 25 ms, much longer.
const MAX_MERGE_GAP_MS: u32 = 5;
/// Shorter blobs are codec noise, not notes (a one-unit note has ~25 ms of
/// sound).
const MIN_REGION_MS: u32 = 15;
/// The fades and the 10 ms RMS window shrink the measured sound region by
/// about this much (measured on encoded files); it is added back before
/// deciding a note's duration in order to centre the rounding boundaries.
const DURATION_BIAS_MS: u32 = 15;
/// Below this peak RMS the file is silence, not a message.
const SILENCE_EPSILON: f32 = 1e-4;

/// FFT size for pitch detection: 46 ms at 44.1 kHz, ~21.5 Hz bins; with
/// parabolic interpolation that resolves the scale's smallest gap (48 Hz).
const FFT_SIZE: usize = 2048;
/// Search band for the fundamental, a margin around [`SCALE_HZ`].
const PITCH_MIN_HZ: f32 = 280.0;
const PITCH_MAX_HZ: f32 = 1100.0;
/// Use the middle of each note: skip the attack, take half the note.
const PITCH_WINDOW_START_FRACTION: f32 = 0.25;
const PITCH_WINDOW_LENGTH_FRACTION: f32 = 0.5;

/// A WAV can hold at most the largest envelope: marker + data notes at the
/// longest duration.
fn analysis_sample_limit(sample_rate: usize) -> usize {
    let max_units = MAX_NOTES as u64 * DURATION_UNITS[DURATION_COUNT - 1] as u64;
    (sample_rate as u64 * max_units * UNIT_MS as u64 / 1_000) as usize + sample_rate
}

/// At least the marker plus one data note, or there is no message in here.
fn minimum_samples(sample_rate: usize) -> usize {
    ms_to_samples(sample_rate, (MARKER_NOTES.len() as u32 + 1) * UNIT_MS)
}

/// Samples in `ms` milliseconds.
fn ms_to_samples(sample_rate: usize, ms: u32) -> usize {
    (sample_rate * ms as usize / 1_000).max(1)
}

/// Raised-cosine gain at sample `i` of a note `span` samples long: 0 at
/// both ends, 1 in the middle. This is what makes note boundaries silent.
fn note_gain(i: usize, span: usize, fade_in: usize, fade_out: usize) -> f32 {
    if i < fade_in {
        return 0.5 - 0.5 * (PI * i as f32 / fade_in as f32).cos();
    }
    let remaining = span - i;
    if remaining < fade_out {
        return 0.5 - 0.5 * (PI * remaining as f32 / fade_out as f32).cos();
    }
    1.0
}

/// Sine-tone melody in a 44.1 kHz WAV file, decoded with FFT pitch detection.
#[derive(Debug, Default, Clone, Copy)]
pub struct MusicWavCarrier;

impl Carrier for MusicWavCarrier {
    fn kind(&self) -> CarrierKind {
        CarrierKind::MusicWav
    }

    fn file_extension(&self) -> &'static str {
        "wav"
    }

    fn mime_type(&self) -> &'static str {
        "audio/wav"
    }

    fn capacity(&self, _opts: &CarrierOptions) -> Option<usize> {
        Some(MAX_ENVELOPE_BYTES)
    }

    fn encode(&self, envelope: &[u8], _opts: &CarrierOptions) -> Result<Vec<u8>, PolyError> {
        let notes = envelope_to_notes(envelope)?;
        let sample_rate = SAMPLE_RATE as usize;
        let unit = ms_to_samples(sample_rate, UNIT_MS);
        let gap = ms_to_samples(sample_rate, NOTE_GAP_MS);
        let fade_in = ms_to_samples(sample_rate, FADE_IN_MS);
        let fade_out = ms_to_samples(sample_rate, FADE_OUT_MS);

        let spec = WavSpec {
            channels: CHANNELS,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: BITS_PER_SAMPLE,
            sample_format: SampleFormat::Int,
        };
        let mut out = Vec::new();
        {
            let mut writer = WavWriter::new(Cursor::new(&mut out), spec)
                .map_err(|e| PolyError::Carrier(format!("could not start WAV: {e}")))?;
            for &(pitch, duration) in &notes {
                let span = DURATION_UNITS[duration as usize] as usize * unit;
                let sound = span - gap.min(span);
                let frequency = SCALE_HZ[pitch as usize];
                for i in 0..sound {
                    let phase = 2.0 * PI * frequency * i as f32 / sample_rate as f32;
                    let gain = note_gain(i, sound, fade_in.min(sound), fade_out.min(sound));
                    let sample = AMPLITUDE * gain * phase.sin();
                    writer
                        .write_sample((sample * i16::MAX as f32) as i16)
                        .map_err(|e| PolyError::Carrier(format!("could not write WAV: {e}")))?;
                }
                // Trailing silence: marks the end of this note.
                for _ in sound..span {
                    writer
                        .write_sample(0i16)
                        .map_err(|e| PolyError::Carrier(format!("could not write WAV: {e}")))?;
                }
            }
            writer
                .finalize()
                .map_err(|e| PolyError::Carrier(format!("could not finish WAV: {e}")))?;
        }
        Ok(out)
    }

    fn decode(&self, file: &[u8]) -> Result<Vec<u8>, PolyError> {
        // RIFF/WAVE check first: this is the O(1) rejection of MIDI and junk.
        if file.len() < 12 || &file[..4] != b"RIFF" || &file[8..12] != b"WAVE" {
            return Err(PolyError::NotThisCarrier);
        }
        let mut reader =
            WavReader::new(Cursor::new(file)).map_err(|_| PolyError::NotThisCarrier)?;
        let spec = reader.spec();
        let sample_rate = spec.sample_rate as usize;
        let channels = spec.channels as usize;
        if sample_rate == 0 || channels == 0 {
            return Err(PolyError::NotThisCarrier);
        }
        let limit = analysis_sample_limit(sample_rate);

        // Read channel 0 into mono float samples, at most `limit` of them.
        let mut mono: Vec<f32> = Vec::new();
        match (spec.sample_format, spec.bits_per_sample) {
            (SampleFormat::Int, 16) => {
                for (i, sample) in reader.samples::<i16>().enumerate() {
                    let value = sample.map_err(|_| PolyError::NotThisCarrier)?;
                    if i % channels == 0 {
                        mono.push(value as f32 / i16::MAX as f32);
                    }
                    if mono.len() >= limit {
                        break;
                    }
                }
            }
            (SampleFormat::Float, 32) => {
                for (i, sample) in reader.samples::<f32>().enumerate() {
                    let value = sample.map_err(|_| PolyError::NotThisCarrier)?;
                    if i % channels == 0 {
                        mono.push(value);
                    }
                    if mono.len() >= limit {
                        break;
                    }
                }
            }
            _ => return Err(PolyError::NotThisCarrier),
        }
        if mono.len() < minimum_samples(sample_rate) {
            return Err(PolyError::NotThisCarrier);
        }

        let notes = detect_notes(&mono, sample_rate)?;
        notes_to_envelope(&notes)
    }
}

/// Find note regions in the samples and turn each into one 5-bit note.
fn detect_notes(samples: &[f32], sample_rate: usize) -> Result<Vec<Note>, PolyError> {
    let hop = ms_to_samples(sample_rate, ENERGY_HOP_MS);
    let window = ms_to_samples(sample_rate, ENERGY_WINDOW_MS);
    if samples.len() < window {
        return Err(PolyError::NotThisCarrier);
    }

    // Per-frame RMS, then a "sound" flag at a fixed fraction of the peak.
    let frame_count = (samples.len() - window) / hop + 1;
    let mut rms = Vec::with_capacity(frame_count);
    for frame in 0..frame_count {
        let start = frame * hop;
        let slice = &samples[start..start + window];
        let power: f32 = slice.iter().map(|s| s * s).sum();
        rms.push((power / window as f32).sqrt());
    }
    let peak = rms.iter().copied().fold(0.0f32, f32::max);
    if peak <= SILENCE_EPSILON {
        return Err(PolyError::NotThisCarrier);
    }
    let threshold = peak * SOUND_THRESHOLD_RATIO;
    let sound: Vec<bool> = rms.iter().map(|&value| value >= threshold).collect();

    // Contiguous sound runs, bridging dips shorter than MAX_MERGE_GAP_MS.
    let merge_frames = ms_to_samples(sample_rate, MAX_MERGE_GAP_MS).div_ceil(hop);
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut run_start: Option<usize> = None;
    for (frame, &is_sound) in sound.iter().enumerate() {
        if is_sound && run_start.is_none() {
            run_start = Some(frame);
        } else if !is_sound && let Some(start) = run_start.take() {
            runs.push((start, frame - 1));
        }
    }
    if let Some(start) = run_start {
        runs.push((start, sound.len() - 1));
    }
    let mut regions: Vec<(usize, usize)> = Vec::with_capacity(runs.len());
    for run in runs {
        if let Some(last) = regions.last_mut()
            && run.0 <= last.1 + merge_frames + 1
        {
            last.1 = run.1;
        } else {
            regions.push(run);
        }
    }

    // One region = one note. Region length plus the measured fade/window
    // bias, over the unit length, is the duration.
    let unit = ms_to_samples(sample_rate, UNIT_MS);
    let bias = ms_to_samples(sample_rate, DURATION_BIAS_MS);
    let minimum = ms_to_samples(sample_rate, MIN_REGION_MS);
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let mut notes: Vec<Note> = Vec::new();
    for (first, last) in regions {
        let start = first * hop;
        let end = (last * hop + window).min(samples.len());
        if end - start < minimum {
            continue;
        }
        let units = ((end - start + bias) as f32 / unit as f32).round() as u32;
        let duration = (units.clamp(1, DURATION_COUNT as u32) - 1) as u8;
        let pitch = detect_pitch(&samples[start..end], sample_rate, fft.as_ref());
        if notes.len() < MAX_NOTES {
            notes.push((pitch, duration));
        }
    }
    if notes.is_empty() {
        return Err(PolyError::NotThisCarrier);
    }
    Ok(notes)
}

/// FFT the middle of one note region and snap the strongest peak to the
/// nearest scale pitch. The scale neighbours are ≥ 12% apart, far more than
/// the interpolation error even after a voice codec.
fn detect_pitch(region: &[f32], sample_rate: usize, fft: &dyn rustfft::Fft<f32>) -> u8 {
    let length = region.len();
    let start = (length as f32 * PITCH_WINDOW_START_FRACTION) as usize;
    let take = ((length as f32 * PITCH_WINDOW_LENGTH_FRACTION) as usize)
        .clamp(64, FFT_SIZE)
        .min(length - start);
    let real = &region[start..start + take];

    // Hann window, zero-padded to FFT_SIZE.
    let mut buffer = vec![Complex::new(0.0f32, 0.0); FFT_SIZE];
    let denominator = real.len().saturating_sub(1).max(1) as f32;
    for (i, &sample) in real.iter().enumerate() {
        let window = 0.5 - 0.5 * (2.0 * PI * i as f32 / denominator).cos();
        buffer[i] = Complex::new(sample * window, 0.0);
    }
    fft.process(&mut buffer);

    let bin_hz = sample_rate as f32 / FFT_SIZE as f32;
    let min_bin = ((PITCH_MIN_HZ / bin_hz).ceil() as usize).max(1);
    let max_bin = ((PITCH_MAX_HZ / bin_hz).floor() as usize).min(FFT_SIZE / 2 - 1);
    let mut peak = min_bin;
    let mut peak_magnitude = 0.0f32;
    for (bin, value) in buffer.iter().enumerate().take(max_bin + 1).skip(min_bin) {
        let magnitude = value.norm();
        if magnitude > peak_magnitude {
            peak_magnitude = magnitude;
            peak = bin;
        }
    }

    // Parabolic interpolation over the three bins around the peak.
    let (left, center, right) = (
        buffer[peak - 1].norm(),
        buffer[peak].norm(),
        buffer[(peak + 1).min(FFT_SIZE / 2)].norm(),
    );
    let curvature = left - 2.0 * center + right;
    let offset = if curvature.abs() > f32::EPSILON {
        (0.5 * (left - right) / curvature).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    let frequency = (peak as f32 + offset) * bin_hz;

    let mut best = 0usize;
    let mut best_error = f32::MAX;
    for (index, &candidate) in SCALE_HZ.iter().enumerate() {
        let error = (frequency - candidate).abs() / candidate;
        if error < best_error {
            best_error = error;
            best = index;
        }
    }
    best as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        poly_core::test_utils::assert_carrier_roundtrip(&MusicWavCarrier);
    }

    /// A minimal "POLY" envelope.
    fn sample_envelope() -> Vec<u8> {
        let mut bytes = b"POLY".to_vec();
        bytes.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0, 0]);
        bytes.extend_from_slice(&[0x5A; 32]);
        bytes.extend_from_slice(&[0; 4]);
        bytes
    }

    #[test]
    fn file_is_a_cd_quality_wav() {
        let file = MusicWavCarrier
            .encode(&sample_envelope(), &CarrierOptions::default())
            .unwrap();
        let reader = WavReader::new(Cursor::new(&file)).unwrap();
        let spec = reader.spec();
        assert_eq!(
            (spec.sample_rate, spec.channels, spec.bits_per_sample),
            (SAMPLE_RATE, CHANNELS, BITS_PER_SAMPLE)
        );

        // The four marker notes are lowest-scale "E", ~329.6 Hz: check the
        // first note's region through the same detector the decoder uses.
        let mono: Vec<f32> = WavReader::new(Cursor::new(&file))
            .unwrap()
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / i16::MAX as f32)
            .collect();
        let notes = detect_notes(&mono, SAMPLE_RATE as usize).unwrap();
        assert_eq!(&notes[..MARKER_NOTES.len()], &MARKER_NOTES[..]);
    }

    #[test]
    fn every_duration_rounds_back_exactly() {
        // Build a payload that uses all four durations and check the
        // decoder maps region lengths back to the same units.
        let mut envelope = b"POLY".to_vec();
        envelope.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0, 0]);
        envelope.extend((0..64).map(|i| (i * 7) as u8));
        envelope.extend_from_slice(&[0; 4]);
        let file = MusicWavCarrier
            .encode(&envelope, &CarrierOptions::default())
            .unwrap();
        let decoded = MusicWavCarrier.decode(&file).unwrap();
        assert_eq!(&decoded[..envelope.len()], &envelope[..]);
    }
}
