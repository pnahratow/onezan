//! The window: the instrument at the top, a status bar, the keyboard at the bottom.
//!
//! The top shows the plugin's own editor ([`crate::embed`]). It is a native window over ours,
//! so nothing of ours can be drawn over it: the bar's menus and tooltips open downwards, over
//! the keyboard. On Linux that needs X11, so the
//! window runs on X11 (XWayland under a Wayland desktop).

use crate::chords::{self, Chord, Reading};
use crate::embed::{Embedded, PixelRect};
use crate::fingering::Hand;
use crate::fingering::{Preview, Scale, Shown};
use crate::keyboard::{self, KeyLook};
use crate::progress::{Practice, Recorder};
use crate::session::{Options, Remembered, Section, Session, port_name};
use crate::settings::{MetronomeSettings, Settings, Store};
use crate::waterfall::{self, Run, Song};
use anyhow::{Result, anyhow};
use egui::{Color32, RichText};
use onezan_engine::metronome::{MAX_BPM, MIN_BPM};
use onezan_engine::{Click, Keys, Metronome, MidiChange, MidiTiming, Output, Verdict};
use onezan_vst3_host::PluginInfo;
use onezan_vst3_host::{Editor, MidiEvent};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How often the plugin's editor is serviced (its timers and X11 connection) while it is open.
const EDITOR_IDLE: Duration = Duration::from_millis(4);

/// How long settings stay unchanged before they are saved (a tempo drag saves once, at its end).
const SAVE_DELAY: Duration = Duration::from_secs(1);

/// The window's size at the first start.
const DEFAULT_SIZE: [f32; 2] = [1200.0, 760.0];
const MIN_SIZE: [f32; 2] = [640.0, 420.0];

/// The embedded editor, shared by the window and the loop that services it (UI thread only).
type Shared = Rc<RefCell<Option<Embedded>>>;

/// Opens the window and runs until it is closed.
///
/// # Errors
///
/// When no window can be opened.
pub fn run(mut opts: Options) -> Result<()> {
    let (store, settings) = Store::open();
    opts.remembered = Remembered {
        plugin: settings.instrument.clone(),
        midi: settings.midi.clone(),
        output: settings.audio.clone().map(Into::into),
        timing: settings.midi_timing.as_deref().and_then(MidiTiming::from_name),
    };
    let size = settings
        .window
        .filter(|s| s.iter().all(|v| v.is_finite()))
        .map_or(DEFAULT_SIZE, |[w, h]| [w.max(MIN_SIZE[0]), h.max(MIN_SIZE[1])]);
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("onezan")
            .with_app_id("onezan")
            .with_inner_size(size)
            .with_min_inner_size(MIN_SIZE),
        ..Default::default()
    };
    let mut event_loop = {
        let mut b = winit::event_loop::EventLoop::<eframe::UserEvent>::with_user_event();
        #[cfg(target_os = "linux")]
        {
            use winit::platform::x11::EventLoopBuilderExtX11;
            // VST3 editors embed only in X11 windows.
            b.with_x11();
        }
        b.build().map_err(|e| anyhow!("cannot open the window: {e}"))?
    };
    let embedded: Shared = Rc::default();
    let shared = Rc::clone(&embedded);
    let mut app = eframe::create_native(
        "onezan",
        native,
        Box::new(move |cc| Ok(Box::new(App::new(&cc.egui_ctx, &opts, shared, store, settings)))),
        &event_loop,
    );
    // Our own loop rather than eframe's: while the plugin's editor is open it is serviced every
    // few milliseconds without repainting the window (the plugin's timers fire 60+ times a second).
    loop {
        let timeout = embedded.borrow().is_some().then_some(EDITOR_IDLE);
        if let eframe::EframePumpStatus::Exit(_) = app.pump_eframe_app(&mut event_loop, timeout) {
            break;
        }
        if let Some(e) = embedded.borrow().as_ref() {
            e.editor().idle();
        }
    }
    Ok(())
}

/// The windows the bar's buttons open, each an OS window of its own (the plugin's editor would
/// cover anything drawn inside the main window).
#[derive(Default)]
struct OpenWindows {
    options: bool,
    scales: bool,
    instruments: bool,
}

/// A choice made in one of the bar's menus, applied after the bar is drawn.
enum Pick {
    /// The MIDI port (a query for [`Session::choose_midi`]); `None`: the automatic choice.
    Midi(Option<String>),
    /// The audio output; `None`: the automatic choice.
    Output(Option<Output>),
    Timing(MidiTiming),
}

/// What the bar's menus list, read when a menu opens (listing audio outputs takes a moment).
#[derive(Default)]
struct MenuLists {
    ports: Option<Vec<String>>,
    outputs: Option<Vec<Output>>,
}

/// The MIDI keyboard as the bar shows it.
#[derive(Clone)]
enum MidiState {
    Connected(String),
    Waiting,
    Failed(String),
}

struct App {
    // Dropped in this order: the editor (removed from its window), then the plugin.
    /// The editor, until it is embedded at the first frame.
    editor: Option<Editor>,
    embedded: Shared,
    /// Why the middle shows no editor.
    editor_note: Option<String>,
    /// The window was sized to the editor once (or has a saved size, which wins).
    sized: bool,
    session: Option<Session>,
    /// Why there is no session.
    error: Option<String>,
    keys: Arc<Keys>,
    midi: Arc<Mutex<MidiState>>,
    range: RangeInclusive<u8>,
    /// The key held down with the mouse on the on-screen keyboard.
    mouse_key: Option<u8>,
    /// The sustain pedal as pressed with the mouse (the bar's Pedal light).
    mouse_pedal: MousePedal,
    /// The metronome's tap tempo.
    tap: TapTempo,
    preview: Preview,
    /// The last chord played, shown dimmed once the keys are up.
    last_chord: Option<Chord>,
    /// Which of the windows of their own are open.
    open: OpenWindows,
    /// The installed instruments, scanned when that window first opens (and on "Rescan").
    instruments: Option<Vec<PluginInfo>>,
    /// The starred instruments (class ids, in the order starred); listed first in that window.
    favorite_instruments: Vec<String>,
    /// The starred scales (by name, in the order starred); listed first in the scales window.
    favorite_scales: Vec<String>,
    /// Why the last switch of instrument did not work.
    instrument_note: Option<String>,
    /// Why the last audio output chosen did not work.
    audio_note: Option<String>,
    menus: MenuLists,
    /// What the app was started with; a switch of instrument starts again from it.
    opts: Options,
    scale_order: ScaleOrder,
    /// Records the notes played (`None`: no practice history this run).
    recorder: Option<Recorder>,
    /// The scale the recorder was last told about.
    recorded_scale: Option<String>,
    progress_path: Option<PathBuf>,
    /// Practice per scale, as last read for the scales window, and when.
    practice: HashMap<String, Practice>,
    practice_read: Option<Instant>,
    /// Its size at the last frame: a desktop that resizes it gets it repainted.
    options_size: Option<egui::Rect>,
    store: Store,
    /// The settings as read at the start: what cannot be read back now (the metronome without
    /// audio, the keyboard while unplugged) keeps its saved value.
    saved: Settings,
    /// The main window's inner size at the last frame, in points.
    window_size: Option<[f32; 2]>,
    /// When the settings first differed from the saved ones.
    changed_at: Option<Instant>,
    notes: Notes,
    /// The built-in piano's volume (1.0: its own level), kept while another instrument plays.
    piano_volume: f32,
    /// Dark (the default) or light.
    theme: egui::ThemePreference,
}

/// The falling notes, shown in place of the instrument's window.
#[derive(Default)]
#[expect(clippy::struct_excessive_bools, reason = "independent switches")]
struct Notes {
    /// Shown (the bar's Notes button).
    on: bool,
    /// The notes wait at the keys until played; otherwise they keep the metronome's time.
    wait: bool,
    /// The song opened, and its file; `None`: the scale shown falls.
    song: Option<(Song, PathBuf)>,
    /// Why the last song did not open.
    song_note: Option<String>,
    run: Option<Run>,
    /// The run switched the metronome on, so it switches it off at its end.
    started_metronome: bool,
    /// The keys held at the last frame, to see which were pressed since.
    was_held: Vec<u8>,
    /// The window's clock at the last frame, seconds.
    last_time: Option<f64>,
    /// Where the keyboard was drawn, and its keys: the notes fall onto them.
    keyboard: Option<(egui::Rect, RangeInclusive<u8>)>,
    /// The answer of a file dialog open on a thread of its own.
    picking: Option<mpsc::Receiver<Option<PathBuf>>>,
    /// The app plays the falling notes through the instrument (for fun: normally you hear only
    /// what you play).
    play: bool,
    /// The notes the app is playing now, as (key, velocity).
    sounding: Vec<(u8, u8)>,
    /// The run's beat at the last frame, to find the notes it passed since.
    last_beat: Option<f64>,
    /// Playing along without hearing the metronome (it keeps the time, silently).
    silent: bool,
}

fn midi_state(c: &MidiChange) -> MidiState {
    match c {
        MidiChange::Connected(p) => MidiState::Connected(p.clone()),
        MidiChange::Disconnected(_) => MidiState::Waiting,
        MidiChange::Failed { port, error } => MidiState::Failed(format!("cannot open {port}: {error}")),
    }
}

impl App {
    fn new(ctx: &egui::Context, opts: &Options, embedded: Shared, store: Store, settings: Settings) -> Self {
        style_buttons(ctx);
        let theme = theme_from_name(settings.theme.as_deref());
        ctx.set_theme(theme);
        crate::staff::install(ctx);
        let keys = Arc::new(Keys::default());
        let progress_path = crate::progress::default_path();
        let recorder = progress_path.as_deref().and_then(Recorder::start);
        if let Some(r) = &recorder {
            keys.set_note_listener(r.note_sink());
        }
        let repaint = ctx.clone();
        keys.set_waker(move || repaint.request_repaint());
        let midi = Arc::new(Mutex::new(MidiState::Waiting));
        let (session, error) = match start_session(ctx, opts, &keys, &midi) {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(e)),
        };
        let (editor, editor_note) = session.as_ref().map_or((None, None), |s| make_editor(ctx, s));
        if let Some(m) = session.as_ref().map(Session::metronome) {
            apply_metronome(m, &settings.metronome, opts.metronome.is_none());
        }
        let piano_volume = settings
            .piano_volume
            .filter(|v| v.is_finite())
            .map_or(1.0, |v| v.clamp(0.0, onezan_engine::MAX_PIANO_VOLUME));
        apply_piano_volume(session.as_ref(), piano_volume);
        let mut notes = Notes {
            on: settings.notes_view,
            wait: settings.wait,
            play: settings.play_notes,
            silent: settings.notes_silent,
            ..Notes::default()
        };
        if let Some(path) = &settings.song {
            match read_song(path) {
                Ok(song) => notes.song = Some((song, path.clone())),
                Err(e) => notes.song_note = Some(e),
            }
        }
        Self {
            editor,
            embedded,
            editor_note,
            sized: settings.window.is_some(),
            session,
            error,
            keys,
            midi,
            range: keyboard::DEFAULT_RANGE,
            mouse_key: None,
            mouse_pedal: MousePedal::default(),
            tap: TapTempo::default(),
            preview: settings
                .scale
                .as_deref()
                .and_then(Preview::from_name)
                .unwrap_or_default(),
            last_chord: None,
            open: OpenWindows::default(),
            instruments: None,
            favorite_instruments: settings.favorite_instruments.clone(),
            favorite_scales: settings.favorite_scales.clone(),
            instrument_note: None,
            audio_note: None,
            menus: MenuLists::default(),
            opts: opts.clone(),
            scale_order: ScaleOrder::Learning,
            recorder,
            recorded_scale: None,
            progress_path,
            practice: HashMap::new(),
            practice_read: None,
            options_size: None,
            store,
            window_size: settings.window,
            saved: settings,
            changed_at: None,
            notes,
            piano_volume,
            theme,
        }
    }

    /// The settings as they are now.
    fn settings(&self) -> Settings {
        let metronome = self.session.as_ref().map_or_else(
            || self.saved.metronome.clone(),
            |s| {
                let m = s.metronome();
                MetronomeSettings {
                    bpm: Some(m.bpm()),
                    volume: Some(m.volume()),
                    sound: Some(m.click().name().to_owned()),
                }
            },
        );
        let midi = match &*self.midi.lock().unwrap_or_else(PoisonError::into_inner) {
            MidiState::Connected(p) => Some(port_name(p).to_owned()),
            _ => self.saved.midi.clone(),
        };
        let (audio, midi_timing) = self.session.as_ref().map_or_else(
            || (self.saved.audio.clone(), self.saved.midi_timing.clone()),
            |s| {
                (
                    s.chosen_output().cloned().map(Into::into),
                    s.timing().map(|t| t.name().to_owned()),
                )
            },
        );
        Settings {
            audio,
            midi_timing,
            metronome,
            scale: (self.preview != Preview::Off).then(|| self.preview.name()),
            window: self.window_size,
            instrument: self.session.as_ref().map_or_else(
                || self.saved.instrument.clone(),
                // The built-in piano standing in is not a choice: keep the one saved.
                |s| {
                    if s.is_fallback() {
                        self.saved.instrument.clone()
                    } else {
                        Some(s.instrument().id().to_owned())
                    }
                },
            ),
            midi,
            favorite_instruments: self.favorite_instruments.clone(),
            favorite_scales: self.favorite_scales.clone(),
            notes_view: self.notes.on,
            wait: self.notes.wait,
            play_notes: self.notes.play,
            notes_silent: self.notes.silent,
            theme: theme_name(self.theme).map(str::to_owned),
            piano_volume: Some(self.piano_volume).filter(|v| (v - 1.0).abs() > f32::EPSILON),
            song: self.notes.song.as_ref().map(|(_, p)| p.clone()),
        }
    }

    /// Saves the settings once they have stayed the same for [`SAVE_DELAY`].
    fn keep_settings(&mut self, ctx: &egui::Context) {
        if let Some(r) = ctx.input(|i| i.viewport().inner_rect) {
            self.window_size = Some([r.width().round(), r.height().round()]);
        }
        let now = self.settings();
        if !self.store.changed(&now) {
            self.changed_at = None;
            return;
        }
        let since = *self.changed_at.get_or_insert_with(Instant::now);
        let waited = since.elapsed();
        if waited >= SAVE_DELAY {
            self.store.save(&now);
            self.changed_at = None;
        } else {
            ctx.request_repaint_after(SAVE_DELAY.saturating_sub(waited));
        }
    }

    fn bar(&mut self, ui: &mut egui::Ui) {
        let (notes_label, running) = (self.mode_label(), self.running());
        let (session, midi, keys, preview) = (self.session.as_ref(), &self.midi, &self.keys, self.preview);
        let lists = &mut self.menus;
        let (mut pick, mut pedal_clicked) = (None, false);
        let (options, scales, instruments) = (
            &mut self.open.options,
            &mut self.open.scales,
            &mut self.open.instruments,
        );
        let metronome = session.map(|s| Arc::clone(s.metronome()));
        let tap = &mut self.tap;
        let notes_view = self.notes.on;
        // With a song open, the metronome button switches the click (the song needs the
        // metronome's time); without one it starts and stops the metronome as ever.
        let has_song = self.notes.song.is_some();
        let mut switch_view = false;
        let silent = &mut self.notes.silent;
        // The left part gives way (truncated) when the window is narrow; the picker stays whole.
        egui::Sides::new().shrink_left().truncate().show(
            ui,
            |ui| {
                ui.label(RichText::new("onezan").strong().size(18.0));
                ui.separator();
                let section = |label| session.and_then(|s| s.section(label));
                let short = |label| section(label).map_or("none", |s: &Section| s.short.as_str());
                let r = toggle(ui, instruments, format!("Instrument: {}", shorten(short("plugin"))));
                if let Some(s) = section("plugin") {
                    r.on_hover_text(format!("Click to choose another.\n\n{}", explain(s)));
                }
                ui.separator();

                let midi = midi.lock().unwrap_or_else(PoisonError::into_inner).clone();
                let (text, dot) = match &midi {
                    MidiState::Connected(p) => (p.clone(), Color32::from_rgb(34, 197, 94)),
                    MidiState::Waiting => ("plug in a keyboard".to_string(), Color32::GRAY),
                    MidiState::Failed(e) => (e.clone(), Color32::from_rgb(239, 68, 68)),
                };
                let connected = match &midi {
                    MidiState::Connected(p) => Some(p.as_str()),
                    _ => None,
                };
                let r = menu_item(ui, "MIDI", &shorten(&text), Some(dot), section("MIDI in"));
                let shown = egui::Popup::menu(&r)
                    .align(egui::RectAlign::BOTTOM_START)
                    .align_alternatives(&[egui::RectAlign::BOTTOM_END])
                    .show(|ui| {
                        let ports = lists
                            .ports
                            .get_or_insert_with(|| onezan_engine::midi_input_ports().unwrap_or_default());
                        midi_menu(ui, ports, connected, session.and_then(Session::chosen_midi), &mut pick);
                    });
                if shown.is_none() {
                    lists.ports = None;
                }
                // Always there, lit while the pedal is down: the layout does not move. A click
                // presses or releases it.
                let pedal = if keys.sustain() {
                    Color32::from_rgb(251, 191, 36)
                } else {
                    Color32::from_gray(70)
                };
                pedal_clicked = pedal_item(ui, pedal);
                // The switch between the app's two sides: the instrument, and the falling notes.
                let hover = if notes_view {
                    "Back to the instrument's own window, to change its sound (a song playing goes on)"
                } else {
                    "Falling notes in place of the instrument's window: a song (a MIDI file) comes down to \
                     the keys, to play as it arrives"
                };
                switch_view = mode_button(ui, notes_view, &notes_label).on_hover_text(hover).clicked();
                ui.separator();
            },
            |ui| {
                toggle(ui, options, "Options")
                    .on_hover_text("Audio output, MIDI timing and the click's sound, in a window of its own");
                ui.separator();
                toggle(ui, scales, format!("Scale: {}", preview.name()))
                    .on_hover_text("Choose the scale whose fingering the keyboard shows, in a window of its own");
                if let Some(m) = &metronome {
                    ui.separator();
                    metronome_controls(
                        ui,
                        m,
                        tap,
                        (has_song && (notes_view || running)).then_some(&mut *silent),
                    );
                }
            },
        );
        if let Some(p) = pick {
            self.apply(p);
        }
        if switch_view {
            self.notes.on = !self.notes.on;
        }
        if pedal_clicked {
            let event = self.mouse_pedal.click(self.keys.sustain());
            self.keys.set_sustain(self.mouse_pedal.pressed);
            if let Some(s) = self.session.as_mut() {
                s.play(event);
            }
        }
    }

    /// Releases the pedal the mouse pressed once the keyboard's pedal is released.
    fn follow_pedal(&mut self) {
        if let Some(event) = self.mouse_pedal.follow(self.keys.sustain())
            && let Some(s) = self.session.as_mut()
        {
            s.play(event);
        }
    }

    /// Applies a choice from the bar's menus.
    fn apply(&mut self, pick: Pick) {
        let Some(s) = self.session.as_mut() else { return };
        match pick {
            Pick::Midi(query) => s.choose_midi(query),
            Pick::Timing(t) => s.set_timing(t),
            Pick::Output(o) => {
                self.audio_note = s.set_output(o.as_ref()).err().map(|e| {
                    let name = o
                        .as_ref()
                        .map_or_else(|| "the automatic choice".to_owned(), Output::label);
                    format!("{name} did not work ({e:#}); playing through the automatic choice")
                });
                crate::report::print_report(s);
            }
        }
    }

    /// The options, in a window of their own: the plugin's editor covers anything drawn over
    /// the main window.
    fn options_window(&mut self, ctx: &egui::Context) {
        let metronome = self.metronome().cloned();
        let builder = egui::ViewportBuilder::default()
            .with_title("onezan options")
            .with_app_id("onezan")
            .with_inner_size([420.0, 560.0]);
        let size = &mut self.options_size;
        let (session, audio_note, lists) = (self.session.as_ref(), self.audio_note.as_deref(), &mut self.menus);
        let theme = &mut self.theme;
        let mut pick = None;
        let close = ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("options"), builder, |ui, _| {
            let now = ui.ctx().input(|i| i.viewport().inner_rect);
            if now != *size {
                // Tiling desktops resize new windows at once; seen once without a repaint.
                *size = now;
                ui.ctx().request_repaint();
            }
            egui::CentralPanel::default().show(ui, |ui| {
                ui.heading("Metronome");
                ui.add_space(6.0);
                match &metronome {
                    Some(m) => metronome_options(ui, m),
                    None => {
                        ui.label("No audio: nothing to set.");
                    }
                }
                let Some(s) = session else { return };
                let in_use = |label| {
                    s.section(label)
                        .map_or_else(String::new, |sec: &Section| sec.short.clone())
                };
                ui.add_space(12.0);
                ui.heading("Audio output");
                ui.label(RichText::new(format!("In use: {}", in_use("audio out"))).weak())
                    .on_hover_text(s.section("audio out").map_or_else(String::new, explain));
                ui.add_space(4.0);
                // Listed when the window opens (it takes a moment), again after it is reopened.
                let outputs = lists.outputs.get_or_insert_with(onezan_engine::audio_outputs);
                output_menu(ui, outputs, s, audio_note, &mut pick);
                ui.add_space(12.0);
                ui.heading("MIDI timing");
                ui.label(RichText::new(format!("In use: {}", in_use("timing"))).weak())
                    .on_hover_text(s.section("timing").map_or_else(String::new, explain));
                ui.add_space(4.0);
                timing_menu(ui, s.timing(), &mut pick);
                ui.add_space(12.0);
                ui.heading("Appearance");
                ui.horizontal(|ui| {
                    for (choice, label) in [
                        (egui::ThemePreference::Dark, "Dark"),
                        (egui::ThemePreference::Light, "Light"),
                    ] {
                        if ui.radio_value(theme, choice, label).clicked() {
                            ui.ctx().set_theme(choice);
                        }
                    }
                });
            });
            ui.ctx().input(|i| i.viewport().close_requested())
        });
        if let Some(p) = pick {
            self.apply(p);
        }
        if close {
            self.open.options = false;
            self.options_size = None;
            self.menus.outputs = None;
        }
    }

    /// Switches to the instrument `class_id`: the current one's sound is saved, its editor
    /// removed and audio stopped; then the new one starts (with its own saved sound) on the same
    /// keyboard, metronome and scale. If it fails, the previous instrument comes back.
    fn switch_instrument(&mut self, ctx: &egui::Context, class_id: &str) {
        let now = self.settings();
        let ticking = self.session.as_ref().is_some_and(|s| s.metronome().is_on());
        let chosen_midi = self.session.as_ref().and_then(|s| s.chosen_midi().map(str::to_owned));
        let previous = self.session.as_ref().map(|s| s.instrument().id().to_owned());
        // In this order: the editor leaves its window, then the plugin goes.
        drop(self.embedded.borrow_mut().take());
        drop(self.editor.take());
        if let Some(s) = self.session.take() {
            s.save_sound();
            s.stop();
        }
        let mut opts = self.opts.clone();
        opts.plugin = Some(class_id.to_owned());
        opts.metronome = None;
        // The keyboard in use now, if any (else the usual choice); the output and timing in use.
        opts.remembered.midi.clone_from(&now.midi);
        opts.remembered.output = now.audio.clone().map(Into::into);
        opts.remembered.timing = now.midi_timing.as_deref().and_then(MidiTiming::from_name);
        opts.midi = chosen_midi;
        let session = match start_session(ctx, &opts, &self.keys, &self.midi) {
            Ok(s) => {
                self.instrument_note = None;
                Some(s)
            }
            Err(e) => {
                self.instrument_note = Some(format!("Could not switch: {e}"));
                previous.and_then(|p| {
                    opts.plugin = Some(p);
                    start_session(ctx, &opts, &self.keys, &self.midi).ok()
                })
            }
        };
        if let Some(m) = session.as_ref().map(Session::metronome) {
            apply_metronome(m, &now.metronome, true);
            m.set_on(ticking);
        }
        apply_piano_volume(session.as_ref(), self.piano_volume);
        self.error = session.is_none().then(|| "no instrument could be started".to_owned());
        (self.editor, self.editor_note) = session.as_ref().map_or((None, None), |s| make_editor(ctx, s));
        self.session = session;
    }

    /// The installed instruments, in a window of their own: click one to switch to it.
    fn instruments_window(&mut self, ctx: &egui::Context) {
        let list = self.instruments.get_or_insert_with(|| {
            crate::plugins::installed()
                .into_iter()
                .filter(PluginInfo::is_instrument)
                .collect()
        });
        let current = self.session.as_ref().map(|s| s.instrument().id().to_owned());
        let note = self.instrument_note.clone();
        let builder = egui::ViewportBuilder::default()
            .with_title("onezan instruments")
            .with_app_id("onezan")
            .with_inner_size([440.0, 420.0]);
        let (mut pick, mut rescan, mut toggle_star) = (None, false, None::<String>);
        let favorites = &self.favorite_instruments;
        let close = ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("instruments"), builder, |ui, _| {
            egui::CentralPanel::default().show(ui, |ui| {
                if let Some(n) = &note {
                    ui.label(RichText::new(n).color(Color32::from_rgb(239, 68, 68)));
                    ui.separator();
                }
                ui.horizontal(|ui| {
                    if ui
                        .button("Rescan")
                        .on_hover_text("Look for instruments again, after installing one")
                        .clicked()
                    {
                        rescan = true;
                    }
                    ui.label(RichText::new(format!("{} installed", list.len())).weak());
                });
                ui.separator();
                if list.is_empty() {
                    let dirs = onezan_vst3_host::default_search_paths();
                    let dirs: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
                    ui.label(format!("No VST3 instruments found in:\n{}", dirs.join("\n")));
                    ui.separator();
                }
                // The built-in piano first, then the plugins; starred ones in a group above.
                let mut rows: Vec<(String, String, String)> = Vec::new();
                if crate::piano::available() {
                    rows.push((
                        onezan_engine::BUILT_IN_PIANO.to_owned(),
                        format!("{}   FreePats Upright Piano KW", crate::piano::NAME),
                        "Built into onezan: a Kawai upright piano (public domain)".to_owned(),
                    ));
                }
                rows.extend(list.iter().map(|p| {
                    (
                        p.class_id.clone(),
                        format!("{}   {}", p.name, p.vendor),
                        p.bundle.display().to_string(),
                    )
                }));
                let (starred, rest): (Vec<_>, Vec<_>) = rows.iter().partition(|(id, ..)| favorites.contains(id));
                let mut row = |ui: &mut egui::Ui, (id, text, hover): &(String, String, String)| {
                    ui.horizontal(|ui| {
                        if star(ui, favorites.contains(id)) {
                            toggle_star = Some(id.clone());
                        }
                        let selected = current.as_deref() == Some(id.as_str());
                        let r = ui.selectable_label(selected, text).on_hover_text(hover);
                        if r.clicked() && !selected {
                            pick = Some(id.clone());
                        }
                    });
                };
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    if !starred.is_empty() {
                        ui.label(RichText::new("Favorites").strong());
                        for r in &starred {
                            row(ui, r);
                        }
                        ui.separator();
                        ui.label(RichText::new("All instruments").strong());
                    }
                    for r in &rest {
                        row(ui, r);
                    }
                });
            });
            ui.ctx().input(|i| i.viewport().close_requested())
        });
        if let Some(id) = toggle_star {
            toggle_favorite(&mut self.favorite_instruments, &id);
        }
        if rescan {
            self.instruments = None;
        }
        if let Some(id) = pick {
            self.switch_instrument(ctx, &id);
        }
        if close {
            self.open.instruments = false;
        }
    }

    /// The scales, in a window of their own: pick one, see the time spent on each, in the
    /// learning order or by name; starred ones first.
    fn scales_window(&mut self, ctx: &egui::Context) {
        // The practice figures are read again every few seconds while the window is open.
        if self.practice_read.is_none_or(|t| t.elapsed() >= PRACTICE_REFRESH)
            && let Some(path) = &self.progress_path
        {
            self.practice = crate::progress::practice(path);
            self.practice_read = Some(Instant::now());
        }
        ctx.request_repaint_after(PRACTICE_REFRESH);
        let builder = egui::ViewportBuilder::default()
            .with_title("onezan scales")
            .with_app_id("onezan")
            .with_inner_size([560.0, 640.0]);
        let (preview, order, practice) = (&mut self.preview, &mut self.scale_order, &self.practice);
        let favorites = &mut self.favorite_scales;
        let history = self.recorder.is_some();
        let close = ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("scales"), builder, |ui, _| {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Order");
                    for o in ScaleOrder::ALL {
                        if ui.add(egui::Button::new(o.name()).selected(*order == o)).clicked() {
                            *order = o;
                        }
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(*preview != Preview::Off, egui::Button::new("No scale"))
                        .clicked()
                    {
                        *preview = Preview::Off;
                    }
                    if !history {
                        ui.label(RichText::new("Practice history is off (see the log)").weak());
                    }
                });
                ui.separator();
                scale_list(ui, preview, *order, practice, favorites);
            });
            ui.ctx().input(|i| i.viewport().close_requested())
        });
        if close {
            self.open.scales = false;
            self.practice_read = None;
        }
    }

    /// Tells the recorder when the scale changes.
    fn record_scale(&mut self) {
        let now = (self.preview != Preview::Off).then(|| self.preview.name());
        if now != self.recorded_scale {
            if let Some(r) = &self.recorder {
                r.set_scale(now.clone());
            }
            self.recorded_scale = now;
        }
    }

    /// The middle: the plugin's editor, embedded at the first frame and kept in place.
    fn instrument(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let ppp = ui.ctx().pixels_per_point();
        let r = ui.max_rect();
        let px = |v: f32| (v * ppp).round();
        let area = PixelRect {
            x: to_i32(px(r.left())),
            y: to_i32(px(r.top())),
            w: to_u32(px(r.width())),
            h: to_u32(px(r.height())),
        };
        if let Some(editor) = self.editor.take() {
            match window_id(frame).and_then(|parent| Embedded::new(editor, parent, area)) {
                Ok(e) => *self.embedded.borrow_mut() = Some(e),
                Err(why) => {
                    log::warn!("plugin editor not shown: {why}");
                    self.editor_note = Some(why);
                }
            }
        }
        if let Some(e) = self.embedded.borrow_mut().as_mut() {
            if !self.sized {
                // Once: make the window fit the editor (a tiling desktop may not allow it).
                self.sized = true;
                let (w, h) = (f32::from(to_u16(e.size.0)) / ppp, f32::from(to_u16(e.size.1)) / ppp);
                let other = ui.ctx().content_rect().height() - r.height();
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w.max(640.0), h + other)));
            }
            e.place(area);
            return;
        }
        self.placeholder(ui);
    }

    fn placeholder(&mut self, ui: &mut egui::Ui) {
        if self.error.is_none() && self.session.as_ref().is_some_and(|s| s.instrument().piano().is_some()) {
            self.piano_page(ui);
            return;
        }
        ui.centered_and_justified(|ui| match (&self.session, &self.error) {
            (_, Some(e)) => {
                ui.label(RichText::new(e).color(Color32::from_rgb(239, 68, 68)));
            }
            (Some(s), None) => {
                let name = s.section("plugin").map_or("", |p| p.short.as_str());
                let why = self.editor_note.as_deref().unwrap_or("");
                ui.label(RichText::new(format!("{name}\n\nIts own window cannot be shown here: {why}")).weak());
            }
            (None, None) => {}
        });
    }

    /// The built-in piano's page in place of a plugin's window: a drawing of an upright piano,
    /// what it is, and its volume.
    fn piano_page(&mut self, ui: &mut egui::Ui) {
        let Some(s) = self.session.as_ref() else { return };
        let Some(piano) = s.instrument().piano() else { return };
        let fallback = s.is_fallback();
        let volume = &mut self.piano_volume;
        let grey = ui.visuals().weak_text_color();
        let free = ui.available_height();
        ui.vertical_centered(|ui| {
            // The drawing, name, credit and slider together, about 330 points high, centred.
            let size = egui::vec2(220.0, 190.0);
            let height = if fallback { 370.0 } else { 330.0 };
            ui.add_space(((free - height) / 2.0).max(8.0));
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            upright_piano(ui.painter(), rect, grey);
            ui.add_space(16.0);
            ui.label(RichText::new(crate::piano::NAME).size(18.0).color(grey));
            ui.label(RichText::new("FreePats \"Upright Piano KW\", a Kawai upright piano (public domain)").weak());
            if fallback {
                ui.label(
                    RichText::new(
                        "No VST3 instrument could be loaded. Install one and choose it under Instrument for \
                         other sounds.",
                    )
                    .weak(),
                );
            }
            ui.add_space(16.0);
            // A row of its own, centred under the text (a slider alone keeps to the left).
            let row = egui::Rect::from_center_size(
                egui::pos2(ui.max_rect().center().x, ui.cursor().top() + 12.0),
                egui::vec2(280.0, 24.0),
            );
            ui.spacing_mut().slider_width = 170.0;
            let r = ui.put(
                row,
                percent_slider(volume, onezan_engine::MAX_PIANO_VOLUME).text("Volume"),
            );
            if r.changed() {
                piano.set_volume(*volume);
            }
        });
    }

    fn keyboard(&mut self, ui: &mut egui::Ui) {
        // A key played outside the range widens it, so every played key stays visible.
        for key in 0..=127u8 {
            if self.keys.held(key) && !self.range.contains(&key) {
                self.range = keyboard::widen(&self.range, key);
            }
        }
        // The fingering shown widens it too, but only while it is shown; so does a song falling.
        let song = self.notes.song.as_ref().filter(|_| self.notes.on).map(|(s, _)| s);
        let shown = self.preview.shown();
        let mut range = shown.keys().fold(self.range.clone(), |r, key| keyboard::widen(&r, key));
        for n in song.map_or(&[][..], |s| s.notes.as_slice()) {
            range = keyboard::widen(&range, n.key);
        }
        self.notes.keyboard = Some((ui.available_rect_before_wrap(), range.clone()));
        let (keys, mouse, sounding) = (&self.keys, self.mouse_key, &self.notes.sounding);
        let looks = KeyLooks::new(&shown, &self.held_keys());
        let held = keyboard::show(ui, range, |key| {
            let velocity = keys
                .velocity(key)
                .or_else(|| (mouse == Some(key)).then_some(MOUSE_VELOCITY))
                .or_else(|| sounding.iter().find(|&&(k, _)| k == key).map(|&(_, v)| v));
            looks.look(key, velocity)
        });
        self.play_mouse(held);
    }

    /// The keys held now: on the MIDI keyboard, and with the mouse.
    fn held_keys(&self) -> Vec<u8> {
        (0..=127u8)
            .filter(|&k| self.keys.held(k) || self.mouse_key == Some(k))
            .collect()
    }

    /// The line above the keyboard: the chord held (its symbol, name and notes), the notes when
    /// they make no chord, or the last chord, dimmed, once the keys are up.
    fn chord_line(&mut self, ui: &mut egui::Ui) {
        let reading = chords::read(&self.held_keys());
        if let Some(Reading::Chord(c)) = &reading {
            self.last_chord = Some(c.clone());
        }
        ui.horizontal_centered(|ui| match (&reading, &self.last_chord) {
            (Some(Reading::Chord(c)), _) => {
                ui.label(RichText::new(&c.symbol).strong().size(22.0));
                ui.label(RichText::new(&c.name).size(15.0));
                ui.label(RichText::new(&c.notes).weak().size(15.0));
            }
            (Some(Reading::Notes { notes, interval }), _) => {
                ui.label(RichText::new(notes).size(15.0));
                if let Some(i) = interval {
                    ui.label(RichText::new(*i).weak().size(15.0));
                }
            }
            (None, Some(c)) => {
                ui.label(RichText::new(&c.symbol).weak().size(22.0));
                ui.label(RichText::new(&c.name).weak().size(15.0));
                ui.label(RichText::new(&c.notes).weak().size(15.0));
            }
            (None, None) => {
                ui.label(RichText::new("Play a chord to see its name and fingering").weak());
            }
        });
    }

    /// Plays the key the mouse holds on the on-screen keyboard: a note at full velocity, released
    /// when the button goes up or the mouse slides to another key.
    fn play_mouse(&mut self, held: Option<u8>) {
        if held == self.mouse_key {
            return;
        }
        if let Some(s) = self.session.as_mut() {
            if let Some(key) = self.mouse_key {
                s.play(note_off(key));
            }
            if let Some(key) = held {
                s.play(note_on(key, MOUSE_VELOCITY));
            }
        }
        self.mouse_key = held;
    }
}

impl App {
    fn metronome(&self) -> Option<&Arc<Metronome>> {
        self.session.as_ref().map(Session::metronome)
    }

    /// The metronome's tempo (100 without audio).
    fn bpm(&self) -> f64 {
        self.metronome().map_or(100.0, |m| f64::from(m.bpm()))
    }

    /// The run while it goes (not yet played through).
    fn live_run(&self) -> Option<&Run> {
        self.notes.run.as_ref().filter(|r| !r.finished())
    }

    fn running(&self) -> bool {
        self.live_run().is_some()
    }

    /// Whether the metronome keeps the time unheard: a song open, played along with, and its
    /// click switched off (the metronome button). Everywhere else it clicks as ever.
    fn metronome_silent(&self) -> bool {
        self.notes.silent && self.notes.song.is_some() && self.live_run().is_some_and(|r| !r.wait)
    }

    /// The song that falls: the one opened, else the scale shown.
    fn falling_song(&self) -> Option<Cow<'_, Song>> {
        match (&self.notes.song, self.preview) {
            (Some((song, _)), _) => Some(Cow::Borrowed(song)),
            (None, Preview::Scale(s)) => Some(Cow::Owned(Song::scale(s))),
            (None, Preview::Off) => None,
        }
    }

    /// The name of the song that falls, without building a scale's notes.
    fn falling_name(&self) -> Option<Cow<'_, str>> {
        match (&self.notes.song, self.preview) {
            (Some((song, _)), _) => Some(Cow::Borrowed(song.name.as_str())),
            (None, Preview::Scale(s)) => Some(Cow::Owned(s.name())),
            (None, Preview::Off) => None,
        }
    }

    /// Starts playing the song through, or stops.
    fn toggle_run(&mut self) {
        let was = self.running();
        self.stop_run();
        if was {
            return;
        }
        let Some(song) = self.falling_song().map(Cow::into_owned) else {
            return;
        };
        let wait = self.notes.wait;
        self.notes.run = Some(Run::new(song, wait));
        if !wait && let Some(m) = self.metronome().cloned() {
            self.notes.started_metronome = !m.is_on();
            // Before it starts, or its first click would sound.
            m.set_silent(self.metronome_silent());
            m.set_on(true);
        }
    }

    fn stop_run(&mut self) {
        self.end_metronome();
        self.release_notes();
        self.notes.run = None;
    }

    /// Plays the notes the run passed since the last frame (when the app plays them): the ones
    /// ending first, so a repeated key sounds again.
    fn play_notes(&mut self) {
        let beat = self.live_run().map(Run::beat);
        let last = std::mem::replace(&mut self.notes.last_beat, beat);
        let (Some(run), Some(now), Some(from), true) = (&self.notes.run, beat, last, self.notes.play) else {
            return;
        };
        let (starts, ends) = run.song.between(from, now);
        let Some(s) = self.session.as_mut() else { return };
        for key in ends {
            if let Some(i) = self.notes.sounding.iter().position(|&(k, _)| k == key) {
                self.notes.sounding.swap_remove(i);
                s.play(note_off(key));
            }
        }
        for (key, velocity) in starts {
            self.notes.sounding.push((key, velocity));
            s.play(note_on(key, velocity));
        }
    }

    /// Stops every note the app is playing.
    fn release_notes(&mut self) {
        self.notes.last_beat = None;
        let sounding = std::mem::take(&mut self.notes.sounding);
        if let Some(s) = self.session.as_mut() {
            for (key, _) in sounding {
                s.play(note_off(key));
            }
        }
    }

    /// Switches the metronome off if the run switched it on.
    fn end_metronome(&mut self) {
        if std::mem::take(&mut self.notes.started_metronome)
            && let Some(m) = self.metronome()
        {
            m.set_on(false);
        }
    }

    /// Opens a song dropped on the window, or chosen in the file dialog.
    fn take_song(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        let picked = self.notes.picking.as_ref().and_then(|rx| rx.try_recv().ok());
        if picked.is_some() {
            self.notes.picking = None;
        }
        if let Some(path) = dropped.or_else(|| picked.flatten()) {
            self.open_song(path);
        }
    }

    /// Reads the song at `path`, shows it falling, and takes its tempo.
    fn open_song(&mut self, path: PathBuf) {
        match read_song(&path) {
            Ok(song) => {
                if let (Some(bpm), Some(m)) = (song.bpm, self.metronome()) {
                    m.set_bpm(bpm);
                }
                self.stop_run();
                self.notes.song = Some((song, path));
                self.notes.song_note = None;
                self.notes.on = true;
            }
            Err(e) => self.notes.song_note = Some(e),
        }
    }

    /// Asks for a song in the system's file dialog, on a thread of its own so the window keeps
    /// drawing; [`Self::take_song`] receives the answer.
    fn pick_song(&mut self, ctx: &egui::Context) {
        if self.notes.picking.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let started = std::thread::Builder::new().name("song dialog".into()).spawn(move || {
            let path = rfd::FileDialog::new()
                .set_title("Open a song")
                .add_filter("MIDI files", &["mid", "midi", "kar"])
                .pick_file();
            let _ = tx.send(path);
            ctx.request_repaint();
        });
        match started {
            Ok(_) => self.notes.picking = Some(rx),
            Err(e) => self.notes.song_note = Some(format!("no file dialog: {e}")),
        }
    }

    /// The keys pressed since the last frame (MIDI keyboard and mouse).
    fn newly_pressed(&mut self) -> Vec<u8> {
        let held = self.held_keys();
        let pressed = held
            .iter()
            .copied()
            .filter(|k| !self.notes.was_held.contains(k))
            .collect();
        self.notes.was_held = held;
        pressed
    }

    /// The mode button's text: where it leads, and the score while a song goes on behind the
    /// plugin's window.
    fn mode_label(&self) -> String {
        match self.live_run() {
            Some(r) if !self.notes.on => {
                let (hit, all) = r.score();
                format!("Learn Song ▶ {hit}/{all}")
            }
            _ if self.notes.on => "Edit instrument".to_owned(),
            _ => "Learn Song".to_owned(),
        }
    }

    /// Moves the run on, every frame and whichever view is shown: the song keeps going (and
    /// playing, and judging the keys) while the plugin's window is shown to change its sound.
    fn tick_run(&mut self, ctx: &egui::Context) {
        let time = ctx.input(|i| i.time);
        let dt = self.notes.last_time.map_or(0.0, |t| (time - t).clamp(0.0, 0.25));
        self.notes.last_time = Some(time);
        let metronome = self.metronome().cloned();
        let bpm = self.bpm();
        let pressed = self.newly_pressed();

        // A run of another song (the scale or song changed) stops.
        let stale = self
            .notes
            .run
            .as_ref()
            .is_some_and(|r| Some(r.song.name.as_str()) != self.falling_name().as_deref());
        if stale {
            self.stop_run();
        }
        let mut stopped = false;
        if let Some(run) = self.notes.run.as_mut().filter(|r| !r.finished()) {
            let beat = metronome.as_ref().and_then(|m| m.beat_now());
            // Play along: the metronome switched off stops the run.
            stopped = !run.wait && metronome.as_ref().is_some_and(|m| !m.is_on());
            run.advance(beat, dt * bpm / 60.0, bpm);
            for key in pressed {
                run.press(key, bpm, time);
            }
            ctx.request_repaint();
        }
        if stopped {
            self.stop_run();
        }
        self.play_notes();
        if self.notes.run.as_ref().is_some_and(Run::finished) {
            self.end_metronome();
            self.release_notes();
        }
        if let Some(m) = self.metronome() {
            m.set_silent(self.metronome_silent());
        }
    }

    /// The top in the notes view: the controls, and the notes falling onto the keys.
    fn falling_notes(&mut self, ui: &mut egui::Ui) {
        let time = ui.input(|i| i.time);
        let bpm = self.bpm();
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(8, 4))
            .fill(ui.visuals().panel_fill)
            .show(ui, |ui| self.notes_controls(ui));

        let area = ui.available_rect_before_wrap();
        ui.allocate_rect(area, egui::Sense::hover());
        let painter = ui.painter_at(area);
        let spans = self
            .notes
            .keyboard
            .as_ref()
            .map_or_else(Vec::new, |(r, range)| keyboard::key_spans(*r, range.clone()));
        let run = self.notes.run.as_ref();
        let Some(song) = run.map(|r| Cow::Borrowed(&r.song)).or_else(|| self.falling_song()) else {
            painter.rect_filled(area, 0.0, waterfall::BACKGROUND);
            painter.text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                "Open a song: a MIDI file, or drop one here",
                egui::FontId::proportional(18.0),
                Color32::from_gray(170),
            );
            return;
        };
        let beat = run.map_or(-waterfall::COUNT_IN, Run::beat);
        waterfall::draw(&painter, area, &spans, &song, run, beat, bpm, time);
        let big = |text: String, size: f32| {
            painter.text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::proportional(size),
                Color32::from_gray(230),
            );
        };
        match run {
            None => big(
                if self.notes.wait {
                    "Space: start. The notes wait at the keys until you play them.".to_owned()
                } else if self.notes.silent && self.notes.song.is_some() {
                    "Space: start. The metronome keeps the time, unheard.".to_owned()
                } else {
                    "Space: start, with the metronome".to_owned()
                },
                20.0,
            ),
            Some(r) if r.finished() => {
                let (hit, all) = r.score();
                big(
                    format!(
                        "{hit} of {all} notes {}  ·  best streak {}\nSpace: again",
                        if r.wait { "played" } else { "on time" },
                        r.best_streak
                    ),
                    24.0,
                );
            }
            Some(r) if !r.wait && r.beat() < 0.0 => {
                #[expect(clippy::cast_possible_truncation, reason = "a count-in of a few beats")]
                #[expect(clippy::as_conversions, reason = "no as-free f64 to i64 conversion")]
                let left = (-r.beat()).ceil() as i64;
                big(left.to_string(), 72.0);
            }
            Some(_) => {}
        }
    }

    fn notes_controls(&mut self, ui: &mut egui::Ui) {
        let mut start = false;
        let mut pick = false;
        let mut close = false;
        let mut key_scale = None;
        ui.horizontal(|ui| {
            ui.set_min_width(ui.available_width());
            let running = self.notes.run.as_ref().is_some_and(|r| !r.finished());
            start = ui
                .button(if running { "■ Stop" } else { "▶ Start" })
                .on_hover_text("Space")
                .clicked();
            let mut wait = self.notes.wait;
            if ui
                .checkbox(&mut wait, "Wait for me")
                .on_hover_text("The notes stop at the keys until you play them, with no metronome")
                .changed()
            {
                self.notes.wait = wait;
                self.stop_run();
            }
            let mut play = self.notes.play;
            if ui
                .checkbox(&mut play, "Play the notes")
                .on_hover_text(
                    "While it runs (Start or Space), the app plays the falling notes through the instrument, \
                     to listen along",
                )
                .changed()
            {
                self.notes.play = play;
                if !play {
                    self.release_notes();
                }
            }
            ui.separator();
            if let Some((song, path)) = &self.notes.song {
                ui.label(RichText::new(format!("Song: {}", song.name)).strong())
                    .on_hover_text(path.display().to_string());
                // The key, when the file says (notation programs write it; recordings often not). A
                // click shows its scale's fingering on the keys, another click none.
                if let Some(k) = song.key {
                    let mut shown = self.preview == Preview::Scale(k.scale());
                    if toggle(ui, &mut shown, k.name())
                        .on_hover_text(
                            "The key the file gives (its first key signature). Click to show its scale's \
                             fingering on the keys; again for none.",
                        )
                        .clicked()
                    {
                        key_scale = Some(if shown { Preview::Scale(k.scale()) } else { Preview::Off });
                    }
                }
                close = ui.button("Close").on_hover_text("Close the song").clicked();
            }
            pick = ui
                .button("Open song…")
                .on_hover_text("A MIDI file (.mid); or drop one on the window")
                .clicked();
            if let Some(note) = &self.notes.song_note {
                ui.label(RichText::new(note).color(Color32::from_rgb(239, 68, 68)));
            }
            if let Some(r) = &self.notes.run {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (hit, all) = r.score();
                    ui.label(RichText::new(format!("{hit} / {all}   streak {}", r.streak)).monospace());
                });
            }
        });
        if start {
            self.toggle_run();
        }
        if pick {
            self.pick_song(ui.ctx());
        }
        if close {
            self.stop_run();
            self.notes.song = None;
        }
        if let Some(p) = key_scale {
            self.preview = p;
        }
    }
}

/// Reads a MIDI file as a song, named after the file.
fn read_song(path: &std::path::Path) -> Result<Song, String> {
    let name = path
        .file_stem()
        .map_or_else(|| "song".to_owned(), |s| s.to_string_lossy().into_owned());
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Song::from_midi(&bytes, &name).map_err(|e| format!("{name}: {e}"))
}

/// What the keyboard shows on each key: the scale shown, and over it the chord held.
struct KeyLooks<'a> {
    scale: &'a Shown,
    /// The held chord's fingering (empty when the keys make no chord, or nothing is agreed).
    chord: Vec<(u8, Hand, u8)>,
}

impl<'a> KeyLooks<'a> {
    fn new(scale: &'a Shown, held: &[u8]) -> Self {
        let chord = if matches!(chords::read(held), Some(Reading::Chord(_))) {
            chords::fingering(held)
        } else {
            Vec::new()
        };
        Self { scale, chord }
    }

    /// `key`, struck with `velocity` (`None`: up). A key with a chord finger shows the chord's
    /// fingers (outlined) in place of the scale's; every other key shows the scale's.
    fn look(&self, key: u8, velocity: Option<u8>) -> KeyLook {
        let mut fingers = self.chord.iter().filter(|&&(k, _, _)| k == key).peekable();
        let chord = fingers.peek().is_some();
        let (left, right) = if chord {
            fingers.fold((None, None), |(l, r), &(_, hand, finger)| match hand {
                Hand::Left => (Some(finger), r),
                Hand::Right => (l, Some(finger)),
            })
        } else {
            self.scale.fingers(key)
        };
        KeyLook {
            velocity,
            in_scale: self.scale.in_scale(key),
            left,
            right,
            chord,
        }
    }
}

/// How hard a key played with the mouse is struck: full velocity.
const MOUSE_VELOCITY: u8 = 127;

/// The sustain pedal pressed with the mouse: a click presses it, the next click releases it,
/// and so does releasing the keyboard's own pedal. The light shows one pedal state for both
/// ([`Keys::sustain`]); this only remembers that the mouse pressed it, so that a release on the
/// keyboard also releases what the mouse sent (on channel 0, like the mouse's notes).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct MousePedal {
    pressed: bool,
}

impl MousePedal {
    /// A click on the light, with the pedal as shown now: what to play.
    const fn click(&mut self, down: bool) -> MidiEvent {
        self.pressed = !down;
        pedal_event(!down)
    }

    /// The pedal as shown now; a release (from the keyboard, or the keyboard unplugged) while
    /// the mouse holds the pedal gives the release to play for the mouse's channel.
    fn follow(&mut self, down: bool) -> Option<MidiEvent> {
        (self.pressed && !down).then(|| {
            self.pressed = false;
            pedal_event(false)
        })
    }
}

/// A note the app plays itself (the mouse's, a song's): on channel 0, like the mouse pedal.
const fn note_on(key: u8, velocity: u8) -> MidiEvent {
    MidiEvent::NoteOn {
        channel: 0,
        key,
        velocity,
    }
}

const fn note_off(key: u8) -> MidiEvent {
    MidiEvent::NoteOff {
        channel: 0,
        key,
        velocity: 64,
    }
}

const fn pedal_event(down: bool) -> MidiEvent {
    MidiEvent::ControlChange {
        channel: 0,
        controller: 64,
        value: if down { 127 } else { 0 },
    }
}

/// Starts the instrument, keyboard and audio with `opts`; the bar's MIDI state follows the
/// keyboard. Prints the startup report. The error as text when nothing could be started.
fn start_session(
    ctx: &egui::Context,
    opts: &Options,
    keys: &Arc<Keys>,
    midi: &Arc<Mutex<MidiState>>,
) -> Result<Session, String> {
    let (shared, repaint) = (Arc::clone(midi), ctx.clone());
    let on_midi = move |c: MidiChange| {
        println!("{:<10}{}", "MIDI in", crate::report::midi_change(&c));
        *shared.lock().unwrap_or_else(PoisonError::into_inner) = midi_state(&c);
        repaint.request_repaint();
    };
    match Session::start(opts, Vec::new(), Arc::clone(keys), on_midi) {
        Ok(s) => {
            crate::report::print_report(&s);
            *midi.lock().unwrap_or_else(PoisonError::into_inner) =
                s.first_midi.as_ref().map_or(MidiState::Waiting, midi_state);
            Ok(s)
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            Err(format!("{e:#}"))
        }
    }
}

/// The instrument's own editor, to embed at the next frame; or why there is none.
fn make_editor(ctx: &egui::Context, session: &Session) -> (Option<Editor>, Option<String>) {
    let Some(plugin) = session.instrument().plugin() else {
        return (None, None);
    };
    match plugin.editor() {
        Ok(e) => {
            let repaint = ctx.clone();
            e.set_waker(move || repaint.request_repaint());
            (Some(e), None)
        }
        Err(e) => (None, Some(e.to_string())),
    }
}

/// Gives a newly started built-in piano its volume (a plugin has its own).
fn apply_piano_volume(session: Option<&Session>, volume: f32) {
    if let Some(p) = session.and_then(|s| s.instrument().piano()) {
        p.set_volume(volume);
    }
}

/// Sets the metronome from saved settings; the tempo only when `tempo` (not given on the
/// command line). A sound that no longer exists keeps the default.
fn apply_metronome(m: &Metronome, s: &MetronomeSettings, tempo: bool) {
    if let Some(bpm) = s.bpm.filter(|_| tempo) {
        m.set_bpm(bpm);
    }
    if let Some(v) = s.volume {
        m.set_volume(v);
    }
    if let Some(c) = s
        .sound
        .as_deref()
        .and_then(|n| Click::ALL.into_iter().find(|c| c.name() == n))
    {
        m.set_click(c);
    }
}

/// The metronome's controls, right to left (the bar's right side is laid out that way): Tap,
/// the tempo, then the switch. Its sound and volume are in the options window.
///
/// In the notes view (`silent` given) the button switches the click on and off instead: playing
/// along keeps the metronome's time whether it is heard or not.
fn metronome_controls(ui: &mut egui::Ui, m: &Metronome, tap: &mut TapTempo, silent: Option<&mut bool>) {
    if ui
        .button("Tap")
        .on_hover_text("Tap the tempo: click in time, or press T (a pause of 3 s starts over)")
        .clicked()
    {
        tap_tempo(ui.ctx(), m, tap);
    }
    let mut bpm = m.bpm();
    let r = ui
        .add(egui::DragValue::new(&mut bpm).range(MIN_BPM..=MAX_BPM).suffix(" BPM"))
        .on_hover_text("Tempo: drag, scroll or type");
    if r.changed() {
        m.set_bpm(bpm);
    }
    let on = silent.as_deref().map_or_else(|| m.is_on(), |s| !*s);
    let hover = if silent.is_some() {
        "Click: heard or not while playing along (the notes keep the metronome's time either way)"
    } else {
        "Metronome: start or stop (Space)"
    };
    let r = ui
        .add(egui::Button::new("").selected(on).min_size(egui::vec2(34.0, 24.0)))
        .on_hover_text(hover);
    let colour = ui.style().interact_selectable(&r, on).fg_stroke.color;
    metronome_icon(ui.painter(), r.rect, colour);
    if r.clicked() {
        match silent {
            Some(s) => *s = !*s,
            None => m.set_on(!on),
        }
    }
}

/// One tap, now: sets the tempo once there are two taps in a row.
fn tap_tempo(ctx: &egui::Context, m: &Metronome, tap: &mut TapTempo) {
    if let Some(bpm) = tap.tap(ctx.input(|i| i.time)) {
        m.set_bpm(bpm);
    }
}

/// Tap tempo: the beats per minute of the last few taps.
#[derive(Debug, Default)]
struct TapTempo {
    /// The recent taps, in seconds, oldest first; at most [`TAP_COUNT`].
    taps: Vec<f64>,
}

/// How many taps the tempo is averaged over (the intervals between them: one fewer).
const TAP_COUNT: usize = 5;

/// A pause longer than this starts a new count. Above a beat at [`MIN_BPM`] (3 s).
const TAP_RESET: f64 = 3.2;

impl TapTempo {
    /// A tap at `time` (seconds, on any steady clock): the tempo from the taps so far, from the
    /// second tap of a count on. Clamped to [`MIN_BPM`]..=[`MAX_BPM`].
    fn tap(&mut self, time: f64) -> Option<u32> {
        if self
            .taps
            .last()
            .is_some_and(|&last| !(0.0..=TAP_RESET).contains(&(time - last)))
        {
            self.taps.clear();
        }
        if self.taps.len() == TAP_COUNT {
            self.taps.remove(0);
        }
        self.taps.push(time);
        let (first, last) = (*self.taps.first()?, *self.taps.last()?);
        let intervals = u32::try_from(self.taps.len().saturating_sub(1))
            .ok()
            .filter(|&n| n > 0)?;
        let beat = (last - first) / f64::from(intervals);
        let bpm = (60.0 / beat).round().clamp(f64::from(MIN_BPM), f64::from(MAX_BPM));
        // Clamped to a range u32 holds, so the conversion is exact.
        (MIN_BPM..=MAX_BPM).find(|&b| f64::from(b) >= bpm)
    }
}

/// The bar's switch between the instrument and the falling notes: filled in the accent colour,
/// with an icon for where it leads (falling notes, or an instrument's sliders), so it stands
/// out from the other buttons.
fn mode_button(ui: &mut egui::Ui, notes_on: bool, label: &str) -> egui::Response {
    const ACCENT: Color32 = Color32::from_rgb(124, 58, 237);
    const ACCENT_HOVER: Color32 = Color32::from_rgb(139, 92, 246);
    let font = egui::FontId::proportional(14.0);
    let text = ui.painter().layout_no_wrap(label.to_owned(), font, Color32::WHITE);
    let (icon, pad, h): (f32, f32, f32) = (15.0, 9.0, 24.0);
    let size = egui::vec2(pad.mul_add(2.0, text.size().x + icon + 6.0), h);
    let (rect, r) = ui.allocate_exact_size(size, egui::Sense::click());
    let fill = if r.hovered() { ACCENT_HOVER } else { ACCENT };
    let painter = ui.painter();
    painter.rect_filled(rect, 6.0, fill);
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(icon.mul_add(0.5, rect.left() + pad), rect.center().y),
        egui::vec2(icon, icon),
    );
    if notes_on {
        sliders_icon(painter, icon_rect, Color32::WHITE);
    } else {
        notes_icon(painter, icon_rect, Color32::WHITE);
    }
    let at = egui::pos2(icon_rect.right() + 6.0, rect.center().y - text.size().y / 2.0);
    painter.galley(at, text, Color32::WHITE);
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Notes falling onto a line: three bars at different heights over a base line.
fn notes_icon(painter: &egui::Painter, rect: egui::Rect, colour: Color32) {
    let (w, h) = (rect.width(), rect.height());
    let bar = |x: f32, top: f32, bottom: f32| {
        egui::Rect::from_min_max(
            egui::pos2(w.mul_add(x, rect.left()), h.mul_add(top, rect.top())),
            egui::pos2(w.mul_add(x + 0.22, rect.left()), h.mul_add(bottom, rect.top())),
        )
    };
    for (x, top, bottom) in [(0.04, 0.3, 0.8), (0.39, 0.05, 0.45), (0.74, 0.0, 0.3)] {
        painter.rect_filled(bar(x, top, bottom), 1.5, colour);
    }
    painter.hline(rect.x_range(), rect.bottom() - 0.5, egui::Stroke::new(1.6, colour));
}

/// An instrument's controls: three sliders with their knobs at different heights.
fn sliders_icon(painter: &egui::Painter, rect: egui::Rect, colour: Color32) {
    let (w, h) = (rect.width(), rect.height());
    for (x, knob) in [(0.15, 0.3), (0.5, 0.7), (0.85, 0.45)] {
        let x = w.mul_add(x, rect.left());
        painter.vline(x, rect.y_range(), egui::Stroke::new(1.4, colour));
        painter.circle_filled(egui::pos2(x, h.mul_add(knob, rect.top())), w * 0.13, colour);
    }
}

/// A stylised upright piano in `colour`: outlines, with faint fills, on no background.
fn upright_piano(painter: &egui::Painter, rect: egui::Rect, colour: Color32) {
    let at = |x: f32, y: f32| {
        egui::pos2(
            rect.width().mul_add(x, rect.left()),
            rect.height().mul_add(y, rect.top()),
        )
    };
    let area = |x0, y0, x1, y1| egui::Rect::from_min_max(at(x0, y0), at(x1, y1));
    let line = egui::Stroke::new(1.5, colour);
    let thin = egui::Stroke::new(1.0, colour.gamma_multiply(0.7));
    let face = colour.gamma_multiply(0.12);
    let shape = |r: egui::Rect, radius: f32, fill: Color32, stroke: egui::Stroke| {
        painter.rect(r, radius, fill, stroke, egui::StrokeKind::Middle);
    };
    // The lid, the upper case with its front panel, the lower case with its panel.
    shape(area(0.04, 0.0, 0.96, 0.05), 2.0, colour.gamma_multiply(0.3), line);
    shape(area(0.08, 0.05, 0.92, 0.52), 2.0, face, line);
    shape(area(0.17, 0.12, 0.83, 0.44), 3.0, Color32::TRANSPARENT, thin);
    shape(area(0.08, 0.7, 0.92, 0.94), 2.0, face, line);
    shape(area(0.17, 0.74, 0.83, 0.88), 3.0, Color32::TRANSPARENT, thin);
    // The keyboard between the cheeks: three octaves of white keys, black keys over them.
    shape(area(0.02, 0.52, 0.08, 0.7), 1.5, colour.gamma_multiply(0.3), line);
    shape(area(0.92, 0.52, 0.98, 0.7), 1.5, colour.gamma_multiply(0.3), line);
    let keys = area(0.08, 0.55, 0.92, 0.68);
    shape(area(0.08, 0.52, 0.92, 0.55), 0.0, colour.gamma_multiply(0.3), line);
    shape(keys, 0.0, Color32::TRANSPARENT, line);
    let whites = 21u8;
    let white_w = keys.width() / f32::from(whites);
    for i in 1..whites {
        let x = white_w.mul_add(f32::from(i), keys.left());
        painter.vline(x, keys.y_range(), thin);
    }
    for i in 1..whites {
        // A black key after C, D, F, G and A: on the line before white key `i`.
        if matches!(i % 7, 1 | 2 | 4 | 5 | 6) {
            let x = white_w.mul_add(f32::from(i), keys.left());
            let black = egui::Rect::from_min_max(
                egui::pos2(white_w.mul_add(-0.3, x), keys.top()),
                egui::pos2(white_w.mul_add(0.3, x), keys.height().mul_add(0.6, keys.top())),
            );
            painter.rect_filled(black, 0.5, colour);
        }
    }
    // Two pedals, and the feet.
    for x in [0.43, 0.52] {
        shape(area(x, 0.9, x + 0.05, 0.93), 1.0, colour, egui::Stroke::NONE);
    }
    for x in [0.06, 0.86] {
        shape(area(x, 0.94, x + 0.08, 1.0), 1.0, colour.gamma_multiply(0.3), line);
    }
}

/// A metronome drawn in `rect`: a tapering body with a pendulum leaning right and its weight.
fn metronome_icon(painter: &egui::Painter, rect: egui::Rect, colour: Color32) {
    let s = rect.height().min(rect.width()) * 0.72;
    let c = rect.center();
    let at = |x: f32, y: f32| egui::pos2(x.mul_add(s, c.x), y.mul_add(s, c.y));
    let stroke = egui::Stroke::new((s * 0.09).max(1.2), colour);
    // The body, wide at the bottom; its base slightly below the centre line.
    let body = vec![at(-0.34, 0.45), at(-0.14, -0.45), at(0.14, -0.45), at(0.34, 0.45)];
    painter.add(egui::Shape::closed_line(body, stroke));
    // The pendulum from its pivot near the base, leaning right, with the sliding weight.
    let pivot = at(0.0, 0.3);
    let tip = at(0.3, -0.5);
    painter.line_segment([pivot, tip], stroke);
    painter.circle_filled(pivot.lerp(tip, 0.55), s * 0.09, colour);
}

/// A volume slider from 0 to `max` (1.0 = 100 %), shown and typed as a percentage.
fn percent_slider(value: &mut f32, max: f32) -> egui::Slider<'_> {
    egui::Slider::new(value, 0.0..=max)
        .custom_formatter(|v, _| format!("{:.0} %", v * 100.0))
        .custom_parser(|t| t.trim_end_matches(['%', ' ']).parse::<f64>().ok().map(|v| v / 100.0))
}

/// The metronome's sound and volume. Changing either plays a sample click.
fn metronome_options(ui: &mut egui::Ui, m: &Metronome) {
    egui::Grid::new("metronome options")
        .num_columns(2)
        .spacing([16.0, 10.0])
        .show(ui, |ui| {
            ui.label("Volume");
            let mut volume = m.volume();
            let r = ui.add(percent_slider(&mut volume, 1.0));
            if r.changed() {
                m.set_volume(volume);
            }
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                m.play_sample();
            }
            ui.end_row();

            ui.label("Sound");
            ui.horizontal(|ui| {
                let mut click = m.click();
                for c in Click::ALL {
                    if ui.radio_value(&mut click, c, c.name()).clicked() {
                        m.set_click(c);
                        m.play_sample();
                    }
                }
            });
            ui.end_row();
        });
}

/// A button that stays pressed while `on` (egui's `toggle_value` has no frame until hovered,
/// so it does not look like a button).
fn toggle(ui: &mut egui::Ui, on: &mut bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    let r = ui.add(egui::Button::new(text).selected(*on));
    if r.clicked() {
        *on = !*on;
    }
    r
}

/// How often the scales window reads the practice history again.
const PRACTICE_REFRESH: Duration = Duration::from_secs(3);

/// How the scales window lists the scales.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScaleOrder {
    /// By type, then C, G, D, A, E, B, F, Db, Gb, Bb, Eb, Ab.
    Learning,
    /// Alphabetical: "A harmonic minor", "A major", ...
    Name,
}

impl ScaleOrder {
    const ALL: [Self; 2] = [Self::Learning, Self::Name];

    const fn name(self) -> &'static str {
        match self {
            Self::Learning => "Learning order",
            Self::Name => "Name",
        }
    }
}

/// A star to click: filled for a favorite. Returns whether it was clicked.
fn star(ui: &mut egui::Ui, is_fav: bool) -> bool {
    let (text, tip) = if is_fav {
        (
            RichText::new("★").color(Color32::from_rgb(250, 204, 21)),
            "Remove from favorites",
        )
    } else {
        (RichText::new("☆").weak(), "Add to favorites")
    };
    ui.add(egui::Button::new(text).frame(false))
        .on_hover_text(tip)
        .clicked()
}

/// Adds `id` to `favorites`, or removes it if it is there.
fn toggle_favorite(favorites: &mut Vec<String>, id: &str) {
    if let Some(i) = favorites.iter().position(|f| f == id) {
        favorites.remove(i);
    } else {
        favorites.push(id.to_owned());
    }
}

/// The scales in `order`, starred ones first, as a table: star, name, time spent, last
/// played. Clicking a name shows it; clicking the star (un)marks it as a favorite.
fn scale_list(
    ui: &mut egui::Ui,
    preview: &mut Preview,
    order: ScaleOrder,
    practice: &HashMap<String, Practice>,
    favorites: &mut Vec<String>,
) {
    let mut scales: Vec<(Scale, String, Practice)> = Scale::all()
        .map(|s| {
            let name = s.name();
            let p = practice.get(&name).copied().unwrap_or_default();
            (s, name, p)
        })
        .collect();
    match order {
        ScaleOrder::Learning => {}
        ScaleOrder::Name => scales.sort_by(|a, b| a.1.cmp(&b.1)),
    }
    let (starred, rest): (Vec<_>, Vec<_>) = scales.into_iter().partition(|(_, name, _)| favorites.contains(name));
    let grouped = !starred.is_empty();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok());
    let mut toggle = None;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        egui::Grid::new("scales")
            .num_columns(5)
            .striped(true)
            .spacing([18.0, 6.0])
            .show(ui, |ui| {
                ui.label("");
                ui.label(RichText::new("Key").strong());
                ui.label(RichText::new("Scale").strong());
                ui.label(RichText::new("Time spent").strong());
                ui.label(RichText::new("Last played").strong());
                ui.end_row();
                for (title, list, is_fav) in [("Favorites", starred, true), ("All scales", rest, false)] {
                    if list.is_empty() {
                        continue;
                    }
                    if grouped {
                        ui.label("");
                        ui.label("");
                        ui.label(RichText::new(title).weak());
                        ui.end_row();
                    }
                    for (scale, name, p) in list {
                        if star(ui, is_fav) {
                            toggle = Some(name.clone());
                        }
                        crate::staff::key_signature(ui, scale.key_signature())
                            .on_hover_text(signature_name(scale.key_signature()));
                        let selected = *preview == Preview::Scale(scale);
                        if ui.selectable_label(selected, &name).clicked() {
                            *preview = Preview::Scale(scale);
                        }
                        ui.label(spent(p.time));
                        ui.label(
                            p.last_ms
                                .zip(now)
                                .map_or_else(|| "–".to_owned(), |(t, n)| ago(n.saturating_sub(t))),
                        );
                        ui.end_row();
                    }
                }
            });
    });
    if let Some(name) = toggle {
        toggle_favorite(favorites, &name);
    }
}

/// "No sharps or flats", "1 sharp", "3 flats".
fn signature_name(sig: i8) -> String {
    let n = sig.unsigned_abs();
    let kind = if sig > 0 { "sharp" } else { "flat" };
    match n {
        0 => "No sharps or flats".to_owned(),
        1 => format!("1 {kind}"),
        _ => format!("{n} {kind}s"),
    }
}

/// "45 s", "12 min", "1 h 05 min"; "–" for none.
fn spent(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0 => "–".to_owned(),
        1..60 => format!("{s} s"),
        60..3600 => format!("{} min", s / 60),
        _ => format!("{} h {:02} min", s / 3600, s % 3600 / 60),
    }
}

/// How long ago, from milliseconds: "just now", "5 min ago", "3 h ago", "2 days ago".
fn ago(ms: i64) -> String {
    let min = ms / 60_000;
    match min {
        ..1 => "just now".to_owned(),
        1..60 => format!("{min} min ago"),
        60..1440 => format!("{} h ago", min / 60),
        1440..2880 => "yesterday".to_owned(),
        _ => format!("{} days ago", min / 1440),
    }
}

/// The theme saved as `name`: "light", or else dark (the default).
fn theme_from_name(name: Option<&str>) -> egui::ThemePreference {
    if name == Some("light") {
        egui::ThemePreference::Light
    } else {
        egui::ThemePreference::Dark
    }
}

/// The name a theme is saved under; `None` for the default, dark.
const fn theme_name(theme: egui::ThemePreference) -> Option<&'static str> {
    match theme {
        egui::ThemePreference::Light => Some("light"),
        egui::ThemePreference::Dark | egui::ThemePreference::System => None,
    }
}

/// Buttons that look like buttons: a fill lighter than the panel, a visible border, a brighter
/// hover and a darker press.
fn style_buttons(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        let dark = style.visuals.dark_mode;
        let w = &mut style.visuals.widgets;
        let gray = Color32::from_gray;
        let (fill, edge, hover_fill, hover_edge, press_fill) = if dark {
            (gray(64), gray(112), gray(80), gray(150), gray(46))
        } else {
            (gray(232), gray(150), gray(244), gray(110), gray(212))
        };
        w.inactive.weak_bg_fill = fill;
        w.inactive.bg_stroke = egui::Stroke::new(1.0, edge);
        w.hovered.weak_bg_fill = hover_fill;
        w.hovered.bg_stroke = egui::Stroke::new(1.0, hover_edge);
        w.active.weak_bg_fill = press_fill;
        w.active.bg_stroke = egui::Stroke::new(1.0, hover_edge);
        for v in [&mut w.inactive, &mut w.hovered, &mut w.active] {
            v.corner_radius = egui::CornerRadius::same(4);
        }
    });
}

/// How many characters of a device or plugin name the bar shows.
const NAME_CHARS: usize = 24;

/// `name` cut to [`NAME_CHARS`], with "…" when it was longer (the full name is in the hover
/// report).
fn shorten(name: &str) -> String {
    if name.chars().count() <= NAME_CHARS {
        return name.to_string();
    }
    let mut s: String = name.chars().take(NAME_CHARS.saturating_sub(1)).collect();
    s.push('…');
    s
}

/// The bar's "Pedal ●" item, lit in `colour`; it can be clicked. Returns whether it was.
fn pedal_item(ui: &mut egui::Ui, colour: Color32) -> bool {
    let r = ui
        .horizontal(|ui| {
            ui.label(RichText::new("Pedal").weak());
            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 4.0, colour);
        })
        .response;
    let clicked = ui
        .interact(r.rect, ui.id().with("pedal"), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Click to hold the sustain pedal down; click again, or release the real pedal, to let go")
        .clicked();
    ui.separator();
    clicked
}

/// One item of the bar that opens a menu: "Label ● value" with the value as a button, the
/// report's reasons on hover. Returns the button's response, for the menu.
fn menu_item(
    ui: &mut egui::Ui,
    label: &str,
    value: &str,
    dot: Option<Color32>,
    why: Option<&Section>,
) -> egui::Response {
    ui.label(RichText::new(label).weak());
    if let Some(c) = dot {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, c);
    }
    let text = if value.is_empty() { "…" } else { value };
    let r = ui.button(format!("{text} ⏷"));
    let r = match why {
        Some(s) => r.on_hover_text(format!("Click to choose.\n\n{}", explain(s))),
        None => r.on_hover_text("Click to choose."),
    };
    ui.separator();
    r
}

/// The highest a menu of the bar gets before it scrolls: it opens over the keyboard, below the
/// plugin's editor (which would cover it).
const MENU_HEIGHT: f32 = 170.0;

/// The MIDI menu: automatic, or one of `ports`.
fn midi_menu(
    ui: &mut egui::Ui,
    ports: &[String],
    connected: Option<&str>,
    chosen: Option<&str>,
    pick: &mut Option<Pick>,
) {
    if ui
        .selectable_label(chosen.is_none(), "Automatic: the first keyboard found")
        .clicked()
    {
        *pick = Some(Pick::Midi(None));
    }
    ui.separator();
    if ports.is_empty() {
        ui.label(RichText::new("No MIDI inputs: plug in a keyboard").weak());
    }
    egui::ScrollArea::vertical().max_height(MENU_HEIGHT).show(ui, |ui| {
        for p in ports {
            if ui.selectable_label(connected == Some(p.as_str()), p).clicked() {
                *pick = Some(Pick::Midi(Some(port_name(p).to_owned())));
            }
        }
    });
}

/// The audio menu: automatic, or one of `outputs`, grouped by audio system.
fn output_menu(ui: &mut egui::Ui, outputs: &[Output], session: &Session, note: Option<&str>, pick: &mut Option<Pick>) {
    if let Some(n) = note {
        ui.label(RichText::new(n).color(Color32::from_rgb(239, 68, 68)));
        ui.separator();
    }
    let chosen = session.chosen_output();
    if ui
        .selectable_label(chosen.is_none(), "Automatic: the lowest latency this system offers")
        .clicked()
    {
        *pick = Some(Pick::Output(None));
    }
    ui.separator();
    // Under the automatic choice too: it shows what was picked.
    let in_use = session.output();
    egui::ScrollArea::vertical().max_height(MENU_HEIGHT).show(ui, |ui| {
        let mut backend = "";
        for o in outputs {
            if o.backend != backend {
                backend = &o.backend;
                ui.label(RichText::new(backend).weak());
            }
            if ui.selectable_label(in_use.as_ref() == Some(o), &o.device).clicked() {
                *pick = Some(Pick::Output(Some(o.clone())));
            }
        }
    });
}

/// The MIDI timing menu, each mode with what it does.
fn timing_menu(ui: &mut egui::Ui, now: Option<MidiTiming>, pick: &mut Option<Pick>) {
    for t in MidiTiming::ALL {
        let what = match t {
            MidiTiming::Auto => "Auto: exact when blocks are short, else immediate",
            MidiTiming::Exact => "Exact: notes keep their spacing, one block later",
            MidiTiming::Immediate => "Immediate: no extra delay, but uneven",
        };
        if ui.selectable_label(now == Some(t), what).clicked() {
            *pick = Some(Pick::Timing(t));
        }
    }
}

/// The report section as hover text: what is used and why, and the alternatives.
fn explain(s: &Section) -> String {
    let lines = s.considered.iter().map(|c| match c.verdict {
        Verdict::Used => format!("✔ {}\n    {}", c.name, c.why),
        Verdict::NotUsed => format!("· {}\n    not used: {}", c.name, c.why),
        Verdict::Unavailable => format!("· {}\n    unavailable: {}", c.name, c.why),
    });
    let mut text: Vec<String> = lines.collect();
    if text.is_empty() {
        text.push("nothing found".into());
    }
    if !s.hint.is_empty() {
        text.push(s.hint.to_string());
    }
    text.join("\n")
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Space starts and stops the metronome, unless a text field has the keyboard. With the
        // falling notes shown, or a song going on, it starts and stops the song.
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) && !ctx.egui_wants_keyboard_input() {
            if self.notes.on || self.running() {
                self.toggle_run();
            } else if let Some(m) = self.metronome() {
                m.set_on(!m.is_on());
            }
            ctx.request_repaint();
        }
        // T taps the tempo, likewise.
        if ctx.input(|i| i.key_pressed(egui::Key::T))
            && !ctx.egui_wants_keyboard_input()
            && let Some(m) = self.session.as_ref().map(Session::metronome)
        {
            tap_tempo(ctx, m, &mut self.tap);
            ctx.request_repaint();
        }
        self.keep_settings(ctx);
        if ctx.input(|i| i.viewport().close_requested()) {
            // The editor goes while its window is still there.
            drop(self.embedded.borrow_mut().take());
        }
        if let Some(e) = self.embedded.borrow_mut().as_mut()
            && e.follow_requests()
        {
            ctx.request_repaint();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if self.open.options {
            self.options_window(ui.ctx());
        }
        if self.open.scales {
            self.scales_window(ui.ctx());
        }
        if self.open.instruments {
            self.instruments_window(ui.ctx());
        }
        self.record_scale();
        self.follow_pedal();
        self.take_song(ui.ctx());
        self.tick_run(ui.ctx());
        if let Some(e) = self.embedded.borrow_mut().as_mut() {
            e.set_hidden(self.notes.on);
        }
        // Bottom panels stack upwards: the keyboard at the very bottom, the chord line, the bar.
        egui::Panel::bottom("keyboard")
            .exact_size(200.0)
            .show(ui, |ui| self.keyboard(ui));
        if self.notes.on {
            // The notes fall onto the keys, so the bar and the chord line go to the top. With
            // the editor hidden, the bar's menus can open over the notes.
            egui::Panel::top("bar").exact_size(40.0).show(ui, |ui| self.bar(ui));
            egui::Panel::top("chord")
                .exact_size(36.0)
                .show(ui, |ui| self.chord_line(ui));
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| self.falling_notes(ui));
            return;
        }
        egui::Panel::bottom("chord")
            .exact_size(36.0)
            .show(ui, |ui| self.chord_line(ui));
        egui::Panel::bottom("bar").exact_size(40.0).show(ui, |ui| self.bar(ui));
        let fill = ui.visuals().panel_fill;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(fill))
            .show(ui, |ui| self.instrument(ui, frame));
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let settings = self.settings();
        self.store.save(&settings);
        // The editor is removed before its window and its plugin go.
        drop(self.embedded.borrow_mut().take());
        drop(self.editor.take());
        if let Some(s) = self.session.take() {
            s.save_sound();
            s.stop();
        }
        // Writes the last notes.
        drop(self.recorder.take());
    }
}

/// The window's native id, for the editor's parent: the X11 window on Linux, the `HWND` on Windows.
fn window_id(frame: &eframe::Frame) -> Result<u64, String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = frame.window_handle().map_err(|e| format!("no window handle: {e}"))?;
    match handle.as_raw() {
        #[allow(clippy::useless_conversion, reason = "c_ulong is u32 on 32-bit systems")]
        RawWindowHandle::Xlib(h) => Ok(u64::from(h.window)),
        RawWindowHandle::Xcb(h) => Ok(u64::from(h.window.get())),
        RawWindowHandle::Win32(h) => u64::try_from(h.hwnd.get()).map_err(|_| "bad window handle".into()),
        _ => Err("the window is not an X11 or Win32 window".into()),
    }
}

/// A window coordinate in pixels as an integer. There is no `as`-free float-to-int conversion.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "clamped to a range i32 holds; the fraction is meant to go"
)]
const fn to_i32(v: f32) -> i32 {
    v.clamp(-1.0e9, 1.0e9) as i32
}

fn to_u32(v: f32) -> u32 {
    u32::try_from(to_i32(v)).unwrap_or(0)
}

fn to_u16(n: u32) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tap_tempo_averages_the_last_taps_and_starts_over_after_a_pause() {
        let mut t = TapTempo::default();
        assert_eq!(t.tap(10.0), None, "one tap is no tempo yet");
        assert_eq!(t.tap(10.5), Some(120));
        assert_eq!(t.tap(11.0), Some(120));
        // Uneven taps: the average beat, (11.7 - 10.0) / 3 = 0.567 s.
        assert_eq!(t.tap(11.7), Some(106));
        assert_eq!(t.tap(12.1), Some(114), "(12.1 - 10.0) / 4 = 0.525 s");
        // Only the last TAP_COUNT taps count: the oldest drop out.
        assert_eq!(t.tap(12.5), Some(120), "(12.5 - 10.5) / 4 = 0.5 s");
        assert_eq!(t.tap(12.9), Some(126), "(12.9 - 11.0) / 4 = 0.475 s");
        assert_eq!(t.tap(13.3), Some(150), "(13.3 - 11.7) / 4 = 0.4 s");
        // A long pause: a new count, so no tempo from the first tap after it.
        assert_eq!(t.tap(20.0), None);
        assert_eq!(t.tap(21.0), Some(60));
        // Clamped to the metronome's range.
        let mut fast = TapTempo::default();
        fast.tap(0.0);
        assert_eq!(fast.tap(0.05), Some(MAX_BPM));
        let mut slow = TapTempo::default();
        slow.tap(0.0);
        assert_eq!(slow.tap(3.1), Some(MIN_BPM), "19.4 BPM");
    }

    #[test]
    fn the_mouse_pedal_toggles_and_lets_go_with_the_real_pedal() {
        let mut p = MousePedal::default();
        let value = |e| match e {
            MidiEvent::ControlChange {
                channel: 0,
                controller: 64,
                value,
            } => value,
            other => panic!("not a pedal event: {other:?}"),
        };
        // Click: down. Click again: up.
        assert_eq!(value(p.click(false)), 127);
        assert_eq!(p.follow(true), None, "held while the light is on");
        assert_eq!(value(p.click(true)), 0);
        assert_eq!(p.follow(false), None, "nothing more to release");
        // Down by mouse, then the real pedal goes up: released for the mouse's channel too.
        p.click(false);
        assert_eq!(p.follow(false).map(value), Some(0));
        assert_eq!(p.follow(false), None, "released once");
        // The real pedal down: a click releases it.
        assert_eq!(value(p.click(true)), 0);
        assert_eq!(p.follow(false), None);
    }

    fn c_major() -> Shown {
        Preview::from_name("C major").unwrap().shown()
    }

    #[test]
    fn keys_show_the_scale_when_no_chord_is_held() {
        let scale = c_major();
        let looks = KeyLooks::new(&scale, &[]);
        let c = looks.look(60, None);
        assert!(c.in_scale && !c.chord);
        assert_eq!((c.left, c.right), scale.fingers(60));
        assert_ne!((c.left, c.right), (None, None), "C4 has a finger in C major");
        let c_sharp = looks.look(61, None);
        assert!(!c_sharp.in_scale && (c_sharp.left, c_sharp.right) == (None, None));
    }

    #[test]
    fn a_held_chord_shows_its_fingering_on_its_keys_only() {
        let scale = c_major();
        let looks = KeyLooks::new(&scale, &[60, 64, 67]);
        // C E G in root position: both hands are agreed.
        for (key, left, right) in [(60, 5, 1), (64, 3, 3), (67, 1, 5)] {
            let l = looks.look(key, Some(100));
            assert!(l.chord, "key {key}");
            assert_eq!((l.left, l.right), (Some(left), Some(right)), "key {key}");
        }
        // D, not held: the scale's fingers, filled.
        let d = looks.look(62, None);
        assert!(!d.chord);
        assert_eq!((d.left, d.right), scale.fingers(62));
    }

    #[test]
    fn a_chord_agreed_for_one_hand_hides_the_scales_other_hand_on_its_keys() {
        let scale = c_major();
        assert!(scale.fingers(72).1.is_some(), "C5 has a right-hand finger in the scale");
        // G C E, second inversion: only the left hand's 5 2 1 is agreed.
        let looks = KeyLooks::new(&scale, &[67, 72, 76]);
        let c5 = looks.look(72, Some(100));
        assert!(c5.chord);
        assert_eq!((c5.left, c5.right), (Some(2), None));
    }

    #[test]
    fn held_keys_that_make_no_chord_keep_the_scale() {
        let scale = c_major();
        let looks = KeyLooks::new(&scale, &[60, 62]);
        let c = looks.look(60, Some(90));
        assert!(!c.chord);
        assert_eq!((c.left, c.right), scale.fingers(60));
        assert_eq!(c.velocity, Some(90));
    }

    #[test]
    fn times_read_naturally() {
        assert_eq!(spent(Duration::ZERO), "–");
        assert_eq!(spent(Duration::from_secs(45)), "45 s");
        assert_eq!(spent(Duration::from_secs(12 * 60 + 30)), "12 min");
        assert_eq!(spent(Duration::from_mins(65)), "1 h 05 min");
        assert_eq!(ago(20_000), "just now");
        assert_eq!(ago(5 * 60_000), "5 min ago");
        assert_eq!(ago(3 * 3_600_000), "3 h ago");
        assert_eq!(ago(30 * 3_600_000), "yesterday");
        assert_eq!(ago(3 * 86_400_000), "3 days ago");
    }

    #[test]
    fn long_names_are_cut_with_an_ellipsis() {
        assert_eq!(shorten("Surge XT"), "Surge XT");
        let exact = "x".repeat(NAME_CHARS);
        assert_eq!(shorten(&exact), exact);
        let long = shorten("Midi Through:Midi Through Port-0 14:0");
        assert_eq!(long.chars().count(), NAME_CHARS);
        assert!(long.ends_with('…'));
        // Characters, not bytes: no panic in the middle of a multi-byte one.
        assert_eq!(shorten(&"ö".repeat(30)).chars().count(), NAME_CHARS);
    }
}
