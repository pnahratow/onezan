//! The window: a bar at the top, the instrument in the middle, the keyboard at the bottom.

use crate::fingering::Preview;
use crate::keyboard::{self, KeyLook};
use crate::session::{Options, Section, Session};
use anyhow::{Result, anyhow};
use egui::{Color32, RichText};
use onezan_engine::{Keys, MidiChange, Verdict};
use std::ops::RangeInclusive;
use std::sync::{Arc, Mutex, PoisonError};

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
    eframe::run_native(
        "onezan",
        native,
        Box::new(move |cc| Ok(Box::new(App::new(&cc.egui_ctx, &opts)))),
    )
    .map_err(|e| anyhow!("cannot open the window: {e}"))
}

/// The MIDI keyboard as the bar shows it.
#[derive(Clone)]
enum MidiState {
    Connected(String),
    Waiting,
    Failed(String),
}

struct App {
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
    fn new(ctx: &egui::Context, opts: &Options) -> Self {
        let keys = Arc::new(Keys::default());
        let repaint = ctx.clone();
        keys.set_waker(move || repaint.request_repaint());
        let midi = Arc::new(Mutex::new(MidiState::Waiting));
        let (shared, repaint) = (Arc::clone(&midi), ctx.clone());
        let on_midi = move |c: MidiChange| {
            println!("{:<10}{}", "MIDI in", crate::midi_change(&c));
            *shared.lock().unwrap_or_else(PoisonError::into_inner) = midi_state(&c);
            repaint.request_repaint();
        };
        let (session, error) = match Session::start(opts, Vec::new(), Arc::clone(&keys), on_midi) {
            Ok(s) => {
                crate::print_report(&s);
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
        Self {
            session,
            error,
            keys,
            midi,
            range: keyboard::DEFAULT_RANGE,
            preview: Preview::default(),
        }
    }

    fn nav(&mut self, ui: &mut egui::Ui) {
        let (session, midi, keys, preview) = (self.session.as_ref(), &self.midi, &self.keys, &mut self.preview);
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
                if keys.sustain() {
                    ui.label(RichText::new("pedal").small().strong());
                    ui.separator();
                }
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
            },
        );
    }

    fn instrument(&self, ui: &mut egui::Ui) {
        ui.centered_and_justified(|ui| match (&self.session, &self.error) {
            (_, Some(e)) => {
                ui.label(RichText::new(e).color(Color32::from_rgb(239, 68, 68)));
            }
            (Some(s), None) => {
                let name = s.section("plugin").map_or("", |p| p.short.as_str());
                ui.label(RichText::new(format!("{name}\n\nThe instrument's own window will be shown here.")).weak());
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
                pressed: keys.held(key),
                in_scale: preview.in_scale(key),
                left,
                right,
            }
        });
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
            ui.label(value);
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("nav").exact_size(40.0).show(ui, |ui| self.nav(ui));
        egui::Panel::bottom("keyboard")
            .exact_size(200.0)
            .show(ui, |ui| self.keyboard(ui));
        egui::CentralPanel::default().show(ui, |ui| self.instrument(ui));
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(s) = self.session.take() {
            s.stop();
        }
    }
}
