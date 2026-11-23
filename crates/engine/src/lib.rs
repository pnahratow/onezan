//! The real-time part of onezan: MIDI in, a plugin in the middle, audio out.
//!
//! MIDI arrives on midir's thread, is stamped with its arrival time and passed to the audio
//! callback through lock-free SPSC queues (`rtrb`); the callback drains them at the start of each
//! block, places each event in the block (see [`timing`]) and runs the plugin.
//!
//! - `midi_in`: picks the keyboard and keeps it connected; what is held, for display.
//! - `audio_out`: picks the output with the lowest latency and opens the stream.
//! - `callback`: the audio callback (no allocation, no locks).
//! - `timing`: where in a block each live event plays.
//! - `metronome`: clicks made in the audio callback, on their exact sample.
//! - `report`: what was considered for each input and output, and why.
//! - `offline`: rendering without an audio device, for checks.

mod audio_out;
mod callback;
pub mod metronome;
pub mod midi_in;
mod offline;
mod report;
pub mod timing;

pub use audio_out::{AudioOut, Output, REQUESTED_FRAMES, audio_outputs, request_low_latency};
pub use metronome::{Click, Metronome};
pub use midi_in::{Keys, MidiChange, MidiWatch, choose_midi_port, midi_input_ports};
pub use offline::render_offline;
pub use report::{Considered, Verdict};
pub use rtrb::{Consumer, Producer, RingBuffer};
pub use timing::{MidiTiming, SharedTiming, TimedEvent};

/// Capacity of each MIDI queue (events between two audio blocks).
pub const QUEUE_CAP: usize = 1024;
/// The largest block the plugin is configured for; the plugin processes bigger ones in parts.
const MAX_FRAMES: usize = 1024;
/// The largest audio callback handled in one piece (JACK's largest common block); bigger ones
/// are split.
const MAX_CALLBACK_FRAMES: usize = 8192;

/// A new MIDI event queue: the producer goes to a source, the consumer to [`AudioOut::start`].
#[must_use]
pub fn midi_queue() -> (Producer<TimedEvent>, Consumer<TimedEvent>) {
    RingBuffer::new(QUEUE_CAP)
}
