//! Unplugging and replugging a keyboard, with a virtual ALSA MIDI port standing in for it.
//!
//! Linux only (virtual ports are an ALSA sequencer feature). Without a sequencer (`/dev/snd/seq`)
//! the test prints a note and passes.

#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    use midir::os::unix::VirtualOutput;
    use onezan_engine::{MidiChange, MidiWatch, TimedEvent, midi_queue};
    use onezan_vst3_host::MidiEvent;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    const PORT: &str = "onezan-test-keys";
    const POLL: Duration = Duration::from_millis(20);

    fn keyboard() -> Option<midir::MidiOutputConnection> {
        let Ok(out) = midir::MidiOutput::new("onezan-test") else {
            eprintln!("no ALSA sequencer; skipping");
            return None;
        };
        Some(out.create_virtual(PORT).expect("virtual port"))
    }

    /// Waits up to a second for `pred` to hold.
    fn wait(what: &str, mut pred: impl FnMut() -> bool) {
        let start = Instant::now();
        while !pred() {
            assert!(start.elapsed() < Duration::from_secs(1), "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn receive(rx: &mut rtrb::Consumer<TimedEvent>, got: &mut Vec<MidiEvent>, want: MidiEvent) {
        wait(&format!("{want:?}"), || {
            got.extend(std::iter::from_fn(|| rx.pop().ok().map(|e| e.event)));
            got.contains(&want)
        });
    }

    #[test]
    fn replugged_keyboard_plays_again_and_held_notes_are_released() {
        let Some(mut keys) = keyboard() else { return };
        let (tx, mut rx) = midi_queue();
        let changes = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&changes);
        let (watch, first, _) = MidiWatch::start(Some(PORT.into()), POLL, tx, Arc::default(), move |c| {
            log.lock().unwrap().push(c);
        })
        .unwrap();
        assert!(matches!(first, Some(MidiChange::Connected(_))), "{first:?}");

        let mut got = Vec::new();
        keys.send(&[0x90, 60, 100]).unwrap();
        let on = MidiEvent::NoteOn {
            channel: 0,
            key: 60,
            velocity: 100,
        };
        receive(&mut rx, &mut got, on);

        // Unplug while the key is down: the watcher releases it.
        drop(keys);
        let off = MidiEvent::NoteOff {
            channel: 0,
            key: 60,
            velocity: 64,
        };
        receive(&mut rx, &mut got, off);
        // The change is reported just after the release: wait for it.
        wait("disconnect", || {
            matches!(changes.lock().unwrap().last(), Some(MidiChange::Disconnected(_)))
        });

        // Plug back in: it is opened again and plays.
        let mut keys = keyboard().unwrap();
        wait("reconnect", || {
            matches!(changes.lock().unwrap().last(), Some(MidiChange::Connected(_)))
        });
        keys.send(&[0x90, 62, 90]).unwrap();
        receive(
            &mut rx,
            &mut got,
            MidiEvent::NoteOn {
                channel: 0,
                key: 62,
                velocity: 90,
            },
        );
        drop(watch);
    }
}
