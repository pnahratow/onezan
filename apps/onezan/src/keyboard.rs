//! The on-screen piano keyboard.
//!
//! Each key is drawn in layers, so the scale trainer can add to it without a redesign:
//!
//! 1. the key itself, white or black;
//! 2. its state: in the current scale (a light tint); pressed: a light blue tint, and a blue
//!    fill rising from the front edge like a level meter, as high as the key was struck hard
//!    (velocity 127 fills the key);
//! 3. up to two fingering badges, one per hand: a numbered circle in the hand's colour
//!    (right: blue, left: orange). They sit on the part of the key you see and touch: the lower
//!    end of a black key, and the front of a white key below the black keys. When both hands use
//!    the same key (hands together in unison, contrary motion), the left badge sits above the right
//!    one, matching where the hands are on the piano. A scale's fingering is a filled circle; a
//!    chord's is outlined (a white circle with the hand's colour for its ring and number), so the
//!    two never look alike when a chord is held while a scale is shown.
//!
//! C keys carry their octave name (C4 is middle C) at the very front.
//!
//! The keys can be played with the mouse: [`show`] says which key the pointer holds down.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use std::ops::RangeInclusive;

/// The keys shown by default: 49 keys, C2 to C6 (four octaves). That is the most common small
/// keyboard size (and the user's), and exactly two octaves per hand for scales hands together:
/// left hand C2 to C4, right hand C4 to C6, the hands meeting on middle C.
pub const DEFAULT_RANGE: RangeInclusive<u8> = 36..=84;

/// How one key looks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyLook {
    /// How hard the key was struck (1 to 127) while it is held; `None` when it is up.
    pub velocity: Option<u8>,
    pub in_scale: bool,
    /// The finger (1 to 5) of each hand on this key, if any.
    pub left: Option<u8>,
    pub right: Option<u8>,
    /// The fingers are a chord's (drawn outlined), not the scale's (filled).
    pub chord: bool,
}

pub const RIGHT_HAND: Color32 = Color32::from_rgb(37, 99, 235);
pub const LEFT_HAND: Color32 = Color32::from_rgb(217, 119, 6);

const WHITE: Color32 = Color32::from_rgb(250, 250, 247);
const WHITE_IN_SCALE: Color32 = Color32::from_rgb(254, 240, 199);
const WHITE_PRESSED: Color32 = Color32::from_rgb(214, 230, 255);
const WHITE_METER: Color32 = Color32::from_rgb(59, 130, 246);
const BLACK: Color32 = Color32::from_rgb(28, 28, 32);
const BLACK_IN_SCALE: Color32 = Color32::from_rgb(92, 72, 36);
const BLACK_PRESSED: Color32 = Color32::from_rgb(36, 52, 84);
const BLACK_METER: Color32 = Color32::from_rgb(96, 165, 250);
const EDGE: Color32 = Color32::from_rgb(70, 70, 76);
const LABEL: Color32 = Color32::from_rgb(130, 130, 136);

/// Whether `key` (a MIDI note number) is a black key.
#[must_use]
pub const fn is_black(key: u8) -> bool {
    matches!(key % 12, 1 | 3 | 6 | 8 | 10)
}

/// The name of a C key, "C4" for middle C (60).
fn octave_name(key: u8) -> String {
    format!("C{}", i32::from(key / 12).saturating_sub(1))
}

/// `n` as f32, for layout maths (counts here are small).
fn f(n: usize) -> f32 {
    u16::try_from(n).map_or(f32::MAX, f32::from)
}

/// Where the keys of a range are, in an area.
struct Layout {
    whites: Vec<u8>,
    left: f32,
    top: f32,
    white_w: f32,
    height: f32,
    black_w: f32,
    black_h: f32,
}

impl Layout {
    /// The keys of `range` filling `area`, sitting on its bottom edge; `None` without white keys.
    fn new(area: Rect, range: RangeInclusive<u8>) -> Option<Self> {
        let whites: Vec<u8> = range.filter(|&k| !is_black(k)).collect();
        if whites.is_empty() {
            return None;
        }
        let white_w = area.width() / f(whites.len());
        let height = area.height().min(white_w * 6.5);
        Some(Self {
            whites,
            left: area.left(),
            top: area.bottom() - height,
            white_w,
            height,
            black_w: white_w * 0.6,
            black_h: height * 0.62,
        })
    }

    /// The white keys with their rectangles, left to right.
    fn white_keys(&self) -> impl Iterator<Item = (u8, Rect)> + '_ {
        self.whites.iter().enumerate().map(|(i, &key)| {
            let x = self.white_w.mul_add(f(i), self.left);
            (
                key,
                Rect::from_min_size(pos2(x, self.top), vec2(self.white_w, self.height)),
            )
        })
    }

    /// The black key `key`'s rectangle, if it has a white key below it in the range.
    fn black_rect(&self, key: u8) -> Option<Rect> {
        // A black key sits on the line between the white key below it and the next one.
        let below = key
            .checked_sub(1)
            .and_then(|b| self.whites.iter().position(|&w| w == b))?;
        let line = self.white_w.mul_add(f(below.saturating_add(1)), self.left);
        Some(Rect::from_min_size(
            pos2(line - self.black_w / 2.0, self.top),
            vec2(self.black_w, self.black_h),
        ))
    }

    /// The key at `pos`: black keys lie over the white ones.
    fn key_at(&self, range: RangeInclusive<u8>, pos: Pos2) -> Option<u8> {
        range
            .filter(|&k| is_black(k))
            .find(|&k| self.black_rect(k).is_some_and(|r| r.contains(pos)))
            .or_else(|| self.white_keys().find(|(_, r)| r.contains(pos)).map(|(k, _)| k))
    }
}

/// Draws the keys in `range` filling the available space; `look` says how each key looks.
/// Returns the key the mouse holds down, if any (dragging slides from key to key).
pub fn show(ui: &mut Ui, range: RangeInclusive<u8>, look: impl Fn(u8) -> KeyLook) -> Option<u8> {
    let (area, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let layout = Layout::new(area, range.clone())?;
    let held = response
        .is_pointer_button_down_on()
        .then(|| response.interact_pointer_pos())
        .flatten()
        .and_then(|pos| layout.key_at(range.clone(), pos));
    let (height, black_w) = (layout.height, layout.black_w);
    let badge_r = (layout.white_w * 0.34).clamp(6.0, 12.0);
    let label_h = (height * 0.1).clamp(9.0, 14.0);
    let painter = ui.painter_at(area);

    for (key, rect) in layout.white_keys() {
        let l = look(key);
        let base = if l.in_scale { WHITE_IN_SCALE } else { WHITE };
        face(
            &painter,
            rect.shrink2(vec2(0.5, 0.0)),
            3.0,
            base,
            (WHITE_PRESSED, WHITE_METER),
            l.velocity,
        );
        if key % 12 == 0 {
            painter.text(
                pos2(rect.center().x, rect.bottom() - 2.0),
                Align2::CENTER_BOTTOM,
                octave_name(key),
                FontId::proportional(label_h * 0.85),
                // On the meter's blue when pressed.
                if l.velocity.is_some() { Color32::WHITE } else { LABEL },
            );
        }
        // Badges stack upwards from just above the octave label.
        let lowest = rect.bottom() - label_h - 2.0 - badge_r;
        badges(&painter, rect.center().x, lowest, badge_r, l);
    }

    for key in range.filter(|&k| is_black(k)) {
        let Some(rect) = layout.black_rect(key) else { continue };
        let l = look(key);
        let base = if l.in_scale { BLACK_IN_SCALE } else { BLACK };
        face(&painter, rect, 2.0, base, (BLACK_PRESSED, BLACK_METER), l.velocity);
        let r = badge_r.min(black_w * 0.45);
        badges(&painter, rect.center().x, rect.bottom() - r - 3.0, r, l);
    }
    held
}

/// A key's surface: `base`, or when pressed the tint of `pressed` with its meter colour rising
/// from the front edge in proportion to `velocity`; then the outline.
fn face(
    painter: &egui::Painter,
    rect: Rect,
    radius: f32,
    base: Color32,
    pressed: (Color32, Color32),
    velocity: Option<u8>,
) {
    let (tint, meter) = pressed;
    painter.rect_filled(rect, radius, if velocity.is_some() { tint } else { base });
    if let Some(v) = velocity {
        let height = rect.height() * f32::from(v.min(127)) / 127.0;
        let fill = Rect::from_min_max(pos2(rect.left(), rect.bottom() - height), rect.max);
        painter.rect_filled(fill, radius, meter);
    }
    painter.rect_stroke(rect, radius, Stroke::new(1.0, EDGE), StrokeKind::Inside);
}

/// The fingering badges of one key: right hand at `lowest`, left hand above it.
fn badges(painter: &egui::Painter, x: f32, lowest: f32, r: f32, l: KeyLook) {
    let mut y = lowest;
    for (finger, colour) in [(l.right, RIGHT_HAND), (l.left, LEFT_HAND)] {
        let Some(finger) = finger else { continue };
        badge(painter, pos2(x, y), r, finger, colour, l.chord);
        y -= r.mul_add(2.0, 2.0);
    }
}

fn badge(painter: &egui::Painter, at: Pos2, r: f32, finger: u8, colour: Color32, outlined: bool) {
    let (fill, ring, text) = if outlined {
        (Color32::WHITE, Stroke::new((r * 0.2).max(1.5), colour), colour)
    } else {
        (colour, Stroke::new(1.0, Color32::WHITE), Color32::WHITE)
    };
    painter.circle(at, r, fill, ring);
    painter.text(
        pos2(at.x, at.y + 0.5),
        Align2::CENTER_CENTER,
        finger.to_string(),
        FontId::proportional(r * 1.3),
        text,
    );
}

/// `range` widened to whole octaves so that it includes `key`. Keeps a played key visible.
#[must_use]
pub fn widen(range: &RangeInclusive<u8>, key: u8) -> RangeInclusive<u8> {
    let low = (*range.start()).min(key.saturating_sub(key % 12));
    let high_c = key.saturating_add(12_u8.saturating_sub(key % 12) % 12).min(127);
    let high = (*range.end()).max(high_c);
    low..=high
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_keys_and_names() {
        let black: Vec<u8> = (60..72).filter(|&k| is_black(k)).collect();
        assert_eq!(black, [61, 63, 66, 68, 70]);
        assert_eq!(octave_name(60), "C4");
        assert_eq!(octave_name(36), "C2");
    }

    #[test]
    fn default_range_is_49_keys_with_29_white() {
        assert_eq!(DEFAULT_RANGE.clone().count(), 49);
        assert_eq!(DEFAULT_RANGE.clone().filter(|&k| !is_black(k)).count(), 29);
    }

    #[test]
    fn the_mouse_finds_the_key_under_it() {
        // One octave, C4 to B4: 7 white keys, 70 px each, 300 px high.
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(490.0, 300.0));
        let layout = Layout::new(area, 60..=71).unwrap();
        let at = |x, y| layout.key_at(60..=71, pos2(x, y));
        // Low down, only white keys: C, D, ..., B.
        assert_eq!(at(10.0, 290.0), Some(60));
        assert_eq!(at(80.0, 290.0), Some(62));
        assert_eq!(at(480.0, 290.0), Some(71));
        // High up, on the line between C and D: C#, which lies over both.
        assert_eq!(at(70.0, 50.0), Some(61));
        // Between E and F there is no black key.
        assert_eq!(at(210.0, 50.0), Some(64));
        assert_eq!(at(215.0, 50.0), Some(65));
        assert_eq!(at(500.0, 50.0), None);
    }

    #[test]
    fn widen_keeps_whole_octaves() {
        assert_eq!(widen(&DEFAULT_RANGE, 60), DEFAULT_RANGE);
        assert_eq!(widen(&DEFAULT_RANGE, 31), 24..=84);
        assert_eq!(widen(&DEFAULT_RANGE, 85), 36..=96);
        assert_eq!(widen(&DEFAULT_RANGE, 127), 36..=127);
    }
}
