//! The window: the instrument at the top, a status bar, the keyboard at the bottom.
//!
//! The top shows the plugin's own editor ([`crate::embed`]). It is a native window over ours,
//! so nothing of ours can be drawn over it: the bar's menus and tooltips open downwards, over
//! the keyboard. On Linux that needs X11, so the
//! window runs on X11 (XWayland under a Wayland desktop).

use crate::chords::{self, Chord, Reading};
use crate::embed::{Embedded, PixelRect};
use crate::fingering::Hand;
use crate::fingering::{Preview, Scale};
use crate::keyboard::{self, KeyLook};
use crate::progress::{Practice, Recorder};
use crate::session::{Options, Remembered, Section, Session, port_name};
use crate::settings::{MetronomeSettings, Settings, Store};
use anyhow::{Result, anyhow};
use egui::{Color32, RichText};
use onezan_engine::metronome::{MAX_BPM, MIN_BPM};
use onezan_engine::{Click, Keys, Metronome, MidiChange, MidiTiming, Output, Verdict};
use onezan_vst3_host::PluginInfo;
use onezan_vst3_host::{Editor, MidiEvent};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::rc::Rc;
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
    preview: Preview,
    /// The last chord played, shown dimmed once the keys are up.
    last_chord: Option<Chord>,
    /// Which of the windows of their own are open.
    open: OpenWindows,
    /// The installed instruments, scanned when that window first opens (and on "Rescan").
    instruments: Option<Vec<PluginInfo>>,
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
            preview: settings
                .scale
                .as_deref()
                .and_then(Preview::from_name)
                .unwrap_or_default(),
            last_chord: None,
            open: OpenWindows::default(),
            instruments: None,
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
        let (session, midi, keys, preview) = (self.session.as_ref(), &self.midi, &self.keys, self.preview);
        let (lists, audio_note) = (&mut self.menus, self.audio_note.as_deref());
        let mut pick = None;
        let (options, scales, instruments) = (
            &mut self.open.options,
            &mut self.open.scales,
            &mut self.open.instruments,
        );
        let metronome = session.map(|s| Arc::clone(s.metronome()));
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
                // Always there, lit while the pedal is down: the layout does not move.
                let pedal = if keys.sustain() {
                    Color32::from_rgb(251, 191, 36)
                } else {
                    Color32::from_gray(70)
                };
                item(ui, "Pedal", "", Some(pedal), None);
                if let Some(s) = session {
                    let r = menu_item(ui, "Audio", short("audio out"), None, section("audio out"));
                    let shown = egui::Popup::menu(&r)
                        .align(egui::RectAlign::BOTTOM_START)
                        .align_alternatives(&[egui::RectAlign::BOTTOM_END])
                        .show(|ui| {
                            let outputs = lists.outputs.get_or_insert_with(onezan_engine::audio_outputs);
                            output_menu(ui, outputs, s, audio_note, &mut pick);
                        });
                    if shown.is_none() {
                        lists.outputs = None;
                    }
                    let r = menu_item(ui, "Timing", short("timing"), None, section("timing"));
                    egui::Popup::menu(&r)
                        .align(egui::RectAlign::BOTTOM_START)
                        .align_alternatives(&[egui::RectAlign::BOTTOM_END])
                        .show(|ui| timing_menu(ui, s.timing(), &mut pick));
                }
            },
            |ui| {
                toggle(ui, options, "Options").on_hover_text("Click sound and volume, in a window of its own");
                ui.separator();
                toggle(ui, scales, format!("Scale: {}", preview.name()))
                    .on_hover_text("Choose the scale whose fingering the keyboard shows, in a window of its own");
                if let Some(m) = &metronome {
                    ui.separator();
                    metronome_controls(ui, m);
                }
            },
        );
        if let Some(p) = pick {
            self.apply(p);
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
        let metronome = self.session.as_ref().map(|s| Arc::clone(s.metronome()));
        let builder = egui::ViewportBuilder::default()
            .with_title("onezan options")
            .with_app_id("onezan")
            .with_inner_size([340.0, 200.0]);
        let size = &mut self.options_size;
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
            });
            ui.ctx().input(|i| i.viewport().close_requested())
        });
        if close {
            self.open.options = false;
            self.options_size = None;
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
        let (mut pick, mut rescan) = (None, false);
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
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    if crate::piano::available() {
                        let selected = current.as_deref() == Some(onezan_engine::BUILT_IN_PIANO);
                        let text = format!("{}   FreePats Upright Piano KW", crate::piano::NAME);
                        let r = ui
                            .selectable_label(selected, text)
                            .on_hover_text("Built into onezan: a Kawai upright piano (public domain)");
                        if r.clicked() && !selected {
                            pick = Some(onezan_engine::BUILT_IN_PIANO.to_owned());
                        }
                    }
                    for p in list.iter() {
                        let selected = current.as_deref() == Some(p.class_id.as_str());
                        let text = format!("{}   {}", p.name, p.vendor);
                        let r = ui
                            .selectable_label(selected, text)
                            .on_hover_text(p.bundle.display().to_string());
                        if r.clicked() && !selected {
                            pick = Some(p.class_id.clone());
                        }
                    }
                });
            });
            ui.ctx().input(|i| i.viewport().close_requested())
        });
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
    /// learning order or by time spent.
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
            .with_inner_size([460.0, 640.0]);
        let (preview, order, practice) = (&mut self.preview, &mut self.scale_order, &self.practice);
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
                scale_list(ui, preview, *order, practice);
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

    fn placeholder(&self, ui: &mut egui::Ui) {
        ui.centered_and_justified(|ui| match (&self.session, &self.error) {
            (_, Some(e)) => {
                ui.label(RichText::new(e).color(Color32::from_rgb(239, 68, 68)));
            }
            (Some(s), None) if s.instrument().plugin().is_none() => {
                let mut text = format!(
                    "{}\n\nFreePats \"Upright Piano KW\", a Kawai upright piano (public domain).",
                    crate::piano::NAME
                );
                if s.is_fallback() {
                    text.push_str(
                        "\nNo VST3 instrument could be loaded. Install one and choose it under Instrument \
                         for other sounds.",
                    );
                }
                ui.label(RichText::new(text).weak());
            }
            (Some(s), None) => {
                let name = s.section("plugin").map_or("", |p| p.short.as_str());
                let why = self.editor_note.as_deref().unwrap_or("");
                ui.label(RichText::new(format!("{name}\n\nIts own window cannot be shown here: {why}")).weak());
            }
            (None, None) => {}
        });
    }

    fn keyboard(&mut self, ui: &mut egui::Ui) {
        // A key played outside the range widens it, so every played key stays visible.
        for key in 0..=127u8 {
            if self.keys.held(key) && !self.range.contains(&key) {
                self.range = keyboard::widen(&self.range, key);
            }
        }
        // The fingering shown widens it too, but only while it is shown.
        let shown = self.preview.shown();
        let range = shown.keys().fold(self.range.clone(), |r, key| keyboard::widen(&r, key));
        let (keys, mouse) = (&self.keys, self.mouse_key);
        // A chord held gets its fingering on its keys, in place of the scale's there.
        let held_keys = self.held_keys();
        let chord_fingers = if matches!(chords::read(&held_keys), Some(Reading::Chord(_))) {
            chords::fingering(&held_keys)
        } else {
            Vec::new()
        };
        let held = keyboard::show(ui, range, |key| {
            let mut fingers = chord_fingers.iter().filter(|&&(k, _, _)| k == key).peekable();
            let chord = fingers.peek().is_some();
            let (left, right) = if chord {
                fingers.fold((None, None), |(l, r), &(_, hand, finger)| match hand {
                    Hand::Left => (Some(finger), r),
                    Hand::Right => (l, Some(finger)),
                })
            } else {
                shown.fingers(key)
            };
            KeyLook {
                velocity: keys
                    .velocity(key)
                    .or_else(|| (mouse == Some(key)).then_some(MOUSE_VELOCITY)),
                in_scale: shown.in_scale(key),
                left,
                right,
                chord,
            }
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
                s.play(MidiEvent::NoteOff {
                    channel: 0,
                    key,
                    velocity: 64,
                });
            }
            if let Some(key) = held {
                s.play(MidiEvent::NoteOn {
                    channel: 0,
                    key,
                    velocity: MOUSE_VELOCITY,
                });
            }
        }
        self.mouse_key = held;
    }
}

/// How hard a key played with the mouse is struck: full velocity.
const MOUSE_VELOCITY: u8 = 127;

/// The metronome's controls, right to left (the bar's right side is laid out that way): the
/// tempo, then the switch. Its sound and volume are in the options window.
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

fn metronome_controls(ui: &mut egui::Ui, m: &Metronome) {
    let mut bpm = m.bpm();
    let r = ui
        .add(egui::DragValue::new(&mut bpm).range(MIN_BPM..=MAX_BPM).suffix(" BPM"))
        .on_hover_text("Tempo: drag, scroll or type");
    if r.changed() {
        m.set_bpm(bpm);
    }
    let on = m.is_on();
    let r = ui
        .add(egui::Button::new("").selected(on).min_size(egui::vec2(34.0, 24.0)))
        .on_hover_text("Metronome: start or stop (Space)");
    let colour = ui.style().interact_selectable(&r, on).fg_stroke.color;
    metronome_icon(ui.painter(), r.rect, colour);
    if r.clicked() {
        m.set_on(!on);
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

/// The metronome's sound and volume. Changing either plays a sample click.
fn metronome_options(ui: &mut egui::Ui, m: &Metronome) {
    egui::Grid::new("metronome options")
        .num_columns(2)
        .spacing([16.0, 10.0])
        .show(ui, |ui| {
            ui.label("Volume");
            let mut volume = m.volume();
            let r = ui.add(
                egui::Slider::new(&mut volume, 0.0..=1.0)
                    .custom_formatter(|v, _| format!("{:.0} %", v * 100.0))
                    .custom_parser(|t| t.trim_end_matches(['%', ' ']).parse::<f64>().ok().map(|v| v / 100.0)),
            );
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
    /// Most time spent first.
    Most,
    /// Least time spent first: what needs practice.
    Least,
}

impl ScaleOrder {
    const ALL: [Self; 3] = [Self::Learning, Self::Most, Self::Least];

    const fn name(self) -> &'static str {
        match self {
            Self::Learning => "Learning order",
            Self::Most => "Most practised",
            Self::Least => "Least practised",
        }
    }
}

/// The scales in `order`, as a table: name, time spent, last played. Clicking one shows it.
fn scale_list(ui: &mut egui::Ui, preview: &mut Preview, order: ScaleOrder, practice: &HashMap<String, Practice>) {
    let mut scales: Vec<(Scale, String, Practice)> = Scale::all()
        .map(|s| {
            let name = s.name();
            let p = practice.get(&name).copied().unwrap_or_default();
            (s, name, p)
        })
        .collect();
    // A stable sort: equal times keep the learning order.
    match order {
        ScaleOrder::Learning => {}
        ScaleOrder::Most => scales.sort_by_key(|(_, _, p)| std::cmp::Reverse(p.time)),
        ScaleOrder::Least => scales.sort_by_key(|(_, _, p)| p.time),
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok());
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        egui::Grid::new("scales")
            .num_columns(3)
            .striped(true)
            .spacing([18.0, 6.0])
            .show(ui, |ui| {
                ui.label(RichText::new("Scale").strong());
                ui.label(RichText::new("Time spent").strong());
                ui.label(RichText::new("Last played").strong());
                ui.end_row();
                for (scale, name, p) in scales {
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
            });
    });
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

/// One item of the top bar: "Label value", with the report's reasons on hover.
fn item(ui: &mut egui::Ui, label: &str, value: &str, dot: Option<Color32>, why: Option<&Section>) {
    let r = ui
        .horizontal(|ui| {
            ui.label(RichText::new(label).weak());
            if let Some(c) = dot {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, c);
            }
            if !value.is_empty() {
                ui.label(value);
            }
        })
        .response;
    if let Some(s) = why {
        // A layout's own response does not sense the pointer; this area does.
        ui.interact(r.rect, ui.id().with(label), egui::Sense::hover())
            .on_hover_text(explain(s));
    }
    ui.separator();
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
        // Space starts and stops the metronome, unless a text field has the keyboard.
        if ctx.input(|i| i.key_pressed(egui::Key::Space))
            && !ctx.egui_wants_keyboard_input()
            && let Some(m) = self.session.as_ref().map(Session::metronome)
        {
            m.set_on(!m.is_on());
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
        // Bottom panels stack upwards: the keyboard at the very bottom, the chord line, the bar.
        egui::Panel::bottom("keyboard")
            .exact_size(200.0)
            .show(ui, |ui| self.keyboard(ui));
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
