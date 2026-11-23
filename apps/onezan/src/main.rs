//! onezan: play your MIDI keyboard through a VST3 instrument.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use onezan_engine::{AudioOut, midi_queue};
use onezan_vst3_host::{MidiEvent, Plugin, PluginInfo};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Parser)]
#[command(version, about = "Play your MIDI keyboard through a VST3 instrument")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
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
    },
    /// Render a C major scale offline and report the output level (no audio device needed).
    Render {
        plugin: Option<String>,
        #[arg(long, default_value_t = 48000)]
        sample_rate: u32,
    },
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match Cli::parse().cmd {
        Cmd::List { all } => {
            list(all);
            Ok(())
        }
        Cmd::Midi => {
            let ports = onezan_engine::midi_input_ports()?;
            if ports.is_empty() {
                println!("no MIDI input ports");
            }
            for p in ports {
                println!("{p}");
            }
            Ok(())
        }
        Cmd::Play { plugin, midi, demo } => play(plugin.as_deref(), midi.as_deref(), demo),
        Cmd::Render { plugin, sample_rate } => render(plugin.as_deref(), sample_rate),
    }
}

fn installed() -> Vec<PluginInfo> {
    onezan_vst3_host::scan(&onezan_vst3_host::find_bundles(
        &onezan_vst3_host::default_search_paths(),
    ))
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

/// Resolves the plugin argument to (bundle, class id).
fn resolve(arg: Option<&str>) -> Result<(PathBuf, Option<String>)> {
    if let Some(a) = arg {
        let p = Path::new(a);
        if p.exists() {
            return Ok((p.to_path_buf(), None));
        }
    }
    let plugins = installed();
    let pick = arg.map_or_else(
        || plugins.iter().find(|p| p.is_instrument()),
        |a| {
            let q = a.to_lowercase();
            plugins.iter().find(|p| p.class_id.eq_ignore_ascii_case(a)).or_else(|| {
                plugins
                    .iter()
                    .filter(|p| p.name.to_lowercase().contains(&q))
                    .max_by_key(|p| (p.is_instrument(), p.name.len() == a.len()))
            })
        },
    );
    match pick {
        Some(p) => Ok((p.bundle.clone(), Some(p.class_id.clone()))),
        None if arg.is_none() => bail!("no VST3 instrument installed (try `onezan list --all`)"),
        None => bail!(
            "no installed plugin matches {:?} (see `onezan list`)",
            arg.unwrap_or_default()
        ),
    }
}

fn load(arg: Option<&str>) -> Result<Plugin> {
    let (bundle, class) = resolve(arg)?;
    let plugin = Plugin::load(&bundle, class.as_deref()).with_context(|| format!("loading {}", bundle.display()))?;
    println!("loaded {} ({})", plugin.name(), bundle.display());
    Ok(plugin)
}

/// C major, one octave up and down, from middle C.
const SCALE: [u8; 15] = [60, 62, 64, 65, 67, 69, 71, 72, 71, 69, 67, 65, 64, 62, 60];

fn play(plugin_arg: Option<&str>, midi_arg: Option<&str>, demo: bool) -> Result<()> {
    let mut plugin = load(plugin_arg)?;
    let (midi_tx, midi_rx) = midi_queue();
    let (demo_tx, demo_rx) = midi_queue();

    let midi_in = onezan_engine::open_midi_input(midi_arg, midi_tx)?;
    match (&midi_in, midi_arg) {
        (Some(m), _) => println!("MIDI input: {}", m.port_name),
        (None, Some(q)) => bail!("no MIDI input port matches {q:?} (see `onezan midi`)"),
        (None, None) if !demo => bail!("no MIDI keyboard found (see `onezan midi`, or try --demo)"),
        (None, None) => println!("no MIDI keyboard found; playing the demo only"),
    }

    let audio = AudioOut::start(&mut plugin, vec![midi_rx, demo_rx])?;
    println!(
        "audio: {} @ {} Hz, buffer {}",
        audio.device_name,
        audio.sample_rate,
        audio
            .buffer_frames
            .map_or_else(|| "device default".to_string(), |n| format!("{n} frames"))
    );

    let demo_thread = demo.then(|| std::thread::spawn(move || play_scale(demo_tx)));
    if let Some(t) = demo_thread {
        t.join().ok();
        if midi_in.is_none() {
            std::thread::sleep(Duration::from_millis(1500));
        }
    }
    if midi_in.is_some() {
        println!("playing — press Enter to quit");
        let _ = std::io::stdin().lock().lines().next();
    }
    // The stream (and with it the plugin's processor) goes first, then the plugin.
    audio.stop();
    drop(midi_in);
    plugin.stop();
    Ok(())
}

fn play_scale(mut tx: onezan_engine::Producer<MidiEvent>) {
    let step = Duration::from_millis(250);
    for &key in &SCALE {
        let _ = tx.push(MidiEvent::NoteOn {
            channel: 0,
            key,
            velocity: 90,
        });
        std::thread::sleep(step.mul_f32(0.9));
        let _ = tx.push(MidiEvent::NoteOff {
            channel: 0,
            key,
            velocity: 64,
        });
        std::thread::sleep(step.mul_f32(0.1));
    }
}

fn render(plugin_arg: Option<&str>, sample_rate: u32) -> Result<()> {
    let mut plugin = load(plugin_arg)?;
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
    let (l, r) = onezan_engine::render_offline(&mut plugin, sample_rate, frames, events)?;
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
        bail!("the plugin produced silence");
    }
    Ok(())
}
