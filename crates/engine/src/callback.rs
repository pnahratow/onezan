//! The audio callback: drains the MIDI queues, places each event in the block (see
//! [`crate::timing`]) and runs the instrument. Allocates nothing and takes no locks.

use crate::instrument::Voice;
use crate::metronome::{Clicker, Metronome};
use crate::timing::{self, MidiTiming, SharedTiming, TimedEvent};
use crate::{MAX_CALLBACK_FRAMES, QUEUE_CAP};
use onezan_vst3_host::MidiEvent;
use rtrb::Consumer;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// What the callback drives: renders a part of a block with its events (`(frame, event)`,
/// sorted, relative to the part). [`Voice`] in the app; a recording fake in the tests.
pub trait Render {
    fn process(&mut self, events: &[(usize, MidiEvent)], left: &mut [f32], right: &mut [f32]);
}

impl Render for Voice {
    fn process(&mut self, events: &[(usize, MidiEvent)], left: &mut [f32], right: &mut [f32]) {
        Self::process(self, events, left, right);
    }
}

/// Values the audio callback publishes for the startup report.
pub struct Report {
    /// Frames per callback, once settled.
    pub block: Arc<AtomicU32>,
    /// The audio system's predicted delay until a block is heard, in microseconds, once
    /// settled; 0 when it does not say.
    pub delay_us: Arc<AtomicU32>,
}

/// State owned by the audio callback. Everything is allocated up front.
pub struct Callback<V: Render = Voice> {
    voice: V,
    inputs: Vec<Consumer<TimedEvent>>,
    /// This callback's events as `(frame, event)`, sorted by frame.
    events: Vec<(usize, MidiEvent)>,
    /// The events of one part of a callback too big to process at once, relative to that part.
    part_events: Vec<(usize, MidiEvent)>,
    left: Vec<f32>,
    right: Vec<f32>,
    channels: usize,
    rate: u32,
    timing: Arc<SharedTiming>,
    /// When the previous callback ran: exact timing maps the time since then onto this block.
    prev: Option<Instant>,
    report: Report,
    callbacks: u32,
    metronome: Arc<Metronome>,
    clicker: Clicker,
}

/// Callbacks before the block size is reported: the first ones can be one-off large chunks
/// that fill the device buffer at start-up.
const SETTLE_CALLBACKS: u32 = 4;

impl<V: Render> Callback<V> {
    pub fn new(
        voice: V,
        inputs: Vec<Consumer<TimedEvent>>,
        channels: usize,
        rate: u32,
        timing: Arc<SharedTiming>,
        report: Report,
        metronome: Arc<Metronome>,
    ) -> Self {
        Self {
            voice,
            inputs,
            events: Vec::with_capacity(QUEUE_CAP * 2),
            part_events: Vec::with_capacity(QUEUE_CAP * 2),
            left: vec![0.0; MAX_CALLBACK_FRAMES],
            right: vec![0.0; MAX_CALLBACK_FRAMES],
            channels: channels.max(1),
            rate,
            timing,
            prev: None,
            report,
            callbacks: 0,
            metronome,
            clicker: Clicker::new(rate),
        }
    }

    /// Fills `out`. `delay`: how long until its first frame is heard, as the audio system
    /// predicts it (`None`: it does not say; then about one block, the one playing now).
    pub fn run(&mut self, out: &mut [f32], delay: Option<Duration>) {
        let now = Instant::now();
        let prev = self.prev.replace(now);
        let ch = self.channels;
        let frames = out.len().checked_div(ch).unwrap_or(0);
        let mode = self.timing.get().resolve(frames, self.rate);
        if self.callbacks < SETTLE_CALLBACKS {
            self.callbacks = self.callbacks.saturating_add(1);
        } else {
            self.report
                .block
                .store(u32::try_from(frames).unwrap_or(u32::MAX), Ordering::Relaxed);
            let us = delay.map_or(0, |d| u32::try_from(d.as_micros()).unwrap_or(u32::MAX));
            self.report.delay_us.store(us, Ordering::Relaxed);
        }

        self.events.clear();
        for q in &mut self.inputs {
            while self.events.len() < self.events.capacity() {
                let Ok(ev) = q.pop() else { break };
                let at = match (mode, prev) {
                    (MidiTiming::Exact, Some(p)) => timing::exact_offset(ev.at, p, now, frames),
                    _ => 0,
                };
                self.events.push((at, ev.event));
            }
        }
        timing::sort_by_frame(&mut self.events);

        let mut start = 0usize;
        for chunk in out.chunks_mut(MAX_CALLBACK_FRAMES.saturating_mul(ch)) {
            let n = chunk.len().checked_div(ch).unwrap_or(0);
            let end = start.saturating_add(n);
            let (Some(l), Some(r)) = (self.left.get_mut(..n), self.right.get_mut(..n)) else {
                continue;
            };
            // This part's events, relative to it.
            self.part_events.clear();
            self.part_events.extend(
                self.events
                    .iter()
                    .filter(|(at, _)| (start..end).contains(at))
                    .map(|&(at, ev)| (at.saturating_sub(start), ev)),
            );
            self.voice.process(&self.part_events, l, r);
            // When this part's first frame is heard: the system's delay, else about one block
            // (the block before it is playing).
            let ahead = delay.unwrap_or_else(|| heard_after(frames, self.rate));
            let heard = now.checked_add(ahead.saturating_add(heard_after(start, self.rate)));
            self.clicker.render_heard_at(&self.metronome, l, r, heard);
            start = end;
            for ((frame, &lv), &rv) in chunk.chunks_mut(ch).zip(l.iter()).zip(r.iter()) {
                match frame {
                    [mono] => *mono = f32::midpoint(lv, rv),
                    [fl, fr, rest @ ..] => {
                        *fl = lv;
                        *fr = rv;
                        rest.fill(0.0);
                    }
                    [] => {}
                }
            }
        }
    }
}

/// How long until frame `frames` of this callback's block is heard: the block before it is
/// playing now, so about one block on.
fn heard_after(frames: usize, rate: u32) -> Duration {
    let frames = u32::try_from(frames).unwrap_or(u32::MAX);
    Duration::try_from_secs_f64(f64::from(frames) / f64::from(rate.max(1))).unwrap_or_default()
}

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects, reason = "frame arithmetic on small test values")]
#[allow(
    clippy::float_cmp,
    reason = "the fake writes exact values that pass through unchanged"
)]
mod tests {
    use super::*;
    use crate::{QUEUE_CAP, midi_queue};
    use rtrb::Producer;
    use std::time::Duration;

    /// Records every event at its frame from the start of the stream, and each part's length;
    /// fills left with 0.5 and right with -0.25.
    #[derive(Default)]
    struct Recorder {
        got: Vec<(usize, MidiEvent)>,
        parts: Vec<usize>,
        done: usize,
    }

    impl Render for Recorder {
        fn process(&mut self, events: &[(usize, MidiEvent)], left: &mut [f32], right: &mut [f32]) {
            for &(at, ev) in events {
                self.got.push((self.done + at, ev));
            }
            left.fill(0.5);
            right.fill(-0.25);
            self.parts.push(left.len());
            self.done += left.len();
        }
    }

    struct Setup {
        cb: Callback<Recorder>,
        queues: Vec<Producer<TimedEvent>>,
        metronome: Arc<Metronome>,
        block: Arc<AtomicU32>,
    }

    fn setup(timing: MidiTiming, channels: usize, sources: usize) -> Setup {
        let (queues, inputs): (Vec<_>, Vec<_>) = (0..sources).map(|_| midi_queue()).unzip();
        let metronome = Arc::new(Metronome::default());
        let block = Arc::new(AtomicU32::new(0));
        let cb = Callback::new(
            Recorder::default(),
            inputs,
            channels,
            48_000,
            Arc::new(SharedTiming::new(timing)),
            Report {
                block: block.clone(),
                delay_us: Arc::default(),
            },
            Arc::clone(&metronome),
        );
        Setup {
            cb,
            queues,
            metronome,
            block,
        }
    }

    fn note(key: u8) -> MidiEvent {
        MidiEvent::NoteOn {
            channel: 0,
            key,
            velocity: 100,
        }
    }

    fn keys(got: &[(usize, MidiEvent)]) -> Vec<u8> {
        got.iter()
            .map(|(_, e)| match e {
                MidiEvent::NoteOn { key, .. } => *key,
                _ => 0,
            })
            .collect()
    }

    /// Runs one callback of `frames` frames.
    fn run(s: &mut Setup, frames: usize, channels: usize) -> Vec<f32> {
        let mut out = vec![9.0; frames * channels];
        s.cb.run(&mut out, None);
        out
    }

    #[test]
    fn every_event_reaches_the_voice_once_in_order() {
        let mut s = setup(MidiTiming::Immediate, 2, 1);
        for k in 60..65 {
            s.queues[0].push(TimedEvent::now(note(k))).unwrap();
        }
        run(&mut s, 512, 2);
        run(&mut s, 512, 2);
        assert_eq!(keys(&s.cb.voice.got), [60, 61, 62, 63, 64]);
        // Immediate: at the start of the block they arrived before.
        assert!(s.cb.voice.got.iter().all(|&(at, _)| at == 0));
    }

    #[test]
    fn events_from_every_source_are_delivered() {
        let mut s = setup(MidiTiming::Immediate, 2, 3);
        for (i, q) in s.queues.iter_mut().enumerate() {
            q.push(TimedEvent::now(note(60 + u8::try_from(i).unwrap()))).unwrap();
        }
        run(&mut s, 256, 2);
        let mut got = keys(&s.cb.voice.got);
        got.sort_unstable();
        assert_eq!(got, [60, 61, 62]);
    }

    #[test]
    fn a_flood_of_events_is_delayed_not_lost() {
        // More than one callback takes (2 × QUEUE_CAP): the rest waits in the queues.
        let mut s = setup(MidiTiming::Immediate, 2, 3);
        for q in &mut s.queues {
            for _ in 0..QUEUE_CAP {
                q.push(TimedEvent::now(note(60))).unwrap();
            }
        }
        run(&mut s, 256, 2);
        assert!(
            s.cb.voice.got.len() < 3 * QUEUE_CAP,
            "all in one callback: the test needs more"
        );
        run(&mut s, 256, 2);
        assert_eq!(s.cb.voice.got.len(), 3 * QUEUE_CAP);
    }

    #[test]
    fn exact_timing_keeps_events_in_order_inside_the_block() {
        let mut s = setup(MidiTiming::Exact, 2, 1);
        run(&mut s, 1024, 2);
        std::thread::sleep(Duration::from_millis(3));
        s.queues[0].push(TimedEvent::now(note(60))).unwrap();
        std::thread::sleep(Duration::from_millis(3));
        s.queues[0].push(TimedEvent::now(note(61))).unwrap();
        std::thread::sleep(Duration::from_millis(3));
        run(&mut s, 1024, 2);
        let got = &s.cb.voice.got;
        assert_eq!(keys(got), [60, 61]);
        let (a, b) = (got[0].0 - 1024, got[1].0 - 1024);
        assert!(a < b && b < 1024, "frames {a} and {b} in a block of 1024");
        assert!(a > 0, "exact timing put the first event at the block's start");
    }

    #[test]
    fn a_callback_too_big_for_one_part_is_split_and_keeps_its_events() {
        let total = MAX_CALLBACK_FRAMES + 1808;
        let mut s = setup(MidiTiming::Exact, 2, 1);
        run(&mut s, total, 2);
        std::thread::sleep(Duration::from_millis(5));
        // Arrives late in the interval: placed near the end, in the second part.
        s.queues[0].push(TimedEvent::now(note(60))).unwrap();
        run(&mut s, total, 2);
        let v = &s.cb.voice;
        assert_eq!(v.parts, [MAX_CALLBACK_FRAMES, 1808, MAX_CALLBACK_FRAMES, 1808]);
        assert_eq!(v.got.len(), 1, "the event was lost or repeated");
        let at = v.got[0].0 - total;
        assert!(at < total, "frame {at} lies outside the callback of {total}");
    }

    #[test]
    fn every_channel_layout_is_filled() {
        let mut s = setup(MidiTiming::Immediate, 1, 1);
        assert!(run(&mut s, 64, 1).iter().all(|&x| x == f32::midpoint(0.5, -0.25)));
        let mut s = setup(MidiTiming::Immediate, 2, 1);
        assert!(run(&mut s, 64, 2).chunks(2).all(|f| f == [0.5, -0.25]));
        // Further channels are silent, not left as they were.
        let mut s = setup(MidiTiming::Immediate, 4, 1);
        assert!(run(&mut s, 64, 4).chunks(4).all(|f| f == [0.5, -0.25, 0.0, 0.0]));
    }

    #[test]
    fn the_metronome_is_mixed_in_only_when_on() {
        let mut s = setup(MidiTiming::Immediate, 2, 1);
        assert!(run(&mut s, 2048, 2).chunks(2).all(|f| f == [0.5, -0.25]));
        s.metronome.set_on(true);
        assert!(
            run(&mut s, 2048, 2).chunks(2).any(|f| f != [0.5, -0.25]),
            "no click in the output"
        );
    }

    #[test]
    fn the_block_size_is_reported_once_settled() {
        let mut s = setup(MidiTiming::Immediate, 2, 1);
        for _ in 0..SETTLE_CALLBACKS {
            run(&mut s, 256, 2);
        }
        assert_eq!(s.block.load(Ordering::Relaxed), 0, "reported while settling");
        run(&mut s, 256, 2);
        assert_eq!(s.block.load(Ordering::Relaxed), 256);
    }
}
