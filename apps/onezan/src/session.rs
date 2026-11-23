//! A running instrument: the plugin, the MIDI keyboard and the audio output, as `play` and the
//! window both use them.

use crate::plugins::load;
use anyhow::Result;
use onezan_engine::midi_in::POLL;
use onezan_engine::{AudioOut, Considered, Consumer, Keys, MidiChange, MidiTiming, MidiWatch, TimedEvent, midi_queue};
use onezan_vst3_host::Plugin;
use std::sync::Arc;
use std::time::Duration;

/// What to start; `None` means "pick the best by itself".
pub struct Options {
    pub plugin: Option<String>,
    pub midi: Option<String>,
    pub timing: MidiTiming,
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
        let (mut plugin, plugins) = load(opts.plugin.as_deref())?;
        let mut report = vec![Section {
            label: "plugin",
            considered: plugins,
            detail: String::new(),
            short: plugin.name().to_string(),
            hint: "choose another with `onezan play NAME` (see `onezan list`)",
        }];
        let (midi_tx, midi_rx) = midi_queue();
        let (watch, first_midi, ports) = MidiWatch::start(opts.midi.clone(), POLL, midi_tx, keys, on_midi)?;
        report.push(Section {
            label: "MIDI in",
            considered: ports,
            detail: String::new(),
            short: String::new(),
            hint: "choose another with --midi NAME",
        });
        let mut inputs = vec![midi_rx];
        inputs.extend(extra);
        let audio = AudioOut::start(&mut plugin, inputs, opts.timing)?;
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
            plugin,
            report,
            first_midi,
        })
    }

    pub const fn plugin(&self) -> &Plugin {
        &self.plugin
    }

    /// The report section called `label`.
    pub fn section(&self, label: &str) -> Option<&Section> {
        self.report.iter().find(|s| s.label == label)
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
