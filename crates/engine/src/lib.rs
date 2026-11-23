//! The real-time part of onezan: MIDI in, a plugin in the middle, audio out.
//!
//! MIDI arrives on midir's thread, is stamped with its arrival time and passed to the audio
//! callback through lock-free SPSC queues (`rtrb`); the callback drains them at the start of each
//! block, places each event in the block (see [`timing`]) and runs the plugin.

pub mod midi_in;
pub mod timing;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use onezan_vst3_host::{MidiEvent, Plugin, Processor};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::time::{Duration, Instant};

pub use midi_in::{MidiChange, MidiWatch};
pub use rtrb::{Consumer, Producer, RingBuffer};
pub use timing::{MidiTiming, TimedEvent};

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

// ---- the startup report ----------------------------------------------------------------------

/// What was done with one option the engine considered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Used,
    /// Available, but another option was better.
    NotUsed,
    /// Not usable on this system.
    Unavailable,
}

/// One option considered for an input or output, and why it was or was not used. Lets the app
/// tell the user what it picked, what else there is, and how to choose differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Considered {
    pub name: String,
    pub verdict: Verdict,
    pub why: String,
}

impl Considered {
    fn new(name: impl Into<String>, verdict: Verdict, why: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            verdict,
            why: why.into(),
        }
    }
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

/// A loopback port ("Midi Through"): it only echoes what other programs send, it is no keyboard.
fn is_loopback(port: &str) -> bool {
    port.to_lowercase().contains("through")
}

/// Picks the MIDI port: the first whose name contains `query` (case-insensitive), or, without a
/// query, the first that is not a loopback. Returns its index and a verdict for every port.
#[must_use]
pub fn choose_midi_port(ports: &[String], query: Option<&str>) -> (Option<usize>, Vec<Considered>) {
    let q = query.map(str::to_lowercase);
    let fits = |p: &String| {
        q.as_ref()
            .map_or_else(|| !is_loopback(p), |q| p.to_lowercase().contains(q))
    };
    let chosen = ports.iter().position(fits);
    let considered = ports
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let (verdict, why) = match (Some(i) == chosen, &q) {
                (true, Some(q)) => (Verdict::Used, format!("matches \"{q}\"")),
                (true, None) => (Verdict::Used, "the first port that is not a loopback".to_string()),
                (false, Some(q)) if !fits(p) => (Verdict::NotUsed, format!("does not match \"{q}\"")),
                (false, None) if is_loopback(p) => (
                    Verdict::NotUsed,
                    "a loopback that echoes other programs, not a keyboard".to_string(),
                ),
                (false, _) => (
                    Verdict::NotUsed,
                    "another port was picked first; choose with --midi".to_string(),
                ),
            };
            Considered::new(p.clone(), verdict, why)
        })
        .collect();
    (chosen, considered)
}

// ---- audio output ----------------------------------------------------------------------------

/// The block size asked of PipeWire, in frames at 48 kHz (5.3 ms). PipeWire grants it, or the
/// smallest block the system allows if that is larger (`default.clock.min-quantum`).
pub const REQUESTED_FRAMES: u32 = 256;

/// The sample rate tried first on the ALSA fallback: PipeWire's default graph rate, so no
/// resampling happens.
const PREFERRED_RATE: u32 = 48_000;

/// Asks PipeWire for short audio blocks ([`REQUESTED_FRAMES`]) by setting `PIPEWIRE_LATENCY`,
/// unless it is already set. Applies to the JACK path (pipewire-jack); harmless elsewhere.
///
/// # Safety
///
/// Sets an environment variable, so no other thread may run yet: call it first thing in `main`.
pub unsafe fn request_low_latency() {
    if std::env::var_os("PIPEWIRE_LATENCY").is_none() {
        // SAFETY: the caller guarantees the process is still single-threaded.
        unsafe { std::env::set_var("PIPEWIRE_LATENCY", format!("{REQUESTED_FRAMES}/{PREFERRED_RATE}")) };
    }
}

/// A running audio stream that plays the plugin. Dropping it stops the sound; drop it before
/// the [`Plugin`].
pub struct AudioOut {
    /// The audio system in use: "JACK", "ALSA", "WASAPI", ...
    pub backend: String,
    pub device_name: String,
    pub sample_rate: u32,
    /// The block size the backend announced, if it has a fixed one (JACK does). The system can
    /// override it: see [`AudioOut::block_frames`] for what is really used.
    pub buffer_frames: Option<u32>,
    /// The block size asked of the system, when the backend takes a request (JACK on PipeWire,
    /// through `PIPEWIRE_LATENCY`).
    pub requested_frames: Option<u32>,
    /// Every output considered, and why it was or was not used.
    pub considered: Vec<Considered>,
    /// Frames in the latest audio callback, 0 until [`SETTLE_CALLBACKS`] have run.
    block: Arc<AtomicU32>,
    /// The MIDI timing setting, and the mode it resolved to in the latest callback.
    timing: MidiTiming,
    applied: Arc<AtomicU8>,
    stream: cpal::Stream,
}

/// The best output available: JACK on Linux when a server (usually PipeWire) answers, else the
/// platform default (ALSA on Linux, WASAPI on Windows). Also returns the verdict on each.
fn pick_output() -> Result<(cpal::HostId, cpal::Device, Vec<Considered>)> {
    let host = cpal::default_host();
    let default = host.default_output_device();
    let default_name = |d: &cpal::Device| {
        format!(
            "{}, device \"{}\"",
            host.id().name(),
            d.name().unwrap_or_else(|_| "?".into())
        )
    };
    #[cfg(target_os = "linux")]
    {
        let jack = cpal::host_from_id(cpal::HostId::Jack)
            .ok()
            .and_then(|h| h.default_output_device());
        if let Some(d) = jack {
            let mut considered = vec![Considered::new(
                "JACK",
                Verdict::Used,
                "runs at the audio server's own block size: the lowest latency this system offers",
            )];
            considered.push(default.as_ref().map_or_else(
                || Considered::new(host.id().name(), Verdict::Unavailable, "no output device"),
                |a| {
                    Considered::new(
                        default_name(a),
                        Verdict::NotUsed,
                        "works too, but on a PipeWire system it adds a larger buffer of its own",
                    )
                },
            ));
            return Ok((cpal::HostId::Jack, d, considered));
        }
        let jack_missing = Considered::new("JACK", Verdict::Unavailable, jack_missing_why(in_path("pw-jack")));
        let device = default.context("no audio output device (and no JACK server)")?;
        let considered = vec![
            jack_missing,
            Considered::new(default_name(&device), Verdict::Used, "the system's default output"),
        ];
        Ok((host.id(), device, considered))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let device = default.context("no audio output device")?;
        let considered = vec![Considered::new(
            default_name(&device),
            Verdict::Used,
            "the system's default output",
        )];
        Ok((host.id(), device, considered))
    }
}

/// Why JACK is not used, when no server answered.
///
/// Some distributions (Debian, Ubuntu, Mint) keep PipeWire's `libjack` out of the library path, so
/// programs reach it only when started through `pw-jack`. Nothing is done automatically: the
/// report only says so.
#[cfg(target_os = "linux")]
fn jack_missing_why(pw_jack: bool) -> String {
    if pw_jack {
        let args: Vec<String> = std::env::args().collect();
        format!(
            "no JACK server answered, but pw-jack is installed: for lower latency, start with `pw-jack {}`",
            args.join(" ")
        )
    } else {
        "no JACK server answered; on PipeWire systems the pipewire-jack package provides one".to_string()
    }
}

/// Whether an executable `name` is in `$PATH`.
#[cfg(target_os = "linux")]
fn in_path(name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| {
            std::fs::metadata(dir.join(name)).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}

#[cfg(target_os = "linux")]
fn is_jack(id: cpal::HostId) -> bool {
    id == cpal::HostId::Jack
}

#[cfg(not(target_os = "linux"))]
const fn is_jack(_: cpal::HostId) -> bool {
    false
}

/// The block size requested through `PIPEWIRE_LATENCY` ("frames/rate").
fn requested_pipewire_frames() -> Option<u32> {
    let v = std::env::var("PIPEWIRE_LATENCY").ok()?;
    v.split('/').next()?.trim().parse().ok()
}

/// A 32-bit float stereo (or wider) config: [`PREFERRED_RATE`] if the device offers it, else
/// the device's default rate.
fn pick_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig> {
    let default = device
        .default_output_config()
        .context("cannot query the output device")?;
    let f32_configs = || {
        device
            .supported_output_configs()
            .into_iter()
            .flatten()
            .filter(|c| c.sample_format() == cpal::SampleFormat::F32 && c.channels() >= 2)
    };
    let preferred = f32_configs().find_map(|c| c.try_with_sample_rate(cpal::SampleRate(PREFERRED_RATE)));
    if let Some(c) = preferred {
        return Ok(c);
    }
    if default.sample_format() == cpal::SampleFormat::F32 {
        return Ok(default);
    }
    f32_configs()
        .find_map(|c| c.try_with_sample_rate(default.sample_rate()))
        .context("the output device does not support 32-bit float")
}

impl AudioOut {
    /// Opens the best output (see [`pick_output`]), starts `plugin` at its sample rate and plays
    /// it, feeding it the events from `inputs`, placed in each block as `timing` says.
    ///
    /// # Errors
    ///
    /// When there is no usable output device (32-bit float), the plugin cannot start, or the
    /// stream cannot be opened.
    pub fn start(plugin: &mut Plugin, inputs: Vec<Consumer<TimedEvent>>, timing: MidiTiming) -> Result<Self> {
        let (host_id, device, considered) = pick_output()?;
        let device_name = device.name().unwrap_or_else(|_| "unknown device".into());
        let supported = pick_config(&device)?;
        let sample_rate = supported.sample_rate();
        let buffer_frames = match supported.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } if min == max => Some(*min),
            _ => None,
        };
        // Always the backend's own buffer size. JACK has exactly one. On ALSA, asking cpal 0.16
        // for a fixed size (any size, tested 256..1024) through PipeWire's ALSA plugin makes
        // every poll return POLLERR, and the stream never plays.
        let config = supported.config();
        let channels = usize::from(config.channels);

        let processor = plugin.start(f64::from(sample_rate.0), MAX_FRAMES)?;
        let block = Arc::new(AtomicU32::new(0));
        let applied = Arc::new(AtomicU8::new(0));
        let report = Report {
            block: block.clone(),
            applied: applied.clone(),
        };
        let mut cb = Callback::new(processor, inputs, channels, sample_rate.0, timing, report);
        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _| cb.run(data),
                |e| match e {
                    // JACK reports the graph changing its block size (PipeWire enforcing its
                    // minimum, another app asking for less) as an error. It is not one.
                    cpal::StreamError::BackendSpecific { err }
                        if err.description.starts_with("buffer size changed") =>
                    {
                        log::debug!("audio stream: {}", err.description);
                    }
                    e => log::error!("audio stream: {e}"),
                },
                None,
            )
            .context("cannot open the audio stream")?;
        stream.play().context("cannot start the audio stream")?;
        Ok(Self {
            backend: host_id.name().to_string(),
            requested_frames: requested_pipewire_frames().filter(|_| is_jack(host_id)),
            considered,
            device_name,
            sample_rate: sample_rate.0,
            buffer_frames,
            block,
            timing,
            applied,
            stream,
        })
    }

    /// The MIDI timing report: the setting, what it resolved to for the current block, why, and
    /// the alternative. Call after [`AudioOut::block_frames`] has returned a size.
    #[must_use]
    pub fn midi_timing(&self) -> Option<Vec<Considered>> {
        let applied = match self.applied.load(Ordering::Relaxed) {
            APPLIED_EXACT => MidiTiming::Exact,
            APPLIED_IMMEDIATE => MidiTiming::Immediate,
            _ => return None,
        };
        let frames = usize::try_from(self.block.load(Ordering::Relaxed)).ok()?;
        let block = timing::block_duration(frames, self.sample_rate);
        Some(timing::timing_considered(self.timing, applied, block))
    }

    /// The frames per audio callback actually in use, once the stream has settled: waits up to
    /// `timeout`. `None` if it did not settle in time. This is the latency the app adds.
    #[must_use]
    pub fn block_frames(&self, timeout: Duration) -> Option<u32> {
        let until = Instant::now().checked_add(timeout)?;
        loop {
            match self.block.load(Ordering::Relaxed) {
                0 if Instant::now() < until => std::thread::sleep(Duration::from_millis(5)),
                0 => return None,
                n => return Some(n),
            }
        }
    }

    pub fn stop(self) {
        drop(self.stream);
    }
}

/// Values the audio callback publishes for the startup report.
struct Report {
    /// Frames per callback, once settled.
    block: Arc<AtomicU32>,
    /// [`APPLIED_EXACT`] or [`APPLIED_IMMEDIATE`], once settled.
    applied: Arc<AtomicU8>,
}

const APPLIED_EXACT: u8 = 1;
const APPLIED_IMMEDIATE: u8 = 2;

/// State owned by the audio callback. Everything is allocated up front.
struct Callback {
    processor: Processor,
    inputs: Vec<Consumer<TimedEvent>>,
    /// This callback's events as `(frame, event)`, sorted by frame.
    events: Vec<(usize, MidiEvent)>,
    /// The events of one part of a callback too big to process at once, relative to that part.
    part_events: Vec<(usize, MidiEvent)>,
    left: Vec<f32>,
    right: Vec<f32>,
    channels: usize,
    rate: u32,
    timing: MidiTiming,
    /// When the previous callback ran: exact timing maps the time since then onto this block.
    prev: Option<Instant>,
    report: Report,
    callbacks: u32,
}

/// Callbacks before the block size is reported: the first ones can be one-off large chunks
/// that fill the device buffer at start-up.
const SETTLE_CALLBACKS: u32 = 4;

impl Callback {
    fn new(
        processor: Processor,
        inputs: Vec<Consumer<TimedEvent>>,
        channels: usize,
        rate: u32,
        timing: MidiTiming,
        report: Report,
    ) -> Self {
        Self {
            processor,
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
        }
    }

    fn run(&mut self, out: &mut [f32]) {
        let now = Instant::now();
        let prev = self.prev.replace(now);
        let ch = self.channels;
        let frames = out.len().checked_div(ch).unwrap_or(0);
        let mode = self.timing.resolve(frames, self.rate);
        if self.callbacks < SETTLE_CALLBACKS {
            self.callbacks = self.callbacks.saturating_add(1);
        } else {
            self.report
                .block
                .store(u32::try_from(frames).unwrap_or(u32::MAX), Ordering::Relaxed);
            let applied = if mode == MidiTiming::Exact {
                APPLIED_EXACT
            } else {
                APPLIED_IMMEDIATE
            };
            self.report.applied.store(applied, Ordering::Relaxed);
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
            self.processor.process(&self.part_events, l, r);
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

// ---- offline ---------------------------------------------------------------------------------

/// Renders `frames` frames of `plugin` offline, applying each `(frame, event)` at its frame.
/// For checks without an audio device.
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
        let start = i.saturating_mul(BLOCK);
        let end = start.saturating_add(BLOCK);
        block_events.clear();
        while let Some((at, ev)) = pending.next_if(|(at, _)| *at < end) {
            block_events.push((at.saturating_sub(start), ev));
        }
        p.process(&block_events, l, r);
    }
    drop(p);
    plugin.stop();
    Ok((left, right))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(ToString::to_string).collect()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn jack_hint_names_pw_jack_only_when_installed() {
        assert!(jack_missing_why(true).contains("start with `pw-jack "));
        assert!(!jack_missing_why(false).contains("pw-jack "));
        assert!(in_path("sh"));
        assert!(!in_path("onezan-no-such-program"));
    }

    #[test]
    fn midi_skips_loopback_without_query() {
        let ports = names(&["Midi Through Port-0", "EasyKeys49 MIDI 1", "Other Keys"]);
        let (chosen, c) = choose_midi_port(&ports, None);
        assert_eq!(chosen, Some(1));
        assert_eq!(
            c.iter().map(|c| c.verdict).collect::<Vec<_>>(),
            [Verdict::NotUsed, Verdict::Used, Verdict::NotUsed]
        );
        assert!(c[0].why.contains("loopback"));
        assert!(c[2].why.contains("--midi"));
    }

    #[test]
    fn midi_query_matches_case_insensitively() {
        let ports = names(&["Midi Through Port-0", "EasyKeys49 MIDI 1"]);
        let (chosen, c) = choose_midi_port(&ports, Some("THROUGH"));
        assert_eq!(chosen, Some(0));
        assert!(c[1].why.contains("does not match"));
    }

    #[test]
    fn midi_none_when_only_loopback() {
        let (chosen, c) = choose_midi_port(&names(&["Midi Through Port-0"]), None);
        assert_eq!(chosen, None);
        assert_eq!(c[0].verdict, Verdict::NotUsed);
    }
}
