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
use std::time::Instant;

/// Values the audio callback publishes for the startup report.
pub struct Report {
    /// Frames per callback, once settled.
    pub block: Arc<AtomicU32>,
}

/// State owned by the audio callback. Everything is allocated up front.
pub struct Callback {
    voice: Voice,
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

impl Callback {
    pub fn new(
        voice: Voice,
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

    pub fn run(&mut self, out: &mut [f32]) {
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
            self.clicker.render(&self.metronome, l, r);
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
