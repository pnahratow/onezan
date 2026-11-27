//! A running instrument: the plugin or built-in piano, the MIDI keyboard and the audio output,
//! as `play` and the window both use them.

use crate::plugins::{Chosen, Installed, choose};
use anyhow::Result;
use onezan_engine::midi_in::POLL;
use onezan_engine::{
    AudioOut, Considered, Consumer, Instrument, Keys, Metronome, MidiChange, MidiTiming, MidiWatch, Output, Producer,
    TimedEvent, choose_midi_port, midi_queue,
};
use onezan_vst3_host::MidiEvent;
use std::sync::Arc;
use std::time::Duration;

/// What to start; `None` means "pick the best by itself".
#[derive(Clone)]
pub struct Options {
    pub plugin: Option<String>,
    pub midi: Option<String>,
    /// `None`: the one used last time, else auto.
    pub timing: Option<MidiTiming>,
    /// `None`: the one chosen last time, else the best the system offers.
    pub output: Option<Output>,
    /// Start the metronome at this tempo.
    pub metronome: Option<u32>,
    /// What was used last time, tried when nothing is given above. Best effort: what no longer
    /// works gives way to the automatic choice.
    pub remembered: Remembered,
}

/// The choices used last time (from the settings).
#[derive(Clone, Default)]
pub struct Remembered {
    /// The plugin's class id.
    pub plugin: Option<String>,
    /// The MIDI port's name, without its ALSA address (see [`port_name`]).
    pub midi: Option<String>,
    /// The audio output chosen by hand.
    pub output: Option<Output>,
    pub timing: Option<MidiTiming>,
}

/// A MIDI port's name without the ALSA address at its end ("Keystation 49 MIDI 1 24:0" →
/// "Keystation 49 MIDI 1"): the address can change between starts, the name does not.
pub fn port_name(port: &str) -> &str {
    let address = |a: &str| {
        a.split_once(':').is_some_and(|(c, p)| {
            !c.is_empty() && !p.is_empty() && c.bytes().chain(p.bytes()).all(|b| b.is_ascii_digit())
        })
    };
    match port.rsplit_once(' ') {
        Some((name, a)) if address(a) => name,
        _ => port,
    }
}

/// Loads the instrument asked for; else the one used last time; else (or when that fails) the
/// automatic choice (see [`load`]). Then a plugin's sound from last time.
fn load_instrument(opts: &Options) -> Result<(Chosen<Instrument>, String)> {
    let mut loaded = choose(
        &mut Installed::default(),
        opts.plugin.as_deref(),
        opts.remembered.plugin.as_deref(),
    )?;
    let note = std::mem::take(&mut loaded.note);
    let sound = loaded
        .instrument
        .plugin_mut()
        .map_or_default(crate::instrument_state::restore);
    let detail = [note, sound]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    Ok((loaded, detail))
}

/// The MIDI port to ask for: the one given; else the one used last time if it is plugged in now
/// (else any keyboard, so a missing one is not waited for).
fn midi_query(opts: &Options) -> Option<String> {
    opts.midi.clone().or_else(|| {
        let last = opts.remembered.midi.as_deref()?;
        let ports = onezan_engine::midi_input_ports().unwrap_or_default();
        ports.iter().any(|p| port_name(p) == last).then(|| last.to_owned())
    })
}

/// One section of the startup report: what was considered, a detail for the one in use, and
/// how to choose differently.
pub struct Section {
    pub label: &'static str,
    pub considered: Vec<Considered>,
    pub detail: String,
    /// What the window's top bar shows.
    pub short: String,
    pub hint: &'static str,
}

pub struct Session {
    // Dropped in this order: MIDI input, then audio (and the instrument's voice), then the
    // instrument.
    watch: Option<MidiWatch>,
    audio: Option<AudioOut>,
    /// The output chosen by hand; `None`: the automatic choice.
    chosen_output: Option<Output>,
    /// The MIDI port asked for by hand; `None`: the automatic choice.
    chosen_midi: Option<String>,
    /// Notes played on the screen (see [`Session::play`]).
    screen: Producer<TimedEvent>,
    metronome: Arc<Metronome>,
    instrument: Instrument,
    /// The built-in piano stands in because no plugin could be loaded.
    fallback: bool,
    /// The startup report: plugin, MIDI in, audio out, timing.
    pub report: Vec<Section>,
    /// The result of the first attempt to open a MIDI port (`None`: no keyboard yet).
    pub first_midi: Option<MidiChange>,
}

impl Session {
    /// Loads the plugin, opens the keyboard and starts audio. `extra` are further event sources
    /// (the demo). `keys` gets what the keyboard holds; `on_midi` every later port change.
    pub fn start(
        opts: &Options,
        extra: Vec<Consumer<TimedEvent>>,
        keys: Arc<Keys>,
        on_midi: impl FnMut(MidiChange) + Send + 'static,
    ) -> Result<Self> {
        let (loaded, detail) = load_instrument(opts)?;
        let (mut instrument, fallback) = (loaded.instrument, loaded.fallback);
        let mut report = vec![Section {
            label: "plugin",
            considered: loaded.considered,
            detail,
            short: instrument.name().to_string(),
            hint: "choose another with `onezan play NAME` (see `onezan list`)",
        }];
        let (midi_tx, midi_rx) = midi_queue();
        let (watch, first_midi, ports) = MidiWatch::start(midi_query(opts), POLL, midi_tx, keys, on_midi)?;
        report.push(Section {
            label: "MIDI in",
            considered: ports,
            detail: String::new(),
            short: String::new(),
            hint: "choose another with --midi NAME, or in the window (click MIDI)",
        });
        let (mut screen, screen_rx) = midi_queue();
        let mut inputs = vec![midi_rx, screen_rx];
        inputs.extend(extra);
        let metronome = Arc::new(Metronome::default());
        if let Some(bpm) = opts.metronome {
            metronome.set_bpm(bpm);
            metronome.set_on(true);
        }
        let timing = opts.timing.or(opts.remembered.timing).unwrap_or_default();
        let metro = Arc::clone(&metronome);
        let (audio, chosen_output) = match (&opts.output, &opts.remembered.output) {
            (Some(o), _) => (
                AudioOut::start(&mut instrument, inputs, timing, metro, Some(o))?,
                Some(o.clone()),
            ),
            (None, Some(last)) => {
                // The queues went into the first attempt: a new one for the keyboard (only the
                // window remembers an output, and it has no other sources).
                match AudioOut::start(&mut instrument, inputs, timing, metro, Some(last)) {
                    Ok(a) => (a, Some(last.clone())),
                    Err(e) => {
                        log::warn!("the audio output chosen last time: {e:#}");
                        let (tx, rx) = midi_queue();
                        watch.set_queue(tx);
                        let (s, screen_rx) = midi_queue();
                        screen = s;
                        let inputs = vec![rx, screen_rx];
                        let a = AudioOut::start(&mut instrument, inputs, timing, Arc::clone(&metronome), None)?;
                        (a, None)
                    }
                }
            }
            (None, None) => (AudioOut::start(&mut instrument, inputs, timing, metro, None)?, None),
        };
        report.extend(audio_sections(&audio));
        Ok(Self {
            watch: Some(watch),
            audio: Some(audio),
            chosen_output,
            chosen_midi: opts.midi.clone(),
            screen,
            metronome,
            instrument,
            fallback,
            report,
            first_midi,
        })
    }

    /// The metronome's settings (any thread).
    pub const fn metronome(&self) -> &Arc<Metronome> {
        &self.metronome
    }

    pub const fn instrument(&self) -> &Instrument {
        &self.instrument
    }

    /// Whether the built-in piano is only standing in, because no plugin could be loaded.
    pub const fn is_fallback(&self) -> bool {
        self.fallback
    }

    /// The report section called `label`.
    pub fn section(&self, label: &str) -> Option<&Section> {
        self.report.iter().find(|s| s.label == label)
    }

    /// The audio output in use.
    pub fn output(&self) -> Option<Output> {
        self.audio.as_ref().map(AudioOut::output)
    }

    /// The output chosen by hand; `None`: the automatic choice.
    pub const fn chosen_output(&self) -> Option<&Output> {
        self.chosen_output.as_ref()
    }

    /// The MIDI port asked for by hand; `None`: the automatic choice.
    pub fn chosen_midi(&self) -> Option<&str> {
        self.chosen_midi.as_deref()
    }

    /// Plays `event` as if from the keyboard (the on-screen keyboard), from the next audio block.
    /// Not shown on the keys nor recorded: that is for the MIDI keyboard.
    pub fn play(&mut self, event: MidiEvent) {
        // A full queue drops it; the audio thread is not keeping up anyway.
        let _ = self.screen.push(TimedEvent::now(event));
    }

    /// The MIDI timing setting.
    pub fn timing(&self) -> Option<MidiTiming> {
        self.audio.as_ref().map(AudioOut::timing)
    }

    /// Switches the keyboard to the port `query` picks (`None`: the automatic choice). The
    /// result arrives through the `on_midi` given to [`Session::start`].
    pub fn choose_midi(&mut self, query: Option<String>) {
        if let Some(w) = &self.watch {
            w.choose(query.clone());
        }
        let ports = onezan_engine::midi_input_ports().unwrap_or_default();
        let (_, considered) = choose_midi_port(&ports, query.as_deref());
        if let Some(s) = self.report.iter_mut().find(|s| s.label == "MIDI in") {
            s.considered = considered;
        }
        self.chosen_midi = query;
    }

    /// Changes the MIDI timing; the next audio block uses it.
    pub fn set_timing(&mut self, t: MidiTiming) {
        if let Some(a) = &self.audio {
            a.set_timing(t);
        }
        self.refresh_audio_report();
    }

    /// Plays through `output` (`None`: the automatic choice). The audio stream starts again; the
    /// plugin, its editor and the keyboard stay. Other event sources given to [`Session::start`]
    /// (the demo) are not reconnected. When `output` does not work, the automatic choice is
    /// started instead and the error returned.
    ///
    /// # Errors
    ///
    /// When `output` cannot be used (the automatic choice is playing then), or nothing can.
    pub fn set_output(&mut self, output: Option<&Output>) -> Result<()> {
        let timing = self.timing().unwrap_or_default();
        if let Some(a) = self.audio.take() {
            a.stop();
        }
        let start = |instrument: &mut Instrument, screen: &mut Producer<TimedEvent>, output: Option<&Output>| {
            let (tx, rx) = midi_queue();
            if let Some(w) = &self.watch {
                w.set_queue(tx);
            }
            let (s, screen_rx) = midi_queue();
            *screen = s;
            AudioOut::start(
                instrument,
                vec![rx, screen_rx],
                timing,
                Arc::clone(&self.metronome),
                output,
            )
        };
        let result = match start(&mut self.instrument, &mut self.screen, output) {
            Ok(a) => {
                self.audio = Some(a);
                self.chosen_output = output.cloned();
                Ok(())
            }
            Err(e) if output.is_some() => {
                self.audio = Some(start(&mut self.instrument, &mut self.screen, None)?);
                self.chosen_output = None;
                Err(e)
            }
            Err(e) => Err(e),
        };
        self.refresh_audio_report();
        result
    }

    /// The report's audio and timing sections, as they are now.
    fn refresh_audio_report(&mut self) {
        self.report.retain(|s| s.label != "audio out" && s.label != "timing");
        if let Some(a) = &self.audio {
            self.report.extend(audio_sections(a));
        }
    }

    /// Saves a plugin's sound for the next start (the built-in piano has none).
    pub fn save_sound(&self) {
        if let Some(p) = self.instrument.plugin() {
            crate::instrument_state::save(p);
        }
    }

    pub fn stop(mut self) {
        drop(self.watch.take());
        if let Some(a) = self.audio.take() {
            a.stop();
        }
        self.instrument.stop();
    }
}

/// The report's sections for the audio output and the MIDI timing (once the block size is
/// known: waits for it up to a second).
fn audio_sections(audio: &AudioOut) -> Vec<Section> {
    let mut sections = vec![Section {
        label: "audio out",
        considered: audio.considered.clone(),
        detail: audio_detail(audio),
        short: audio_short(audio),
        hint: "choose another in the window (Options, Audio output)",
    }];
    if let Some(t) = audio.midi_timing() {
        sections.push(Section {
            label: "timing",
            short: audio.timing_mode().map_or_else(String::new, |m| m.name().to_string()),
            considered: t,
            detail: String::new(),
            hint: "choose with --midi-timing exact|immediate|auto, or in the window (Options, MIDI timing)",
        });
    }
    sections
}

/// "JACK, 5.3 ms": the backend and the block length in use.
fn audio_short(audio: &AudioOut) -> String {
    let block = audio.block_frames(Duration::from_secs(1)).or(audio.buffer_frames);
    block.map_or_else(
        || audio.backend.clone(),
        |n| {
            let ms = f64::from(n) * 1000.0 / f64::from(audio.sample_rate);
            format!("{}, {ms:.1} ms", audio.backend)
        },
    )
}

/// The audio line's settings: rate and the block size really in use, with its latency, and a
/// note when the system did not grant the block size asked for.
fn audio_detail(audio: &AudioOut) -> String {
    let ms = |n: u32| f64::from(n) * 1000.0 / f64::from(audio.sample_rate);
    let block = audio.block_frames(Duration::from_secs(1)).or(audio.buffer_frames);
    let block_text = block.map_or_else(
        || "block size unknown".to_string(),
        |n| format!("block {n} frames ({:.1} ms)", ms(n)),
    );
    let not_granted = match (block, audio.requested_frames) {
        (Some(n), Some(asked)) if n > asked => {
            format!(
                "; asked for {asked} ({:.1} ms), the system's minimum is higher",
                ms(asked)
            )
        }
        _ => String::new(),
    };
    format!("{} Hz, {block_text}{not_granted}", audio.sample_rate)
}

#[cfg(test)]
mod tests {
    use super::port_name;

    #[test]
    fn port_names_lose_their_alsa_address() {
        assert_eq!(
            port_name("SWISSONIC EasyKeys49:SWISSONIC EasyKeys49 MIDI 1 24:0"),
            "SWISSONIC EasyKeys49:SWISSONIC EasyKeys49 MIDI 1"
        );
        assert_eq!(
            port_name("Midi Through:Midi Through Port-0 14:0"),
            "Midi Through:Midi Through Port-0"
        );
        assert_eq!(port_name("Keystation"), "Keystation");
        assert_eq!(port_name("Some Port 2"), "Some Port 2");
        assert_eq!(port_name("A 1:"), "A 1:");
    }
}
