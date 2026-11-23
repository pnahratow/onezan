//! The window: the instrument at the top, a status bar, the keyboard at the bottom.
//!
//! The top shows the plugin's own editor ([`crate::embed`]). It is a native window over ours,
//! so nothing of ours can be drawn over it: the bar's menus and tooltips open downwards, over
//! the keyboard. On Linux that needs X11, so the
//! window runs on X11 (XWayland under a Wayland desktop).

use crate::embed::{Embedded, PixelRect};
use crate::fingering::Preview;
use crate::keyboard::{self, KeyLook};
use crate::session::{Options, Section, Session};
use anyhow::{Result, anyhow};
use egui::{Color32, RichText};
use onezan_engine::metronome::{MAX_BPM, MIN_BPM};
use onezan_engine::{Keys, Metronome, MidiChange, Verdict};
use onezan_vst3_host::Editor;
use std::cell::RefCell;
use std::ops::RangeInclusive;
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// How often the plugin's editor is serviced (its timers and X11 connection) while it is open.
const EDITOR_IDLE: Duration = Duration::from_millis(4);

/// The embedded editor, shared by the window and the loop that services it (UI thread only).
type Shared = Rc<RefCell<Option<Embedded>>>;

/// Opens the window and runs until it is closed.
///
/// # Errors
///
/// When no window can be opened.
pub fn run(opts: Options) -> Result<()> {
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("onezan")
            .with_app_id("onezan")
            .with_inner_size([1200.0, 760.0])
            .with_min_inner_size([640.0, 420.0]),
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
        Box::new(move |cc| Ok(Box::new(App::new(&cc.egui_ctx, &opts, shared)))),
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
    /// The window was sized to the editor once.
    sized: bool,
    session: Option<Session>,
    /// Why there is no session.
    error: Option<String>,
    keys: Arc<Keys>,
    midi: Arc<Mutex<MidiState>>,
    range: RangeInclusive<u8>,
    preview: Preview,
}

fn midi_state(c: &MidiChange) -> MidiState {
    match c {
        MidiChange::Connected(p) => MidiState::Connected(p.clone()),
        MidiChange::Disconnected(_) => MidiState::Waiting,
        MidiChange::Failed { port, error } => MidiState::Failed(format!("cannot open {port}: {error}")),
    }
}

impl App {
    fn new(ctx: &egui::Context, opts: &Options, embedded: Shared) -> Self {
        let keys = Arc::new(Keys::default());
        let repaint = ctx.clone();
        keys.set_waker(move || repaint.request_repaint());
        let midi = Arc::new(Mutex::new(MidiState::Waiting));
        let (shared, repaint) = (Arc::clone(&midi), ctx.clone());
        let on_midi = move |c: MidiChange| {
            println!("{:<10}{}", "MIDI in", crate::report::midi_change(&c));
            *shared.lock().unwrap_or_else(PoisonError::into_inner) = midi_state(&c);
            repaint.request_repaint();
        };
        let (session, error) = match Session::start(opts, Vec::new(), Arc::clone(&keys), on_midi) {
            Ok(s) => {
                crate::report::print_report(&s);
                if let Some(c) = &s.first_midi {
                    *midi.lock().unwrap_or_else(PoisonError::into_inner) = midi_state(c);
                }
                (Some(s), None)
            }
            Err(e) => {
                eprintln!("error: {e:#}");
                (None, Some(format!("{e:#}")))
            }
        };
        let (editor, editor_note) = match session.as_ref().map(|s| s.plugin().editor()) {
            Some(Ok(e)) => {
                let repaint = ctx.clone();
                e.set_waker(move || repaint.request_repaint());
                (Some(e), None)
            }
            Some(Err(e)) => (None, Some(e.to_string())),
            None => (None, None),
        };
        Self {
            editor,
            embedded,
            editor_note,
            sized: false,
            session,
            error,
            keys,
            midi,
            range: keyboard::DEFAULT_RANGE,
            preview: Preview::default(),
        }
    }

    fn bar(&mut self, ui: &mut egui::Ui) {
        let (session, midi, keys, preview) = (self.session.as_ref(), &self.midi, &self.keys, &mut self.preview);
        let metronome = session.map(|s| Arc::clone(s.metronome()));
        // The left part gives way (truncated) when the window is narrow; the picker stays whole.
        egui::Sides::new().shrink_left().truncate().show(
            ui,
            |ui| {
                ui.label(RichText::new("onezan").strong().size(18.0));
                ui.separator();
                let section = |label| session.and_then(|s| s.section(label));
                let short = |label| section(label).map_or("none", |s: &Section| s.short.as_str());
                item(ui, "Instrument", short("plugin"), None, section("plugin"));

                let midi = midi.lock().unwrap_or_else(PoisonError::into_inner).clone();
                let (text, dot) = match &midi {
                    MidiState::Connected(p) => (p.clone(), Color32::from_rgb(34, 197, 94)),
                    MidiState::Waiting => ("plug in a keyboard".to_string(), Color32::GRAY),
                    MidiState::Failed(e) => (e.clone(), Color32::from_rgb(239, 68, 68)),
                };
                item(ui, "MIDI", &text, Some(dot), section("MIDI in"));
                // Always there, lit while the pedal is down: the layout does not move.
                let pedal = if keys.sustain() {
                    Color32::from_rgb(251, 191, 36)
                } else {
                    Color32::from_gray(70)
                };
                item(ui, "Pedal", "", Some(pedal), None);
                item(ui, "Audio", short("audio out"), None, section("audio out"));
                if section("timing").is_some() {
                    item(ui, "Timing", short("timing"), None, section("timing"));
                }
            },
            |ui| {
                egui::ComboBox::from_id_salt("scale")
                    .selected_text(format!("Fingering: {}", preview.name()))
                    .show_ui(ui, |ui| {
                        for p in Preview::ALL {
                            ui.selectable_value(preview, p, p.name());
                        }
                    })
                    .response
                    .on_hover_text("Which scale fingering the keyboard shows");
                if let Some(m) = &metronome {
                    ui.separator();
                    metronome_controls(ui, m);
                }
            },
        );
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
        let (keys, preview) = (&self.keys, self.preview);
        keyboard::show(ui, self.range.clone(), |key| {
            let (left, right) = preview.fingers(key);
            KeyLook {
                velocity: keys.velocity(key),
                in_scale: preview.in_scale(key),
                left,
                right,
            }
        });
    }
}

/// The metronome's controls, right to left (the bar's right side is laid out that way): the
/// tempo, then the switch.
fn metronome_controls(ui: &mut egui::Ui, m: &Metronome) {
    let mut bpm = m.bpm();
    let r = ui
        .add(egui::DragValue::new(&mut bpm).range(MIN_BPM..=MAX_BPM).suffix(" BPM"))
        .on_hover_text("Tempo: drag, scroll or type");
    if r.changed() {
        m.set_bpm(bpm);
    }
    let mut on = m.is_on();
    if ui
        .toggle_value(&mut on, "Metronome")
        .on_hover_text("Start or stop the metronome (Space)")
        .clicked()
    {
        m.set_on(on);
    }
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
        // Bottom panels stack upwards: the keyboard at the very bottom, the bar above it.
        egui::Panel::bottom("keyboard")
            .exact_size(200.0)
            .show(ui, |ui| self.keyboard(ui));
        egui::Panel::bottom("bar").exact_size(40.0).show(ui, |ui| self.bar(ui));
        let fill = ui.visuals().panel_fill;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(fill))
            .show(ui, |ui| self.instrument(ui, frame));
    }
}

impl Drop for App {
    fn drop(&mut self) {
        // The editor is removed before its window and its plugin go.
        drop(self.embedded.borrow_mut().take());
        drop(self.editor.take());
        if let Some(s) = self.session.take() {
            s.stop();
        }
    }
}

/// The window's native id, for the editor's parent: the X11 window on Linux.
fn window_id(frame: &eframe::Frame) -> Result<u64, String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = frame.window_handle().map_err(|e| format!("no window handle: {e}"))?;
    match handle.as_raw() {
        #[allow(clippy::useless_conversion, reason = "c_ulong is u32 on 32-bit systems")]
        RawWindowHandle::Xlib(h) => Ok(u64::from(h.window)),
        RawWindowHandle::Xcb(h) => Ok(u64::from(h.window.get())),
        _ => Err("the window is not an X11 window".into()),
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
