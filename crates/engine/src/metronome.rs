//! The metronome: clicks made inside the audio callback, so every beat lands on its exact sample
//! regardless of block size or UI load.
//!
//! [`Metronome`] holds the settings (any thread may change them). [`Clicker`] lives in the audio
//! callback: it counts frames to the next beat and mixes a short click into the output after the
//! plugin.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering, fence};
use std::time::{Duration, Instant};

pub const MIN_BPM: u32 = 20;
pub const MAX_BPM: u32 = 300;

/// The click's sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Click {
    /// A short, bright tick.
    #[default]
    Tick,
    /// A hollow knock, like a woodblock.
    Wood,
    /// A longer, soft beep.
    Beep,
}

impl Click {
    pub const ALL: [Self; 3] = [Self::Tick, Self::Wood, Self::Beep];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Tick => "Tick",
            Self::Wood => "Woodblock",
            Self::Beep => "Beep",
        }
    }

    /// Two partials: (frequency in Hz, gain, decay time constant in seconds).
    const fn partials(self) -> [(f32, f32, f32); 2] {
        match self {
            Self::Tick => [(1_500.0, 1.0, 0.012), (4_500.0, 0.25, 0.003)],
            Self::Wood => [(820.0, 1.0, 0.025), (2_150.0, 0.5, 0.008)],
            Self::Beep => [(1_000.0, 0.8, 0.03), (2_000.0, 0.1, 0.015)],
        }
    }

    const fn index(self) -> u8 {
        match self {
            Self::Tick => 0,
            Self::Wood => 1,
            Self::Beep => 2,
        }
    }

    const fn from_index(i: u8) -> Self {
        match i {
            1 => Self::Wood,
            2 => Self::Beep,
            _ => Self::Tick,
        }
    }
}

/// The metronome's settings, shared by the app and the audio callback.
pub struct Metronome {
    on: AtomicBool,
    bpm: AtomicU32,
    /// 0.0 to 1.0, as `f32` bits.
    volume: AtomicU32,
    click: AtomicU8,
    /// One click to play now (to try a sound), metronome on or off.
    sample: AtomicBool,
    /// Keeps the beat without clicking (the falling notes follow it, unheard).
    silent: AtomicBool,
    /// Where the beat is, published by the audio callback for the window ([`Self::beat_now`]).
    clock: BeatClock,
}

/// The beat position at a moment, written by the audio callback and read by the window: a
/// sequence lock (odd while written), so a reader never sees half of an update.
#[derive(Default)]
struct BeatClock {
    seq: AtomicU64,
    /// Beats since the metronome was switched on (`f64` bits); the first click is beat 0.
    beats: AtomicU64,
    /// When that position is heard, in nanoseconds since [`epoch`].
    at: AtomicU64,
    /// Beats per second at that moment (`f64` bits); 0 while the metronome is off.
    rate: AtomicU64,
}

/// The moment the beat clock counts from (the first time anything asks).
fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

fn nanos_since_epoch(t: Instant) -> u64 {
    u64::try_from(t.saturating_duration_since(epoch()).as_nanos()).unwrap_or(u64::MAX)
}

impl BeatClock {
    fn publish(&self, beats: f64, at: Instant, rate: f64) {
        self.seq.fetch_add(1, Ordering::Relaxed);
        fence(Ordering::Release);
        self.beats.store(beats.to_bits(), Ordering::Relaxed);
        self.at.store(nanos_since_epoch(at), Ordering::Relaxed);
        self.rate.store(rate.to_bits(), Ordering::Relaxed);
        fence(Ordering::Release);
        self.seq.fetch_add(1, Ordering::Relaxed);
    }

    /// (beats, at, rate), as last published whole.
    fn read(&self) -> (f64, u64, f64) {
        loop {
            let before = self.seq.load(Ordering::Acquire);
            let beats = f64::from_bits(self.beats.load(Ordering::Relaxed));
            let at = self.at.load(Ordering::Relaxed);
            let rate = f64::from_bits(self.rate.load(Ordering::Relaxed));
            fence(Ordering::Acquire);
            if before.is_multiple_of(2) && self.seq.load(Ordering::Relaxed) == before {
                return (beats, at, rate);
            }
            std::hint::spin_loop();
        }
    }
}

impl Default for Metronome {
    fn default() -> Self {
        Self {
            on: AtomicBool::new(false),
            bpm: AtomicU32::new(100),
            volume: AtomicU32::new(0.6f32.to_bits()),
            click: AtomicU8::new(Click::Tick.index()),
            sample: AtomicBool::new(false),
            silent: AtomicBool::new(false),
            clock: BeatClock::default(),
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

    #[must_use]
    pub fn click(&self) -> Click {
        Click::from_index(self.click.load(Ordering::Relaxed))
    }

    pub fn set_click(&self, click: Click) {
        self.click.store(click.index(), Ordering::Relaxed);
    }

    /// Keeps the beat without clicking ([`Self::beat_now`] still counts), or clicks again.
    pub fn set_silent(&self, silent: bool) {
        self.silent.store(silent, Ordering::Relaxed);
    }

    /// Plays one click now, with the current sound and volume (to try them).
    pub fn play_sample(&self) {
        self.sample.store(true, Ordering::Relaxed);
    }

    /// The beat being heard now, counted from the first click after switching on (beat 0.0;
    /// 1.5 is half way between the second and third click). `None` while the metronome is off,
    /// or switched on and not yet clicked. For drawing in time with the clicks: it follows the
    /// audio callback, one block ahead (when the block is heard), and runs on between callbacks.
    #[must_use]
    pub fn beat_now(&self) -> Option<f64> {
        self.beat_at(Instant::now())
    }

    fn beat_at(&self, t: Instant) -> Option<f64> {
        if !self.is_on() {
            return None;
        }
        let (beats, at, rate) = self.clock.read();
        if rate <= 0.0 {
            return None;
        }
        let now = nanos_since_epoch(t);
        let ahead = if now >= at {
            Duration::from_nanos(now.saturating_sub(at)).as_secs_f64()
        } else {
            -Duration::from_nanos(at.saturating_sub(now)).as_secs_f64()
        };
        Some(ahead.mul_add(rate, beats))
    }
}

/// The level below which a click has ended, relative to full volume.
const SILENT: f32 = 0.001;
/// Headroom: the click at full volume stays well under full scale next to the plugin.
const LEVEL: f32 = 0.5;

/// One decaying sine, starting at its peak.
struct Partial {
    phase: f32,
    step: f32,
    level: f32,
    decay: f32,
}

impl Partial {
    fn next(&mut self) -> f32 {
        let s = self.phase.cos() * self.level;
        self.phase = (self.phase + self.step) % std::f32::consts::TAU;
        self.level *= self.decay;
        s
    }
}

/// One sounding click.
struct Voice([Partial; 2]);

impl Voice {
    fn next(&mut self) -> f32 {
        self.0.iter_mut().map(Partial::next).sum()
    }

    fn ended(&self) -> bool {
        self.0.iter().all(|p| p.level.abs() < SILENT * LEVEL)
    }
}

/// The audio-thread half: counts frames to the next beat and makes the clicks.
pub struct Clicker {
    rate: f64,
    /// Frames until the next beat starts (fractional, so odd tempos do not drift).
    until_next: f64,
    /// Clicks since the metronome was switched on.
    clicks: u64,
    was_on: bool,
    voice: Option<Voice>,
}

impl Clicker {
    #[must_use]
    pub fn new(rate: u32) -> Self {
        Self {
            rate: f64::from(rate.max(1)),
            until_next: 0.0,
            clicks: 0,
            was_on: false,
            voice: None,
        }
    }

    /// Mixes the clicks of the next `left.len()` frames into `left` and `right`. A click still
    /// sounding when the metronome is switched off rings out. (Tests; the callback also
    /// publishes the beat, with [`Self::render_heard_at`].)
    #[cfg(test)]
    pub fn render(&mut self, m: &Metronome, left: &mut [f32], right: &mut [f32]) {
        self.render_heard_at(m, left, right, None);
    }

    /// [`Self::render`], and publishes where the beat is at the block's first frame, which is
    /// heard at `heard` ([`Metronome::beat_now`]).
    pub fn render_heard_at(&mut self, m: &Metronome, left: &mut [f32], right: &mut [f32], heard: Option<Instant>) {
        let on = m.is_on();
        if on && !self.was_on {
            // Switched on: the first click comes at once.
            self.until_next = 0.0;
            self.clicks = 0;
        }
        self.was_on = on;
        if m.sample.swap(false, Ordering::Relaxed) {
            self.voice = Some(self.voice(m));
        }
        let silent = m.silent.load(Ordering::Relaxed);
        let bpm = f64::from(m.bpm().clamp(MIN_BPM, MAX_BPM));
        let period = self.rate * 60.0 / bpm;
        if let Some(at) = heard {
            // Beat `clicks - 1` started `period - until_next` frames ago (before the first
            // click: beat 0 is due now).
            #[expect(clippy::cast_precision_loss, reason = "click counts stay far below 2^52")]
            #[expect(clippy::as_conversions, reason = "no as-free u64 to f64 conversion")]
            let done = self.clicks as f64;
            let beats = done - 1.0 + (period - self.until_next) / period;
            m.clock.publish(beats, at, if on { bpm / 60.0 } else { 0.0 });
        }
        if !on && self.voice.is_none() {
            return;
        }
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            if on {
                if self.until_next <= 0.0 {
                    if !silent {
                        self.voice = Some(self.voice(m));
                    }
                    self.until_next += period;
                    self.clicks = self.clicks.saturating_add(1);
                }
                self.until_next -= 1.0;
            }
            if let Some(v) = &mut self.voice {
                let s = v.next();
                *l += s;
                *r += s;
                if v.ended() {
                    self.voice = None;
                }
            }
        }
    }

    /// A click with the metronome's current sound and volume.
    fn voice(&self, m: &Metronome) -> Voice {
        #[expect(clippy::cast_possible_truncation, reason = "a sample rate fits an f32 well enough")]
        #[expect(clippy::as_conversions, reason = "no as-free f64 to f32 conversion")]
        let rate = self.rate as f32;
        let volume = m.volume() * LEVEL;
        Voice(m.click().partials().map(|(hz, gain, decay)| Partial {
            phase: 0.0,
            step: std::f32::consts::TAU * hz / rate,
            level: gain * volume,
            decay: (-1.0 / (decay * rate)).exp(),
        }))
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
    fn volume_scales_the_click() {
        let peak = |volume: f32| {
            let m = on_at(120);
            m.set_volume(volume);
            let s = render(&m, &mut Clicker::new(RATE), 1000, 256);
            s.iter().fold(0.0f32, |p, x| p.max(x.abs()))
        };
        assert!(peak(0.0) < f32::EPSILON, "silent at 0");
        assert!(
            (peak(0.5) / peak(1.0) - 0.5).abs() < 0.01,
            "half the level at half volume"
        );
    }

    #[test]
    fn every_sound_clicks_briefly_and_they_differ() {
        let mut heard = Vec::new();
        for click in Click::ALL {
            let m = on_at(60);
            m.set_click(click);
            let s = render(&m, &mut Clicker::new(RATE), 24_000, 256);
            let end = s.iter().rposition(|x| x.abs() > 1e-6).unwrap();
            assert!(end > 100 && end < 24_000 / 2, "{click:?} ends after {end} frames");
            heard.push(s[..200].to_vec());
        }
        assert!(heard[0] != heard[1] && heard[1] != heard[2] && heard[0] != heard[2]);
    }

    #[test]
    fn a_sample_click_plays_while_off() {
        let m = Metronome::default();
        let mut c = Clicker::new(RATE);
        m.play_sample();
        let s = render(&m, &mut c, 24_000, 256);
        assert_eq!(onsets(&s), [0], "one click");
        assert!(
            render(&m, &mut c, 24_000, 256).iter().all(|x| *x == 0.0),
            "and only one"
        );
    }

    #[test]
    fn the_beat_clock_follows_the_clicks() {
        let m = on_at(120);
        let mut c = Clicker::new(RATE);
        let t0 = Instant::now();
        let (mut left, mut right) = (vec![0.0; 256], vec![0.0; 256]);
        // Not yet rendered: no beat.
        assert_eq!(m.beat_at(t0), None);
        // The first block: beat 0 is heard at t0.
        c.render_heard_at(&m, &mut left, &mut right, Some(t0));
        assert!(m.beat_at(t0).unwrap().abs() < 1e-9);
        // Half a second on at 120 BPM: one beat later.
        let beat = m.beat_at(t0 + Duration::from_millis(500)).unwrap();
        assert!((beat - 1.0).abs() < 1e-6, "{beat}");
        // After 24 000 frames (one beat), published again: beat 1, wherever the block ends.
        for _ in 0..93 {
            c.render(&m, &mut left, &mut right);
        }
        let t1 = t0 + Duration::from_secs(1);
        c.render_heard_at(&m, &mut left, &mut right, Some(t1));
        let frames = 94.0 * 256.0;
        let beat = m.beat_at(t1).unwrap();
        assert!((beat - frames / 24_000.0).abs() < 1e-9, "{beat}");
        m.set_on(false);
        assert_eq!(m.beat_at(t1), None);
    }

    #[test]
    fn silent_keeps_the_beat_without_clicking() {
        let m = on_at(120);
        m.set_silent(true);
        let mut c = Clicker::new(RATE);
        let (mut left, mut right) = (vec![0.0; 48_000], vec![0.0; 48_000]);
        c.render_heard_at(&m, &mut left, &mut right, Some(Instant::now()));
        assert!(left.iter().all(|x| *x == 0.0), "no clicks");
        let t = Instant::now();
        c.render_heard_at(&m, &mut left, &mut right, Some(t));
        assert!((m.beat_at(t).unwrap() - 2.0).abs() < 1e-9, "but the beat goes on");
        m.set_silent(false);
        let s = render(&m, &mut c, 24_000, 256);
        assert_eq!(onsets(&s), [0], "clicks again on the next beat");
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
