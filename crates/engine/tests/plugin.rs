//! Checks against a real VST3 instrument, rendered offline (no audio device or keyboard).
//!
//! The plugin is `$ONEZAN_TEST_PLUGIN` (a `.vst3` path), else the first installed instrument.
//! Without one, each test prints a note and passes: these are for machines that have a plugin.
//!
//! The tests run one at a time ([`one_at_a_time`]), as the app uses plugins: some cannot have
//! instances on several threads at once (Dexed, a JUCE plugin, hung that way).

#[cfg(test)]
mod tests {
    use onezan_engine::{Instrument, render_offline};
    use onezan_vst3_host::{MidiEvent, Plugin};
    use std::path::PathBuf;
    use std::sync::{Mutex, MutexGuard, PoisonError};

    /// Held for the whole of a test, taken before its plugin is loaded (so it is dropped after).
    fn one_at_a_time() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(PoisonError::into_inner)
    }

    const RATE: u32 = 48_000;
    /// [`RATE`] as a frame count: one second.
    const SEC: usize = 48_000;
    const KEY: u8 = 60;

    fn plugin() -> Option<Plugin> {
        let bundle = std::env::var_os("ONEZAN_TEST_PLUGIN").map(PathBuf::from).or_else(|| {
            let bundles = onezan_vst3_host::find_bundles(&onezan_vst3_host::default_search_paths());
            onezan_vst3_host::scan(&bundles)
                .into_iter()
                .find(onezan_vst3_host::PluginInfo::is_instrument)
                .map(|p| p.bundle)
        });
        let Some(bundle) = bundle else {
            eprintln!("no VST3 instrument installed; skipping");
            return None;
        };
        Some(Plugin::load(&bundle, None).expect("plugin loads"))
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

    /// The first frame whose level exceeds -60 dBFS.
    fn first_sound(l: &[f32], r: &[f32]) -> Option<usize> {
        l.iter().zip(r).position(|(a, b)| a.abs().max(b.abs()) > 1e-3)
    }

    fn peak(s: &[f32]) -> f32 {
        s.iter().fold(0.0, |m, x| m.max(x.abs()))
    }

    #[test]
    fn note_starts_at_its_frame_not_its_block() {
        let _one = one_at_a_time();
        let Some(p) = plugin() else { return };
        // Frame 1000 lies inside the renderer's fourth 256-frame block (768..1024): a host that
        // applied events at the block start would sound from 768.
        let at = 1000;
        let (l, r) = render_offline(&mut Instrument::Plugin(p), RATE, SEC / 2, vec![(at, on())]).unwrap();
        let first = first_sound(&l, &r).expect("the note sounds");
        assert!(first >= at, "sound at frame {first}, before the note at {at}");
        assert!(
            first < at + 256,
            "sound only at frame {first}, long after the note at {at}"
        );
    }

    #[test]
    fn released_note_goes_silent() {
        let _one = one_at_a_time();
        let Some(p) = plugin() else { return };
        let len = SEC * 3;
        let (l, r) = render_offline(&mut Instrument::Plugin(p), RATE, len, vec![(0, on()), (SEC / 4, off())]).unwrap();
        assert!(peak(&l[..SEC / 4]) > 1e-3, "the note sounds while held");
        let tail = len - SEC / 2;
        assert!(
            peak(&l[tail..]).max(peak(&r[tail..])) < 1e-4,
            "still sounding 2.25 s after release"
        );
    }

    #[test]
    fn sustain_pedal_holds_then_releases() {
        let _one = one_at_a_time();
        let Some(p) = plugin() else { return };
        let s = SEC;
        let pedal = |value| MidiEvent::ControlChange {
            channel: 0,
            controller: 64,
            value,
        };
        // Pedal down, note on, note off at 0.25 s (pedal still down), pedal up at 1.5 s.
        let events = vec![(0, pedal(127)), (10, on()), (s / 4, off()), (s * 3 / 2, pedal(0))];
        let (l, r) = render_offline(&mut Instrument::Plugin(p), RATE, s * 4, events).unwrap();
        let held = peak(&l[s..s * 3 / 2]).max(peak(&r[s..s * 3 / 2]));
        assert!(held > 1e-3, "the pedal does not hold the note (level {held})");
        let tail = s * 7 / 2;
        assert!(
            peak(&l[tail..]).max(peak(&r[tail..])) < 1e-4,
            "still sounding 2 s after the pedal is up"
        );
    }

    #[test]
    fn state_restores_into_a_fresh_instance() {
        let _one = one_at_a_time();
        let Some(a) = plugin() else { return };
        let saved = a.state().unwrap();
        assert!(!saved.component.is_empty(), "the plugin wrote no state");
        drop(a);
        // Surge XT keeps its patch as XML: rename it (same length, so its own size fields stay
        // right) to tell a restored state from a fresh one.
        let mut changed = saved;
        let renamed = replace(&mut changed.component, b"name=\"Init Saw\"", b"name=\"Init Sax\"");
        let Some(mut b) = plugin() else { return };
        let state = onezan_vst3_host::State::from_bytes(&changed.to_bytes()).unwrap();
        b.set_state(&state).unwrap();
        let after = b.state().unwrap();
        if renamed {
            assert!(
                contains(&after.component, b"name=\"Init Sax\""),
                "the restored patch name is lost"
            );
        } else {
            eprintln!("not Surge XT's default patch: checked only that the state is taken back");
            assert_eq!(after.component, changed.component);
        }
        // And it still plays.
        let (l, _) = render_offline(&mut Instrument::Plugin(b), RATE, SEC / 2, vec![(0, on())]).unwrap();
        assert!(peak(&l) > 1e-3, "silent after restoring");
    }

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    /// Replaces the first `from` in `bytes` with `to` (same length); whether it was there.
    fn replace(bytes: &mut [u8], from: &[u8], to: &[u8]) -> bool {
        let Some(i) = bytes.windows(from.len()).position(|w| w == from) else {
            return false;
        };
        bytes[i..][..to.len()].copy_from_slice(to);
        true
    }
}
