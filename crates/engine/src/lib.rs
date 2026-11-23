//! The real-time part of onezan: MIDI in, a plugin in the middle, audio out.
//!
//! MIDI arrives on midir's thread and is passed to the audio callback through lock-free SPSC
//! queues (`rtrb`); the callback drains them at the start of each block and runs the plugin.

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use onezan_vst3_host::{MidiEvent, Plugin, Processor};

pub use rtrb::{Consumer, Producer, RingBuffer};

/// Capacity of each MIDI queue (events between two audio blocks).
pub const QUEUE_CAP: usize = 1024;
/// The largest block the plugin is configured for; bigger device buffers are split.
const MAX_FRAMES: usize = 1024;

/// A new MIDI event queue: the producer goes to a source, the consumer to [`AudioOut::start`].
#[must_use]
pub fn midi_queue() -> (Producer<MidiEvent>, Consumer<MidiEvent>) {
    RingBuffer::new(QUEUE_CAP)
}

// ---- MIDI input ------------------------------------------------------------------------------

/// Names of the available MIDI input ports.
///
/// # Errors
///
/// When the system MIDI client cannot be created.
pub fn midi_input_ports() -> Result<Vec<String>> {
    let input = midir::MidiInput::new("onezan")?;
    Ok(input.ports().iter().filter_map(|p| input.port_name(p).ok()).collect())
}

/// An open MIDI input; closes when dropped.
pub struct MidiIn {
    pub port_name: String,
    _conn: midir::MidiInputConnection<()>,
}

/// Opens the first port whose name contains `query` (case-insensitive), or, without a query,
/// the first port that is not a loopback ("Through"). `Ok(None)` when nothing matches.
///
/// # Errors
///
/// When the system MIDI client cannot be created or the port cannot be opened.
pub fn open_midi_input(query: Option<&str>, mut tx: Producer<MidiEvent>) -> Result<Option<MidiIn>> {
    let mut input = midir::MidiInput::new("onezan")?;
    input.ignore(midir::Ignore::All);
    let ports = input.ports();
    let named: Vec<(midir::MidiInputPort, String)> = ports
        .into_iter()
        .filter_map(|p| input.port_name(&p).ok().map(|n| (p, n)))
        .collect();
    let found = match query {
        Some(q) => {
            let q = q.to_lowercase();
            named.into_iter().find(|(_, n)| n.to_lowercase().contains(&q))
        }
        None => named.into_iter().find(|(_, n)| !n.to_lowercase().contains("through")),
    };
    let Some((port, port_name)) = found else {
        return Ok(None);
    };
    let conn = input
        .connect(
            &port,
            "onezan-in",
            move |_stamp, bytes, ()| {
                if let Some(ev) = MidiEvent::parse(bytes) {
                    // A full queue drops the event; the audio thread is not keeping up anyway.
                    let _ = tx.push(ev);
                }
            },
            (),
        )
        .map_err(|e| anyhow!("cannot open MIDI port {port_name}: {e}"))?;
    Ok(Some(MidiIn { port_name, _conn: conn }))
}

// ---- audio output ----------------------------------------------------------------------------

/// A running audio stream that plays the plugin. Dropping it stops the sound; drop it before
/// the [`Plugin`].
pub struct AudioOut {
    pub device_name: String,
    pub sample_rate: u32,
    pub buffer_frames: Option<u32>,
    stream: cpal::Stream,
}

impl AudioOut {
    /// Opens the default output device, starts `plugin` at its sample rate and plays it, feeding
    /// it the events from `inputs`.
    ///
    /// # Errors
    ///
    /// When there is no usable output device (32-bit float), the plugin cannot start, or the
    /// stream cannot be opened.
    pub fn start(plugin: &mut Plugin, inputs: Vec<Consumer<MidiEvent>>) -> Result<Self> {
        let host = cpal::default_host();
        let device = host.default_output_device().context("no audio output device")?;
        let device_name = device.name().unwrap_or_else(|_| "unknown device".into());
        let default = device
            .default_output_config()
            .context("cannot query the output device")?;
        let sample_rate = default.sample_rate();
        // Plugins process f32; ask the device for f32 at its default rate.
        let supported = if default.sample_format() == cpal::SampleFormat::F32 {
            default
        } else {
            device
                .supported_output_configs()?
                .filter(|c| c.sample_format() == cpal::SampleFormat::F32 && c.channels() >= 2)
                .find_map(|c| c.try_with_sample_rate(sample_rate))
                .context("the output device does not support 32-bit float")?
        };
        // The device's default buffer size. Asking cpal 0.16's ALSA backend for a fixed size
        // (any size, tested 256..1024) through PipeWire's ALSA plugin makes every poll return
        // POLLERR, and the stream never plays.
        let config = supported.config();
        let buffer_frames = None;
        let channels = usize::from(config.channels);

        let processor = plugin.start(f64::from(sample_rate.0), MAX_FRAMES)?;
        let mut cb = Callback::new(processor, inputs, channels);
        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _| cb.run(data),
                |e| log::error!("audio stream: {e}"),
                None,
            )
            .context("cannot open the audio stream")?;
        stream.play().context("cannot start the audio stream")?;
        Ok(Self {
            device_name,
            sample_rate: sample_rate.0,
            buffer_frames,
            stream,
        })
    }

    pub fn stop(self) {
        drop(self.stream);
    }
}

/// State owned by the audio callback. Everything is allocated up front.
struct Callback {
    processor: Processor,
    inputs: Vec<Consumer<MidiEvent>>,
    events: Vec<MidiEvent>,
    left: Vec<f32>,
    right: Vec<f32>,
    channels: usize,
}

impl Callback {
    fn new(processor: Processor, inputs: Vec<Consumer<MidiEvent>>, channels: usize) -> Self {
        Self {
            processor,
            inputs,
            events: Vec::with_capacity(QUEUE_CAP * 2),
            left: vec![0.0; MAX_FRAMES],
            right: vec![0.0; MAX_FRAMES],
            channels: channels.max(1),
        }
    }

    fn run(&mut self, out: &mut [f32]) {
        self.events.clear();
        for q in &mut self.inputs {
            while self.events.len() < self.events.capacity() {
                match q.pop() {
                    Ok(ev) => self.events.push(ev),
                    Err(_) => break,
                }
            }
        }
        let ch = self.channels;
        let mut first = true;
        for chunk in out.chunks_mut(MAX_FRAMES.saturating_mul(ch)) {
            let frames = chunk.len().checked_div(ch).unwrap_or(0);
            let (Some(l), Some(r)) = (self.left.get_mut(..frames), self.right.get_mut(..frames)) else {
                continue;
            };
            self.processor.process(if first { &self.events } else { &[] }, l, r);
            first = false;
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

// ---- offline ---------------------------------------------------------------------------------

/// Renders `frames` frames of `plugin` offline, applying each `(frame, event)` at the start of
/// the block containing that frame. For checks without an audio device.
///
/// # Errors
///
/// When the plugin cannot start.
pub fn render_offline(
    plugin: &mut Plugin,
    sample_rate: u32,
    frames: usize,
    mut events: Vec<(usize, MidiEvent)>,
) -> Result<(Vec<f32>, Vec<f32>)> {
    const BLOCK: usize = 256;
    events.sort_by_key(|(at, _)| *at);
    let mut p = plugin.start(f64::from(sample_rate), BLOCK)?;
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    let mut pending = events.into_iter().peekable();
    let mut block_events = Vec::new();
    for (i, (l, r)) in left.chunks_mut(BLOCK).zip(right.chunks_mut(BLOCK)).enumerate() {
        let end = i.saturating_add(1).saturating_mul(BLOCK);
        block_events.clear();
        while let Some((_, ev)) = pending.next_if(|(at, _)| *at < end) {
            block_events.push(ev);
        }
        p.process(&block_events, l, r);
    }
    drop(p);
    plugin.stop();
    Ok((left, right))
}
