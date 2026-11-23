//! The audio output: picks the backend with the lowest latency the system offers (JACK, else the
//! platform default), opens the stream and reports the choice.

use crate::MAX_FRAMES;
use crate::callback::{APPLIED_EXACT, APPLIED_IMMEDIATE, Callback, Report};
use crate::metronome::Metronome;
use crate::report::{Considered, Verdict};
use crate::timing::{self, MidiTiming, TimedEvent};
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use onezan_vst3_host::Plugin;
use rtrb::Consumer;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::time::{Duration, Instant};

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
    /// it, feeding it the events from `inputs`, placed in each block as `timing` says, with the
    /// clicks of `metronome` mixed in.
    ///
    /// # Errors
    ///
    /// When there is no usable output device (32-bit float), the plugin cannot start, or the
    /// stream cannot be opened.
    pub fn start(
        plugin: &mut Plugin,
        inputs: Vec<Consumer<TimedEvent>>,
        timing: MidiTiming,
        metronome: Arc<Metronome>,
    ) -> Result<Self> {
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
        let mut cb = Callback::new(processor, inputs, channels, sample_rate.0, timing, report, metronome);
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

    /// The MIDI timing in use (exact or immediate), once the audio callback has run.
    #[must_use]
    pub fn timing_mode(&self) -> Option<MidiTiming> {
        match self.applied.load(Ordering::Relaxed) {
            APPLIED_EXACT => Some(MidiTiming::Exact),
            APPLIED_IMMEDIATE => Some(MidiTiming::Immediate),
            _ => None,
        }
    }

    /// The MIDI timing report: the setting, what it resolved to for the current block, why, and
    /// the alternative. Call after [`AudioOut::block_frames`] has returned a size.
    #[must_use]
    pub fn midi_timing(&self) -> Option<Vec<Considered>> {
        let applied = self.timing_mode()?;
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

#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    use super::*;

    #[test]
    fn jack_hint_names_pw_jack_only_when_installed() {
        assert!(jack_missing_why(true).contains("start with `pw-jack "));
        assert!(!jack_missing_why(false).contains("pw-jack "));
        assert!(in_path("sh"));
        assert!(!in_path("onezan-no-such-program"));
    }
}
