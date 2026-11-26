//! What makes the sound: a VST3 plugin, or the built-in SoundFont piano.
//!
//! The main-thread half ([`Instrument`]) starts an audio-thread half ([`Voice`]) for the audio
//! callback. The built-in piano plays a SoundFont (SF2) with `rustysynth`; the program embeds
//! one (see `build/fetch-piano.sh`) and hands its bytes to [`Piano::from_sf2`].

use anyhow::{Result, anyhow};
use onezan_vst3_host::{MidiEvent, Plugin, Processor};
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// The id the built-in piano goes by where a plugin's class id would be (settings, `play`).
pub const BUILT_IN_PIANO: &str = "built-in-piano";

pub enum Instrument {
    Plugin(Plugin),
    Piano(Piano),
}

impl Instrument {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Plugin(p) => p.name(),
            Self::Piano(p) => &p.name,
        }
    }

    /// The plugin's class id, or [`BUILT_IN_PIANO`].
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Plugin(p) => p.class_id(),
            Self::Piano(_) => BUILT_IN_PIANO,
        }
    }

    #[must_use]
    pub const fn plugin(&self) -> Option<&Plugin> {
        match self {
            Self::Plugin(p) => Some(p),
            Self::Piano(_) => None,
        }
    }

    /// The built-in piano, when it plays.
    #[must_use]
    pub const fn piano(&self) -> Option<&Piano> {
        match self {
            Self::Plugin(_) => None,
            Self::Piano(p) => Some(p),
        }
    }

    pub const fn plugin_mut(&mut self) -> Option<&mut Plugin> {
        match self {
            Self::Plugin(p) => Some(p),
            Self::Piano(_) => None,
        }
    }

    /// Starts the audio-thread half at `sample_rate`, for blocks of up to `max_frames`.
    ///
    /// # Errors
    ///
    /// When the plugin cannot start, or the synthesizer refuses the sample rate.
    pub fn start(&mut self, sample_rate: u32, max_frames: usize) -> Result<Voice> {
        match self {
            Self::Plugin(p) => Ok(Voice::Plugin(p.start(f64::from(sample_rate), max_frames)?)),
            Self::Piano(p) => Ok(Voice::Piano(p.start(sample_rate)?)),
        }
    }

    /// Stops what [`Instrument::start`] started (after its voice is gone).
    pub fn stop(&mut self) {
        if let Self::Plugin(p) = self {
            p.stop();
        }
    }
}

/// The built-in piano: a SoundFont, parsed once; cheap to clone.
#[derive(Clone)]
pub struct Piano {
    name: String,
    sound_font: Arc<SoundFont>,
    /// Its volume, shared with the voice: 1.0 is the SoundFont's own level (`f32` bits).
    volume: Arc<AtomicU32>,
}

impl Piano {
    /// Parses the SoundFont in `sf2`; `name` is what the window and the report call it.
    ///
    /// # Errors
    ///
    /// When `sf2` is not a SoundFont `rustysynth` can read.
    pub fn from_sf2(name: &str, mut sf2: &[u8]) -> Result<Self> {
        let sound_font = SoundFont::new(&mut sf2).map_err(|e| anyhow!("cannot read the SoundFont: {e:?}"))?;
        Ok(Self {
            name: name.to_owned(),
            sound_font: Arc::new(sound_font),
            volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
        })
    }

    /// The volume: 1.0 is the SoundFont's own level.
    #[must_use]
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    /// Clamped to 0.0..=[`MAX_PIANO_VOLUME`]; heard from the next audio block.
    pub fn set_volume(&self, volume: f32) {
        let v = if volume.is_finite() {
            volume.clamp(0.0, MAX_PIANO_VOLUME)
        } else {
            1.0
        };
        self.volume.store(v.to_bits(), Ordering::Relaxed);
    }

    fn start(&self, sample_rate: u32) -> Result<PianoVoice> {
        let rate = i32::try_from(sample_rate)?;
        let synth = Synthesizer::new(&self.sound_font, &SynthesizerSettings::new(rate))
            .map_err(|e| anyhow!("the built-in piano cannot play at {sample_rate} Hz: {e:?}"))?;
        Ok(PianoVoice {
            level: synth.get_master_volume(),
            synth,
            volume: Arc::clone(&self.volume),
            applied: f32::NAN,
        })
    }
}

/// The loudest the built-in piano goes: twice its own level.
pub const MAX_PIANO_VOLUME: f32 = 2.0;

/// The audio-thread half of an [`Instrument`]. Move it into the audio callback.
#[expect(clippy::large_enum_variant, reason = "one value, owned by the audio callback")]
pub enum Voice {
    Plugin(Processor),
    Piano(PianoVoice),
}

impl Voice {
    /// Renders `left.len()` frames into `left`/`right` (equal lengths). Each event is `(frame,
    /// event)`, sorted by frame; one past the end takes effect at the end. Real-time safe.
    pub fn process(&mut self, events: &[(usize, MidiEvent)], left: &mut [f32], right: &mut [f32]) {
        match self {
            Self::Plugin(p) => p.process(events, left, right),
            Self::Piano(p) => p.process(events, left, right),
        }
    }
}

pub struct PianoVoice {
    synth: Synthesizer,
    /// The synthesizer's own master volume, which volume 1.0 keeps.
    level: f32,
    volume: Arc<AtomicU32>,
    /// The volume last given to the synthesizer.
    applied: f32,
}

impl PianoVoice {
    /// Renders up to each event's frame, then applies it: every event on its exact sample.
    fn process(&mut self, events: &[(usize, MidiEvent)], left: &mut [f32], right: &mut [f32]) {
        let volume = f32::from_bits(self.volume.load(Ordering::Relaxed));
        if volume.to_bits() != self.applied.to_bits() {
            self.applied = volume;
            self.synth.set_master_volume(self.level * volume);
        }
        let frames = left.len().min(right.len());
        let mut done = 0;
        for &(at, event) in events {
            let at = at.min(frames);
            self.render(left, right, done, at);
            done = done.max(at);
            self.apply(event);
        }
        self.render(left, right, done, frames);
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32], from: usize, to: usize) {
        if let (Some(l), Some(r)) = (left.get_mut(from..to), right.get_mut(from..to))
            && !l.is_empty()
        {
            self.synth.render(l, r);
        }
    }

    fn apply(&mut self, event: MidiEvent) {
        let s = &mut self.synth;
        match event {
            MidiEvent::NoteOn { channel, key, velocity } => {
                s.note_on(i32::from(channel), i32::from(key), i32::from(velocity));
            }
            MidiEvent::NoteOff { channel, key, .. } => s.note_off(i32::from(channel), i32::from(key)),
            // The sustain pedal and the other controllers, as MIDI messages.
            MidiEvent::ControlChange {
                channel,
                controller,
                value,
            } => s.process_midi_message(i32::from(channel), 0xB0, i32::from(controller), i32::from(value)),
            MidiEvent::PitchBend { channel, value } => {
                let (lsb, msb) = (value & 0x7F, (value >> 7) & 0x7F);
                s.process_midi_message(i32::from(channel), 0xE0, i32::from(lsb), i32::from(msb));
            }
            MidiEvent::PolyPressure { .. } | MidiEvent::ChannelPressure { .. } => {}
        }
    }
}
