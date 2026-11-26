//! onezan: play your MIDI keyboard through a VST3 instrument.

// Windows: a GUI program, so no console window opens with it (see `console`).
#![cfg_attr(windows, windows_subsystem = "windows")]

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use onezan_engine::{MidiTiming, TimedEvent, midi_queue};
use onezan_vst3_host::MidiEvent;
use std::io::BufRead;
use std::time::Duration;

mod chords;
mod console;
mod embed;
mod fingering;
mod gui;
mod instrument_state;
mod keyboard;
mod piano;
mod plugins;
mod progress;
mod report;
mod session;
mod settings;
mod staff;
mod waterfall;

use plugins::{Installed, choose, installed};
use report::{midi_change, print_report, print_section};
use session::{Options, Remembered, Session};

#[derive(Parser)]
#[command(version, about = "Play your MIDI keyboard through a VST3 instrument")]
struct Cli {
    /// Without a command, the window opens.
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

/// `--midi-timing` (the engine's [`MidiTiming`], which knows nothing of clap).
#[derive(Clone, Copy, ValueEnum)]
enum Timing {
    Auto,
    Exact,
    Immediate,
}

impl From<Timing> for MidiTiming {
    fn from(t: Timing) -> Self {
        match t {
            Timing::Auto => Self::Auto,
            Timing::Exact => Self::Exact,
            Timing::Immediate => Self::Immediate,
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Open the window (the default).
    Gui {
        /// Plugin name (substring), class id, or path to a .vst3 bundle. Default: first instrument.
        plugin: Option<String>,
        /// MIDI input port (substring of its name). Default: first port that is not a loopback.
        #[arg(long)]
        midi: Option<String>,
        /// How notes are placed in the audio block (see `play`). Default: the one chosen last
        /// time in the window, else auto.
        #[arg(long, value_enum)]
        midi_timing: Option<Timing>,
        /// Start the metronome at this many beats per minute.
        #[arg(long, value_name = "BPM")]
        metronome: Option<u32>,
    },
    /// List installed VST3 plugins (instruments only unless --all).
    List {
        #[arg(long)]
        all: bool,
    },
    /// List MIDI input ports.
    Midi,
    /// Play a plugin live from a MIDI keyboard.
    Play {
        /// Plugin name (substring), class id, or path to a .vst3 bundle. Default: first instrument.
        plugin: Option<String>,
        /// MIDI input port (substring of its name). Default: first port that is not a loopback.
        #[arg(long)]
        midi: Option<String>,
        /// Also play a C major scale through the plugin, to check the sound without a keyboard.
        #[arg(long)]
        demo: bool,
        /// How notes are placed in the audio block: exact (steady spacing, one block later),
        /// immediate (next block, uneven), or auto (exact for short blocks).
        #[arg(long, value_enum, default_value_t = Timing::Auto)]
        midi_timing: Timing,
        /// Start the metronome at this many beats per minute.
        #[arg(long, value_name = "BPM")]
        metronome: Option<u32>,
    },
    /// Render a C major scale offline and report the output level (no audio device needed).
    Render {
        plugin: Option<String>,
        #[arg(long, default_value_t = 48000)]
        sample_rate: u32,
    },
}

fn main() -> Result<()> {
    // SAFETY: first thing in main; no other thread exists yet.
    unsafe { onezan_engine::request_low_latency() };
    console::attach();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let cli = Cli::parse();
    if !matches!(cli.cmd, None | Some(Cmd::Gui { .. })) {
        console::ensure();
    }
    match cli.cmd {
        None => gui::run(Options {
            plugin: None,
            midi: None,
            timing: None,
            output: None,
            metronome: None,
            remembered: Remembered::default(),
        }),
        Some(Cmd::Gui {
            plugin,
            midi,
            midi_timing,
            metronome,
        }) => gui::run(Options {
            plugin,
            midi,
            timing: midi_timing.map(Into::into),
            output: None,
            metronome,
            remembered: Remembered::default(),
        }),
        Some(Cmd::List { all }) => {
            list(all);
            Ok(())
        }
        Some(Cmd::Midi) => {
            let ports = onezan_engine::midi_input_ports()?;
            if ports.is_empty() {
                println!("no MIDI input ports");
            }
            for p in ports {
                println!("{p}");
            }
            Ok(())
        }
        Some(Cmd::Play {
            plugin,
            midi,
            demo,
            midi_timing,
            metronome,
        }) => play(
            &Options {
                plugin,
                midi,
                timing: Some(midi_timing.into()),
                output: None,
                metronome,
                remembered: Remembered::default(),
            },
            demo,
        ),
        Some(Cmd::Render { plugin, sample_rate }) => render(plugin.as_deref(), sample_rate),
    }
}

fn list(all: bool) {
    let plugins = installed();
    let shown: Vec<_> = plugins.iter().filter(|p| all || p.is_instrument()).collect();
    if shown.is_empty() {
        println!(
            "no VST3 {}found in {:?}",
            if all { "" } else { "instruments " },
            onezan_vst3_host::default_search_paths()
        );
    }
    if crate::piano::available() {
        println!(
            "{:<28} {:<20} {:<24} {}",
            crate::piano::NAME,
            "FreePats",
            "Instrument|Piano",
            onezan_engine::BUILT_IN_PIANO
        );
    }
    for p in shown {
        println!(
            "{:<28} {:<20} {:<24} {}  {}",
            p.name,
            p.vendor,
            p.sub_categories,
            p.class_id,
            p.bundle.display()
        );
    }
}

/// C major, one octave up and down, from middle C.
const SCALE: [u8; 15] = [60, 62, 64, 65, 67, 69, 71, 72, 71, 69, 67, 65, 64, 62, 60];

fn play(opts: &Options, demo: bool) -> Result<()> {
    let (demo_tx, demo_rx) = midi_queue();
    let session = Session::start(opts, vec![demo_rx], std::sync::Arc::default(), |c| {
        println!("{:<10}{}", "MIDI in", midi_change(&c));
    })?;
    print_report(&session);
    if opts.metronome.is_some() {
        println!("{:<10}{} BPM", "metronome", session.metronome().bpm());
    }

    let demo_thread = demo.then(|| std::thread::spawn(move || play_scale(demo_tx)));
    if let Some(t) = demo_thread {
        t.join().ok();
    }
    println!("playing — press Enter to quit");
    if std::io::stdin().lock().lines().next().is_none() && demo {
        // No terminal (stdin closed): let the demo's last note ring out.
        std::thread::sleep(Duration::from_millis(1500));
    }
    session.stop();
    Ok(())
}

fn play_scale(mut tx: onezan_engine::Producer<TimedEvent>) {
    let step = Duration::from_millis(250);
    for &key in &SCALE {
        let _ = tx.push(TimedEvent::now(MidiEvent::NoteOn {
            channel: 0,
            key,
            velocity: 90,
        }));
        std::thread::sleep(step.mul_f32(0.9));
        let _ = tx.push(TimedEvent::now(MidiEvent::NoteOff {
            channel: 0,
            key,
            velocity: 64,
        }));
        std::thread::sleep(step.mul_f32(0.1));
    }
}

fn render(plugin_arg: Option<&str>, sample_rate: u32) -> Result<()> {
    let loaded = choose(&mut Installed, plugin_arg, None)?;
    let mut instrument = loaded.instrument;
    print_section("plugin", &loaded.considered, "", "");
    // A quarter second per note, sounding for 90% of it; two silent notes' worth at the end.
    let step = usize::try_from(sample_rate / 4).context("sample rate too high")?;
    let sounding = step.saturating_mul(9) / 10;
    let mut events = Vec::new();
    let mut at = 0usize;
    for &key in &SCALE {
        events.push((
            at,
            MidiEvent::NoteOn {
                channel: 0,
                key,
                velocity: 90,
            },
        ));
        events.push((
            at.saturating_add(sounding),
            MidiEvent::NoteOff {
                channel: 0,
                key,
                velocity: 64,
            },
        ));
        at = at.saturating_add(step);
    }
    let frames = at.saturating_add(step.saturating_mul(2));
    let (l, r) = onezan_engine::render_offline(&mut instrument, sample_rate, frames, events)?;
    let peak = l.iter().chain(&r).fold(0.0f32, |m, s| m.max(s.abs()));
    let (sum, count) = l.iter().chain(&r).fold((0.0f64, 0.0f64), |(sum, n), s| {
        let s = f64::from(*s);
        (s.mul_add(s, sum), n + 1.0)
    });
    let rms = if count > 0.0 { (sum / count).sqrt() } else { 0.0 };
    let db = |x: f64| if x > 0.0 { 20.0 * x.log10() } else { f64::NEG_INFINITY };
    println!(
        "rendered {frames} frames: peak {:.1} dBFS, rms {:.1} dBFS",
        db(f64::from(peak)),
        db(rms)
    );
    if peak == 0.0 {
        bail!("the instrument produced silence");
    }
    Ok(())
}
