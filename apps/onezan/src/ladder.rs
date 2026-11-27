//! The tempo ladder for scale exercises: a run played clean moves the metronome up to the next
//! metronome marking, a near miss stays, a run with many notes missed goes one marking down
//! (teachers raise the tempo gradually and step back when it falls apart: see
//! docs/research/learning-scales.md, sections 4 and 6). What counts as clean is here.
//!
//! Past the fastest marking the click does not get faster: two notes go to each click at half
//! the tempo (208 → 108, two a beat), later four, as teachers keep the beat and change the
//! subdivision. The ladder climbs in notes per minute.

use crate::waterfall::Evenness;

/// The traditional metronome markings (Maelzel's): the steps of the ladder.
pub const MARKINGS: [u32; 39] = [
    40, 42, 44, 46, 48, 50, 52, 54, 56, 58, 60, 63, 66, 69, 72, 76, 80, 84, 88, 92, 96, 100, 104, 108, 112, 116, 120,
    126, 132, 138, 144, 152, 160, 168, 176, 184, 192, 200, 208,
];

/// Where the ladder starts for a scale or exercise newly chosen: one note to a click at 60
/// (teachers start at 40 to 60 to the quarter note; exams' first levels ask about 54 to 60).
pub const START: Speed = Speed { bpm: 60, per_beat: 1 };

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

/// The most stars a run gets.
pub const MAX_STARS: u8 = 5;

/// Stars for a run, 0 to 5, in step with the ladder: 5 clean with every gauge good (timing,
/// early or late, loudness, hands together), 4 clean (both go up a marking), 3 a near miss
/// (stays), 2 at least three quarters of the notes on time, 1 at least half, 0 fewer.
pub fn stars(e: &Evenness) -> u8 {
    let share =
        f64::from(u32::try_from(e.played).unwrap_or(u32::MAX)) / f64::from(u32::try_from(e.notes.max(1)).unwrap_or(1));
    let all_good = || e.gauges().iter().all(|g| g.grade() == crate::waterfall::Grade::Good);
    if clean(e) && all_good() {
        5
    } else if clean(e) {
        4
    } else if share >= NEAR_MISS {
        3
    } else if share >= 0.75 {
        2
    } else {
        u8::from(share >= 0.5)
    }
}

/// A step of the ladder: the metronome's tempo, and the notes played to each click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Speed {
    pub bpm: u32,
    pub per_beat: u8,
}

impl Speed {
    pub fn notes_per_minute(self) -> u32 {
        self.bpm.saturating_mul(u32::from(self.per_beat.max(1)))
    }

    /// "88", or "112 ×2" for two notes a click.
    pub fn short(self) -> String {
        match self.per_beat {
            0 | 1 => self.bpm.to_string(),
            n => format!("{} ×{n}", self.bpm),
        }
    }

    /// "88", or "112, two notes a click".
    pub fn long(self) -> String {
        match self.per_beat {
            0 | 1 => self.bpm.to_string(),
            2 => format!("{}, two notes a click", self.bpm),
            n => format!("{}, {n} notes a click", self.bpm),
        }
    }
}

/// The highest tempo with four notes a click: about 10 notes a second.
const TOP_FOUR: u32 = 160;

/// The ladder's steps, slowest first: every marking with one note a click, then two notes a
/// click from where that is faster than the fastest one, then four likewise, up to
/// [`TOP_FOUR`].
pub fn steps() -> Vec<Speed> {
    let mut out: Vec<Speed> = Vec::new();
    for per_beat in [1u8, 2, 4] {
        for bpm in MARKINGS {
            let step = Speed { bpm, per_beat };
            let faster = out
                .last()
                .is_none_or(|l| step.notes_per_minute() > l.notes_per_minute());
            if faster && (per_beat < 4 || bpm <= TOP_FOUR) {
                out.push(step);
            }
        }
    }
    out
}

/// The step after `s` (the first faster one; `s` itself at the top).
pub fn next(s: Speed) -> Speed {
    steps()
        .into_iter()
        .find(|t| t.notes_per_minute() > s.notes_per_minute())
        .unwrap_or(s)
}

/// The step before `s` (the last slower one; `s` itself at the bottom).
pub fn previous(s: Speed) -> Speed {
    steps()
        .into_iter()
        .rev()
        .find(|t| t.notes_per_minute() < s.notes_per_minute())
        .unwrap_or(s)
}

/// The speed after a run at `s`: up when clean, the same after a near miss, down after one
/// with many notes missed.
pub fn after(e: &Evenness, s: Speed) -> Speed {
    if clean(e) {
        return next(s);
    }
    let on_time = f64::from(u32::try_from(e.played).unwrap_or(u32::MAX));
    let all = f64::from(u32::try_from(e.notes).unwrap_or(u32::MAX));
    if on_time >= NEAR_MISS * all { s } else { previous(s) }
}

/// What the end of a run says about the ladder: clean or not, and the speed of the next run.
pub fn verdict(clean: bool, s: Speed, next: Speed) -> String {
    let (now, then) = (s.long(), next.long());
    match (clean, next.notes_per_minute().cmp(&s.notes_per_minute())) {
        (true, std::cmp::Ordering::Greater) => format!("Clean at {now}: next {then}"),
        (true, _) => format!("Clean at {now}, the top of the ladder"),
        (false, std::cmp::Ordering::Less) => format!("Many notes missed: down to {then}"),
        (false, _) => {
            format!("Not clean yet (every note on time, timing within ±{CLEAN_SPREAD_MS:.0} ms): again at {now}")
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

    fn one(bpm: u32) -> Speed {
        Speed { bpm, per_beat: 1 }
    }

    #[test]
    fn the_ladder_climbs_by_metronome_markings() {
        assert_eq!(next(one(60)), one(63));
        assert_eq!(next(one(61)), one(63), "between two markings: the next one up");
        assert_eq!(next(one(88)), one(92));
        assert_eq!(next(one(20)), one(40));
        assert!(MARKINGS.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn past_the_fastest_click_the_notes_per_click_go_up() {
        let two = |bpm| Speed { bpm, per_beat: 2 };
        let four = |bpm| Speed { bpm, per_beat: 4 };
        assert_eq!(next(one(208)), two(108), "208 a minute, then 216");
        assert_eq!(next(two(108)), two(112));
        assert_eq!(next(two(208)), four(108), "416, then 432");
        assert_eq!(next(four(160)), four(160), "the top: 640 notes a minute");
        assert_eq!(previous(two(108)), one(208));
        // Every step faster than the one before.
        let s = steps();
        assert!(s.windows(2).all(|w| w[0].notes_per_minute() < w[1].notes_per_minute()));
        // Three notes a click (chosen by hand) climbs to the next step that is faster.
        assert_eq!(next(Speed { bpm: 80, per_beat: 3 }), two(126), "240, then 252");
        assert_eq!(two(112).short(), "112 ×2");
        assert_eq!(two(112).long(), "112, two notes a click");
    }

    #[test]
    fn a_near_miss_stays_and_many_misses_step_down() {
        assert_eq!(after(&run(29, 29, Some(15.0)), one(88)), one(92), "clean: up");
        assert_eq!(
            after(&run(29, 29, Some(40.0)), one(88)),
            one(88),
            "all on time but uneven: stays"
        );
        assert_eq!(
            after(&run(27, 29, Some(15.0)), one(88)),
            one(88),
            "two missed of 29: stays"
        );
        assert_eq!(after(&run(20, 29, Some(15.0)), one(88)), one(84), "nine missed: down");
        assert_eq!(after(&run(0, 29, None), one(40)), one(40), "the bottom");
        assert_eq!(previous(one(63)), one(60));
    }

    #[test]
    fn stars_follow_the_ladder() {
        assert_eq!(stars(&run(29, 29, Some(15.0))), 5, "clean, every gauge good");
        assert_eq!(stars(&run(29, 29, Some(25.0))), 4, "clean, timing only fair");
        let mut loud = run(29, 29, Some(15.0));
        loud.loudness = Some(9.0);
        assert_eq!(stars(&loud), 4, "clean, loudness only fair");
        assert_eq!(
            stars(&run(29, 29, Some(40.0))),
            3,
            "all on time but uneven: a near miss"
        );
        assert_eq!(stars(&run(27, 29, Some(15.0))), 3, "a near miss");
        assert_eq!(stars(&run(23, 29, Some(15.0))), 2, "79 %");
        assert_eq!(stars(&run(15, 29, Some(15.0))), 1, "52 %");
        assert_eq!(stars(&run(10, 29, Some(15.0))), 0);
        // Up the ladder exactly from four stars.
        for e in [
            run(29, 29, Some(15.0)),
            run(29, 29, Some(25.0)),
            run(27, 29, Some(15.0)),
        ] {
            assert_eq!(stars(&e) >= 4, after(&e, one(88)) != one(88));
        }
    }

    #[test]
    fn gauges_rate_each_measure() {
        use crate::waterfall::Grade;
        let mut e = run(29, 29, Some(14.0));
        e.lean_ms = Some(-18.0);
        e.hands_apart_ms = Some(40.0);
        let g = e.gauges();
        let find = |label: &str| g.iter().find(|x| x.label == label).unwrap();
        assert_eq!(find("Timing evenness").grade(), Grade::Good);
        assert_eq!(find("Early · late").grade(), Grade::Fair);
        assert_eq!(find("Early · late").text, "18 ms early");
        assert_eq!(find("Loudness evenness").grade(), Grade::Good);
        assert_eq!(find("Hands together").grade(), Grade::Poor);
        // Waiting has no timing: loudness only.
        let w = Evenness {
            spread_ms: None,
            lean_ms: None,
            hands_apart_ms: None,
            ..e
        };
        assert_eq!(w.gauges().len(), 1);
    }

    #[test]
    fn the_verdict_says_where_the_next_run_is() {
        assert_eq!(verdict(true, one(88), one(92)), "Clean at 88: next 92");
        assert!(verdict(false, one(88), one(88)).ends_with("again at 88"));
        let top = Speed { bpm: 160, per_beat: 4 };
        assert!(verdict(true, top, top).contains("top"));
        assert_eq!(verdict(false, one(88), one(84)), "Many notes missed: down to 84");
        assert_eq!(
            verdict(true, one(208), Speed { bpm: 108, per_beat: 2 }),
            "Clean at 208: next 108, two notes a click"
        );
    }
}
