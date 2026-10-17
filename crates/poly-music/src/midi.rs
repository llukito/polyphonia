//! Melody in a MIDI file; exact decode (role 3).
//!
//! Data notes carry 5 bits each (3-bit pitch + 2-bit duration); a fixed
//! accompaniment track plays a chord progression that carries no data, so
//! the file sounds like a piece of music. See `crate::codec` for the note
//! codec and `docs/roles/3-music.md` for the design.

use midly::num::{u4, u7, u15, u24, u28};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use poly_core::{Carrier, CarrierKind, CarrierOptions, PolyError};

use crate::codec::{
    DURATION_UNITS, MAX_NOTES, Note, SCALE_MIDI, UNIT_MS, envelope_to_notes, notes_to_envelope,
};

/// MIDI channel 0 carries the data melody (General MIDI piano by default).
const MELODY_CHANNEL: u8 = 0;
/// MIDI channel 1 carries the background chords; the decoder ignores it.
const CHORD_CHANNEL: u8 = 1;

/// Standard MIDI resolution (480 ticks per quarter note).
const MIDI_TICKS_PER_QUARTER: u16 = 480;
/// 250 ms per quarter = 240 BPM. Chosen so one 50 ms data unit is exactly
/// 480 * 50 / 250 = 96 ticks: durations quantize without rounding. The
/// file still sounds like a fast arpeggio, not noise.
const MIDI_TEMPO_US_PER_QUARTER: u32 = 250_000;
/// Ticks per 50 ms unit: 480 ticks/quarter * 50 ms / 250 ms = 96.
const MIDI_TICKS_PER_UNIT: u32 =
    MIDI_TICKS_PER_QUARTER as u32 * UNIT_MS / (MIDI_TEMPO_US_PER_QUARTER / 1_000);

/// Melody is foreground, chords are a soft bed.
const MELODY_VELOCITY: u8 = 96;
const CHORD_VELOCITY: u8 = 40;
/// One chord per second (4 quarter notes). Slow enough to stay musical
/// under a stream of 50–200 ms melody notes, long enough to keep the file small.
const CHORD_TICKS: u32 = 4 * MIDI_TICKS_PER_QUARTER as u32;

/// Track names are informational only; decoding never depends on them.
const MELODY_TRACK_NAME: &[u8] = b"polyphonia melody v1";
const CHORD_TRACK_NAME: &[u8] = b"polyphonia chords";

/// Background progressions (MIDI note numbers), all in E minor so they fit
/// the melody scale. They carry no data; `seed` / `music_style` only pick
/// which one plays, so `decode` never depends on them.
static PROGRESSIONS: [&[&[u8]]; 2] = [
    // i–VI–III–VII: Em, C, G, D
    &[&[47, 52, 59], &[48, 55, 60], &[43, 50, 59], &[50, 57, 62]],
    // i–iv–VI–III: Em, Am, C, G
    &[&[47, 52, 59], &[45, 52, 57], &[48, 55, 60], &[43, 50, 59]],
];

/// Progression used when no style is asked for (and by the conformance test).
const DEFAULT_PROGRESSION: usize = 0;

/// Pick a progression from `music_style` (or `seed`): "minor" (the default)
/// or "aeolian"; anything else keeps the default. Only the accompaniment
/// varies, never the data, so this is safe to change at any time.
fn progression(opts: &CarrierOptions) -> &'static [&'static [u8]] {
    let by_style = match opts.music_style.as_deref() {
        None | Some("minor") => Some(0),
        Some("aeolian") => Some(1),
        Some(_) => None,
    };
    let index = opts
        .seed
        .map(|s| s as usize)
        .or(by_style)
        .unwrap_or(DEFAULT_PROGRESSION);
    PROGRESSIONS[index % PROGRESSIONS.len()]
}

/// Melody in a MIDI file; exact decode.
#[derive(Debug, Default, Clone, Copy)]
pub struct MusicMidiCarrier;

/// One note-on or note-off on the absolute tick timeline.
struct NoteEvent {
    tick: u32,
    key: u8,
    velocity: u8,
    off: bool,
}

/// Turn absolute-tick events into a midly track, with a name and an end
/// marker. At equal ticks, note-offs come first: a repeated pitch is then
/// off-then-on and never two overlapping notes of the same key.
fn track_from_events(
    mut events: Vec<NoteEvent>,
    channel: u8,
    name: &'static [u8],
) -> Vec<TrackEvent<'static>> {
    events.sort_by_key(|e| (e.tick, u8::from(!e.off)));
    let mut track = Vec::with_capacity(events.len() + 3);
    track.push(TrackEvent {
        delta: u28::from(0),
        kind: TrackEventKind::Meta(MetaMessage::TrackName(name)),
    });
    let mut previous_tick = 0u32;
    for event in events {
        let delta = event.tick - previous_tick;
        previous_tick = event.tick;
        let message = if event.off {
            MidiMessage::NoteOff {
                key: u7::from(event.key),
                vel: u7::from(0),
            }
        } else {
            MidiMessage::NoteOn {
                key: u7::from(event.key),
                vel: u7::from(event.velocity),
            }
        };
        track.push(TrackEvent {
            delta: u28::from(delta),
            kind: TrackEventKind::Midi {
                channel: u4::from(channel),
                message,
            },
        });
    }
    track.push(TrackEvent {
        delta: u28::from(0),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });
    track
}

impl Carrier for MusicMidiCarrier {
    fn kind(&self) -> CarrierKind {
        CarrierKind::MusicMidi
    }

    fn file_extension(&self) -> &'static str {
        "mid"
    }

    fn mime_type(&self) -> &'static str {
        "audio/midi"
    }

    fn capacity(&self, _opts: &CarrierOptions) -> Option<usize> {
        Some(crate::codec::MAX_ENVELOPE_BYTES)
    }

    fn encode(&self, envelope: &[u8], opts: &CarrierOptions) -> Result<Vec<u8>, PolyError> {
        let notes = envelope_to_notes(envelope)?;

        // Melody: marker + data, one note after another on exact ticks.
        let mut melody_events = Vec::with_capacity(notes.len() * 2);
        let mut tick = 0u32;
        for &(pitch, duration) in &notes {
            let key = SCALE_MIDI[pitch as usize];
            melody_events.push(NoteEvent {
                tick,
                key,
                velocity: MELODY_VELOCITY,
                off: false,
            });
            tick += MIDI_TICKS_PER_UNIT * DURATION_UNITS[duration as usize] as u32;
            melody_events.push(NoteEvent {
                tick,
                key,
                velocity: 0,
                off: true,
            });
        }
        let total_ticks = tick;

        // Chords: loop the progression for the whole song.
        let chords = progression(opts);
        let mut chord_events = Vec::new();
        let mut chord_tick = 0u32;
        let mut chord_index = 0usize;
        while chord_tick < total_ticks {
            let end = (chord_tick + CHORD_TICKS).min(total_ticks);
            for &key in chords[chord_index % chords.len()] {
                chord_events.push(NoteEvent {
                    tick: chord_tick,
                    key,
                    velocity: CHORD_VELOCITY,
                    off: false,
                });
                chord_events.push(NoteEvent {
                    tick: end,
                    key,
                    velocity: 0,
                    off: true,
                });
            }
            chord_tick += CHORD_TICKS;
            chord_index += 1;
        }

        let mut smf = Smf::new(Header::new(
            Format::Parallel,
            Timing::Metrical(u15::from(MIDI_TICKS_PER_QUARTER)),
        ));
        let mut melody = track_from_events(melody_events, MELODY_CHANNEL, MELODY_TRACK_NAME);
        // Tempo applies to every track in a format-1 file; put it first.
        melody.insert(
            1,
            TrackEvent {
                delta: u28::from(0),
                kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::from(
                    MIDI_TEMPO_US_PER_QUARTER,
                ))),
            },
        );
        smf.tracks.push(melody);
        smf.tracks.push(track_from_events(
            chord_events,
            CHORD_CHANNEL,
            CHORD_TRACK_NAME,
        ));

        let mut out = Vec::new();
        smf.write_std(&mut out)
            .map_err(|e| PolyError::Carrier(format!("could not write MIDI: {e}")))?;
        Ok(out)
    }

    fn decode(&self, file: &[u8]) -> Result<Vec<u8>, PolyError> {
        if !file.starts_with(b"MThd") {
            return Err(PolyError::NotThisCarrier);
        }
        let smf = Smf::parse(file).map_err(|_| PolyError::NotThisCarrier)?;

        // Collect note starts per key (a stack handles repeated pitches).
        let mut starts: [Vec<u32>; 128] = std::array::from_fn(|_| Vec::new());
        let mut notes: Vec<Note> = Vec::new();
        for track in &smf.tracks {
            let mut now = 0u32;
            for event in track {
                now = now.saturating_add(event.delta.as_int());
                let TrackEventKind::Midi { channel, message } = event.kind else {
                    continue;
                };
                if channel.as_int() != MELODY_CHANNEL {
                    continue;
                }
                match message {
                    MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                        starts[key.as_int() as usize].push(now);
                    }
                    MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                        let Some(start) = starts[key.as_int() as usize].pop() else {
                            return Err(PolyError::NotThisCarrier);
                        };
                        let Some(pitch) = SCALE_MIDI.iter().position(|&m| m == key.as_int()) else {
                            return Err(PolyError::NotThisCarrier);
                        };
                        let units = ((now - start) as f32 / MIDI_TICKS_PER_UNIT as f32)
                            .round()
                            .clamp(1.0, 4.0) as u8;
                        if notes.len() < MAX_NOTES {
                            notes.push((pitch as u8, units - 1));
                        }
                    }
                    _ => {}
                }
            }
        }
        notes_to_envelope(&notes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::MARKER_NOTES;

    #[test]
    fn roundtrip() {
        poly_core::test_utils::assert_carrier_roundtrip(&MusicMidiCarrier);
    }

    /// A minimal "POLY" envelope to inspect the produced file structure.
    fn sample_envelope() -> Vec<u8> {
        let mut bytes = b"POLY".to_vec();
        bytes.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0, 0]);
        bytes.extend_from_slice(&[0xAB; 32]);
        bytes.extend_from_slice(&[0; 4]);
        bytes
    }

    #[test]
    fn file_is_a_midi_song_with_two_tracks() {
        let file = MusicMidiCarrier
            .encode(&sample_envelope(), &CarrierOptions::default())
            .unwrap();
        let smf = Smf::parse(&file).expect("player-readable MIDI");
        assert_eq!(smf.header.format, Format::Parallel);
        assert_eq!(smf.tracks.len(), 2);

        // Track 0 starts with a name, a tempo and the four marker notes on E4.
        let kind_of = |i: usize| smf.tracks[0][i].kind;
        assert!(matches!(
            kind_of(0),
            TrackEventKind::Meta(MetaMessage::TrackName(_))
        ));
        assert!(matches!(
            kind_of(1),
            TrackEventKind::Meta(MetaMessage::Tempo(_))
        ));
        // Note-ons and note-offs alternate; the four note-ons are at 2, 4, 6, 8.
        for i in (2..10).step_by(2) {
            assert!(matches!(
                kind_of(i),
                TrackEventKind::Midi {
                    message: MidiMessage::NoteOn { key, .. },
                    ..
                } if key.as_int() == SCALE_MIDI[0]
            ));
        }

        // Track 1 is the accompaniment on channel 1: no data there.
        assert!(smf.tracks[1].iter().all(|event| !matches!(
            event.kind,
            TrackEventKind::Midi { channel, .. } if channel.as_int() == MELODY_CHANNEL
        )));
    }

    #[test]
    fn chords_and_seeds_do_not_change_the_payload() {
        let envelope = sample_envelope();
        let plain = MusicMidiCarrier
            .encode(&envelope, &CarrierOptions::default())
            .unwrap();
        let styled = MusicMidiCarrier
            .encode(
                &envelope,
                &CarrierOptions {
                    music_style: Some("aeolian".into()),
                    seed: Some(1),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_ne!(
            plain, styled,
            "a different accompaniment should produce a different song"
        );
        assert_eq!(
            MusicMidiCarrier.decode(&plain).unwrap()[..envelope.len()],
            envelope[..]
        );
        assert_eq!(
            MusicMidiCarrier.decode(&styled).unwrap()[..envelope.len()],
            envelope[..]
        );
    }

    #[test]
    fn marker_notes_are_four_staccato_notes() {
        assert_eq!(MARKER_NOTES, [(0, 0); 4]);
    }
}
