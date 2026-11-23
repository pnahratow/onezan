//! The metronome: clicks made inside the audio callback, so every beat lands on its exact sample
//! regardless of block size or UI load.
//!
//! [`Metronome`] holds the settings (any thread may change them). [`Clicker`] lives in the audio
//! callback: it counts frames to the next beat and mixes a short click into the output after the
//! plugin.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub const MIN_BPM: u32 = 20;
pub const MAX_BPM: u32 = 300;

/// The metronome's settings, shared by the app and the audio callback.
pub struct Metronome {
    on: AtomicBool,
    bpm: AtomicU32,
    /// 0.0 to 1.0, as `f32` bits.
    volume: AtomicU32,
}

impl Default for Metronome {
    fn default() -> Self {
        Self {
            on: AtomicBool::new(false),
            bpm: AtomicU32::new(100),
            volume: AtomicU32::new(0.6f32.to_bits()),
        }
    }
}

impl Metronome {
    #[must_use]
    pub fn is_on(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    /// Switching on clicks at once.
    pub fn set_on(&self, on: bool) {
        self.on.store(on, Ordering::Relaxed);
    }

    #[must_use]
    pub fn bpm(&self) -> u32 {
        self.bpm.load(Ordering::Relaxed)
    }

    /// Clamped to [`MIN_BPM`]..=[`MAX_BPM`]. Takes effect from the next beat.
    pub fn set_bpm(&self, bpm: u32) {
        self.bpm.store(bpm.clamp(MIN_BPM, MAX_BPM), Ordering::Relaxed);
    }

    #[must_use]
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    /// Clamped to 0.0..=1.0.
    pub fn set_volume(&self, volume: f32) {
        let v = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.volume.store(v.to_bits(), Ordering::Relaxed);
    }
}

/// How a click sounds: a decaying sine starting at its peak (a sharp, short tick).
const CLICK_HZ: f32 = 1_500.0;
/// Decay time constant, and the level below which a click has ended.
const DECAY: f32 = 0.012;
const SILENT: f32 = 0.001;
/// Headroom: the click at full volume stays well under full scale next to the plugin.
const LEVEL: f32 = 0.5;

/// One sounding click.
struct Voice {
    phase: f32,
    step: f32,
    level: f32,
    decay: f32,
}

/// The audio-thread half: counts frames to the next beat and makes the clicks.
pub struct Clicker {
    rate: f64,
    /// Frames until the next beat starts (fractional, so odd tempos do not drift).
    until_next: f64,
    was_on: bool,
    voice: Option<Voice>,
}

impl Clicker {
    #[must_use]
    pub fn new(rate: u32) -> Self {
        Self {
            rate: f64::from(rate.max(1)),
            until_next: 0.0,
            was_on: false,
            voice: None,
        }
    }

    /// Mixes the clicks of the next `left.len()` frames into `left` and `right`. A click still
    /// sounding when the metronome is switched off rings out.
    pub fn render(&mut self, m: &Metronome, left: &mut [f32], right: &mut [f32]) {
        let on = m.is_on();
        if on && !self.was_on {
            // Switched on: the first click comes at once.
            self.until_next = 0.0;
        }
        self.was_on = on;
        if !on && self.voice.is_none() {
            return;
        }
        let period = self.rate * 60.0 / f64::from(m.bpm().clamp(MIN_BPM, MAX_BPM));
        let volume = m.volume() * LEVEL;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            if on {
                if self.until_next <= 0.0 {
                    self.voice = Some(self.voice(volume));
                    self.until_next += period;
                }
                self.until_next -= 1.0;
            }
            if let Some(v) = &mut self.voice {
                let s = v.phase.cos() * v.level;
                *l += s;
                *r += s;
                v.phase = (v.phase + v.step) % std::f32::consts::TAU;
                v.level *= v.decay;
                if v.level < SILENT * LEVEL {
                    self.voice = None;
                }
            }
        }
    }

    fn voice(&self, volume: f32) -> Voice {
        #[expect(clippy::cast_possible_truncation, reason = "a sample rate fits an f32 well enough")]
        #[expect(clippy::as_conversions, reason = "no as-free f64 to f32 conversion")]
        let rate = self.rate as f32;
        Voice {
            phase: 0.0,
            step: std::f32::consts::TAU * CLICK_HZ / rate,
            level: volume,
            decay: (-1.0 / (DECAY * rate)).exp(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// Renders `frames` in parts of `part` frames; returns the left channel.
    fn render(m: &Metronome, c: &mut Clicker, frames: usize, part: usize) -> Vec<f32> {
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        for (lp, rp) in l.chunks_mut(part).zip(r.chunks_mut(part)) {
            c.render(m, lp, rp);
        }
        l
    }

    /// Frames where a click starts: sound after at least 1000 silent frames.
    fn onsets(s: &[f32]) -> Vec<usize> {
        let mut quiet: usize = 1000;
        let mut out = Vec::new();
        for (i, x) in s.iter().enumerate() {
            if x.abs() > 1e-6 {
                if quiet >= 1000 {
                    out.push(i);
                }
                quiet = 0;
            } else {
                quiet = quiet.saturating_add(1);
            }
        }
        out
    }

    fn on_at(bpm: u32) -> Metronome {
        let m = Metronome::default();
        m.set_bpm(bpm);
        m.set_on(true);
        m
    }

    #[test]
    fn beats_land_on_their_sample_whatever_the_block_size() {
        let m = on_at(120);
        for part in [64, 333, 1024, 4096] {
            let mut c = Clicker::new(RATE);
            let s = render(&m, &mut c, 4 * 24_000 + 100, part);
            assert_eq!(onsets(&s), [0, 24_000, 48_000, 72_000, 96_000], "blocks of {part}");
        }
    }

    #[test]
    fn odd_tempos_do_not_drift() {
        // 70 BPM at 48 kHz: 41142.857... frames per beat. The 20th beat after the first starts
        // at frame 822857.14, so on frame 822858, not at 20 × 41143 = 822860.
        let m = on_at(70);
        let mut c = Clicker::new(RATE);
        let s = render(&m, &mut c, 822_900, 1000);
        assert_eq!(onsets(&s).last(), Some(&822_858));
    }

    #[test]
    fn off_is_silent_and_on_again_clicks_at_once() {
        let m = Metronome::default();
        let mut c = Clicker::new(RATE);
        assert!(render(&m, &mut c, 48_000, 256).iter().all(|x| *x == 0.0));
        m.set_on(true);
        let s = render(&m, &mut c, 30_000, 256);
        assert_eq!(onsets(&s)[0], 0, "the first click comes at once");
        m.set_on(false);
        let tail = render(&m, &mut c, 48_000, 256);
        assert!(tail.iter().skip(10_000).all(|x| *x == 0.0), "rings out, then silence");
    }

    #[test]
    fn settings_are_clamped() {
        let m = Metronome::default();
        m.set_bpm(1);
        assert_eq!(m.bpm(), MIN_BPM);
        m.set_bpm(10_000);
        assert_eq!(m.bpm(), MAX_BPM);
        m.set_volume(f32::NAN);
        assert!(m.volume().abs() < f32::EPSILON);
    }
}
