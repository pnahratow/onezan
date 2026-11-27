//! Timing calibration: the player plays a key on each click, and how far the notes arrive from
//! the clicks heard is measured. That offset covers the whole chain the app cannot see (the
//! audio output past what the system reports, the MIDI input, Bluetooth headphones, the
//! player's habit) and is taken off every note when judging timing.

/// Clicks played along to, after the first few (see [`SETTLE_BEATS`]).
pub const CLICKS: usize = 16;

/// The tempo of the calibration's clicks.
pub const BPM: u32 = 80;

/// Beats at the start that are not measured: the player finds the click first.
pub const SETTLE_BEATS: f64 = 2.0;

/// Notes further than this from a click (in beats) are not taken as played to it.
const NEAR_BEAT: f64 = 0.4;

/// A calibration more uneven than this (ms, about a standard deviation, see
/// [`Calibration::outcome`]) is not trusted: try again.
pub const MAX_SPREAD_MS: f64 = 60.0;

/// A calibration under way: the offsets measured so far.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Calibration {
    pub offsets_ms: Vec<f64>,
}

/// How a calibration came out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outcome {
    /// The notes arrive this many milliseconds after the click (negative: before).
    Offset(f64),
    /// Too uneven to tell: the spread, in milliseconds.
    Uneven(f64),
}

impl Calibration {
    /// A note heard at `beat` (the metronome's beat count, the click heard at whole beats), at
    /// `bpm`. Returns whether it was measured.
    pub fn note(&mut self, beat: f64, bpm: f64) -> bool {
        if beat < SETTLE_BEATS || self.done() || bpm <= 0.0 {
            return false;
        }
        let off = beat - beat.round();
        if off.abs() > NEAR_BEAT {
            return false;
        }
        self.offsets_ms.push(off * 60_000.0 / bpm);
        true
    }

    pub const fn done(&self) -> bool {
        self.offsets_ms.len() >= CLICKS
    }

    /// The result, once done: the median offset (an odd note does not pull it), unless the
    /// notes were too uneven to say.
    pub fn outcome(&self) -> Option<Outcome> {
        if !self.done() {
            return None;
        }
        let offset = median(&self.offsets_ms)?;
        // The spread as the median distance from the median, scaled to read like a standard
        // deviation: a stray note or two does not make it uneven.
        let distances: Vec<f64> = self.offsets_ms.iter().map(|o| (o - offset).abs()).collect();
        let spread = median(&distances)? * 1.4826;
        if spread > MAX_SPREAD_MS {
            return Some(Outcome::Uneven(spread));
        }
        Some(Outcome::Offset(offset))
    }
}

fn median(values: &[f64]) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        Some(f64::midpoint(*sorted.get(mid.checked_sub(1)?)?, *sorted.get(mid)?))
    } else {
        sorted.get(mid).copied()
    }
}

/// What the Options window says about a saved offset.
pub fn describe(offset_ms: f64) -> String {
    let ms = offset_ms.abs().round();
    if ms < 1.0 {
        "Your notes arrive with the click: nothing to correct".to_owned()
    } else if offset_ms > 0.0 {
        format!("Your notes arrive {ms:.0} ms after the click you hear: taken off when judging")
    } else {
        format!("Your notes arrive {ms:.0} ms before the click you hear: added when judging")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Notes on every beat from 2 on, each `late` beats after it plus a wobble.
    fn played(late: f64, wobble: &[f64]) -> Calibration {
        let mut c = Calibration::default();
        for i in 0..40_usize {
            let w = wobble
                .get(i.checked_rem(wobble.len()).unwrap_or(0))
                .copied()
                .unwrap_or(0.0);
            c.note(f64::from(u32::try_from(i).unwrap()) + late + w, 60.0);
        }
        c
    }

    #[test]
    fn the_offset_is_the_median_of_the_notes_to_the_clicks() {
        // At 60 BPM a beat is a second: 0.025 beats late is 25 ms.
        let c = played(0.025, &[0.0, 0.004, -0.004]);
        assert!(c.done());
        let Some(Outcome::Offset(ms)) = c.outcome() else {
            panic!("{:?}", c.outcome())
        };
        assert!((ms - 25.0).abs() < 0.5, "{ms}");
    }

    #[test]
    fn the_first_beats_and_stray_notes_are_not_measured() {
        let mut c = Calibration::default();
        assert!(!c.note(0.02, 60.0), "before the player has found the click");
        assert!(!c.note(1.98, 60.0));
        assert!(!c.note(2.5, 60.0), "half way between two clicks");
        assert!(c.note(2.03, 60.0));
        assert!(c.note(2.97, 60.0), "early is fine too");
        assert_eq!(c.offsets_ms.len(), 2);
        assert!((c.offsets_ms[1] + 30.0).abs() < 1e-6);
    }

    #[test]
    fn one_odd_note_does_not_move_the_median() {
        let mut c = played(0.020, &[0.0]);
        c.offsets_ms[3] = 300.0;
        let Some(Outcome::Offset(ms)) = c.outcome() else {
            panic!()
        };
        assert!((ms - 20.0).abs() < 0.5);
    }

    #[test]
    fn notes_all_over_the_place_are_not_trusted() {
        let c = played(0.0, &[0.1, -0.1, 0.15, -0.12]);
        assert!(matches!(c.outcome(), Some(Outcome::Uneven(_))), "{:?}", c.outcome());
    }

    #[test]
    fn the_offset_reads_as_a_sentence() {
        assert!(describe(23.4).contains("23 ms after"));
        assert!(describe(-8.0).contains("8 ms before"));
        assert!(describe(0.2).contains("nothing to correct"));
    }
}
