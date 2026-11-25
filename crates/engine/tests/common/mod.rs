//! The contract of an [`Instrument`], the same for every kind (a VST3 plugin, the built-in
//! piano): what the audio callback and the app rely on, checked by rendering offline.
//!
//! `instrument_contract!(make)` writes one test per promise for the instruments `make()` gives;
//! `make` returns `None` when the machine has none (the tests then pass with a note). Loose on
//! purpose: "within 5 ms", "below -60 dBFS", not exact samples.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test helpers: a failure is a test failure"
)]

use onezan_engine::{Instrument, render_offline};
use onezan_vst3_host::MidiEvent;

pub const RATE: u32 = 48_000;
/// One second of frames at [`RATE`].
pub const SEC: usize = 48_000;
/// -60 dBFS: sounding above it, silent below.
pub const AUDIBLE: f32 = 1e-3;
const KEY: u8 = 60;

pub const fn on(key: u8) -> MidiEvent {
    MidiEvent::NoteOn {
        channel: 0,
        key,
        velocity: 100,
    }
}

pub const fn off(key: u8) -> MidiEvent {
    MidiEvent::NoteOff {
        channel: 0,
        key,
        velocity: 64,
    }
}

const fn pedal(value: u8) -> MidiEvent {
    MidiEvent::ControlChange {
        channel: 0,
        controller: 64,
        value,
    }
}

pub fn peak(l: &[f32], r: &[f32]) -> f32 {
    l.iter().chain(r).fold(0.0, |m, x| m.max(x.abs()))
}

/// The first frame above [`AUDIBLE`].
pub fn first_sound(l: &[f32], r: &[f32]) -> Option<usize> {
    l.iter().zip(r).position(|(a, b)| a.abs().max(b.abs()) > AUDIBLE)
}

/// Silent until the note, then sounding within 5 ms of its frame, wherever the frame falls in
/// a block (the renderer's blocks are 256 frames: one applied at its block's start would sound
/// from 768).
pub fn a_note_starts_at_its_own_frame(mut i: Instrument) {
    let at = 1000;
    let (l, r) = render_offline(&mut i, RATE, SEC / 2, vec![(at, on(KEY))]).unwrap();
    let first = first_sound(&l, &r).expect("the note sounds");
    assert!(first >= at, "sound at frame {first}, before the note at {at}");
    assert!(first < at + SEC / 200, "sound only at frame {first}, the note at {at}");
}

/// Sounding while held; silent within 2 s of its release.
pub fn a_released_note_dies_away(mut i: Instrument) {
    let len = SEC * 3;
    let (l, r) = render_offline(&mut i, RATE, len, vec![(0, on(KEY)), (SEC / 4, off(KEY))]).unwrap();
    assert!(peak(&l[..SEC / 4], &r[..SEC / 4]) > AUDIBLE, "silent while held");
    let tail = SEC / 4 + SEC * 2;
    assert!(
        peak(&l[tail..], &r[tail..]) < AUDIBLE,
        "still sounding 2 s after release"
    );
}

/// With the pedal down a released note keeps sounding; pedal up, it dies away.
pub fn the_sustain_pedal_holds_then_releases(mut i: Instrument) {
    let s = SEC;
    let events = vec![(0, pedal(127)), (10, on(KEY)), (s / 4, off(KEY)), (s * 3 / 2, pedal(0))];
    let (l, r) = render_offline(&mut i, RATE, s * 4, events).unwrap();
    let held = peak(&l[s..s * 3 / 2], &r[s..s * 3 / 2]);
    assert!(held > AUDIBLE, "the pedal does not hold the note (level {held})");
    let tail = s * 7 / 2;
    assert!(
        peak(&l[tail..], &r[tail..]) < AUDIBLE,
        "still sounding 2 s after the pedal is up"
    );
}

/// Ten notes at once all sound, and all stop.
pub fn a_big_chord_sounds_and_stops(mut i: Instrument) {
    let keys = [48, 52, 55, 60, 64, 67, 72, 76, 79, 84];
    let mut events: Vec<(usize, MidiEvent)> = keys.iter().map(|&k| (0, on(k))).collect();
    events.extend(keys.iter().map(|&k| (SEC / 2, off(k))));
    let (l, r) = render_offline(&mut i, RATE, SEC * 3, events).unwrap();
    assert!(peak(&l[..SEC / 2], &r[..SEC / 2]) > AUDIBLE, "the chord is silent");
    let tail = SEC / 2 + SEC * 2;
    assert!(
        peak(&l[tail..], &r[tail..]) < AUDIBLE,
        "still sounding 2 s after release"
    );
}

/// An event at or past the end of a block is not lost: it sounds by the next block.
pub fn an_event_at_the_end_of_a_block_is_not_lost(mut i: Instrument) {
    const BLOCK: usize = 256;
    let mut voice = i.start(RATE, BLOCK).unwrap();
    let (mut l, mut r) = (vec![0.0; BLOCK], vec![0.0; BLOCK]);
    voice.process(&[(BLOCK, on(KEY)), (BLOCK + 100, on(KEY + 4))], &mut l, &mut r);
    let mut heard = false;
    for _ in 0..4 {
        voice.process(&[], &mut l, &mut r);
        heard |= peak(&l, &r) > AUDIBLE;
    }
    drop(voice);
    i.stop();
    assert!(heard, "notes given past the end of a block were never heard");
}

/// One test per promise above, for the instruments `$make()` gives (`None`: skipped). With a
/// second argument, an expression whose value is held for the whole test (a lock).
#[macro_export]
macro_rules! instrument_contract {
    ($make:expr) => {
        $crate::instrument_contract!($make, ());
    };
    ($make:expr, $guard:expr) => {
        #[test]
        fn a_note_starts_at_its_own_frame() {
            let _held = $guard;
            if let Some(i) = $make() {
                common::a_note_starts_at_its_own_frame(i);
            }
        }

        #[test]
        fn a_released_note_dies_away() {
            let _held = $guard;
            if let Some(i) = $make() {
                common::a_released_note_dies_away(i);
            }
        }

        #[test]
        fn the_sustain_pedal_holds_then_releases() {
            let _held = $guard;
            if let Some(i) = $make() {
                common::the_sustain_pedal_holds_then_releases(i);
            }
        }

        #[test]
        fn a_big_chord_sounds_and_stops() {
            let _held = $guard;
            if let Some(i) = $make() {
                common::a_big_chord_sounds_and_stops(i);
            }
        }

        #[test]
        fn an_event_at_the_end_of_a_block_is_not_lost() {
            let _held = $guard;
            if let Some(i) = $make() {
                common::an_event_at_the_end_of_a_block_is_not_lost(i);
            }
        }
    };
}
