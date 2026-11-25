//! Checks of the built-in piano, rendered offline: the same as `plugin.rs` checks for a VST3
//! instrument. Needs the SoundFont `build/fetch-piano.sh` downloads; without it, each test prints
//! a note and passes.

#[cfg(test)]
mod tests {
    use onezan_engine::{Instrument, Piano, render_offline};
    use onezan_vst3_host::MidiEvent;
    use std::path::Path;

    const RATE: u32 = 48_000;
    const SEC: usize = 48_000;
    const KEY: u8 = 60;

    fn piano() -> Option<Instrument> {
        let sf2 = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/piano/UprightPianoKW-20220221.sf2");
        let Ok(bytes) = std::fs::read(&sf2) else {
            eprintln!("{} missing (build/fetch-piano.sh); skipping", sf2.display());
            return None;
        };
        Some(Instrument::Piano(Piano::from_sf2("piano", &bytes).unwrap()))
    }

    const fn on() -> MidiEvent {
        MidiEvent::NoteOn {
            channel: 0,
            key: KEY,
            velocity: 100,
        }
    }

    const fn off() -> MidiEvent {
        MidiEvent::NoteOff {
            channel: 0,
            key: KEY,
            velocity: 64,
        }
    }

    fn first_sound(l: &[f32], r: &[f32]) -> Option<usize> {
        l.iter().zip(r).position(|(a, b)| a.abs().max(b.abs()) > 1e-3)
    }

    fn peak(l: &[f32], r: &[f32]) -> f32 {
        l.iter().chain(r).fold(0.0, |m, x| m.max(x.abs()))
    }

    #[test]
    fn note_starts_at_its_frame() {
        let Some(mut p) = piano() else { return };
        // Not on a block boundary (offline blocks are 256 frames, the synth's own 64).
        let at = 1000;
        let (l, r) = render_offline(&mut p, RATE, SEC / 2, vec![(at, on())]).unwrap();
        let first = first_sound(&l, &r).expect("the note sounds");
        assert!(
            (at..at + 48).contains(&first),
            "sound starts at frame {first}, the note at {at}"
        );
        assert!(peak(&l, &r) > 0.05, "too quiet: {}", peak(&l, &r));
    }

    #[test]
    fn released_note_dies_away() {
        let Some(mut p) = piano() else { return };
        let (l, r) = render_offline(&mut p, RATE, SEC * 3, vec![(0, on()), (SEC / 4, off())]).unwrap();
        let tail = SEC * 2;
        assert!(
            peak(&l[tail..], &r[tail..]) < 1e-3,
            "still sounding 1.75 s after release"
        );
    }

    #[test]
    fn sustain_pedal_holds_then_releases() {
        let Some(mut p) = piano() else { return };
        let pedal = |value| MidiEvent::ControlChange {
            channel: 0,
            controller: 64,
            value,
        };
        let s = SEC;
        let events = vec![(0, pedal(127)), (10, on()), (s / 4, off()), (s * 3 / 2, pedal(0))];
        let (l, r) = render_offline(&mut p, RATE, s * 4, events).unwrap();
        let held = peak(&l[s..s * 3 / 2], &r[s..s * 3 / 2]);
        assert!(held > 1e-3, "the pedal does not hold the note (level {held})");
        let tail = s * 7 / 2;
        assert!(
            peak(&l[tail..], &r[tail..]) < 1e-3,
            "still sounding 2 s after the pedal is up"
        );
    }
}
