//! Shared note codec for the music carriers (role 3).
//!
//! Every data note carries 5 bits: a 3-bit index into an 8-note scale (the
//! pitch) and a 2-bit index into four durations. At an average of 2.5
//! duration units per note and a 50 ms unit, that is 8 notes/s =
//! 5 bits x 8 = 40 bits/s, the capacity `docs/SPEC.md` §Carriers documents.
//!
//! The stream starts with a fixed four-note marker. `decode` requires the
//! marker *and* the `POLY` envelope magic, so ordinary MIDI/WAV files are
//! rejected quickly instead of being mistaken for a message.

use poly_core::PolyError;
use poly_core::envelope::MAGIC;

/// Bits carried by one note: 3 pitch bits + 2 duration bits.
pub(crate) const BITS_PER_NOTE: usize = 5;
/// Number of distinct pitches (3 bits => 8).
pub(crate) const PITCH_COUNT: usize = 8;
/// Number of distinct durations (2 bits => 4).
pub(crate) const DURATION_COUNT: usize = 4;

/// Scale in Hz: E minor pentatonic across 1.5 octaves
/// (E4 G4 A4 B4 D5 E5 G5 B5). Every neighbouring pair is at least a whole
/// tone (~12%) apart, so FFT pitch detection cannot confuse them, and all
/// notes sit in 300–1000 Hz, the core of the 300–3400 Hz telephone band
/// that every voice codec keeps.
pub(crate) const SCALE_HZ: [f32; PITCH_COUNT] = [
    329.63, 392.00, 440.00, 493.88, 587.33, 659.26, 783.99, 987.77,
];

/// The same scale as MIDI note numbers (E4 G4 A4 B4 D5 E5 G5 B5), so the
/// MIDI path is exact: a pitch index maps to the same note in both carriers.
pub(crate) const SCALE_MIDI: [u8; PITCH_COUNT] = [64, 67, 69, 71, 74, 76, 79, 83];

/// Duration of the shortest note unit in ms. With an average of 2.5 units
/// per note this gives the 8 notes/s (40 bits/s) of `docs/SPEC.md`.
pub(crate) const UNIT_MS: u32 = 50;
/// The four durations in units (2 bits): 50, 100, 150, 200 ms.
pub(crate) const DURATION_UNITS: [u8; DURATION_COUNT] = [1, 2, 3, 4];

/// Start marker: four staccato lowest-scale notes (pitch 0, duration 1).
/// Written at the very start of every message; `decode` returns
/// [`PolyError::NotThisCarrier`] when it is missing.
pub(crate) const MARKER_NOTES: [Note; 4] = [(0, 0), (0, 0), (0, 0), (0, 0)];

/// Largest envelope the music carriers write: 1024 bytes = 8192 bits
/// = 1639 data notes + 4 marker notes. At an average of 125 ms per note
/// that is a ~205 s song (worst case, all 200 ms notes: ~329 s) and an
/// ~18 MB WAV. Long enough for a SESSION-mode text, short enough to send
/// through a messenger, and keeps the conformance test practical.
/// Raising it changes no format, only this constant.
pub(crate) const MAX_ENVELOPE_BYTES: usize = 1024;

/// Upper bound on decoded notes: the marker plus the data notes of the
/// largest envelope, with slack. Bounds the work `decode` does on hostile
/// input (random audio can produce thousands of blobs).
pub(crate) const MAX_NOTES: usize =
    MARKER_NOTES.len() + (MAX_ENVELOPE_BYTES * 8).div_ceil(BITS_PER_NOTE) + 8;

/// A melody note: `(pitch index into [`SCALE_HZ`], duration index into
/// [`DURATION_UNITS`])`.
pub(crate) type Note = (u8, u8);

/// Envelope bytes -> start marker + 5-bit data notes (bit 7 of the first
/// byte first). The last note is zero-padded when the bit count is not a
/// multiple of 5. Rejects envelopes over [`MAX_ENVELOPE_BYTES`] with
/// [`PolyError::CarrierTooSmall`] before doing any work.
pub(crate) fn envelope_to_notes(envelope: &[u8]) -> Result<Vec<Note>, PolyError> {
    let needed = envelope.len();
    if needed > MAX_ENVELOPE_BYTES {
        return Err(PolyError::CarrierTooSmall {
            needed,
            available: MAX_ENVELOPE_BYTES,
        });
    }

    let total_bits = needed * 8;
    let data_notes = total_bits.div_ceil(BITS_PER_NOTE);
    let mut notes = Vec::with_capacity(MARKER_NOTES.len() + data_notes);
    notes.extend_from_slice(&MARKER_NOTES);

    for group in 0..data_notes {
        let mut value = 0u8;
        for k in 0..BITS_PER_NOTE {
            let bit_index = group * BITS_PER_NOTE + k;
            let bit = if bit_index < total_bits {
                (envelope[bit_index / 8] >> (7 - bit_index % 8)) & 1
            } else {
                0 // padding after the last byte
            };
            value = (value << 1) | bit;
        }
        notes.push((value >> 2, value & 0b11));
    }
    Ok(notes)
}

/// Data notes -> envelope bytes. Requires the start marker and the `POLY`
/// envelope magic; otherwise returns [`PolyError::NotThisCarrier`] so
/// auto-detect moves on. The padding note at the end is dropped, and the
/// result is the envelope possibly followed by up to one extra zero byte,
/// which [`poly_core::Envelope::from_bytes`] ignores.
pub(crate) fn notes_to_envelope(notes: &[Note]) -> Result<Vec<u8>, PolyError> {
    if notes.len() <= MARKER_NOTES.len() || notes[..MARKER_NOTES.len()] != MARKER_NOTES {
        return Err(PolyError::NotThisCarrier);
    }
    let data = &notes[MARKER_NOTES.len()..];
    for &(pitch, duration) in data {
        if pitch as usize >= PITCH_COUNT || duration as usize >= DURATION_COUNT {
            return Err(PolyError::NotThisCarrier);
        }
    }

    let byte_len = data.len() * BITS_PER_NOTE / 8;
    let mut envelope = vec![0u8; byte_len];
    for bit_index in 0..byte_len * 8 {
        let (pitch, duration) = data[bit_index / BITS_PER_NOTE];
        let value = (pitch << 2) | duration;
        // Bit 0 of the note is its last bit; `k` is the value bit to emit.
        let k = BITS_PER_NOTE - 1 - bit_index % BITS_PER_NOTE;
        if (value >> k) & 1 == 1 {
            envelope[bit_index / 8] |= 1 << (7 - bit_index % 8);
        }
    }

    if !envelope.starts_with(&MAGIC) {
        return Err(PolyError::NotThisCarrier);
    }
    Ok(envelope)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny LCG so the tests need no dependency and still exercise
    /// arbitrary bit patterns.
    fn pseudo_random(len: usize, seed: u8) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        let mut x = seed;
        for i in 0..len {
            x = x.wrapping_mul(37).wrapping_add(11).wrapping_add(i as u8);
            out.push(x);
        }
        out
    }

    #[test]
    fn bits_roundtrip() {
        for len in [4usize, 5, 16, 23, 44, 111, 367, MAX_ENVELOPE_BYTES] {
            let mut envelope = MAGIC.to_vec();
            envelope.extend_from_slice(&pseudo_random(len - 4, 7));
            let notes = envelope_to_notes(&envelope).unwrap();
            let back = notes_to_envelope(&notes).unwrap();
            assert_eq!(&back[..envelope.len()], &envelope[..], "length {len}");
            assert!(back.len() <= envelope.len() + 1);
        }
    }

    #[test]
    fn note_count_matches_bit_rate() {
        // "hi"-sized fake envelope: 111 bytes = 888 bits -> 178 notes,
        // plus the 4-note marker: 182 x 125 ms ≈ 22.8 s. SPEC §3 example.
        assert_eq!(envelope_to_notes(&[b'P'; 111]).unwrap().len(), 4 + 178);
    }

    #[test]
    fn over_capacity_is_carrier_too_small() {
        assert_eq!(
            envelope_to_notes(&[0u8; MAX_ENVELOPE_BYTES + 1]),
            Err(PolyError::CarrierTooSmall {
                needed: MAX_ENVELOPE_BYTES + 1,
                available: MAX_ENVELOPE_BYTES,
            })
        );
    }

    #[test]
    fn marker_and_magic_are_required() {
        assert_eq!(notes_to_envelope(&[]), Err(PolyError::NotThisCarrier));

        let notes = envelope_to_notes(&pseudo_random(64, 3)).unwrap();
        let mut shuffled = notes[1..].to_vec();
        shuffled.push((0, 0));
        assert_eq!(notes_to_envelope(&shuffled), Err(PolyError::NotThisCarrier));

        // Marker present but the payload is not a Polyphonia envelope.
        let mut text = MARKER_NOTES.to_vec();
        text.extend((0..200).map(|i| ((i % 8) as u8, (i % 4) as u8)));
        assert_eq!(notes_to_envelope(&text), Err(PolyError::NotThisCarrier));
    }
}
