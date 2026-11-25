//! Music notation, drawn with Bravura (Steinberg's `SMuFL` font, SIL Open Font License 1.1,
//! `assets/fonts/`): for now a treble clef with a key signature, as the scales window lists them.
//!
//! In Bravura one em is four staff spaces, and every glyph sits on its baseline: the G clef's
//! on the G line, an accidental's on the line or space it alters.

use egui::{FontFamily, FontId, Sense, Stroke, Ui, pos2, vec2};

/// The font family holding Bravura (only Bravura, so no text font stands in for its glyphs).
const FAMILY: &str = "bravura";

/// `SMuFL` code points.
const G_CLEF: char = '\u{E050}';
const SHARP: char = '\u{E262}';
const FLAT: char = '\u{E260}';

/// Where the sharps and flats of a key signature go on the treble staff, in order, as steps
/// (lines and spaces) above the bottom line, E4: F# C# G# D# A# E# B#; Bb Eb Ab Db Gb Cb Fb.
const SHARPS: [i8; 7] = [8, 5, 9, 6, 3, 7, 4];
const FLATS: [i8; 7] = [4, 7, 3, 6, 2, 5, 1];

/// The G clef's line (G4), in steps above the bottom line.
const G_LINE: i8 = 2;

/// The space between two staff lines, in points.
const SPACE: f32 = 4.5;

/// Extents in staff spaces (Bravura's metadata): the G clef's width and its reach below and
/// above the bottom line; an accidental's width plus the gap to the next.
const CLEF_WIDTH: f32 = 2.7;
const BELOW: f32 = 1.7;
const ABOVE: f32 = 5.5;
const ACCIDENTAL: f32 = 1.1;

/// Makes Bravura available to [`key_signature`]. Call once, at the start: it sets egui's fonts
/// (the defaults plus Bravura).
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        FAMILY.to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../../assets/fonts/Bravura.otf"
        ))),
    );
    // The text fonts come after Bravura only for egui's replacement character, which it
    // looks for in every family.
    let text = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let family = std::iter::once(FAMILY.to_owned()).chain(text).collect();
    fonts.families.insert(FontFamily::Name(FAMILY.into()), family);
    ctx.set_fonts(fonts);
}

/// A treble staff with a G clef and the key signature of `sig` sharps (positive) or flats
/// (negative). Always as wide as seven accidentals, so a column of them lines up.
pub fn key_signature(ui: &mut Ui, sig: i8) -> egui::Response {
    let s = SPACE;
    let width = 7.0f32.mul_add(ACCIDENTAL, 0.5 + CLEF_WIDTH + 0.6 + 0.4) * s;
    let (rect, response) = ui.allocate_exact_size(vec2(width, (BELOW + ABOVE) * s), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter_at(rect);
    let ink = ui.visuals().text_color();
    let bottom = BELOW.mul_add(-s, rect.max.y);
    let y = |step: i8| f32::from(step).mul_add(-s / 2.0, bottom);
    let line = Stroke::new(1.0, ui.visuals().weak_text_color());
    for step in [0, 2, 4, 6, 8] {
        painter.hline(rect.x_range(), y(step), line);
    }
    let font = FontId::new(4.0 * s, FontFamily::Name(FAMILY.into()));
    let glyph = |c: char, x: f32, step: i8| {
        let galley = painter.layout_no_wrap(c.to_string(), font.clone(), ink);
        // Where the galley puts the glyph's baseline, from its top.
        let baseline = galley
            .rows
            .first()
            .and_then(|r| r.glyphs.first().map(|g| r.pos.y + g.pos.y))
            .unwrap_or(0.0);
        painter.galley(pos2(x, y(step) - baseline), galley, ink);
    };
    let left = 0.5f32.mul_add(s, rect.min.x);
    glyph(G_CLEF, left, G_LINE);
    let (symbol, steps) = if sig >= 0 { (SHARP, SHARPS) } else { (FLAT, FLATS) };
    let mut x = (CLEF_WIDTH + 0.6).mul_add(s, left);
    for &step in steps.iter().take(usize::from(sig.unsigned_abs())) {
        glyph(symbol, x, step);
        x = ACCIDENTAL.mul_add(s, x);
    }
    response
}
