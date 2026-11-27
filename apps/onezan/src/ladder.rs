//! The tempo ladder for scale exercises: a run played clean moves the metronome up to the next
//! metronome marking, a near miss stays, a run with many notes missed goes one marking down
//! (teachers raise the tempo gradually and step back when it falls apart: see
//! docs/research/learning-scales.md, sections 4 and 6). What counts as clean is here.

use crate::waterfall::Evenness;

/// The traditional metronome markings (Maelzel's): the steps of the ladder.
pub const MARKINGS: [u32; 39] = [
    40, 42, 44, 46, 48, 50, 52, 54, 56, 58, 60, 63, 66, 69, 72, 76, 80, 84, 88, 92, 96, 100, 104, 108, 112, 116, 120,
    126, 132, 138, 144, 152, 160, 168, 176, 184, 192, 200, 208,
];

/// The most the timing may spread (its standard deviation) for a clean run, in milliseconds.
/// onezan's own choice (no teaching source puts a number on it): about 3 to 10 % of the time
/// between two notes over the ladder's tempos.
pub const CLEAN_SPREAD_MS: f64 = 30.0;

/// A run with at least this share of its notes on time, but not clean, stays at its tempo;
/// fewer, and the ladder steps down.
pub const NEAR_MISS: f64 = 0.9;

/// Whether a run was clean: every note on time, and the timing within [`CLEAN_SPREAD_MS`].
pub fn clean(e: &Evenness) -> bool {
    e.notes > 0 && e.played == e.notes && e.spread_ms.is_some_and(|s| s <= CLEAN_SPREAD_MS)
}

/// The next marking above `bpm` (`bpm` itself at the top).
pub fn next(bpm: u32) -> u32 {
    MARKINGS.into_iter().find(|&m| m > bpm).unwrap_or(bpm)
}

/// The marking below `bpm` (`bpm` itself at the bottom).
pub fn previous(bpm: u32) -> u32 {
    MARKINGS.into_iter().rev().find(|&m| m < bpm).unwrap_or(bpm)
}

/// The tempo after a run at `bpm`: up when clean, the same after a near miss, down after one
/// with many notes missed.
pub fn after(e: &Evenness, bpm: u32) -> u32 {
    if clean(e) {
        return next(bpm);
    }
    let on_time = f64::from(u32::try_from(e.played).unwrap_or(u32::MAX));
    let all = f64::from(u32::try_from(e.notes).unwrap_or(u32::MAX));
    if on_time >= NEAR_MISS * all { bpm } else { previous(bpm) }
}

/// What the end of a run says about the ladder: clean or not, and the tempo the next run is at.
pub fn verdict(clean: bool, bpm: u32, next: u32) -> String {
    match (clean, next.cmp(&bpm)) {
        (true, std::cmp::Ordering::Greater) => format!("Clean at {bpm}: next {next}"),
        (true, _) => format!("Clean at {bpm}, the top of the ladder"),
        (false, std::cmp::Ordering::Less) => format!("Many notes missed: down to {next}"),
        (false, _) => {
            format!("Not clean yet (every note on time, timing within ±{CLEAN_SPREAD_MS:.0} ms): again at {bpm}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(played: usize, notes: usize, spread: Option<f64>) -> Evenness {
        Evenness {
            played,
            notes,
            spread_ms: spread,
            lean_ms: Some(0.0),
            loudness: Some(5.0),
            hands_apart_ms: None,
        }
    }

    #[test]
    fn clean_is_every_note_on_time_and_even_timing() {
        assert!(clean(&run(29, 29, Some(18.0))));
        assert!(clean(&run(29, 29, Some(CLEAN_SPREAD_MS))));
        assert!(!clean(&run(28, 29, Some(10.0))), "a note missed");
        assert!(!clean(&run(29, 29, Some(45.0))), "uneven");
        assert!(!clean(&run(29, 29, None)), "no timing (waiting)");
        assert!(!clean(&run(0, 0, None)));
    }

    #[test]
    fn the_ladder_climbs_by_metronome_markings() {
        assert_eq!(next(60), 63);
        assert_eq!(next(61), 63, "between two markings: the next one up");
        assert_eq!(next(88), 92);
        assert_eq!(next(20), 40);
        assert_eq!(next(208), 208, "the top");
        assert!(MARKINGS.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn a_near_miss_stays_and_many_misses_step_down() {
        assert_eq!(after(&run(29, 29, Some(15.0)), 88), 92, "clean: up");
        assert_eq!(after(&run(29, 29, Some(40.0)), 88), 88, "all on time but uneven: stays");
        assert_eq!(after(&run(27, 29, Some(15.0)), 88), 88, "two missed of 29: stays");
        assert_eq!(after(&run(20, 29, Some(15.0)), 88), 84, "nine missed: down");
        assert_eq!(after(&run(0, 29, None), 40), 40, "the bottom");
        assert_eq!(previous(63), 60);
    }

    #[test]
    fn the_verdict_says_where_the_next_run_is() {
        assert_eq!(verdict(true, 88, 92), "Clean at 88: next 92");
        assert!(verdict(false, 88, 88).ends_with("again at 88"));
        assert!(verdict(true, 208, 208).contains("top"));
        assert_eq!(verdict(false, 88, 84), "Many notes missed: down to 84");
    }
}
