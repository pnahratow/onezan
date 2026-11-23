//! A running instrument: the plugin, the MIDI keyboard and the audio output, as `play` and the
//! window both use them.

use crate::plugins::load;
use anyhow::Result;
use onezan_engine::midi_in::POLL;
use onezan_engine::{
    AudioOut, Considered, Consumer, Keys, Metronome, MidiChange, MidiTiming, MidiWatch, TimedEvent, Verdict, midi_queue,
};
use onezan_vst3_host::Plugin;
use std::sync::Arc;
use std::time::Duration;

/// What to start; `None` means "pick the best by itself".
pub struct Options {
    pub plugin: Option<String>,
    pub midi: Option<String>,
    pub timing: MidiTiming,
    /// Start the metronome at this tempo.
    pub metronome: Option<u32>,
    /// What was used last time, tried when nothing is given above. Best effort: what no longer
    /// works gives way to the automatic choice.
    pub remembered: Remembered,
}

/// The choices used last time (from the settings).
#[derive(Default)]
pub struct Remembered {
    /// The plugin's class id.
    pub plugin: Option<String>,
    /// The MIDI port's name, without its ALSA address (see [`port_name`]).
    pub midi: Option<String>,
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

/// Loads the plugin asked for; else the one used last time; else (or when that fails) the
/// automatic choice. Then its sound from last time.
fn load_plugin(opts: &Options) -> Result<(Plugin, Vec<Considered>, String)> {
    let (mut plugin, considered, note) = match (&opts.plugin, &opts.remembered.plugin) {
        (Some(asked), _) => {
            let (p, c) = load(Some(asked))?;
            (p, c, String::new())
        }
        (None, Some(last)) => match load(Some(last)) {
            Ok((p, c)) => {
                let c = c
                    .into_iter()
                    .map(|mut c| {
                        c.why = match c.verdict {
                            Verdict::Used => "the instrument used last time".into(),
                            _ => "not the one used last time".into(),
                        };
                        c
                    })
                    .collect();
                (p, c, String::new())
            }
            Err(e) => {
                log::warn!("the instrument used last time: {e:#}");
                let (p, c) = load(None)?;
                (p, c, "the one used last time failed to load".to_string())
            }
        },
        (None, None) => {
            let (p, c) = load(None)?;
            (p, c, String::new())
        }
    };
    let sound = crate::instrument_state::restore(&mut plugin);
    let detail = [note, sound]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    Ok((plugin, considered, detail))
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
    // Dropped in this order: MIDI input, then audio (and the plugin's processor), then the plugin.
    watch: Option<MidiWatch>,
    audio: Option<AudioOut>,
    metronome: Arc<Metronome>,
    plugin: Plugin,
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
        let (mut plugin, plugins, detail) = load_plugin(opts)?;
        let mut report = vec![Section {
            label: "plugin",
            considered: plugins,
            detail,
            short: plugin.name().to_string(),
            hint: "choose another with `onezan play NAME` (see `onezan list`)",
        }];
        let (midi_tx, midi_rx) = midi_queue();
        let (watch, first_midi, ports) = MidiWatch::start(midi_query(opts), POLL, midi_tx, keys, on_midi)?;
        report.push(Section {
            label: "MIDI in",
            considered: ports,
            detail: String::new(),
            short: String::new(),
            hint: "choose another with --midi NAME",
        });
        let mut inputs = vec![midi_rx];
        inputs.extend(extra);
        let metronome = Arc::new(Metronome::default());
        if let Some(bpm) = opts.metronome {
            metronome.set_bpm(bpm);
            metronome.set_on(true);
        }
        let audio = AudioOut::start(&mut plugin, inputs, opts.timing, Arc::clone(&metronome))?;
        report.push(Section {
            label: "audio out",
            considered: audio.considered.clone(),
            detail: audio_detail(&audio),
            short: audio_short(&audio),
            hint: "",
        });
        if let Some(t) = audio.midi_timing() {
            report.push(Section {
                label: "timing",
                short: audio.timing_mode().map_or_else(String::new, |m| m.name().to_string()),
                considered: t,
                detail: String::new(),
                hint: "choose with --midi-timing exact|immediate|auto",
            });
        }
        Ok(Self {
            watch: Some(watch),
            audio: Some(audio),
            metronome,
            plugin,
            report,
            first_midi,
        })
    }

    /// The metronome's settings (any thread).
    pub const fn metronome(&self) -> &Arc<Metronome> {
        &self.metronome
    }

    pub const fn plugin(&self) -> &Plugin {
        &self.plugin
    }

    /// The report section called `label`.
    pub fn section(&self, label: &str) -> Option<&Section> {
        self.report.iter().find(|s| s.label == label)
    }

    /// Saves the instrument's sound for the next start.
    pub fn save_sound(&self) {
        crate::instrument_state::save(&self.plugin);
    }

    pub fn stop(mut self) {
        drop(self.watch.take());
        if let Some(a) = self.audio.take() {
            a.stop();
        }
        self.plugin.stop();
    }
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
