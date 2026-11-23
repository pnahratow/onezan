//! Where in an audio block a live MIDI event plays.
//!
//! The audio callback runs once per block and drains the events that arrived since the last
//! one. Two ways to place them:
//!
//! - **Exact**: each event goes to the frame matching its arrival time within the previous
//!   callback interval, so the block plays one interval later. Notes keep their spacing; every
//!   note is delayed by one block.
//! - **Immediate**: every event goes to the start of the block. No extra delay, but a note waits
//!   between 0 and one block, at random: timing gets uneven.
//!
//! With short blocks the extra block of exact timing costs little, so it wins. With long blocks
//! the extra delay hurts more than the unevenness, so immediate wins. [`MidiTiming::Auto`]
//! decides per callback, so it follows block size changes.

use crate::report::{Considered, Verdict};
use onezan_vst3_host::MidiEvent;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

/// A MIDI event with the time it arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimedEvent {
    pub at: Instant,
    pub event: MidiEvent,
}

impl TimedEvent {
    /// `event`, arrived now.
    #[must_use]
    pub fn now(event: MidiEvent) -> Self {
        Self {
            at: Instant::now(),
            event,
        }
    }
}

/// How live MIDI events are placed in the audio block (the setting).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MidiTiming {
    /// [`MidiTiming::Exact`] while blocks are at most [`EXACT_MAX_BLOCK`], else immediate.
    #[default]
    Auto,
    Exact,
    Immediate,
}

/// The longest block for which [`MidiTiming::Auto`] picks exact timing: one extra block of delay
/// is then barely noticeable (512 frames at 48 kHz is 10.7 ms).
pub const EXACT_MAX_BLOCK: Duration = Duration::from_millis(11);

/// The duration of `frames` at `rate` Hz.
#[must_use]
pub fn block_duration(frames: usize, rate: u32) -> Duration {
    let nanos = u128::try_from(frames)
        .unwrap_or(u128::MAX)
        .saturating_mul(1_000_000_000)
        .checked_div(u128::from(rate))
        .unwrap_or(0);
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

impl MidiTiming {
    pub const ALL: [Self; 3] = [Self::Auto, Self::Exact, Self::Immediate];

    /// The setting called `name` (as [`MidiTiming::name`] gives it).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == name)
    }

    const fn to_u8(self) -> u8 {
        match self {
            Self::Auto => 0,
            Self::Exact => 1,
            Self::Immediate => 2,
        }
    }

    const fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Exact,
            2 => Self::Immediate,
            _ => Self::Auto,
        }
    }

    /// The mode in use for a block of `frames` at `rate` Hz: never [`MidiTiming::Auto`].
    #[must_use]
    pub fn resolve(self, frames: usize, rate: u32) -> Self {
        match self {
            Self::Auto if block_duration(frames, rate) <= EXACT_MAX_BLOCK => Self::Exact,
            Self::Auto => Self::Immediate,
            fixed => fixed,
        }
    }

    /// What the report and command line call it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Exact => "exact",
            Self::Immediate => "immediate",
        }
    }
}

/// The timing setting, shared with the audio callback: it can change while audio runs.
#[derive(Debug, Default)]
pub struct SharedTiming(AtomicU8);

impl SharedTiming {
    #[must_use]
    pub const fn new(t: MidiTiming) -> Self {
        Self(AtomicU8::new(t.to_u8()))
    }

    #[must_use]
    pub fn get(&self) -> MidiTiming {
        MidiTiming::from_u8(self.0.load(Ordering::Relaxed))
    }

    pub fn set(&self, t: MidiTiming) {
        self.0.store(t.to_u8(), Ordering::Relaxed);
    }
}

/// The frame for an event that arrived at `at`, in a block of `frames`.
///
/// The previous callback ran at `prev` and this one at `now`: the interval `prev..now` is mapped
/// onto the block. Events from before `prev` go to frame 0.
#[must_use]
pub fn exact_offset(at: Instant, prev: Instant, now: Instant, frames: usize) -> usize {
    let window = now.saturating_duration_since(prev).as_nanos();
    let since = at.saturating_duration_since(prev).as_nanos().min(window);
    let offset = since
        .saturating_mul(u128::try_from(frames).unwrap_or(u128::MAX))
        .checked_div(window)
        .unwrap_or(0);
    usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(frames.saturating_sub(1))
}

/// Sorts `(frame, event)` pairs by frame, keeping arrival order for equal frames.
///
/// A note-off and a new note-on of the same key must stay in order. Insertion sort: no
/// allocation, and the input is nearly sorted (each source is in order already).
pub fn sort_by_frame(events: &mut [(usize, MidiEvent)]) {
    for i in 1..events.len() {
        let mut j = i;
        while let Some(k) = j.checked_sub(1) {
            match (events.get(k), events.get(j)) {
                (Some(a), Some(b)) if a.0 > b.0 => events.swap(k, j),
                _ => break,
            }
            j = k;
        }
    }
}

/// The startup report on MIDI timing: what `setting` resolved to for blocks of `block`, why, and
/// the alternative.
#[must_use]
pub fn timing_considered(setting: MidiTiming, applied: MidiTiming, block: Duration) -> Vec<Considered> {
    let ms = block.as_secs_f64() * 1000.0;
    let limit = EXACT_MAX_BLOCK.as_secs_f64() * 1000.0;
    let exact = format!("exact: notes keep their spacing, {ms:.1} ms later");
    let immediate = format!("immediate: each note at the next block, 0 to {ms:.1} ms late, uneven");
    let (used_why, other_why) = match (setting, applied) {
        (MidiTiming::Auto, MidiTiming::Exact) => (
            format!("the block is short ({ms:.1} ms, at most {limit:.0} ms), so the extra delay is small"),
            "lower delay on average, but uneven".to_string(),
        ),
        (MidiTiming::Auto, _) => (
            format!("the block is long ({ms:.1} ms, over {limit:.0} ms): a whole block of extra delay would hurt more"),
            format!("steady timing, but {ms:.1} ms more delay"),
        ),
        _ => ("chosen by hand".to_string(), "not chosen".to_string()),
    };
    let (used, other) = if applied == MidiTiming::Exact {
        (exact, immediate)
    } else {
        (immediate, exact)
    };
    vec![
        Considered::new(used, Verdict::Used, used_why),
        Considered::new(other, Verdict::NotUsed, other_why),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: MidiEvent = MidiEvent::NoteOn {
        channel: 0,
        key: 60,
        velocity: 100,
    };
    const OFF: MidiEvent = MidiEvent::NoteOff {
        channel: 0,
        key: 60,
        velocity: 64,
    };

    #[test]
    fn auto_picks_exact_for_short_blocks_only() {
        assert_eq!(MidiTiming::Auto.resolve(256, 48_000), MidiTiming::Exact);
        assert_eq!(MidiTiming::Auto.resolve(512, 48_000), MidiTiming::Exact);
        assert_eq!(MidiTiming::Auto.resolve(1024, 48_000), MidiTiming::Immediate);
        assert_eq!(MidiTiming::Auto.resolve(1280, 48_000), MidiTiming::Immediate);
        assert_eq!(MidiTiming::Exact.resolve(4096, 48_000), MidiTiming::Exact);
        assert_eq!(MidiTiming::Immediate.resolve(64, 48_000), MidiTiming::Immediate);
    }

    #[test]
    fn block_duration_is_frames_over_rate() {
        assert_eq!(block_duration(480, 48_000), Duration::from_millis(10));
        assert_eq!(block_duration(1, 0), Duration::ZERO);
    }

    #[test]
    fn exact_offset_maps_interval_onto_block() {
        let prev = Instant::now();
        let now = prev + Duration::from_millis(10);
        let at = |ms| prev + Duration::from_millis(ms);
        assert_eq!(exact_offset(at(0), prev, now, 480), 0);
        assert_eq!(exact_offset(at(5), prev, now, 480), 240);
        // At or past the end of the interval: the last frame.
        assert_eq!(exact_offset(at(10), prev, now, 480), 479);
        assert_eq!(exact_offset(at(20), prev, now, 480), 479);
        // Before the interval: frame 0.
        let early = prev.checked_sub(Duration::from_millis(1)).unwrap();
        assert_eq!(exact_offset(early, prev, now, 480), 0);
        // A zero-length interval or block does not divide by zero.
        assert_eq!(exact_offset(prev, prev, prev, 480), 0);
        assert_eq!(exact_offset(at(5), prev, now, 0), 0);
    }

    #[test]
    fn sort_is_stable() {
        let mut v = vec![(5, NOTE), (2, OFF), (5, OFF), (0, NOTE), (2, NOTE)];
        sort_by_frame(&mut v);
        assert_eq!(v, [(0, NOTE), (2, OFF), (2, NOTE), (5, NOTE), (5, OFF)]);
    }

    #[test]
    fn report_explains_the_choice() {
        let short = Duration::from_micros(5333);
        let c = timing_considered(MidiTiming::Auto, MidiTiming::Exact, short);
        assert!(c[0].name.starts_with("exact"));
        assert!(c[0].why.contains("short"));
        let long = Duration::from_micros(26_667);
        let c = timing_considered(MidiTiming::Auto, MidiTiming::Immediate, long);
        assert!(c[0].name.starts_with("immediate"));
        assert!(c[0].why.contains("long"));
        let c = timing_considered(MidiTiming::Exact, MidiTiming::Exact, long);
        assert!(c[0].why.contains("by hand"));
    }
}
