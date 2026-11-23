//! The on-screen piano keyboard.
//!
//! Each key is drawn in layers, so the scale trainer can add to it without a redesign:
//!
//! 1. the key itself, white or black;
//! 2. its state: in the current scale (a light tint), pressed (a strong colour);
//! 3. up to two fingering badges, one per hand: a numbered circle in the hand's colour
//!    (right: blue, left: orange). They sit on the part of the key you see and touch: the lower
//!    end of a black key, and the front of a white key below the black keys. When both hands use
//!    the same key (hands together in unison, contrary motion), the left badge sits above the right
//!    one, matching where the hands are on the piano.
//!
//! C keys carry their octave name (C4 is middle C) at the very front.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use std::ops::RangeInclusive;

/// The keys shown by default: 49 keys, C2 to C6 (four octaves). That is the most common small
/// keyboard size (and the user's), and exactly two octaves per hand for scales hands together:
/// left hand C2 to C4, right hand C4 to C6, the hands meeting on middle C.
pub const DEFAULT_RANGE: RangeInclusive<u8> = 36..=84;

/// How one key looks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyLook {
    pub pressed: bool,
    pub in_scale: bool,
    /// The finger (1 to 5) of each hand on this key, if any.
    pub left: Option<u8>,
    pub right: Option<u8>,
}

pub const RIGHT_HAND: Color32 = Color32::from_rgb(37, 99, 235);
pub const LEFT_HAND: Color32 = Color32::from_rgb(217, 119, 6);

const WHITE: Color32 = Color32::from_rgb(250, 250, 247);
const WHITE_IN_SCALE: Color32 = Color32::from_rgb(254, 240, 199);
const WHITE_PRESSED: Color32 = Color32::from_rgb(125, 180, 255);
const BLACK: Color32 = Color32::from_rgb(28, 28, 32);
const BLACK_IN_SCALE: Color32 = Color32::from_rgb(92, 72, 36);
const BLACK_PRESSED: Color32 = Color32::from_rgb(40, 96, 190);
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

/// Draws the keys in `range` filling the available space; `look` says how each key looks.
pub fn show(ui: &mut Ui, range: RangeInclusive<u8>, look: impl Fn(u8) -> KeyLook) {
    let (area, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let whites: Vec<u8> = range.clone().filter(|&k| !is_black(k)).collect();
    if whites.is_empty() {
        return;
    }
    let white_w = area.width() / f(whites.len());
    let height = area.height().min(white_w * 6.5);
    let top = area.bottom() - height;
    let black_w = white_w * 0.6;
    let black_h = height * 0.62;
    let badge_r = (white_w * 0.34).clamp(6.0, 12.0);
    let label_h = (height * 0.1).clamp(9.0, 14.0);
    let painter = ui.painter_at(area);

    for (i, &key) in whites.iter().enumerate() {
        let x = white_w.mul_add(f(i), area.left());
        let rect = Rect::from_min_size(pos2(x, top), vec2(white_w, height));
        let l = look(key);
        let fill = if l.pressed {
            WHITE_PRESSED
        } else if l.in_scale {
            WHITE_IN_SCALE
        } else {
            WHITE
        };
        painter.rect(
            rect.shrink2(vec2(0.5, 0.0)),
            3.0,
            fill,
            Stroke::new(1.0, EDGE),
            StrokeKind::Inside,
        );
        if key % 12 == 0 {
            painter.text(
                pos2(rect.center().x, rect.bottom() - 2.0),
                Align2::CENTER_BOTTOM,
                octave_name(key),
                FontId::proportional(label_h * 0.85),
                LABEL,
            );
        }
        // Badges stack upwards from just above the octave label.
        let lowest = rect.bottom() - label_h - 2.0 - badge_r;
        badges(&painter, rect.center().x, lowest, badge_r, l);
    }

    for key in range.filter(|&k| is_black(k)) {
        // A black key sits on the line between the white key below it and the next one.
        let Some(below) = key.checked_sub(1).and_then(|b| whites.iter().position(|&w| w == b)) else {
            continue;
        };
        let line = white_w.mul_add(f(below.saturating_add(1)), area.left());
        let rect = Rect::from_min_size(pos2(line - black_w / 2.0, top), vec2(black_w, black_h));
        let l = look(key);
        let fill = if l.pressed {
            BLACK_PRESSED
        } else if l.in_scale {
            BLACK_IN_SCALE
        } else {
            BLACK
        };
        painter.rect(rect, 2.0, fill, Stroke::new(1.0, EDGE), StrokeKind::Inside);
        let r = badge_r.min(black_w * 0.45);
        badges(&painter, rect.center().x, rect.bottom() - r - 3.0, r, l);
    }
}

/// The fingering badges of one key: right hand at `lowest`, left hand above it.
fn badges(painter: &egui::Painter, x: f32, lowest: f32, r: f32, l: KeyLook) {
    let mut y = lowest;
    for (finger, colour) in [(l.right, RIGHT_HAND), (l.left, LEFT_HAND)] {
        let Some(finger) = finger else { continue };
        badge(painter, pos2(x, y), r, finger, colour);
        y -= r.mul_add(2.0, 2.0);
    }
}

fn badge(painter: &egui::Painter, at: Pos2, r: f32, finger: u8, colour: Color32) {
    painter.circle(at, r, colour, Stroke::new(1.0, Color32::WHITE));
    painter.text(
        pos2(at.x, at.y + 0.5),
        Align2::CENTER_CENTER,
        finger.to_string(),
        FontId::proportional(r * 1.3),
        Color32::WHITE,
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
    fn widen_keeps_whole_octaves() {
        assert_eq!(widen(&DEFAULT_RANGE, 60), DEFAULT_RANGE);
        assert_eq!(widen(&DEFAULT_RANGE, 31), 24..=84);
        assert_eq!(widen(&DEFAULT_RANGE, 85), 36..=96);
        assert_eq!(widen(&DEFAULT_RANGE, 127), 36..=127);
    }
}
